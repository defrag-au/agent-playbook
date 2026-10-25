//! SHA-2 helpers.
//!
//! OCFL content addressing uses `sha256` or `sha512`; the default is `sha512`, which is what
//! this store writes. The exported-tree digest is a separate convention of ours: `sha256` over
//! the sorted `(logical path, file digest)` pairs, so a consumer can compare two exports without
//! knowing which OCFL version produced them.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256, Sha512};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "sha256" => Ok(Self::Sha256),
            "sha512" => Ok(Self::Sha512),
            other => Err(format!(
                "unsupported digest algorithm `{other}` — expected `sha256` or `sha512`"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }
}

pub fn bytes_hex(bytes: &[u8], algorithm: Algorithm) -> String {
    match algorithm {
        Algorithm::Sha256 => hex(&Sha256::digest(bytes)),
        Algorithm::Sha512 => hex(&Sha512::digest(bytes)),
    }
}

pub fn file_hex(path: &Path, algorithm: Algorithm) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; 64 * 1024];
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        match algorithm {
            Algorithm::Sha256 => sha256.update(&buffer[..read]),
            Algorithm::Sha512 => sha512.update(&buffer[..read]),
        }
    }
    Ok(match algorithm {
        Algorithm::Sha256 => hex(&sha256.finalize()),
        Algorithm::Sha512 => hex(&sha512.finalize()),
    })
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// The digest of a whole exported tree, independent of the store's OCFL digest algorithm.
///
/// Returns `sha256:<hex>` so it reads the same way as every other digest in `export.conf`.
pub fn tree_hex(root: &Path) -> Result<String, String> {
    let files = crate::fsx::walk_files(root)?;
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        let digest = file_hex(&path, Algorithm::Sha256)?;
        hasher.update(relative.as_bytes());
        hasher.update(b"\n");
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    Ok(format!("sha256:{}", hex(&hasher.finalize())))
}
