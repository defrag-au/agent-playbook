//! Transactions: the detail record, its UTxOs, its bytes and its status.

use serde::Deserialize;

use super::{
    ada_amount, ada_from, ada_from_u64, asset_label, field, optional_text, timestamp, Num, Record,
    Row,
};

#[derive(Debug, Deserialize)]
pub struct PaymentAddr {
    #[serde(default)]
    pub bech32: String,
}

#[derive(Debug, Deserialize)]
pub struct InlineDatum {
    /// Koios sends `{"bytes": null, "value": null}` for an output that carries an inline
    /// datum field with no datum in it, so the bytes are optional rather than required.
    #[serde(default)]
    pub bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReferenceScript {
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub size: Option<Num>,
    #[serde(default, rename = "type")]
    pub script_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TxAsset {
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
    pub asset_list: Vec<TxAsset>,
    #[serde(default)]
    pub inline_datum: Option<InlineDatum>,
    #[serde(default)]
    pub datum_hash: Option<String>,
    #[serde(default)]
    pub reference_script: Option<ReferenceScript>,
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
    pub block_hash: String,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub absolute_slot: Num,
    #[serde(default)]
    pub tx_timestamp: Num,
    #[serde(default)]
    pub tx_block_index: Num,
    #[serde(default)]
    pub tx_size: Num,
    #[serde(default)]
    pub total_output: Num,
    #[serde(default)]
    pub fee: Num,
    #[serde(default)]
    pub treasury_donation: Num,
    #[serde(default)]
    pub deposit: Num,
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
    /// `inputs_requested` is the caller's `--no-inputs`; the answer says so rather than
    /// printing an input count of zero that a reader would take as fact.
    pub fn lines_with(&self, inputs_requested: bool, assets_requested: bool) -> Vec<String> {
        let mut out = vec![
            field("tx_hash", &self.tx_hash),
            field(
                "block",
                format!(
                    "{} · epoch {} · slot {}",
                    self.block_height.as_str(),
                    self.epoch_no.as_str(),
                    self.absolute_slot.as_str()
                ),
            ),
            field("time", timestamp(&self.tx_timestamp)),
            field("size", format!("{} bytes", self.tx_size.as_str())),
            field("fee", ada_amount(&self.fee)),
            field("output", ada_amount(&self.total_output)),
        ];
        if self.deposit.u64() != Some(0) && !self.deposit.is_empty() {
            out.push(field("deposit", ada_amount(&self.deposit)));
        }
        if inputs_requested {
            let total: u64 = self
                .inputs
                .iter()
                .filter_map(|input| input.value.u64())
                .sum();
            out.push(field(
                "inputs",
                format!("{} · {} ADA", self.inputs.len(), ada_from_u64(total)),
            ));
        } else {
            out.push(field("inputs", "not requested (--no-inputs)"));
        }
        let output_total: u64 = self
            .outputs
            .iter()
            .filter_map(|output| output.value.u64())
            .sum();
        out.push(field(
            "outputs",
            format!(
                "{} · {} ADA",
                self.outputs.len(),
                ada_from_u64(output_total)
            ),
        ));
        if assets_requested {
            out.push(field(
                "minted",
                format!("{} assets", self.assets_minted.len()),
            ));
            for asset in &self.assets_minted {
                out.push(field(
                    "  asset",
                    format!(
                        "{}  {}.{}",
                        asset.quantity.as_str(),
                        asset.policy_id,
                        asset_label(&asset.asset_name, None)
                    ),
                ));
            }
        } else {
            out.push(field("minted", "not requested (--no-assets)"));
        }
        if let Some(before) = self.invalid_before.as_ref().filter(|num| !num.is_empty()) {
            out.push(field("invalid before", before.as_str()));
        }
        if let Some(after) = self.invalid_after.as_ref().filter(|num| !num.is_empty()) {
            out.push(field("invalid after", after.as_str()));
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
pub struct TxUtxosRow {
    pub tx_hash: String,
    #[serde(default)]
    pub inputs: Vec<TxIo>,
    #[serde(default)]
    pub outputs: Vec<TxIo>,
}

impl Row for TxUtxosRow {
    fn line(&self) -> String {
        let input_total: u64 = self
            .inputs
            .iter()
            .filter_map(|input| input.value.u64())
            .sum();
        let output_total: u64 = self
            .outputs
            .iter()
            .filter_map(|output| output.value.u64())
            .sum();
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
pub struct TxCborRow {
    pub tx_hash: String,
    #[serde(default)]
    pub cbor: String,
}

#[derive(Debug, Deserialize)]
pub struct TxStatusRow {
    pub tx_hash: String,
    #[serde(default)]
    pub num_confirmations: Num,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub block_time: Option<Num>,
}

impl Row for TxStatusRow {
    fn line(&self) -> String {
        let mut line = format!(
            "{}  confirmations {}",
            self.tx_hash,
            if self.num_confirmations.is_empty() {
                "-"
            } else {
                self.num_confirmations.as_str()
            }
        );
        if let Some(height) = self.block_height.as_ref().filter(|num| !num.is_empty()) {
            line.push_str(&format!("  block {}", height.as_str()));
        }
        if let Some(time) = self.block_time.as_ref() {
            line.push_str(&format!("  {}", timestamp(time)));
        }
        line
    }
}

/// One datum, keyed by its hash. `bytes` is the CBOR; the decoded `value` is answered by
/// `--json`, because a Plutus value is an arbitrary tree and this file has no shape for it.
#[derive(Debug, Deserialize)]
pub struct DatumRow {
    pub datum_hash: String,
    #[serde(default)]
    pub creation_tx_hash: Option<String>,
    #[serde(default)]
    pub bytes: Option<String>,
}

impl DatumRow {
    fn hex(&self) -> Option<&str> {
        self.bytes.as_deref()
    }
}

impl Row for DatumRow {
    fn line(&self) -> String {
        let datum = self.hex().unwrap_or("");
        let prefix: String = datum.chars().take(72).collect();
        format!(
            "{}  {} hex chars  {}  created {}",
            self.datum_hash,
            datum.len(),
            if prefix.is_empty() { "-" } else { &prefix },
            optional_text(self.creation_tx_hash.as_deref()),
        )
    }
}

/// One UTxO row, shared by `/address_utxos`, `/account_utxos`, `/credential_utxos`,
/// `/asset_utxos`, `/utxo_info`, `/address_outputs` and `/script_utxos` — the shapes agree on
/// the fields printed here.
#[derive(Debug, Deserialize)]
pub struct UtxoRow {
    pub tx_hash: String,
    #[serde(default)]
    pub tx_index: Num,
    #[serde(default)]
    pub address: String,
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
    pub asset_list: Vec<TxAsset>,
    #[serde(default)]
    pub is_spent: Option<bool>,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub script_hash: Option<String>,
}

impl UtxoRow {
    fn datum_marker(&self) -> &'static str {
        if self.inline_datum.is_some() {
            "inline"
        } else if self
            .datum_hash
            .as_deref()
            .is_some_and(|hash| !hash.is_empty())
        {
            "hash"
        } else {
            "-"
        }
    }

    fn asset_count(&self) -> usize {
        self.asset_list.len()
    }
}

impl Row for UtxoRow {
    fn line(&self) -> String {
        let address = if self.address.is_empty() {
            self.script_hash.as_deref().unwrap_or("-").to_string()
        } else {
            self.address.clone()
        };
        let spent = match self.is_spent {
            Some(true) => "  SPENT",
            _ => "",
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
