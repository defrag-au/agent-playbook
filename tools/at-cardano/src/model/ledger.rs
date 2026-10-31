//! Addresses, payment credentials and stake accounts.

use serde::Deserialize;

use super::{ada_amount, ada_from, asset_label, field, optional_text, timestamp, Num, Record, Row};

#[derive(Debug, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    #[serde(default)]
    pub balance: Num,
    #[serde(default)]
    pub stake_address: Option<String>,
    /// Koios reports whether the address is a script address as a flag, not a hash.
    #[serde(default)]
    pub script_address: Option<bool>,
    #[serde(default)]
    pub utxo_set: Vec<AddressUtxoRef>,
}

#[derive(Debug, Deserialize)]
pub struct AddressUtxoRef {
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub tx_index: Num,
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
                match self.script_address {
                    Some(true) => "yes".to_string(),
                    Some(false) => "-".to_string(),
                    None => "-".to_string(),
                },
            ),
            field("utxos", self.utxo_set.len().to_string()),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct AddressTxRow {
    pub tx_hash: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_time: Num,
}

impl Row for AddressTxRow {
    fn line(&self) -> String {
        format!(
            "{}  epoch {:<6} block {:<10} {}",
            self.tx_hash,
            self.epoch_no.as_str(),
            self.block_height.as_str(),
            timestamp(&self.block_time),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct AddressListRow {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub address_hex: Option<String>,
    #[serde(default)]
    pub has_script: Option<bool>,
    #[serde(default)]
    pub payment_cred: Option<String>,
    #[serde(default)]
    pub stake_address: Option<String>,
}

impl Row for AddressListRow {
    fn line(&self) -> String {
        format!(
            "{}  script {:<4}  stake {:<64}  cred {}",
            self.address,
            self.has_script.map(super::yes_no).unwrap_or("-"),
            optional_text(self.stake_address.as_deref()),
            optional_text(self.payment_cred.as_deref()),
        )
    }
}

/// One native-asset holding, keyed by an address (`/address_assets`) or a stake key
/// (`/account_assets`).
#[derive(Debug, Deserialize)]
pub struct AssetHoldingRow {
    #[serde(default)]
    pub stake_address: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub payment_address: Option<String>,
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
            .or(self.payment_address.as_deref())
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
pub struct AccountInfo {
    pub stake_address: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub delegated_drep: Option<String>,
    #[serde(default)]
    pub delegated_pool: Option<String>,
    #[serde(default)]
    pub total_balance: Num,
    #[serde(default)]
    pub utxo: Num,
    #[serde(default)]
    pub rewards_available: Num,
    #[serde(default)]
    pub rewards: Num,
    #[serde(default)]
    pub withdrawals: Num,
    #[serde(default)]
    pub deposit: Num,
    #[serde(default)]
    pub reserves: Num,
    #[serde(default)]
    pub treasury: Num,
    #[serde(default)]
    pub epoch_no: Option<Num>,
    #[serde(default)]
    pub block_height: Option<Num>,
    #[serde(default)]
    pub block_time: Option<Num>,
}

impl Record for AccountInfo {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("stake", &self.stake_address),
            field("status", optional_text(self.status.as_deref())),
            field("pool", optional_text(self.delegated_pool.as_deref())),
            field("drep", optional_text(self.delegated_drep.as_deref())),
            field("balance", ada_amount(&self.total_balance)),
            field("utxo", ada_amount(&self.utxo)),
            field("rewards", ada_amount(&self.rewards_available)),
            field("earned", ada_amount(&self.rewards)),
            field("withdrawn", ada_amount(&self.withdrawals)),
            field("deposit", ada_amount(&self.deposit)),
            field("reserves", ada_amount(&self.reserves)),
            field("treasury", ada_amount(&self.treasury)),
        ];
        if let Some(epoch) = self.epoch_no.as_ref().filter(|num| !num.is_empty()) {
            out.push(field("epoch", epoch.as_str()));
        }
        if let Some(height) = self.block_height.as_ref().filter(|num| !num.is_empty()) {
            out.push(field("block", height.as_str()));
        }
        if let Some(time) = self.block_time.as_ref().filter(|num| !num.is_empty()) {
            out.push(field("time", timestamp(time)));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountListRow {
    #[serde(default)]
    pub stake_address: String,
    #[serde(default)]
    pub stake_address_hex: Option<String>,
    #[serde(default)]
    pub script_hash: Option<String>,
}

impl Row for AccountListRow {
    fn line(&self) -> String {
        format!(
            "{}  script {}",
            self.stake_address,
            optional_text(self.script_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct RewardRow {
    #[serde(default)]
    pub earned_epoch: Num,
    #[serde(default)]
    pub spendable_epoch: Num,
    #[serde(default)]
    pub amount: Num,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub pool_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccountRewardRow {
    pub stake_address: String,
    #[serde(default)]
    pub rewards: Vec<RewardRow>,
}

impl Record for AccountRewardRow {
    fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for reward in &self.rewards {
            out.push(format!(
                "{}  earned {:<6} spendable {:<6} {:>14} ADA  {:<10} {}",
                self.stake_address,
                reward.earned_epoch.as_str(),
                reward.spendable_epoch.as_str(),
                super::ada_from(&reward.amount),
                optional_text(reward.kind.as_deref()),
                optional_text(reward.pool_id.as_deref()),
            ));
        }
        if out.is_empty() {
            out.push(format!("{}  no rewards", self.stake_address));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountRewardHistoryRow {
    pub stake_address: String,
    #[serde(default)]
    pub earned_epoch: Num,
    #[serde(default)]
    pub spendable_epoch: Num,
    #[serde(default)]
    pub amount: Num,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub pool_id_bech32: Option<String>,
}

impl Row for AccountRewardHistoryRow {
    fn line(&self) -> String {
        format!(
            "{}  earned {:<6} spendable {:<6} {:>14} ADA  {:<10} {}",
            self.stake_address,
            self.earned_epoch.as_str(),
            self.spendable_epoch.as_str(),
            super::ada_from(&self.amount),
            optional_text(self.kind.as_deref()),
            optional_text(self.pool_id_bech32.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountUpdate {
    #[serde(default)]
    pub action_type: Option<String>,
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub absolute_slot: Num,
    #[serde(default)]
    pub block_time: Num,
}

impl AccountUpdate {
    fn line(&self, stake: &str) -> String {
        update_line(
            stake,
            self.action_type.as_deref(),
            &self.tx_hash,
            &self.epoch_no,
            &self.absolute_slot,
            &self.block_time,
        )
    }
}

fn update_line(
    stake: &str,
    action_type: Option<&str>,
    tx_hash: &str,
    epoch_no: &Num,
    absolute_slot: &Num,
    block_time: &Num,
) -> String {
    format!(
        "{}  {:<18} epoch {:<6} slot {:<10} {}  {}",
        stake,
        optional_text(action_type),
        epoch_no.as_str(),
        absolute_slot.as_str(),
        timestamp(block_time),
        tx_hash,
    )
}

#[derive(Debug, Deserialize)]
pub struct AccountUpdateRow {
    pub stake_address: String,
    #[serde(default)]
    pub updates: Vec<AccountUpdate>,
}

impl Record for AccountUpdateRow {
    fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .updates
            .iter()
            .map(|update| update.line(&self.stake_address))
            .collect();
        if out.is_empty() {
            out.push(format!("{}  no updates", self.stake_address));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountUpdateHistoryRow {
    pub stake_address: String,
    #[serde(default)]
    pub action_type: Option<String>,
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub absolute_slot: Num,
    #[serde(default)]
    pub block_time: Num,
}

impl Row for AccountUpdateHistoryRow {
    fn line(&self) -> String {
        update_line(
            &self.stake_address,
            self.action_type.as_deref(),
            &self.tx_hash,
            &self.epoch_no,
            &self.absolute_slot,
            &self.block_time,
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountAddressRow {
    pub stake_address: String,
    #[serde(default)]
    pub addresses: Vec<String>,
}

impl Record for AccountAddressRow {
    fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .addresses
            .iter()
            .map(|address| format!("{}  {address}", self.stake_address))
            .collect();
        if out.is_empty() {
            out.push(format!("{}  no addresses", self.stake_address));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountStakeRow {
    pub stake_address: String,
    #[serde(default)]
    pub pool_id_bech32: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub active_stake: Num,
}

impl Row for AccountStakeRow {
    fn line(&self) -> String {
        format!(
            "{}  epoch {:<6} {:>14} ADA  {}",
            self.stake_address,
            self.epoch_no.as_str(),
            ada_from(&self.active_stake),
            optional_text(self.pool_id_bech32.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct StakeHistoryEntry {
    #[serde(default)]
    pub pool_id: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub active_stake: Num,
}

#[derive(Debug, Deserialize)]
pub struct AccountHistoryRow {
    pub stake_address: String,
    #[serde(default)]
    pub history: Vec<StakeHistoryEntry>,
}

impl Record for AccountHistoryRow {
    fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .history
            .iter()
            .map(|entry| {
                format!(
                    "{}  epoch {:<6} {:>14} ADA  {}",
                    self.stake_address,
                    entry.epoch_no.as_str(),
                    ada_from(&entry.active_stake),
                    optional_text(entry.pool_id.as_deref()),
                )
            })
            .collect();
        if out.is_empty() {
            out.push(format!("{}  no stake history", self.stake_address));
        }
        out
    }
}
