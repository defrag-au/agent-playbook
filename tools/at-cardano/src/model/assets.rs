//! Assets and policies.

use serde::Deserialize;

use super::{
    asset_label, field, optional_text, optional_timestamp, optional_value, timestamp, Num, Record,
    Row,
};

#[derive(Debug, Deserialize)]
pub struct AssetInfoRow {
    /// `/policy_asset_info` and `/policy_asset_mints` do not echo the policy, because it was
    /// the request rather than the answer.
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
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
        // The two listings this serves send a decoded name beside the hex, so the hex is only
        // worth a column when it adds something — and it does: it builds an `asset` unit, and
        // it tells two tokens that share a CIP-67 label apart.
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
pub struct AssetListRow {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub asset_name_ascii: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

impl Row for AssetListRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<20} {}",
            self.fingerprint.as_deref().unwrap_or("-"),
            asset_label(&self.asset_name, self.asset_name_ascii.as_deref()),
            if self.policy_id.is_empty() {
                String::new()
            } else {
                self.policy_id.clone()
            },
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PolicyAssetRow {
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub total_supply: Option<Num>,
    #[serde(default)]
    pub decimals: Option<Num>,
}

impl Row for PolicyAssetRow {
    fn line(&self) -> String {
        format!(
            "{}  supply {:<18} decimals {:<3} {}",
            asset_label(&self.asset_name, None),
            optional_value(self.total_supply.as_ref()),
            optional_value(self.decimals.as_ref()),
            self.fingerprint.as_deref().unwrap_or("-"),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenRegistryRow {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub asset_name_ascii: Option<String>,
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub decimals: Option<Num>,
}

impl Row for TokenRegistryRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<12} {:<20}  {}",
            self.policy_id,
            optional_text(self.ticker.as_deref()),
            asset_label(&self.asset_name, self.asset_name_ascii.as_deref()),
            optional_text(self.url.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct AssetAddressRow {
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub payment_address: Option<String>,
    #[serde(default)]
    pub stake_address: Option<String>,
    #[serde(default)]
    pub quantity: Num,
}

impl Row for AssetAddressRow {
    fn line(&self) -> String {
        let owner = optional_text(
            self.payment_address
                .as_deref()
                .or(self.stake_address.as_deref()),
        );
        // `GET /asset_addresses` answers for an asset named in the request, so the row does not
        // repeat it; `GET /policy_asset_addresses` does, and then the unit earns a column.
        if self.asset_name.is_empty() {
            format!("{:>20}  {owner}", self.quantity.as_str())
        } else {
            format!(
                "{:<20} {:>20}  {owner}",
                asset_label(&self.asset_name, None),
                self.quantity.as_str(),
            )
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct NftAddressRow {
    #[serde(default)]
    pub payment_address: Option<String>,
    #[serde(default)]
    pub stake_address: Option<String>,
}

impl Record for NftAddressRow {
    fn lines(&self) -> Vec<String> {
        vec![
            field("payment", optional_text(self.payment_address.as_deref())),
            field("stake", optional_text(self.stake_address.as_deref())),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct AssetSummaryRow {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub total_transactions: Num,
    #[serde(default)]
    pub staked_wallets: Num,
    #[serde(default)]
    pub unstaked_addresses: Num,
    #[serde(default)]
    pub addresses: Num,
}

impl Record for AssetSummaryRow {
    fn lines(&self) -> Vec<String> {
        vec![
            field("unit", format!("{}.{}", self.policy_id, self.asset_name)),
            field(
                "fingerprint",
                self.fingerprint.as_deref().unwrap_or("-").to_string(),
            ),
            field("txs", self.total_transactions.as_str()),
            field("addresses", self.addresses.as_str()),
            field("staked", self.staked_wallets.as_str()),
            field("unstaked", self.unstaked_addresses.as_str()),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct MintingTx {
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub quantity: Num,
}

#[derive(Debug, Deserialize)]
pub struct AssetHistoryRow {
    #[serde(default)]
    pub policy_id: String,
    #[serde(default)]
    pub asset_name: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub minting_txs: Vec<MintingTx>,
}

impl Record for AssetHistoryRow {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("unit", format!("{}.{}", self.policy_id, self.asset_name)),
            field(
                "fingerprint",
                self.fingerprint.as_deref().unwrap_or("-").to_string(),
            ),
            field("mints", self.minting_txs.len().to_string()),
        ];
        for mint in &self.minting_txs {
            let direction = if mint.quantity.as_str().starts_with('-') {
                "burn"
            } else {
                "mint"
            };
            out.push(field(
                "  tx",
                format!(
                    "{}  {:<4} {:>18}  {}",
                    mint.tx_hash,
                    direction,
                    mint.quantity.as_str(),
                    timestamp(&mint.block_time),
                ),
            ));
        }
        out
    }
}
