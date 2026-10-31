//! Chain-level answers: the tip, network totals and parameters, epochs, and blocks.

use serde::Deserialize;

use super::{
    ada_amount, ada_from, field, moment, optional_text, optional_value, timestamp, Num, Record, Row,
};

#[derive(Debug, Deserialize)]
pub struct Tip {
    pub hash: String,
    #[serde(default)]
    pub era: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub abs_slot: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    /// Current Koios sends `block_height`; the published schema still lists the deprecated
    /// `block_no` beside it, so both are read and the live one wins.
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_no: Num,
    #[serde(default)]
    pub block_time: Num,
}

impl Record for Tip {
    fn lines(&self) -> Vec<String> {
        let height = if self.block_height.is_empty() {
            optional_value(Some(&self.block_no))
        } else {
            self.block_height.as_str().to_string()
        };
        vec![
            field("hash", &self.hash),
            field(
                "epoch",
                match &self.era {
                    Some(era) if !era.is_empty() => format!("{} ({era})", self.epoch_no),
                    _ => self.epoch_no.as_str().to_string(),
                },
            ),
            field("block", height),
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
pub struct EraRow {
    #[serde(default)]
    pub era: String,
    #[serde(default)]
    pub protocol_major: Num,
    #[serde(default)]
    pub protocol_minor: Num,
    #[serde(default)]
    pub ledger_protocol: Option<String>,
    #[serde(default)]
    pub consensus_mechanism: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub first_block_time: Num,
}

impl Row for EraRow {
    fn line(&self) -> String {
        format!(
            "{:<14} protocol {}.{}  {:<14} epoch {:<6} {}{}",
            self.era,
            self.protocol_major.as_str(),
            self.protocol_minor.as_str(),
            optional_text(self.ledger_protocol.as_deref()),
            self.epoch_no.as_str(),
            timestamp(&self.first_block_time),
            match self.notes.as_deref() {
                Some(notes) if !notes.is_empty() => format!("  {notes}"),
                _ => String::new(),
            },
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct GenesisRow {
    #[serde(default)]
    pub networkmagic: Num,
    #[serde(default)]
    pub networkid: Option<String>,
    #[serde(default)]
    pub epochlength: Num,
    #[serde(default)]
    pub slotlength: Num,
    #[serde(default)]
    pub maxlovelacesupply: Num,
    #[serde(default)]
    pub systemstart: Num,
    #[serde(default)]
    pub activeslotcoeff: Num,
    #[serde(default)]
    pub slotsperkesperiod: Num,
    #[serde(default)]
    pub maxkesrevolutions: Num,
    #[serde(default)]
    pub securityparam: Num,
    #[serde(default)]
    pub updatequorum: Num,
}

impl Record for GenesisRow {
    fn lines(&self) -> Vec<String> {
        vec![
            field("network", optional_text(self.networkid.as_deref())),
            field("magic", self.networkmagic.as_str()),
            field(
                "epoch length",
                format!("{} slots", self.epochlength.as_str()),
            ),
            field("slot length", format!("{}s", self.slotlength.as_str())),
            field(
                "supply",
                format!("{} lovelace", self.maxlovelacesupply.as_str()),
            ),
            field("system start", moment(&self.systemstart)),
            field("security", format!("k={}", self.securityparam.as_str())),
            field("quorum", self.updatequorum.as_str()),
            field("active slot", self.activeslotcoeff.as_str()),
            field(
                "kes period",
                format!("{} slots", self.slotsperkesperiod.as_str()),
            ),
            field("max kes", self.maxkesrevolutions.as_str()),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct TotalsRow {
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub circulation: Num,
    #[serde(default)]
    pub treasury: Num,
    #[serde(default)]
    pub reward: Num,
    #[serde(default)]
    pub supply: Num,
    #[serde(default)]
    pub reserves: Num,
    #[serde(default)]
    pub fees: Num,
    #[serde(default)]
    pub deposits_stake: Num,
    #[serde(default)]
    pub deposits_drep: Num,
    #[serde(default)]
    pub deposits_proposal: Num,
    #[serde(default)]
    pub treasury_withdrawal: Num,
    #[serde(default)]
    pub reserves_withdrawal: Num,
}

impl Row for TotalsRow {
    fn line(&self) -> String {
        // Every amount here is lovelace on the wire and ADA in the line, including the
        // deposits and withdrawals.
        format!(
            "epoch {:<6} supply {:>18} ADA  circulating {:>18}  treasury {:>14}  reserves \
             {:>18}  rewards {:>14}  fees {:>12}  deposits {}/{}/{}  withdrawn {}/{}",
            self.epoch_no.as_str(),
            ada_from(&self.supply),
            ada_from(&self.circulation),
            ada_from(&self.treasury),
            ada_from(&self.reserves),
            ada_from(&self.reward),
            ada_from(&self.fees),
            ada_from(&self.deposits_stake),
            ada_from(&self.deposits_drep),
            ada_from(&self.deposits_proposal),
            ada_from(&self.treasury_withdrawal),
            ada_from(&self.reserves_withdrawal),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct ParamUpdateRow {
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub epoch_no: Num,
}

impl Row for ParamUpdateRow {
    fn line(&self) -> String {
        format!(
            "epoch {:<6} block {:<10} {}  {}",
            self.epoch_no.as_str(),
            self.block_height.as_str(),
            timestamp(&self.block_time),
            self.tx_hash,
        )
    }
}

/// `/cli_protocol_params` answers one object in `cardano-cli`'s camelCase vocabulary rather
/// than the array-of-rows shape the rest of the API uses.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolParams {
    #[serde(default)]
    pub collateral_percentage: Num,
    #[serde(default)]
    pub max_block_body_size: Num,
    #[serde(default)]
    pub max_block_header_size: Num,
    #[serde(default)]
    pub max_collateral_inputs: Num,
    #[serde(default)]
    pub max_tx_size: Num,
    #[serde(default)]
    pub max_value_size: Num,
    #[serde(default)]
    pub min_pool_cost: Num,
    #[serde(default)]
    pub monetary_expansion: Num,
    #[serde(default)]
    pub pool_pledge_influence: Num,
    #[serde(default)]
    pub pool_retire_max_epoch: Num,
    #[serde(default)]
    pub protocol_version: Option<ProtocolVersion>,
    #[serde(default)]
    pub stake_address_deposit: Num,
    #[serde(default)]
    pub stake_pool_deposit: Num,
    #[serde(default)]
    pub stake_pool_target_num: Num,
    #[serde(default)]
    pub treasury_cut: Num,
    #[serde(default)]
    pub tx_fee_fixed: Num,
    #[serde(default)]
    pub tx_fee_per_byte: Num,
    #[serde(default)]
    pub utxo_cost_per_byte: Num,
}

#[derive(Debug, Deserialize)]
pub struct ProtocolVersion {
    #[serde(default)]
    pub major: Num,
    #[serde(default)]
    pub minor: Num,
}

impl Record for ProtocolParams {
    fn lines(&self) -> Vec<String> {
        let version = match &self.protocol_version {
            Some(version) => format!("{}.{}", version.major, version.minor),
            None => "-".to_string(),
        };
        vec![
            field("protocol", version),
            field(
                "tx fee",
                format!(
                    "{} lovelace + {} / byte",
                    self.tx_fee_fixed.as_str(),
                    self.tx_fee_per_byte.as_str()
                ),
            ),
            field("max tx", format!("{} bytes", self.max_tx_size.as_str())),
            field(
                "max block",
                format!(
                    "{} body · {} header",
                    self.max_block_body_size.as_str(),
                    self.max_block_header_size.as_str()
                ),
            ),
            field(
                "max value",
                format!("{} bytes", self.max_value_size.as_str()),
            ),
            field(
                "collateral",
                format!(
                    "{}% · max {} inputs",
                    self.collateral_percentage.as_str(),
                    self.max_collateral_inputs.as_str()
                ),
            ),
            field("min pool cost", ada_amount(&self.min_pool_cost)),
            field("pool deposit", ada_amount(&self.stake_pool_deposit)),
            field("key deposit", ada_amount(&self.stake_address_deposit)),
            field(
                "utxo cost",
                format!("{} / byte", self.utxo_cost_per_byte.as_str()),
            ),
            field("expansion", self.monetary_expansion.as_str()),
            field("treasury cut", self.treasury_cut.as_str()),
            field("pledge infl", self.pool_pledge_influence.as_str()),
            field(
                "pool retire",
                format!("{} epochs", self.pool_retire_max_epoch.as_str()),
            ),
            field("pool target", self.stake_pool_target_num.as_str()),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct EpochInfo {
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub era: Option<String>,
    #[serde(default)]
    pub out_sum: Num,
    #[serde(default)]
    pub fees: Num,
    #[serde(default)]
    pub tx_count: Num,
    #[serde(default)]
    pub blk_count: Num,
    #[serde(default)]
    pub start_time: Num,
    #[serde(default)]
    pub end_time: Num,
    #[serde(default)]
    pub active_stake: Num,
    #[serde(default)]
    pub total_rewards: Num,
    #[serde(default)]
    pub avg_blk_reward: Num,
}

impl Record for EpochInfo {
    fn lines(&self) -> Vec<String> {
        vec![
            field("epoch", self.epoch_no.as_str()),
            field("era", optional_text(self.era.as_deref())),
            field("start", timestamp(&self.start_time)),
            field("end", timestamp(&self.end_time)),
            field("blocks", self.blk_count.as_str()),
            field("txs", self.tx_count.as_str()),
            field("output", ada_amount(&self.out_sum)),
            field("fees", ada_amount(&self.fees)),
            field("active stake", ada_amount(&self.active_stake)),
            field("rewards", ada_amount(&self.total_rewards)),
            field("avg block", ada_amount(&self.avg_blk_reward)),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct EpochParams {
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub era: Option<String>,
    #[serde(default)]
    pub min_fee_a: Num,
    #[serde(default)]
    pub min_fee_b: Num,
    #[serde(default)]
    pub max_block_size: Num,
    #[serde(default)]
    pub max_tx_size: Num,
    #[serde(default)]
    pub max_val_size: Num,
    #[serde(default)]
    pub key_deposit: Num,
    #[serde(default)]
    pub pool_deposit: Num,
    #[serde(default)]
    pub max_epoch: Num,
    #[serde(default)]
    pub optimal_pool_count: Num,
    #[serde(default)]
    pub influence: Num,
    #[serde(default)]
    pub monetary_expand_rate: Num,
    #[serde(default)]
    pub treasury_growth_rate: Num,
    #[serde(default)]
    pub decentralisation: Num,
    #[serde(default)]
    pub protocol_major: Num,
    #[serde(default)]
    pub protocol_minor: Num,
    #[serde(default)]
    pub min_pool_cost: Num,
    #[serde(default)]
    pub coins_per_utxo_size: Num,
    #[serde(default)]
    pub collateral_percent: Num,
    #[serde(default)]
    pub max_collateral_inputs: Num,
    #[serde(default)]
    pub price_mem: Num,
    #[serde(default)]
    pub price_step: Num,
    #[serde(default)]
    pub max_tx_ex_mem: Num,
    #[serde(default)]
    pub max_tx_ex_steps: Num,
    #[serde(default)]
    pub max_block_ex_mem: Num,
    #[serde(default)]
    pub max_block_ex_steps: Num,
    #[serde(default)]
    pub gov_action_deposit: Num,
    #[serde(default)]
    pub gov_action_lifetime: Num,
    #[serde(default)]
    pub drep_deposit: Num,
    #[serde(default)]
    pub drep_activity: Num,
    #[serde(default)]
    pub committee_min_size: Num,
    #[serde(default)]
    pub committee_max_term_length: Num,
}

impl Record for EpochParams {
    fn lines(&self) -> Vec<String> {
        vec![
            field("epoch", self.epoch_no.as_str()),
            field("era", optional_text(self.era.as_deref())),
            field(
                "protocol",
                format!(
                    "{}.{}",
                    self.protocol_major.as_str(),
                    self.protocol_minor.as_str()
                ),
            ),
            field(
                "tx fee",
                format!(
                    "{} lovelace + {} / byte",
                    self.min_fee_b.as_str(),
                    self.min_fee_a.as_str()
                ),
            ),
            field("max tx", format!("{} bytes", self.max_tx_size.as_str())),
            field(
                "max block",
                format!("{} bytes", self.max_block_size.as_str()),
            ),
            field("max value", format!("{} bytes", self.max_val_size.as_str())),
            field(
                "ex units",
                format!(
                    "tx {} mem / {} steps · block {} mem / {} steps",
                    self.max_tx_ex_mem.as_str(),
                    self.max_tx_ex_steps.as_str(),
                    self.max_block_ex_mem.as_str(),
                    self.max_block_ex_steps.as_str()
                ),
            ),
            field(
                "prices",
                format!(
                    "{} mem · {} step",
                    self.price_mem.as_str(),
                    self.price_step.as_str()
                ),
            ),
            field(
                "collateral",
                format!(
                    "{}% · max {} inputs",
                    self.collateral_percent.as_str(),
                    self.max_collateral_inputs.as_str()
                ),
            ),
            field("min pool cost", ada_amount(&self.min_pool_cost)),
            field("pool deposit", ada_amount(&self.pool_deposit)),
            field("key deposit", ada_amount(&self.key_deposit)),
            field(
                "utxo cost",
                format!("{} / byte", self.coins_per_utxo_size.as_str()),
            ),
            field(
                "pool count",
                format!(
                    "k={} · influence {}",
                    self.optimal_pool_count.as_str(),
                    self.influence.as_str()
                ),
            ),
            field("retire", format!("{} epochs", self.max_epoch.as_str())),
            field(
                "expansion",
                format!(
                    "{} · treasury {}",
                    self.monetary_expand_rate.as_str(),
                    self.treasury_growth_rate.as_str()
                ),
            ),
            field("decentral", self.decentralisation.as_str()),
            field("gov deposit", ada_amount(&self.gov_action_deposit)),
            field(
                "gov lifetime",
                format!("{} epochs", self.gov_action_lifetime.as_str()),
            ),
            field("drep deposit", ada_amount(&self.drep_deposit)),
            field(
                "drep activity",
                format!("{} epochs", self.drep_activity.as_str()),
            ),
            field(
                "committee",
                format!(
                    "min {} · term {} epochs",
                    self.committee_min_size.as_str(),
                    self.committee_max_term_length.as_str()
                ),
            ),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct EpochBlockProtocolRow {
    #[serde(default)]
    pub proto_major: Num,
    #[serde(default)]
    pub proto_minor: Num,
    #[serde(default)]
    pub era: Option<String>,
    #[serde(default)]
    pub blocks: Num,
}

impl Row for EpochBlockProtocolRow {
    fn line(&self) -> String {
        format!(
            "{:<14} protocol {}.{}  {} blocks",
            optional_text(self.era.as_deref()),
            self.proto_major.as_str(),
            self.proto_minor.as_str(),
            self.blocks.as_str(),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockInfo {
    pub hash: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub era: Option<String>,
    #[serde(default)]
    pub abs_slot: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_size: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub tx_count: Num,
    #[serde(default)]
    pub pool: Option<String>,
    #[serde(default)]
    pub vrf_key: Option<String>,
    #[serde(default)]
    pub proto_major: Num,
    #[serde(default)]
    pub proto_minor: Num,
    #[serde(default)]
    pub total_output: Num,
    #[serde(default)]
    pub total_fees: Num,
    #[serde(default)]
    pub num_confirmations: Num,
}

impl Record for BlockInfo {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("hash", &self.hash),
            field("height", self.block_height.as_str()),
            field(
                "epoch",
                match &self.era {
                    Some(era) if !era.is_empty() => format!("{} ({era})", self.epoch_no),
                    _ => self.epoch_no.as_str().to_string(),
                },
            ),
            field(
                "slot",
                format!(
                    "{} (epoch slot {})",
                    self.abs_slot.as_str(),
                    self.epoch_slot.as_str()
                ),
            ),
            field("time", timestamp(&self.block_time)),
            field("txs", self.tx_count.as_str()),
            field("size", format!("{} bytes", self.block_size.as_str())),
            field("pool", optional_text(self.pool.as_deref())),
            field(
                "protocol",
                format!(
                    "{}.{}",
                    self.proto_major.as_str(),
                    self.proto_minor.as_str()
                ),
            ),
        ];
        if !self.total_output.is_empty() {
            out.push(field("total output", ada_amount(&self.total_output)));
        }
        if !self.total_fees.is_empty() {
            out.push(field("total fees", ada_amount(&self.total_fees)));
        }
        if !self.num_confirmations.is_empty() {
            out.push(field("confirmed", self.num_confirmations.as_str()));
        }
        if let Some(vrf) = self.vrf_key.as_deref() {
            if !vrf.is_empty() {
                out.push(field("vrf", vrf));
            }
        }
        out
    }
}

impl Row for BlockInfo {
    fn line(&self) -> String {
        format!(
            "{}  height {:<10} epoch {:<6} {:>6} txs  {:>8} bytes  {}",
            self.hash,
            self.block_height.as_str(),
            self.epoch_no.as_str(),
            self.tx_count.as_str(),
            self.block_size.as_str(),
            timestamp(&self.block_time),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockTxRow {
    pub tx_hash: String,
    #[serde(default)]
    pub block_hash: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub tx_timestamp: Num,
}

impl Row for BlockTxRow {
    fn line(&self) -> String {
        let time = if self.block_time.is_empty() {
            &self.tx_timestamp
        } else {
            &self.block_time
        };
        format!(
            "{}  block {:<10} {}",
            self.tx_hash,
            self.block_height.as_str(),
            timestamp(time),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockTxCborRow {
    pub tx_hash: String,
    #[serde(default)]
    pub block_hash: String,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub absolute_slot: Num,
    #[serde(default)]
    pub tx_timestamp: Num,
    #[serde(default)]
    pub cbor: String,
    #[serde(default)]
    pub valid_contract: Option<bool>,
}

impl Row for BlockTxCborRow {
    fn line(&self) -> String {
        format!(
            "{}  {:>7} hex chars  block {:<10} {}  valid {}",
            self.tx_hash,
            self.cbor.len(),
            self.block_height.as_str(),
            timestamp(&self.tx_timestamp),
            self.valid_contract.map(super::yes_no).unwrap_or("-"),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct MetalabelRow {
    #[serde(default)]
    pub key: Num,
}

impl Row for MetalabelRow {
    fn line(&self) -> String {
        self.key.as_str().to_string()
    }
}
