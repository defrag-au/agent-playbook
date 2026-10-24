//! Reading a file as text, and saying why it is not.
//!
//! Shared because every inspector that prints file contents has to make the same three-way
//! distinction — text, binary, not UTF-8 — and a different answer in one tool would be a
//! different contract. No I/O beyond one read, and no write path.

use std::fs;
use std::path::Path;

use crate::contract::Fail;

pub enum Content {
    Text(String),
    /// Byte offset of the first NUL in the sampling window.
    Binary(usize),
    /// Byte offset of the first invalid sequence.
    NotUtf8(usize),
}

/// Read a file the caller has already resolved and contained. `name` is the form a message
/// should use — a path relative to whatever root the caller named, never an invented one.
pub fn read_text(path: &Path, name: &str) -> Result<Content, Fail> {
    let bytes =
        fs::read(path).map_err(|e| Fail::environment(format!("cannot read {name}: {e}")))?;
    if let Some(offset) = bytes.iter().take(8192).position(|byte| *byte == 0) {
        return Ok(Content::Binary(offset));
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Content::Text(text)),
        Err(error) => Ok(Content::NotUtf8(error.utf8_error().valid_up_to())),
    }
}
