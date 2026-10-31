//! Small filesystem helpers: permissions, sizes and digests.

use std::fs;
use std::io::Read;
use std::path::Path;

use at_core::contract::Fail;
use sha2::{Digest, Sha256};

/// Set a mode on Unix; a no-op elsewhere. The tool targets macOS and Linux, but the crate still
/// has to compile on a host without `PermissionsExt`.
pub fn set_mode(path: &Path, mode: u32) -> Result<(), Fail> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, permissions).map_err(|error| {
            Fail::environment(format!("cannot set mode on {}: {error}", path.display()))
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

/// Lowercase SHA-256, read in chunks so a large ciphertext is never held in memory.
pub fn sha256_file(path: &Path) -> Result<String, Fail> {
    let mut file = fs::File::open(path)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            Fail::environment(format!("cannot read {}: {error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(contents: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("travel-secrets-sha-{}-{stamp}", std::process::id()));
        fs::write(&path, contents).expect("write");
        path
    }

    #[test]
    fn bytes_are_reported_in_binary_units() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1024), "1.0 KiB");
        assert_eq!(human(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn a_known_digest_matches() {
        let path = temp_file("abc");
        assert_eq!(
            sha256_file(&path).expect("hash"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_file(path).expect("cleanup");
    }
}
