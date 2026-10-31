//! Assemble the payload: validate the whitelist, verify the recipient, then decrypt and
//! re-encrypt each entry into a self-contained store.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use at_core::contract::{plural, Fail, Outcome, Report};

use crate::manifest::{Entry, Manifest};
use crate::selection;
use crate::whitelist::Whitelist;
use crate::{fsutil, gpg, selection::Entry as Selected, TOOL};

#[allow(clippy::too_many_arguments)]
pub fn build(
    store: &Path,
    whitelist_path: &Path,
    recipient: &str,
    out: &Path,
    allow_non_tty: bool,
) -> Result<Outcome, Fail> {
    if !allow_non_tty && !std::io::stdin().is_terminal() {
        return Err(Fail::refused(
            "travel-secrets decrypts the source store · run it from an interactive terminal, \
             or pass --allow-non-tty only when pinentry is arranged another way",
        ));
    }

    let out = prepare_output(out)?;
    let whitelist = Whitelist::read(whitelist_path)?;
    let entries = selection::select(store, &whitelist)?;
    let recipient = gpg::inspect(recipient)?;
    let source_note = source_recipient_note(store, &recipient.fingerprint)?;

    let staging = out.join("store.tmp");
    fs::create_dir(&staging).map_err(|error| {
        Fail::environment(format!("cannot create {}: {error}", staging.display()))
    })?;
    fsutil::set_mode(&staging, 0o700)?;

    let gpg_id = staging.join(".gpg-id");
    fs::write(&gpg_id, format!("{}\n", recipient.fingerprint)).map_err(|error| {
        Fail::environment(format!("cannot write {}: {error}", gpg_id.display()))
    })?;
    fsutil::set_mode(&gpg_id, 0o600)?;

    let assembled = match assemble(store, &staging, &entries, &recipient) {
        Ok(assembled) => assembled,
        Err(error) => {
            if let Err(cleanup) = fs::remove_dir_all(&staging) {
                eprintln!("{TOOL}: could not remove {}: {cleanup}", staging.display());
            }
            return Err(error);
        }
    };

    let store_dir = out.join("store");
    fs::rename(&staging, &store_dir).map_err(|error| {
        Fail::environment(format!(
            "cannot move {} to {}: {error}",
            staging.display(),
            store_dir.display()
        ))
    })?;
    assembled.manifest.write(&out)?;

    let mut report = Report::new();
    report.header(
        TOOL,
        "build",
        &store.display().to_string(),
        &format!(
            "{} · {}",
            short(&recipient.fingerprint, 16),
            plural(assembled.manifest.entries.len(), "entry")
        ),
    );

    for entry in &assembled.manifest.entries {
        report.content(format!(
            "{} · {} · {}",
            entry.entry,
            fsutil::human(entry.bytes),
            short(&entry.sha256, 12)
        ));
    }

    report.bound(format!(
        "payload: {} · {} · {}",
        plural(assembled.manifest.entries.len(), "entry"),
        fsutil::human(assembled.total_bytes),
        store_dir.display()
    ));
    report.bound(format!("recipient: {}", recipient.fingerprint));
    if let Some(note) = source_note {
        report.bound(note);
    }
    report.bound(format!(
        "restore on the target: rsync -a {}/ ~/.password-store/",
        store_dir.display()
    ));

    Ok(Outcome::from_report(report))
}

struct Assembled {
    manifest: Manifest,
    total_bytes: u64,
}

fn assemble(
    store: &Path,
    staging: &Path,
    entries: &[Selected],
    recipient: &gpg::Recipient,
) -> Result<Assembled, Fail> {
    let mut manifest_entries = Vec::with_capacity(entries.len());
    let mut total_bytes = 0;

    for entry in entries {
        let source = store.join(&entry.file);
        let destination = staging.join(&entry.file);

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                Fail::environment(format!("cannot create {}: {error}", parent.display()))
            })?;
            fsutil::set_mode(parent, 0o700)?;
        }

        gpg::decrypt_to_encrypt(&source, &destination, recipient)?;

        let bytes = fs::metadata(&destination)
            .map_err(|error| {
                Fail::environment(format!("cannot read {}: {error}", destination.display()))
            })?
            .len();
        let sha256 = fsutil::sha256_file(&destination)?;
        total_bytes += bytes;

        manifest_entries.push(Entry {
            entry: entry.entry.clone(),
            bytes,
            sha256,
        });
    }

    Ok(Assembled {
        manifest: Manifest {
            recipient: recipient.fingerprint.clone(),
            entries: manifest_entries,
        },
        total_bytes,
    })
}

fn prepare_output(out: &Path) -> Result<PathBuf, Fail> {
    match fs::symlink_metadata(out) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(Fail::refused(format!(
                    "{} is a symlink · choose a real directory",
                    out.display()
                )));
            }
            if !metadata.is_dir() {
                return Err(Fail::refused(format!(
                    "{} is not a directory",
                    out.display()
                )));
            }
            let mut entries = fs::read_dir(out).map_err(|error| {
                Fail::environment(format!("cannot read {}: {error}", out.display()))
            })?;
            if entries.next().is_some() {
                return Err(Fail::refused(format!(
                    "{} is not empty · remove the old payload or choose a new directory",
                    out.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(out).map_err(|error| {
                Fail::environment(format!("cannot create {}: {error}", out.display()))
            })?;
        }
        Err(error) => {
            return Err(Fail::environment(format!(
                "cannot read {}: {error}",
                out.display()
            )));
        }
    }

    fs::canonicalize(out)
        .map_err(|error| Fail::environment(format!("cannot resolve {}: {error}", out.display())))
}

fn source_recipient_note(store: &Path, recipient: &str) -> Result<Option<String>, Fail> {
    let path = store.join(".gpg-id");
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let present = raw
                .lines()
                .any(|line| line.trim().eq_ignore_ascii_case(recipient));
            Ok(present.then(|| {
                format!("recipient {recipient} is already a recipient of the source store")
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Fail::environment(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

fn short(value: &str, width: usize) -> &str {
    &value[..value.len().min(width)]
}
