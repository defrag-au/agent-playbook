//! Typed Koios responses, and the one-line projections `at-chain` prints.
//!
//! Only fields the summaries use are modelled; serde ignores the rest. Identifiers are
//! required, everything else is optional or defaulted, because a Koios response that adds or
//! drops a field should narrow an answer rather than fail the whole batch.
//!
//! No `serde_json::Value` is a model here. Dynamic payloads (metadata, redeemers, Plutus
//! values) are answered by `--json` or by the raw verbs, never by a tree this file invents.

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

pub trait Record {
    fn lines(&self) -> Vec<String>;
}

pub trait Row {
    fn line(&self) -> String;
}

fn field(label: &str, value: impl Into<String>) -> String {
    format!("{label:<13} {}", value.into())
}

fn ada_from(num: &Num) -> String {
    match num.u64() {
        Some(value) => format!("{}.{:06}", value / 1_000_000, value % 1_000_000),
        None if num.is_empty() => "-".to_string(),
        None => num.as_str().to_string(),
    }
}

fn ada_from_u64(value: u64) -> String {
    format!("{}.{:06}", value / 1_000_000, value % 1_000_000)
}

fn ada_amount(num: &Num) -> String {
    if num.is_empty() {
        "-".to_string()
    } else {
        format!("{} ADA", ada_from(num))
    }
}

fn timestamp(num: &Num) -> String {
    match num.u64() {
        Some(0) | None => "-".to_string(),
        Some(seconds) => iso8601_utc(UNIX_EPOCH + Duration::from_secs(seconds)),
    }
}

fn optional_timestamp(num: Option<&Num>) -> String {
    num.map(timestamp).unwrap_or_else(|| "-".to_string())
}

fn optional_value(num: Option<&Num>) -> String {
    match num {
        Some(num) if !num.is_empty() => num.as_str().to_string(),
        _ => "-".to_string(),
    }
}

