//! The two gpg operations: resolve a recipient, and decrypt→encrypt through a pipe.
//!
//! There is no `pass` here on purpose. `pass` follows symlinks on read and writes through them
//! on insert, and its delete path on a linked directory is unsafe; this tool needs the store's
//! files, not the manage script's traversal.
//!
//! Plaintext exists only in the pipe between the two gpg processes. The source store is opened
//! read-only, and the output is verified to be encrypted to the expected encryption subkey.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use at_core::contract::Fail;

use crate::fsutil;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    /// The primary fingerprint, which is what `pass` writes into `.gpg-id`.
    pub fingerprint: String,
    /// The 16-hex id of the encryption key (a subkey in the usual case).
    pub encryption_key_id: String,
}

struct Record {
    validity: String,
    key_id: String,
    capabilities: String,
    fingerprint: Option<String>,
}

impl Record {
    fn from_fields(fields: &[&str]) -> Record {
        Record {
            validity: fields.get(1).copied().unwrap_or("").to_string(),
            key_id: fields.get(4).copied().unwrap_or("").to_string(),
            capabilities: fields.get(11).copied().unwrap_or("").to_string(),
            fingerprint: None,
        }
    }

    fn unusable(&self) -> bool {
        matches!(self.validity.as_str(), "r" | "e" | "i" | "d")
    }

    fn encrypts(&self) -> bool {
        self.capabilities.contains('e') && !self.unusable()
    }
}

