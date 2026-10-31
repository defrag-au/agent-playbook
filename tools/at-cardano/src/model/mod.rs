//! Typed Koios responses, and the one-line projections `at-cardano` prints.
//!
//! Only fields the summaries use are modelled; serde ignores the rest. Identifiers are
//! required, everything else is optional or defaulted, because a Koios response that adds or
//! drops a field should narrow an answer rather than fail the whole batch.
//!
//! No `serde_json::Value` is a model here. Dynamic payloads (metadata, redeemers, Plutus
//! values, `pool metadata`, `proposal` metadata) are answered by `--json` or by the raw verbs,
//! never by a tree this file invents.
//!
//! One module per family: [`chain`] (tip, network, epochs, blocks), [`tx`], [`ledger`]
//! (addresses, credentials, accounts), [`assets`], [`pools`], [`governance`] and [`scripts`].
//! The vocabulary below — [`Num`], [`Record`], [`Row`] and the formatting helpers — is shared.

pub mod assets;
pub mod chain;
pub mod governance;
pub mod ledger;
pub mod pools;
pub mod scripts;
pub mod tx;

pub use assets::*;
pub use chain::*;
pub use governance::*;
pub use ledger::*;
pub use pools::*;
pub use scripts::*;
pub use tx::*;

use std::fmt;
use std::time::{Duration, UNIX_EPOCH};

use at_core::contract::iso8601_utc;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

/// A JSON number or a numeric string, kept as text.
///
/// Koios is inconsistent about which one it sends for a given field — lovelace and asset
/// quantities arrive as strings, heights and timestamps as numbers — so the model accepts
/// both and the printer decides how to render it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Num(String);

impl Num {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn u64(&self) -> Option<u64> {
        self.0.parse().ok()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct NumVisitor;

        impl Visitor<'_> for NumVisitor {
            type Value = Num;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a number or a numeric string")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(Num(value.to_string()))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(Num(value.to_string()))
            }

            fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E> {
                Ok(Num(value.to_string()))
            }

            fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E> {
                Ok(Num(value.to_string()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(Num(value.to_string()))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Num(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(Num(value))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(Num::default())
            }
        }

        deserializer.deserialize_any(NumVisitor)
    }
}

/// One target, several labelled lines — a block, an address, a transaction.
pub trait Record {
    fn lines(&self) -> Vec<String>;
}

/// One line per row — a delegator, a holding, a vote.
pub trait Row {
    fn line(&self) -> String;
}

/// A record whose body is a nested list is capped at this many lines; the cut is stated.
pub(crate) const MAX_NESTED_LINES: usize = 100;

pub(crate) fn field(label: &str, value: impl Into<String>) -> String {
    format!("{label:<13} {}", value.into())
}

pub(crate) fn ada_from(num: &Num) -> String {
    match num.u64() {
        Some(value) => format!("{}.{:06}", value / 1_000_000, value % 1_000_000),
        None if num.is_empty() => "-".to_string(),
        None => num.as_str().to_string(),
    }
}

pub(crate) fn ada_from_u64(value: u64) -> String {
    format!("{}.{:06}", value / 1_000_000, value % 1_000_000)
}

pub(crate) fn ada_amount(num: &Num) -> String {
    if num.is_empty() {
        "-".to_string()
    } else {
        format!("{} ADA", ada_from(num))
    }
}

pub(crate) fn timestamp(num: &Num) -> String {
    match num.u64() {
        Some(0) | None => "-".to_string(),
        Some(seconds) => iso8601_utc(UNIX_EPOCH + Duration::from_secs(seconds)),
    }
}

/// A moment Koios sends either as a unix timestamp or as an ISO string — the genesis system
/// start is a number on mainnet and has been a string in the schema. Render either.
pub(crate) fn moment(num: &Num) -> String {
    match num.u64() {
        Some(seconds) => iso8601_utc(UNIX_EPOCH + Duration::from_secs(seconds)),
        None if num.is_empty() => "-".to_string(),
        None => num.as_str().to_string(),
    }
}

pub(crate) fn optional_timestamp(num: Option<&Num>) -> String {
    num.map(timestamp).unwrap_or_else(|| "-".to_string())
}

pub(crate) fn optional_value(num: Option<&Num>) -> String {
    match num {
        Some(num) if !num.is_empty() => num.as_str().to_string(),
        _ => "-".to_string(),
    }
}

/// Koios sends a margin as a fraction (`0.1` is 10%) and a saturation as a percentage
/// (`94.52` is 94.52%). Only the fraction needs converting, and saying which is which here
/// keeps the two from being confused in a renderer.
pub(crate) fn fraction_as_percent(num: &Num) -> String {
    match num.as_str().parse::<f64>() {
        Ok(value) => format!("{:.2}%", value * 100.0),
        Err(_) if num.is_empty() => "-".to_string(),
        Err(_) => num.as_str().to_string(),
    }
}

/// An optional string, with `-` standing in for absent.
pub(crate) fn optional_text(value: Option<&str>) -> String {
    match value {
        Some(text) if !text.is_empty() => text.to_string(),
        _ => "-".to_string(),
    }
}

/// A boolean Koios sends as a flag, rendered only when it is true.
pub(crate) fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// An asset name is hex on the wire. Show the ASCII spelling when the response carries it,
/// otherwise decode the hex when it is printable, otherwise keep the hex.
pub(crate) fn asset_label(asset_name: &str, ascii: Option<&str>) -> String {
    if let Some(ascii) = ascii {
        if !ascii.is_empty() {
            return ascii.to_string();
        }
    }
    if asset_name.is_empty() {
        return "<empty>".to_string();
    }
    hex_to_utf8(asset_name).unwrap_or_else(|| asset_name.to_string())
}

pub(crate) fn hex_to_utf8(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    let text = String::from_utf8(bytes).ok()?;
    if text
        .chars()
        .all(|character| character.is_ascii_graphic() || character == ' ')
    {
        Some(text)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(json: &str) -> Num {
        serde_json::from_str(json).expect("a number, a numeric string or null")
    }

    #[test]
    fn an_absent_amount_is_a_dash_not_a_zero() {
        assert_eq!(ada_amount(&num("null")), "-");
        assert_eq!(ada_amount(&num("\"\"")), "-");
    }

    #[test]
    fn a_present_amount_is_lovelace_scaled_and_labelled() {
        assert_eq!(ada_amount(&num("\"0\"")), "0.000000 ADA");
        assert_eq!(ada_amount(&num("\"1234567\"")), "1.234567 ADA");
        assert_eq!(ada_amount(&num("2000000")), "2.000000 ADA");
        assert_eq!(ada_amount(&num("\"n/a\"")), "n/a ADA");
    }

    #[test]
    fn printable_hex_names_decode_and_the_rest_stay_hex() {
        assert_eq!(asset_label("416c69656e73", None), "Aliens");
        assert_eq!(asset_label("000de14056414c", None), "000de14056414c");
        assert_eq!(asset_label("", Some("Aliens")), "Aliens");
    }
}
