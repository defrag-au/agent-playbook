//! The closed key grammar.
//!
//! A key is a name from this list or one literal character. There is no sequence language and no
//! escape pass-through: a scenario cannot smuggle arbitrary bytes past the key grammar, which is
//! what keeps `key` a description of an intent rather than a way to write to stdin.

use anyhow::{bail, Result};

pub fn encode(name: &str) -> Result<Vec<u8>> {
    let lower = name.to_ascii_lowercase();
    let sequence: &[u8] = match lower.as_str() {
        "enter" | "return" => b"\r",
        "esc" | "escape" => b"\x1b",
        "tab" => b"\t",
        "backspace" | "bs" => b"\x7f",
        "delete" | "del" => b"\x1b[3~",
        "insert" | "ins" => b"\x1b[2~",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" | "pgup" => b"\x1b[5~",
        "pagedown" | "pgdn" => b"\x1b[6~",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "space" => b" ",
        "f1" => b"\x1bOP",
        "f2" => b"\x1bOQ",
        "f3" => b"\x1bOR",
        "f4" => b"\x1bOS",
        "f5" => b"\x1b[15~",
        "f6" => b"\x1b[17~",
        "f7" => b"\x1b[18~",
        "f8" => b"\x1b[19~",
        "f9" => b"\x1b[20~",
        "f10" => b"\x1b[21~",
        "f11" => b"\x1b[23~",
        "f12" => b"\x1b[24~",
        _ => {
            if let Some(control) = lower.strip_prefix("ctrl-") {
                return control_key(control);
            }
            if let Some(alt) = name.strip_prefix("alt-") {
                let mut bytes = vec![0x1b];
                bytes.extend_from_slice(alt.as_bytes());
                if bytes.len() == 1 {
                    bail!("alt- needs a key after the dash");
                }
                return Ok(bytes);
            }
            if name.chars().count() == 1 {
                return Ok(name.as_bytes().to_vec());
            }
            bail!(
                "unknown key {name:?}; use a name such as enter, esc, tab, up, ctrl-c or a single \
                 character"
            );
        }
    };
    Ok(sequence.to_vec())
}

fn control_key(name: &str) -> Result<Vec<u8>> {
    let character = match name {
        "space" | "@" => 0x00,
        "[" => 0x1b,
        "\\" => 0x1c,
        "]" => 0x1d,
        "^" => 0x1e,
        "_" => 0x1f,
        "?" => 0x7f,
        name if name.chars().count() == 1 => {
            let character = name.chars().next().expect("one character");
            let ascii = character.to_ascii_lowercase();
            if !ascii.is_ascii_alphabetic() {
                bail!("ctrl-{name} is not a letter, `@`, `[`, `\\`, `]`, `^`, `_` or `?`");
            }
            return Ok(vec![(ascii as u8) & 0x1f]);
        }
        _ => bail!("ctrl-{name} is not a letter, `@`, `[`, `\\`, `]`, `^`, `_` or `?`"),
    };
    Ok(vec![character])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys() {
        assert_eq!(encode("Enter").unwrap(), b"\r");
        assert_eq!(encode("esc").unwrap(), b"\x1b");
        assert_eq!(encode("up").unwrap(), b"\x1b[A");
        assert_eq!(encode("space").unwrap(), b" ");
    }

    #[test]
    fn control_keys() {
        assert_eq!(encode("ctrl-c").unwrap(), vec![0x03]);
        assert_eq!(encode("ctrl-a").unwrap(), vec![0x01]);
        assert_eq!(encode("ctrl-space").unwrap(), vec![0x00]);
    }

    #[test]
    fn alt_is_escape_prefixed() {
        assert_eq!(encode("alt-x").unwrap(), b"\x1bx");
    }

    #[test]
    fn a_single_character_is_itself() {
        assert_eq!(encode("q").unwrap(), b"q");
        assert_eq!(encode("A").unwrap(), b"A");
        assert_eq!(encode("é").unwrap(), "é".as_bytes());
    }

    #[test]
    fn unknown_names_are_refused() {
        assert!(encode("hyper-key").is_err());
        assert!(encode("").is_err());
    }
}
