//! Scripts: the detail record, the published lists, redeemers and reference UTxOs.

use serde::Deserialize;

use super::{field, optional_text, optional_value, Num, Record, Row};

#[derive(Debug, Deserialize)]
pub struct ScriptInfo {
    pub script_hash: String,
    #[serde(default)]
    pub creation_tx_hash: Option<String>,
    #[serde(default, rename = "type")]
    pub script_type: Option<String>,
    #[serde(default)]
    pub size: Num,
    #[serde(default)]
    pub bytes: Option<String>,
}

impl Record for ScriptInfo {
    fn lines(&self) -> Vec<String> {
        let size = if self.size.is_empty() {
            "-".to_string()
        } else {
            format!("{} bytes", self.size.as_str())
        };
        let mut out = vec![
            field("script", &self.script_hash),
            field("type", optional_text(self.script_type.as_deref())),
            field("size", size),
            field(
                "created by",
                optional_text(self.creation_tx_hash.as_deref()),
            ),
        ];
        if let Some(bytes) = self.bytes.as_deref() {
            if !bytes.is_empty() {
                let prefix: String = bytes.chars().take(72).collect();
                out.push(field("bytes", prefix));
            }
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct ScriptListRow {
    #[serde(default)]
    pub script_hash: String,
    #[serde(default)]
    pub creation_tx_hash: Option<String>,
    #[serde(default, rename = "type")]
    pub script_type: Option<String>,
    #[serde(default)]
    pub size: Num,
}

impl Row for ScriptListRow {
    fn line(&self) -> String {
        // The listing does not always carry a size; an empty column would read as if the word
        // after it were the value.
        let size = if self.size.is_empty() {
            "-".to_string()
        } else {
            format!("{} bytes", self.size.as_str())
        };
        format!(
            "{}  {:<14} {:>12}  {}",
            self.script_hash,
            optional_text(self.script_type.as_deref()),
            size,
            optional_text(self.creation_tx_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct RedeemerRow {
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub tx_index: Num,
    #[serde(default)]
    pub unit_mem: Option<Num>,
    #[serde(default)]
    pub unit_steps: Option<Num>,
    #[serde(default)]
    pub fee: Num,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub datum_hash: Option<String>,
}

impl Row for RedeemerRow {
    fn line(&self) -> String {
        format!(
            "{}:{}  {:<8} mem {:<14} steps {:<16} fee {:>10}  {}",
            self.tx_hash,
            self.tx_index.as_str(),
            optional_text(self.purpose.as_deref()),
            optional_value(self.unit_mem.as_ref()),
            optional_value(self.unit_steps.as_ref()),
            self.fee.as_str(),
            optional_text(self.datum_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct RedeemerGroup {
    pub script_hash: String,
    #[serde(default)]
    pub redeemers: Vec<RedeemerRow>,
}

impl Record for RedeemerGroup {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![field("script", &self.script_hash)];
        if self.redeemers.is_empty() {
            out.push(field("redeemers", "none"));
        }
        for redeemer in &self.redeemers {
            out.push(field("  redeemer", redeemer.line()));
        }
        out
    }
}

/// `/script_utxos` answers the reference UTxO, not the full UTxO row.
#[derive(Debug, Deserialize)]
pub struct ScriptUtxoRow {
    #[serde(default)]
    pub script_hash: String,
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub tx_index: Num,
}

impl Row for ScriptUtxoRow {
    fn line(&self) -> String {
        format!(
            "{}:{}  {}",
            self.tx_hash,
            self.tx_index.as_str(),
            self.script_hash,
        )
    }
}