/// Resolve a recipient in the current keyring. Public keys only; this reads no secret material.
pub fn inspect(recipient: &str) -> Result<Recipient, Fail> {
    let output = Command::new("gpg")
        .args(["--batch", "--with-colons", "--list-keys", recipient])
        .output()
        .map_err(|error| Fail::environment(format!("cannot run gpg: {error}")))?;

    if !output.status.success() {
        return Err(Fail::refused(format!(
            "cannot resolve recipient `{recipient}` · {}",
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }

    parse_recipient(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_recipient(raw: &str) -> Result<Recipient, Fail> {
    let mut publishes: Vec<Record> = Vec::new();
    let mut subkeys: Vec<Record> = Vec::new();
    let mut last_was_subkey = false;

    for line in raw.lines() {
        let fields = line.split(':').collect::<Vec<_>>();
        match fields.first().copied().unwrap_or("") {
            "pub" => {
                publishes.push(Record::from_fields(&fields));
                last_was_subkey = false;
            }
            "sub" => {
                subkeys.push(Record::from_fields(&fields));
                last_was_subkey = true;
            }
            "fpr" => {
                let Some(fingerprint) = fields.get(9).copied() else {
                    continue;
                };
                if fingerprint.is_empty() {
                    continue;
                }
                let record = if last_was_subkey {
                    subkeys.last_mut()
                } else {
                    publishes.last_mut()
                };
                if let Some(record) = record {
                    if record.fingerprint.is_none() {
                        record.fingerprint = Some(fingerprint.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    if publishes.len() != 1 {
        return Err(Fail::usage(format!(
            "the recipient resolves to {} keys · use one full fingerprint",
            publishes.len()
        )));
    }

    let publish = publishes.remove(0);
    if publish.unusable() {
        return Err(Fail::refused(
            "the recipient key is revoked, expired, invalid or disabled",
        ));
    }
    let fingerprint = publish
        .fingerprint
        .clone()
        .ok_or_else(|| Fail::environment("gpg printed a public key without a fingerprint"))?;

    let encryption_key_id = if publish.encrypts() {
        publish.key_id.clone()
    } else {
        subkeys
            .iter()
            .find(|subkey| subkey.encrypts())
            .map(|subkey| subkey.key_id.clone())
            .ok_or_else(|| Fail::refused("the recipient has no usable encryption subkey"))?
    };

    if encryption_key_id.is_empty() {
        return Err(Fail::refused(
            "the recipient's encryption key has no key id",
        ));
    }

    Ok(Recipient {
        fingerprint,
        encryption_key_id: encryption_key_id.to_ascii_uppercase(),
    })
}

/// Decrypt `source` and encrypt the stream to `recipient`. No plaintext file is created.
pub fn decrypt_to_encrypt(
    source: &Path,
    destination: &Path,
    recipient: &Recipient,
) -> Result<(), Fail> {
    let mut decrypt = Command::new("gpg")
        .args(["--quiet", "--decrypt"])
        .arg(source)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| Fail::environment(format!("cannot run gpg decrypt: {error}")))?;

    let plaintext = decrypt
        .stdout
        .take()
        .ok_or_else(|| Fail::environment("gpg decrypt produced no stdout"))?;

    let encrypt = Command::new("gpg")
        .args([
            "--batch",
            "--yes",
            "--quiet",
            "--no-throw-keyids",
            "--no-encrypt-to",
            "--trust-model",
            "always",
            "--encrypt",
            "--recipient",
        ])
        .arg(&recipient.fingerprint)
        .arg("--output")
        .arg(destination)
        .stdin(Stdio::from(plaintext))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| Fail::environment(format!("cannot run gpg encrypt: {error}")))?;

    let decrypt_status = decrypt
        .wait()
        .map_err(|error| Fail::environment(format!("cannot wait for gpg decrypt: {error}")))?;

    // Encryption is checked first: if it failed, decrypt's own failure is usually the broken pipe
    // that followed, and reporting that would point at the wrong side of the operation.
    if !encrypt.status.success() {
        let _ = fs::remove_file(destination);
        return Err(Fail::environment(format!(
            "gpg could not encrypt {} · {}",
            destination.display(),
            first_line(&String::from_utf8_lossy(&encrypt.stderr))
        )));
    }

    if !decrypt_status.success() {
        let _ = fs::remove_file(destination);
        return Err(Fail::environment(format!(
            "gpg could not decrypt {} · see gpg's message above",
            source.display()
        )));
    }

    verify_recipient(destination, &recipient.encryption_key_id)?;
    fsutil::set_mode(destination, 0o600)
}

/// Read the packet headers and confirm the expected encryption key is one of the recipients.
/// This needs no secret key, but `gpg --list-packets` still tries to decrypt when one is
/// available; with no secret key it prints the key id and exits non-zero. The id is therefore the
/// answer, and the exit status is only consulted when no id was printed at all.
fn verify_recipient(path: &Path, expected: &str) -> Result<(), Fail> {
    let output = Command::new("gpg")
        .args(["--batch", "--list-packets"])
        .arg(path)
        .output()
        .map_err(|error| Fail::environment(format!("cannot run gpg list-packets: {error}")))?;

    let expected = expected.to_ascii_uppercase();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if mentions_key_id(&stdout, &expected) || mentions_key_id(&stderr, &expected) {
        return Ok(());
    }

    if !output.status.success() {
        return Err(Fail::environment(format!(
            "cannot inspect the encryption packets of {} · {}",
            path.display(),
            last_line(&stderr)
        )));
    }

    Err(Fail::refused(format!(
        "{} is not encrypted to the expected recipient key {expected} · check that gpg's \
         throw-keyids setting is not hiding the key id",
        path.display()
    )))
}

/// A key id appears in two shapes, depending on which stream gpg used: `keyid <id>` in the packet
/// listing, and `ID <id>` in the "encrypted with …" summary. Both are 16 hex digits.
fn mentions_key_id(text: &str, expected: &str) -> bool {
    for line in text.lines() {
        for marker in ["keyid ", "ID "] {
            let mut rest = line;
            while let Some(at) = rest.find(marker) {
                let candidate = &rest[at + marker.len()..];
                let end = candidate
                    .find(|character: char| !character.is_ascii_hexdigit())
                    .unwrap_or(candidate.len());
                if end == 16 && candidate[..end].eq_ignore_ascii_case(expected) {
                    return true;
                }
                rest = if end < candidate.len() {
                    &candidate[end + 1..]
                } else {
                    ""
                };
            }
        }
    }
    false
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "no stderr".to_string())
}

fn last_line(text: &str) -> String {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "no stderr".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_KEY: &str = "\
pub:u:2048:1:1111111111111111:1371525757::::::scESC:::::::
fpr:::::::::AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:
sub:u:2048:1:2222222222222222:1731494237::::::e:::::::
fpr:::::::::BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB:
";

    #[test]
    fn a_recipient_uses_the_encryption_subkey() {
        let recipient = parse_recipient(ONE_KEY).expect("recipient");
        assert_eq!(
            recipient.fingerprint,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
        assert_eq!(recipient.encryption_key_id, "2222222222222222");
    }

    #[test]
    fn a_key_without_an_encryption_subkey_is_refused() {
        let raw = ONE_KEY.replace("::::::e:::::::", "::::::s:::::::");
        let error = parse_recipient(&raw).expect_err("no encryption key");
        assert!(error.message.contains("no usable encryption subkey"));
    }

    #[test]
    fn multiple_keys_are_ambiguous() {
        let raw = format!("{ONE_KEY}{ONE_KEY}");
        let error = parse_recipient(&raw).expect_err("ambiguous");
        assert!(error.message.contains("resolves to 2 keys"));
    }

    #[test]
    fn a_revoked_key_is_refused() {
        let raw = ONE_KEY.replacen("pub:u:", "pub:r:", 1);
        let error = parse_recipient(&raw).expect_err("revoked");
        assert!(error.message.contains("revoked"));
    }

    #[test]
    fn a_key_id_is_found_in_the_packet_listing() {
        let listing = ":pubkey enc packet: version 3, algo 18, keyid 19990289A6A735CF\n";
        assert!(mentions_key_id(listing, "19990289A6A735CF"));
        assert!(!mentions_key_id(listing, "0000000000000000"));
    }

    #[test]
    fn a_key_id_is_found_in_the_gpg_summary() {
        let summary = "gpg: encrypted with cv25519 key, ID 19990289A6A735CF, created 2026-10-31\n";
        assert!(mentions_key_id(summary, "19990289A6A735CF"));
    }

    #[test]
    fn a_different_key_id_does_not_match_a_prefix_or_an_extension() {
        let listing = ":pubkey enc packet: version 3, algo 18, keyid 19990289A6A735CF\n";
        assert!(!mentions_key_id(listing, "19990289A6A735C"));
        assert!(!mentions_key_id(listing, "19990289A6A735CF0"));
    }
}
