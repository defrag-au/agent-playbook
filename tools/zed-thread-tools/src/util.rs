//! Small shared helpers for stable names and BLOB serialization.

/// A lowercase, ASCII, dash-separated name safe for a directory component.
pub(crate) fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "thread".to_owned()
    } else {
        slug.chars().take(48).collect()
    }
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

pub(crate) fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }

    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        decoded.push((high << 4) | low);
    }
    Some(decoded)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{hex_decode, hex_encode, slugify};

    #[test]
    fn slug_is_stable_and_ascii() {
        assert_eq!(
            slugify("Zed agent thread analyser recovery"),
            "zed-agent-thread-analyser-recovery"
        );
        assert_eq!(slugify("!!!"), "thread");
    }

    #[test]
    fn hex_round_trips() {
        let bytes = [0_u8, 1, 15, 16, 255];
        assert_eq!(hex_encode(&bytes), "00010f10ff");
        assert_eq!(hex_decode("00010f10ff"), Some(bytes.to_vec()));
        assert_eq!(hex_decode("0"), None);
        assert_eq!(hex_decode("zz"), None);
    }
}
