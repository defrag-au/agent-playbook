//! The payload manifest: flat `key: value` text, the same shape as the other control files.
//!
//! It records what was carried, not what was allowed: entry, ciphertext size and SHA-256, and
//! the recipient fingerprint. It is written last, after the store has been assembled.

use std::fs;
use std::path::Path;

use at_core::contract::Fail;

use crate::fsutil;

pub const FILE: &str = "manifest";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub entry: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub recipient: String,
    pub entries: Vec<Entry>,
}

impl Manifest {
    pub fn render(&self) -> String {
        let mut out = format!(
            "version: {VERSION}\nrecipient: {}\nentries: {}\n",
            self.recipient,
            self.entries.len()
        );
        for entry in &self.entries {
            out.push_str(&format!(
                "\nentry: {}\nbytes: {}\nsha256: {}\n",
                entry.entry, entry.bytes, entry.sha256
            ));
        }
        out
    }

    pub fn write(&self, dir: &Path) -> Result<(), Fail> {
        let path = dir.join(FILE);
        fs::write(&path, self.render()).map_err(|error| {
            Fail::environment(format!("cannot write {}: {error}", path.display()))
        })?;
        fsutil::set_mode(&path, 0o600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_names_its_recipient_and_entries() {
        let manifest = Manifest {
            recipient: "ABCDEF".to_string(),
            entries: vec![Entry {
                entry: "github/foo".to_string(),
                bytes: 123,
                sha256: "a".repeat(64),
            }],
        };
        let rendered = manifest.render();
        assert!(rendered.contains("version: 1"));
        assert!(rendered.contains("recipient: ABCDEF"));
        assert!(rendered.contains("entries: 1"));
        assert!(rendered.contains("entry: github/foo"));
        assert!(rendered.contains("bytes: 123"));
        assert!(rendered.contains(&"a".repeat(64)));
    }
}
