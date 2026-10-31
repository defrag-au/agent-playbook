//! Round-trip tests for the gpg path.
//!
//! Every key here is a throwaway key in an isolated temporary `GNUPGHOME`. The tests never read
//! the real store or the real keyring, and nothing is written outside the fixture's temp root.
//!
//! An agent must not run this file against a real keyring; the human runs
//! `cargo test -p travel-secrets --test encrypt` and reads the result.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    primary: PathBuf,
    travel: PathBuf,
    travel_fpr: String,
    store: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Fixture {
        let root = temp_root(label);
        let primary = root.join("primary");
        let travel = root.join("travel");
        let store = root.join("store");
        fs::create_dir_all(&store).expect("store");

        let primary_fpr = generate_key(&primary, "primary@test.invalid");
        let travel_fpr = generate_key(&travel, "travel@test.invalid");
        import_public(&root, &primary, &travel, &travel_fpr);

        // The primary's real gpg.conf carries `throw-keyids`; the tool has to override it at
        // encryption time or `verify_recipient` cannot see the key id it is checking.
        fs::write(primary.join("gpg.conf"), "throw-keyids\n").expect("gpg.conf");

        for (entry, secret) in [
            ("github/foo", "alpha"),
            ("github/sub/bar", "beta"),
            ("cloudflare/api", "gamma"),
            ("personal/secret", "delta"),
        ] {
            let destination = store.join(format!("{entry}.gpg"));
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).expect("entry directory");
            }
            encrypt_file(&primary, &primary_fpr, secret, &destination);
        }
        fs::write(store.join(".gpg-id"), format!("{primary_fpr}\n")).expect("gpg-id");

        Fixture {
            root,
            primary,
            travel,
            travel_fpr,
            store,
        }
    }

    fn whitelist(&self, value: &str) -> PathBuf {
        let path = self.root.join("credentials.md");
        fs::write(&path, format!("pass: {value}\n")).expect("whitelist");
        path
    }

    fn run(&self, whitelist: &Path, out: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_travel-secrets"))
            .env("GNUPGHOME", &self.primary)
            .arg("--store")
            .arg(&self.store)
            .arg("--whitelist")
            .arg(whitelist)
            .arg("--recipient")
            .arg(&self.travel_fpr)
            .arg("--out")
            .arg(out)
            .arg("--allow-non-tty")
            .output()
            .expect("run travel-secrets")
    }

    fn out(&self, label: &str) -> PathBuf {
        self.root.join(format!("out-{label}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        kill_agent(&self.primary);
        kill_agent(&self.travel);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn temp_root(label: &str) -> PathBuf {
    // gpg-agent binds a Unix socket under GNUPGHOME, and macOS's sun_path is 104 bytes. A long
    // per-session TMPDIR (a Zed agent terminal's is already ~70) plus a fixture name overflows it
    // before any key is generated, so prefer the short /tmp and fall back only if it is absent.
    let base = if Path::new("/tmp").is_dir() {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let root = base.join(format!("ts-{label}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).expect("temp root");
    root
}

fn kill_agent(home: &Path) {
    let _ = Command::new("gpgconf")
        .arg("--homedir")
        .arg(home)
        .args(["--kill", "gpg-agent"])
        .status();
}

fn gpg(home: &Path, args: &[&str]) -> Output {
    Command::new("gpg")
        .arg("--homedir")
        .arg(home)
        .args(args)
        .output()
        .expect("run gpg")
}

fn generate_key(home: &Path, email: &str) -> String {
    fs::create_dir_all(home).expect("gnupg home");
    fs::set_permissions(home, fs::Permissions::from_mode(0o700)).expect("gnupg mode");
    // An empty passphrase still goes through the agent, and loopback mode is refused unless the
    // agent is told it is allowed. Without this the fixture fails before it reaches the tool.
    fs::write(home.join("gpg-agent.conf"), "allow-loopback-pinentry\n").expect("gpg-agent.conf");

    let output = gpg(
        home,
        &[
            "--batch",
            "--passphrase",
            "",
            "--pinentry-mode",
            "loopback",
            "--quick-gen-key",
            email,
            "default",
            "default",
            "1d",
        ],
    );
    assert!(
        output.status.success(),
        "key generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let listed = gpg(home, &["--with-colons", "--list-keys", email]);
    assert!(
        listed.status.success(),
        "key listing failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .find(|line| line.starts_with("fpr:"))
        .and_then(|line| line.split(':').nth(9))
        .map(str::to_string)
        .expect("a fingerprint")
}

fn import_public(root: &Path, primary: &Path, travel: &Path, travel_fpr: &str) {
    let exported = gpg(travel, &["--armor", "--export", travel_fpr]);
    assert!(
        exported.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(!exported.stdout.is_empty(), "empty export");

    // `gpg --import` takes a file, and the exported body has to be one before it can be imported.
    let path = root.join("travel.pub.asc");
    fs::write(&path, &exported.stdout).expect("public key file");

    let imported = gpg(
        primary,
        &[
            "--batch",
            "--import",
            path.to_str().expect("public key path is UTF-8"),
        ],
    );
    assert!(
        imported.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );
}

fn encrypt_file(home: &Path, recipient: &str, plaintext: &str, destination: &Path) {
    let source = destination.with_extension("plaintext");
    fs::write(&source, plaintext).expect("plaintext");
    let output = gpg(
        home,
        &[
            "--batch",
            "--yes",
            "--trust-model",
            "always",
            "--recipient",
            recipient,
            "--encrypt",
            "--output",
            destination.to_str().expect("destination path is UTF-8"),
            source.to_str().expect("source path is UTF-8"),
        ],
    );
    fs::remove_file(&source).expect("remove plaintext");
    assert!(
        output.status.success(),
        "encryption failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn decrypt(home: &Path, path: &Path) -> Output {
    Command::new("gpg")
        .arg("--homedir")
        .arg(home)
        .args(["--batch", "--quiet", "--decrypt"])
        .arg(path)
        .output()
        .expect("run gpg decrypt")
}

/// Every file under `store/`, relative to it and sorted.
fn files_under(store: &Path) -> Vec<String> {
    let mut found = Vec::new();
    walk(store, store, &mut found);
    found.sort();
    found
}

fn gpg_entries(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|file| file.ends_with(".gpg"))
        .cloned()
        .collect()
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("metadata");
        if metadata.is_dir() {
            walk(root, &path, found);
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("inside the store")
                .to_string_lossy()
                .replace('\\', "/");
            found.push(relative);
        }
    }
}

#[test]
fn only_the_whitelisted_entries_are_carried() {
    let fixture = Fixture::new("selection");
    let out = fixture.out("selection");
    let whitelist = fixture.whitelist("github, cloudflare/api");

    let output = fixture.run(&whitelist, &out);
    assert!(
        output.status.success(),
        "travel-secrets failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let store = out.join("store");
    let files = files_under(&store);
    assert_eq!(
        gpg_entries(&files),
        ["cloudflare/api.gpg", "github/foo.gpg", "github/sub/bar.gpg"]
    );
    assert_eq!(
        files,
        [
            ".gpg-id",
            "cloudflare/api.gpg",
            "github/foo.gpg",
            "github/sub/bar.gpg"
        ],
        "the output store holds only entries and its own .gpg-id"
    );
    assert!(
        !store.join("personal/secret.gpg").exists(),
        "a non-whitelisted entry was carried"
    );

    let gpg_id = fs::read_to_string(store.join(".gpg-id")).expect("output .gpg-id");
    assert_eq!(gpg_id.trim(), fixture.travel_fpr);
}

#[test]
fn every_entry_is_encrypted_to_the_travel_key_and_only_that_key() {
    let fixture = Fixture::new("recipient");
    let out = fixture.out("recipient");
    let whitelist = fixture.whitelist("github, cloudflare/api");

    let output = fixture.run(&whitelist, &out);
    assert!(
        output.status.success(),
        "travel-secrets failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let store = out.join("store");
    for (file, plaintext) in [
        ("github/foo.gpg", "alpha"),
        ("github/sub/bar.gpg", "beta"),
        ("cloudflare/api.gpg", "gamma"),
    ] {
        let path = store.join(file);

        let travel = decrypt(&fixture.travel, &path);
        assert!(
            travel.status.success(),
            "{file} did not decrypt with the travel key: {}",
            String::from_utf8_lossy(&travel.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&travel.stdout).trim(), plaintext);

        let primary = decrypt(&fixture.primary, &path);
        assert!(
            !primary.status.success(),
            "{file} decrypted with the source keyring, so it is still encrypted to the primary"
        );
    }
}

#[test]
fn the_manifest_records_the_recipient_and_every_carried_entry() {
    let fixture = Fixture::new("manifest");
    let out = fixture.out("manifest");
    let whitelist = fixture.whitelist("github, cloudflare/api");

    let output = fixture.run(&whitelist, &out);
    assert!(
        output.status.success(),
        "travel-secrets failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = fs::read_to_string(out.join("manifest")).expect("manifest");
    assert!(manifest.contains("version: 1"), "{manifest}");
    assert!(
        manifest.contains(&format!("recipient: {}", fixture.travel_fpr)),
        "{manifest}"
    );
    assert!(manifest.contains("entries: 3"), "{manifest}");

    let store = out.join("store");
    for file in gpg_entries(&files_under(&store)) {
        let entry = file.trim_end_matches(".gpg");
        assert!(manifest.contains(&format!("entry: {entry}")), "{manifest}");

        let bytes = fs::metadata(store.join(&file)).expect("metadata").len();
        let sha256 = travel_secrets::fsutil::sha256_file(&store.join(&file)).expect("sha256");
        assert!(manifest.contains(&format!("bytes: {bytes}")), "{manifest}");
        assert!(manifest.contains(&sha256), "{manifest}");
    }

    assert!(
        !manifest.contains("personal/secret"),
        "the manifest names an entry that was not carried"
    );
}

#[test]
fn a_missing_entry_is_refused_and_writes_no_payload() {
    let fixture = Fixture::new("missing");
    let out = fixture.out("missing");
    let whitelist = fixture.whitelist("does-not-exist");

    let output = fixture.run(&whitelist, &out);
    assert_eq!(output.status.code(), Some(4), "expected a refusal");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a directory or a `.gpg` entry"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!out.join("store").exists());
    assert!(!out.join("manifest").exists());
}

#[test]
fn a_symlink_in_the_store_is_refused() {
    let fixture = Fixture::new("symlink");
    let out = fixture.out("symlink");
    std::os::unix::fs::symlink("github/foo.gpg", fixture.store.join("link.gpg")).expect("symlink");
    let whitelist = fixture.whitelist("link");

    let output = fixture.run(&whitelist, &out);
    assert_eq!(output.status.code(), Some(4), "expected a refusal");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("symlink"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!out.join("store").exists());
}

#[test]
fn a_second_run_into_the_same_output_is_refused() {
    let fixture = Fixture::new("occupied");
    let out = fixture.out("occupied");
    let whitelist = fixture.whitelist("github");

    let first = fixture.run(&whitelist, &out);
    assert!(
        first.status.success(),
        "travel-secrets failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = fixture.run(&whitelist, &out);
    assert_eq!(second.status.code(), Some(4), "expected a refusal");
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("not empty"),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
}