/// An asset name is hex on the wire. Show the ASCII spelling when the response carries it,
/// otherwise decode the hex when it is printable, otherwise keep the hex.
fn asset_label(asset_name: &str, ascii: Option<&str>) -> String {
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

fn hex_to_utf8(hex: &str) -> Option<String> {
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

// ---- records: one target, several labelled lines -------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Tip {
    pub hash: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub abs_slot: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_time: Num,
}

impl Record for Tip {
    fn lines(&self) -> Vec<String> {
        vec![
            field("hash", &self.hash),
            field("epoch", self.epoch_no.as_str()),
            field("block", self.block_height.as_str()),
            field(
                "slot",
                format!(
                    "{} (epoch slot {})",
                    self.abs_slot.as_str(),
                    self.epoch_slot.as_str()
                ),
            ),
            field("time", timestamp(&self.block_time)),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    #[serde(default)]
    pub balance: Num,
    #[serde(default)]
    pub stake_address: Option<String>,
    #[serde(default)]
    pub script_hash: Option<String>,
}

impl Record for AddressInfo {
    fn lines(&self) -> Vec<String> {
        vec![
            field("address", &self.address),
            field("balance", ada_amount(&self.balance)),
            field(
                "stake",
                self.stake_address.as_deref().unwrap_or("-").to_string(),
            ),
            field(
                "script",
                self.script_hash.as_deref().unwrap_or("-").to_string(),
            ),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountInfo {
    pub stake_address: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub delegated_pool: Option<String>,
    #[serde(default)]
    pub total_balance: Option<Num>,
    #[serde(default)]
    pub utxo: Option<Num>,
    #[serde(default)]
    pub rewards_available: Option<Num>,
    #[serde(default)]
    pub rewards: Option<Num>,
    #[serde(default)]
    pub withdrawals: Option<Num>,
    #[serde(default)]
    pub reserves: Option<Num>,
    #[serde(default)]
    pub treasury: Option<Num>,
    #[serde(default)]
    pub epoch_no: Option<Num>,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub block_time: Option<Num>,
}

impl Record for AccountInfo {
    fn lines(&self) -> Vec<String> {
        // `rewards_available` is null until the stake address is registered, so the suffix
        // only belongs on a value that exists.
        let rewards = match &self.rewards_available {
            Some(num) if !num.is_empty() => format!("{} ADA available", ada_from(num)),
            _ => "-".to_string(),
        };
        let mut out = vec![
            field("stake", &self.stake_address),
            field("status", self.status.as_deref().unwrap_or("-")),
            field(
                "pool",
                self.delegated_pool.as_deref().unwrap_or("-").to_string(),
            ),
            field(
                "balance",
                self.total_balance
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "utxo",
                self.utxo
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field("rewards", rewards),
        ];
        if let Some(rewards) = &self.rewards {
            out.push(field("earned", ada_amount(rewards)));
        }
        if let Some(withdrawals) = &self.withdrawals {
            out.push(field("withdrawn", ada_amount(withdrawals)));
        }
        if let Some(reserves) = &self.reserves {
            out.push(field("reserves", ada_amount(reserves)));
        }
        if let Some(treasury) = &self.treasury {
            out.push(field("treasury", ada_amount(treasury)));
        }
        out.push(field("epoch", optional_value(self.epoch_no.as_ref())));
        out.push(field("block", optional_value(self.block_height.as_ref())));
        out.push(field("time", optional_timestamp(self.block_time.as_ref())));
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct PaymentAddr {
    #[serde(default)]
    pub bech32: String,
}

#[derive(Debug, Deserialize)]
pub struct TxIo {
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub tx_index: Num,
    #[serde(default)]
    pub value: Num,
    #[serde(default)]
    pub payment_addr: Option<PaymentAddr>,
    #[serde(default)]
    pub stake_addr: Option<String>,
    #[serde(default)]
    pub asset_list: Option<Vec<UtxoAsset>>,
    #[serde(default)]
    pub inline_datum: Option<InlineDatum>,
    #[serde(default)]
    pub datum_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MintedAsset {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub quantity: Num,
}

#[derive(Debug, Deserialize)]
pub struct TxInfo {
    pub tx_hash: String,
    #[serde(default)]
    pub block_hash: Option<String>,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub epoch_no: Option<Num>,
    #[serde(default)]
    pub epoch_slot: Option<Num>,
    #[serde(default)]
    pub absolute_slot: Option<Num>,
    #[serde(default)]
    pub tx_timestamp: Option<Num>,
    #[serde(default)]
    pub tx_block_index: Option<Num>,
    #[serde(default)]
    pub tx_size: Option<Num>,
    #[serde(default)]
    pub total_output: Option<Num>,
    #[serde(default)]
    pub fee: Option<Num>,
    #[serde(default)]
    pub deposit: Option<Num>,
    #[serde(default)]
    pub invalid_before: Option<Num>,
    #[serde(default)]
    pub invalid_after: Option<Num>,
    #[serde(default)]
    pub inputs: Vec<TxIo>,
    #[serde(default)]
    pub outputs: Vec<TxIo>,
    #[serde(default)]
    pub assets_minted: Vec<MintedAsset>,
}

impl TxInfo {
    /// `inputs_requested` and `assets_requested` are the caller's flags, so a summary never
    /// prints `0 inputs` for a response that was asked not to include them.
    pub fn lines_with(&self, inputs_requested: bool, assets_requested: bool) -> Vec<String> {
        let mut out = vec![field("tx_hash", &self.tx_hash)];
        out.push(field(
            "block",
            format!(
                "{} · epoch {} · slot {}",
                optional_value(self.block_height.as_ref()),
                optional_value(self.epoch_no.as_ref()),
                optional_value(self.absolute_slot.as_ref()),
            ),
        ));
        out.push(field(
            "time",
            optional_timestamp(self.tx_timestamp.as_ref()),
        ));
        out.push(field(
            "size",
            self.tx_size
                .as_ref()
                .map(|num| format!("{} bytes", num.as_str()))
                .unwrap_or_else(|| "-".to_string()),
        ));
        out.push(field(
            "fee",
            self.fee
                .as_ref()
                .map(ada_amount)
                .unwrap_or_else(|| "-".to_string()),
        ));
        out.push(field(
            "output",
            self.total_output
                .as_ref()
                .map(ada_amount)
                .unwrap_or_else(|| "-".to_string()),
        ));
        if let Some(deposit) = &self.deposit {
            if deposit.u64() != Some(0) {
                out.push(field("deposit", ada_amount(deposit)));
            }
        }
        if inputs_requested {
            let total: u64 = self.inputs.iter().filter_map(|io| io.value.u64()).sum();
            out.push(field(
                "inputs",
                format!("{} · {} ADA", self.inputs.len(), ada_from_u64(total)),
            ));
            let output_total: u64 = self.outputs.iter().filter_map(|io| io.value.u64()).sum();
            out.push(field(
                "outputs",
                format!(
                    "{} · {} ADA",
                    self.outputs.len(),
                    ada_from_u64(output_total)
                ),
            ));
        } else {
            out.push(field("inputs", "not requested (--no-inputs)"));
        }
        if assets_requested {
            out.push(field(
                "minted",
                format!("{} assets", self.assets_minted.len()),
            ));
        } else {
            out.push(field("minted", "not requested (--no-assets)"));
        }
        if let Some(after) = &self.invalid_after {
            out.push(field("invalid after", after.as_str()));
        }
        if let Some(before) = &self.invalid_before {
            out.push(field("invalid before", before.as_str()));
        }
        out
    }
}

impl Record for TxInfo {
    fn lines(&self) -> Vec<String> {
        self.lines_with(true, true)
    }
}

#[derive(Debug, Deserialize)]
pub struct AssetInfoRow {
    #[serde(default)]
    pub policy_id: String,
    pub asset_name: String,
    #[serde(default)]
    pub asset_name_ascii: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub minting_tx_hash: Option<String>,
    #[serde(default)]
    pub total_supply: Option<Num>,
    #[serde(default)]
    pub mint_cnt: Option<Num>,
    #[serde(default)]
    pub burn_cnt: Option<Num>,
    #[serde(default)]
    pub creation_time: Option<Num>,
    #[serde(default)]
    pub decimals: Option<Num>,
}

impl Record for AssetInfoRow {
    fn lines(&self) -> Vec<String> {
        let policy = if self.policy_id.is_empty() {
            "-"
        } else {
            self.policy_id.as_str()
        };
        let mut out = vec![
            field("policy", policy),
            field("asset", &self.asset_name),
            field(
                "name",
                asset_label(&self.asset_name, self.asset_name_ascii.as_deref()),
            ),
            field(
                "fingerprint",
                self.fingerprint.as_deref().unwrap_or("-").to_string(),
            ),
            field("supply", optional_value(self.total_supply.as_ref())),
            field(
                "minted",
                format!(
                    "{} · burned {}",
                    optional_value(self.mint_cnt.as_ref()),
                    optional_value(self.burn_cnt.as_ref())
                ),
            ),
            field("created", optional_timestamp(self.creation_time.as_ref())),
        ];
        if let Some(decimals) = &self.decimals {
            out.push(field("decimals", decimals.as_str()));
        }
        out.push(field(
            "minting tx",
            self.minting_tx_hash.as_deref().unwrap_or("-").to_string(),
        ));
        out
    }
}

impl Row for AssetInfoRow {
    fn line(&self) -> String {
        let label = if self.asset_name.is_empty() {
            "<empty>".to_string()
        } else {
            asset_label(&self.asset_name, self.asset_name_ascii.as_deref())
        };
        let unit = if self.policy_id.is_empty() {
            label.clone()
        } else {
            format!("{}.{}", self.policy_id, label)
        };
        // `/policy_asset_info` sends a decoded name beside the hex, so the hex is only worth a
        // column when it adds something — and it does: it is what builds an `asset` unit, and
        // it is what tells two tokens that share a CIP-67 label apart.
        let hex = if self.asset_name.is_empty() || label == self.asset_name {
            String::new()
        } else {
            format!("  {}", self.asset_name)
        };
        format!(
            "{unit}{hex}  supply {}  minted {} · burned {}  {}",
            optional_value(self.total_supply.as_ref()),
            optional_value(self.mint_cnt.as_ref()),
            optional_value(self.burn_cnt.as_ref()),
            optional_timestamp(self.creation_time.as_ref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockInfo {
    pub hash: String,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub abs_slot: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub tx_count: Option<Num>,
    #[serde(default, rename = "block_size")]
    pub size: Option<Num>,
}

impl Record for BlockInfo {
    fn lines(&self) -> Vec<String> {
        vec![
            field("hash", &self.hash),
            field("height", self.block_height.as_str()),
            field("epoch", self.epoch_no.as_str()),
            field("slot", self.abs_slot.as_str()),
            field("time", timestamp(&self.block_time)),
            field("txs", optional_value(self.tx_count.as_ref())),
            field("size", optional_value(self.size.as_ref())),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct ScriptInfo {
    pub script_hash: String,
    #[serde(rename = "type", default)]
    pub script_type: Option<String>,
    #[serde(default)]
    pub serialised_size: Option<Num>,
}

impl Record for ScriptInfo {
    fn lines(&self) -> Vec<String> {
        vec![
            field("script", &self.script_hash),
            field("type", self.script_type.as_deref().unwrap_or("-")),
            field(
                "size",
                self.serialised_size
                    .as_ref()
                    .map(|num| format!("{} bytes", num.as_str()))
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ]
    }
}

// ---- rows: one line per result -------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct InlineDatum {
    #[serde(default)]
    pub bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReferenceScript {
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub size: Option<Num>,
    #[serde(rename = "type", default)]
    pub script_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UtxoAsset {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub quantity: Num,
}

#[derive(Debug, Deserialize)]
pub struct UtxoRow {
    pub tx_hash: String,
    #[serde(default)]
    pub tx_index: Num,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub value: Num,
    #[serde(default)]
    pub stake_address: Option<String>,
    #[serde(default)]
    pub payment_cred: Option<String>,
    #[serde(default)]
    pub datum_hash: Option<String>,
    #[serde(default)]
    pub inline_datum: Option<InlineDatum>,
    #[serde(default)]
    pub reference_script: Option<ReferenceScript>,
    #[serde(default)]
    pub asset_list: Option<Vec<UtxoAsset>>,
    #[serde(default)]
    pub is_spent: Option<bool>,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub block_time: Option<Num>,
    #[serde(default)]
    pub epoch_no: Option<Num>,
}

impl UtxoRow {
    fn datum_marker(&self) -> &'static str {
        if self.inline_datum.is_some() {
            "inline"
        } else if self.datum_hash.is_some() {
            "hash"
        } else {
            "-"
        }
    }

    fn asset_count(&self) -> usize {
        self.asset_list.as_ref().map_or(0, Vec::len)
    }
}

impl Row for UtxoRow {
    fn line(&self) -> String {
        let address = self.address.as_deref().unwrap_or("-");
        let spent = if self.is_spent == Some(true) {
            "  SPENT"
        } else {
            ""
        };
        format!(
            "{}:{}  {:>14} ADA  {} assets  {:<6}  {}{}",
            self.tx_hash,
            self.tx_index.as_str(),
            ada_from(&self.value),
            self.asset_count(),
            self.datum_marker(),
            address,
            spent,
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct TxUtxosRow {
    pub tx_hash: String,
    #[serde(default)]
    pub inputs: Vec<TxIo>,
    #[serde(default)]
    pub outputs: Vec<TxIo>,
}

impl Row for TxUtxosRow {
    fn line(&self) -> String {
        let input_total: u64 = self.inputs.iter().filter_map(|io| io.value.u64()).sum();
        let output_total: u64 = self.outputs.iter().filter_map(|io| io.value.u64()).sum();
        format!(
            "{}  inputs {} · {} ADA  outputs {} · {} ADA",
            self.tx_hash,
            self.inputs.len(),
            ada_from_u64(input_total),
            self.outputs.len(),
            ada_from_u64(output_total),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct AddressTxRow {
    pub tx_hash: String,
    #[serde(default)]
    pub epoch_no: Option<Num>,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub block_time: Option<Num>,
}

impl Row for AddressTxRow {
    fn line(&self) -> String {
        format!(
            "{}  height {:>10}  {}",
            self.tx_hash,
            optional_value(self.block_height.as_ref()),
            optional_timestamp(self.block_time.as_ref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct AssetHoldingRow {
    #[serde(default)]
    pub stake_address: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub asset_name_ascii: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub quantity: Num,
    #[serde(default)]
    pub decimals: Option<Num>,
}

impl Row for AssetHoldingRow {
    fn line(&self) -> String {
        let owner = self
            .stake_address
            .as_deref()
            .or(self.address.as_deref())
            .unwrap_or("-");
        let name = asset_label(&self.asset_name, self.asset_name_ascii.as_deref());
        let unit = if self.policy_id.is_empty() {
            name
        } else {
            format!("{}.{}", self.policy_id, name)
        };
        format!(
            "{}  {}  qty {}  {}",
            owner,
            unit,
            self.quantity.as_str(),
            self.fingerprint.as_deref().unwrap_or("-"),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct TxCborRow {
    pub tx_hash: String,
    #[serde(default)]
    pub cbor: String,
}

#[derive(Debug, Deserialize)]
pub struct TxStatusRow {
    pub tx_hash: String,
    #[serde(default)]
    pub num_confirmations: Option<Num>,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub block_time: Option<Num>,
}

impl Row for TxStatusRow {
    fn line(&self) -> String {
        let mut out = format!(
            "{}  confirmations {}",
            self.tx_hash,
            optional_value(self.num_confirmations.as_ref()),
        );
        if let Some(height) = &self.block_height {
            out.push_str(&format!("  block {}", height.as_str()));
        }
        if let Some(time) = &self.block_time {
            out.push_str(&format!("  {}", timestamp(time)));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct DatumRow {
    pub datum_hash: String,
    #[serde(default)]
    pub bytes: Option<String>,
    #[serde(default)]
    pub datum: Option<String>,
}

impl DatumRow {
    fn hex(&self) -> Option<&str> {
        self.bytes.as_deref().or(self.datum.as_deref())
    }
}

impl Row for DatumRow {
    fn line(&self) -> String {
        let datum = self.hex().unwrap_or("");
        let prefix: String = datum.chars().take(72).collect();
        format!(
            "{}  {} hex chars  {}",
            self.datum_hash,
            datum.len(),
            if prefix.is_empty() { "-" } else { &prefix },
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockTxRow {
    pub tx_hash: String,
}

impl Row for BlockTxRow {
    fn line(&self) -> String {
        self.tx_hash.clone()
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
}
