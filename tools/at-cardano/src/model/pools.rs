//! Stake pools.

use serde::Deserialize;

use super::{
    ada_amount, ada_from, field, fraction_as_percent, optional_text, optional_timestamp,
    optional_value, timestamp, Num, Record, Row,
};

#[derive(Debug, Clone, Deserialize)]
pub struct PoolMeta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Relay {
    #[serde(default)]
    pub dns: Option<String>,
    #[serde(default)]
    pub srv: Option<String>,
    #[serde(default)]
    pub ipv4: Option<String>,
    #[serde(default)]
    pub ipv6: Option<String>,
    #[serde(default)]
    pub port: Option<Num>,
}

impl Relay {
    fn label(&self) -> String {
        let host = self
            .dns
            .as_deref()
            .or(self.ipv4.as_deref())
            .or(self.ipv6.as_deref())
            .unwrap_or("-");
        let srv = match self.srv.as_deref() {
            Some(srv) if !srv.is_empty() => format!(" srv {srv}"),
            _ => String::new(),
        };
        let port = match self.port.as_ref() {
            Some(port) if !port.is_empty() => format!(":{}", port.as_str()),
            _ => String::new(),
        };
        format!("{host}{port}{srv}")
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolInfo {
    pub pool_id_bech32: String,
    #[serde(default)]
    pub pool_id_hex: Option<String>,
    #[serde(default)]
    pub active_epoch_no: Num,
    #[serde(default)]
    pub vrf_key_hash: Option<String>,
    #[serde(default)]
    pub margin: Option<Num>,
    #[serde(default)]
    pub fixed_cost: Option<Num>,
    #[serde(default)]
    pub pledge: Option<Num>,
    #[serde(default)]
    pub deposit: Option<Num>,
    #[serde(default)]
    pub reward_addr: Option<String>,
    #[serde(default)]
    pub owners: Option<Vec<String>>,
    #[serde(default)]
    pub relays: Vec<Relay>,
    #[serde(default)]
    pub meta_url: Option<String>,
    #[serde(default)]
    pub meta_hash: Option<String>,
    #[serde(default)]
    pub meta_json: Option<PoolMeta>,
    #[serde(default)]
    pub pool_status: Option<String>,
    #[serde(default)]
    pub retiring_epoch: Option<Num>,
    #[serde(default)]
    pub active_stake: Option<Num>,
    #[serde(default)]
    pub sigma: Option<Num>,
    #[serde(default)]
    pub block_count: Option<Num>,
    #[serde(default)]
    pub live_pledge: Option<Num>,
    #[serde(default)]
    pub live_stake: Option<Num>,
    #[serde(default)]
    pub live_delegators: Option<Num>,
    #[serde(default)]
    pub live_saturation: Option<Num>,
    #[serde(default)]
    pub voting_power: Option<Num>,
}

impl Record for PoolInfo {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field("pool", &self.pool_id_bech32),
            field("status", optional_text(self.pool_status.as_deref())),
        ];
        if let Some(meta) = &self.meta_json {
            let name = optional_text(meta.name.as_deref());
            let ticker = match meta.ticker.as_deref() {
                Some(ticker) if !ticker.is_empty() => format!(" [{ticker}]"),
                _ => String::new(),
            };
            if name != "-" || !ticker.is_empty() {
                out.push(field("name", format!("{name}{ticker}")));
            }
        }
        out.extend([
            field("active epoch", self.active_epoch_no.as_str()),
            field(
                "margin",
                self.margin
                    .as_ref()
                    .map(fraction_as_percent)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "fixed cost",
                self.fixed_cost
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "pledge",
                self.pledge
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "live pledge",
                self.live_pledge
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "active stake",
                self.active_stake
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "live stake",
                self.live_stake
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field("delegators", optional_value(self.live_delegators.as_ref())),
            field(
                "saturation",
                optional_value(self.live_saturation.as_ref()).to_string() + "%",
            ),
            field("blocks", optional_value(self.block_count.as_ref())),
            field(
                "voting power",
                self.voting_power
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field("reward addr", optional_text(self.reward_addr.as_deref())),
            field("relays", format!("{}", self.relays.len())),
            field(
                "meta",
                format!(
                    "{}  {}",
                    optional_text(self.meta_url.as_deref()),
                    optional_text(self.meta_hash.as_deref())
                ),
            ),
        ]);
        if let Some(owners) = &self.owners {
            if !owners.is_empty() {
                out.push(field("owners", owners.join(" ")));
            }
        }
        if let Some(retiring) = self.retiring_epoch.as_ref().filter(|num| !num.is_empty()) {
            out.push(field("retiring", retiring.as_str()));
        }
        if let Some(vrf) = self.vrf_key_hash.as_deref().filter(|vrf| !vrf.is_empty()) {
            out.push(field("vrf", vrf));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolListRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub pool_status: Option<String>,
    #[serde(default)]
    pub active_stake: Option<Num>,
    #[serde(default)]
    pub margin: Option<Num>,
    #[serde(default)]
    pub fixed_cost: Option<Num>,
    #[serde(default)]
    pub pledge: Option<Num>,
    #[serde(default)]
    pub retiring_epoch: Option<Num>,
}

impl Row for PoolListRow {
    fn line(&self) -> String {
        let retiring = match self.retiring_epoch.as_ref().filter(|num| !num.is_empty()) {
            Some(epoch) => format!("  retiring {}", epoch.as_str()),
            None => String::new(),
        };
        format!(
            "{}  {:<8} {:<10} active {:>14} ADA  margin {:<8} fixed {}  pledge {}{retiring}",
            self.pool_id_bech32,
            optional_text(self.ticker.as_deref()),
            optional_text(self.pool_status.as_deref()),
            self.active_stake
                .as_ref()
                .map(ada_from)
                .unwrap_or_else(|| "-".to_string()),
            self.margin
                .as_ref()
                .map(fraction_as_percent)
                .unwrap_or_else(|| "-".to_string()),
            self.fixed_cost
                .as_ref()
                .map(ada_from)
                .unwrap_or_else(|| "-".to_string()),
            self.pledge
                .as_ref()
                .map(ada_from)
                .unwrap_or_else(|| "-".to_string()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolSnapshotRow {
    #[serde(default)]
    pub snapshot: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub pool_stake: Num,
    #[serde(default)]
    pub active_stake: Num,
}

impl Row for PoolSnapshotRow {
    fn line(&self) -> String {
        format!(
            "{:<6} epoch {:<6} pool {:>16} ADA  network {:>16} ADA",
            optional_text(self.snapshot.as_deref()),
            self.epoch_no.as_str(),
            ada_from(&self.pool_stake),
            ada_from(&self.active_stake),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolDelegatorRow {
    pub stake_address: String,
    #[serde(default)]
    pub amount: Num,
    #[serde(default)]
    pub active_epoch_no: Num,
    #[serde(default)]
    pub latest_delegation_tx_hash: Option<String>,
}

impl Row for PoolDelegatorRow {
    fn line(&self) -> String {
        format!(
            "{}  {:>16} ADA  since epoch {:<6} {}",
            self.stake_address,
            ada_from(&self.amount),
            self.active_epoch_no.as_str(),
            optional_text(self.latest_delegation_tx_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolDelegatorHistoryRow {
    pub stake_address: String,
    #[serde(default)]
    pub amount: Num,
    #[serde(default)]
    pub epoch_no: Num,
}

impl Row for PoolDelegatorHistoryRow {
    fn line(&self) -> String {
        format!(
            "{}  epoch {:<6} {:>16} ADA",
            self.stake_address,
            self.epoch_no.as_str(),
            ada_from(&self.amount),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolInvalidDelegatorRow {
    pub stake_address: String,
    #[serde(default)]
    pub epoch_no: Num,
}

impl Row for PoolInvalidDelegatorRow {
    fn line(&self) -> String {
        format!("{}  epoch {}", self.stake_address, self.epoch_no.as_str())
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolBlockRow {
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub abs_slot: Num,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_hash: String,
    #[serde(default)]
    pub block_time: Num,
}

impl Row for PoolBlockRow {
    fn line(&self) -> String {
        format!(
            "{}  height {:<10} epoch {:<6} slot {:<10} {}",
            self.block_hash,
            self.block_height.as_str(),
            self.epoch_no.as_str(),
            self.abs_slot.as_str(),
            timestamp(&self.block_time),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolHistoryRow {
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub active_stake: Num,
    #[serde(default)]
    pub active_stake_pct: Num,
    #[serde(default)]
    pub saturation_pct: Num,
    #[serde(default)]
    pub block_cnt: Num,
    #[serde(default)]
    pub delegator_cnt: Num,
    #[serde(default)]
    pub margin: Option<Num>,
    #[serde(default)]
    pub fixed_cost: Option<Num>,
    #[serde(default)]
    pub pool_fees: Num,
    #[serde(default)]
    pub deleg_rewards: Num,
    #[serde(default)]
    pub member_rewards: Num,
    #[serde(default)]
    pub epoch_ros: Num,
}

impl Row for PoolHistoryRow {
    fn line(&self) -> String {
        format!(
            "epoch {:<6} stake {:>16} ADA ({:>6}% · sat {:>6}%)  {:>5} blocks  {:>6} delegators  \
             fees {:>12}  rewards {:>12}/{:<12}  ros {:>6}%",
            self.epoch_no.as_str(),
            ada_from(&self.active_stake),
            optional_value(Some(&self.active_stake_pct)),
            optional_value(Some(&self.saturation_pct)),
            self.block_cnt.as_str(),
            self.delegator_cnt.as_str(),
            ada_from(&self.pool_fees),
            ada_from(&self.deleg_rewards),
            ada_from(&self.member_rewards),
            optional_value(Some(&self.epoch_ros)),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolUpdateRow {
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub update_type: Option<String>,
    #[serde(default)]
    pub active_epoch_no: Num,
    #[serde(default)]
    pub margin: Option<Num>,
    #[serde(default)]
    pub fixed_cost: Option<Num>,
    #[serde(default)]
    pub pledge: Option<Num>,
    #[serde(default)]
    pub retiring_epoch: Option<Num>,
}

impl Row for PoolUpdateRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<12} epoch {:<6}  margin {:<8} fixed {}  pledge {}  {}",
            self.pool_id_bech32,
            optional_text(self.update_type.as_deref()),
            self.active_epoch_no.as_str(),
            self.margin
                .as_ref()
                .map(fraction_as_percent)
                .unwrap_or_else(|| "-".to_string()),
            self.fixed_cost
                .as_ref()
                .map(ada_from)
                .unwrap_or_else(|| "-".to_string()),
            self.pledge
                .as_ref()
                .map(ada_from)
                .unwrap_or_else(|| "-".to_string()),
            timestamp(&self.block_time),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolRegistrationRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub tx_hash: String,
    #[serde(default)]
    pub block_hash: Option<String>,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub epoch_slot: Num,
    #[serde(default)]
    pub active_epoch_no: Num,
}

impl Row for PoolRegistrationRow {
    fn line(&self) -> String {
        format!(
            "{}  tx {}  height {:<10} epoch {:<6} active {:<6} {}",
            self.pool_id_bech32,
            self.tx_hash,
            self.block_height.as_str(),
            self.epoch_no.as_str(),
            self.active_epoch_no.as_str(),
            optional_text(self.block_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolOwnerRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub stake_address: String,
    #[serde(default)]
    pub declared_pledge: Num,
    #[serde(default)]
    pub epoch: Num,
    #[serde(default)]
    pub active_stake: Num,
}

impl Row for PoolOwnerRow {
    fn line(&self) -> String {
        format!(
            "{}  owner {}  pledged {:>16} ADA  epoch {:<6} active {:>16} ADA",
            self.pool_id_bech32,
            self.stake_address,
            ada_from(&self.declared_pledge),
            self.epoch.as_str(),
            ada_from(&self.active_stake),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolRelayRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub relays: Vec<Relay>,
}

impl Record for PoolRelayRow {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![field("pool", &self.pool_id_bech32)];
        if self.relays.is_empty() {
            out.push(field("relays", "none registered"));
        }
        for relay in &self.relays {
            out.push(field("  relay", relay.label()));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolMetadataRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub meta_url: Option<String>,
    #[serde(default)]
    pub meta_hash: Option<String>,
}

impl Row for PoolMetadataRow {
    fn line(&self) -> String {
        format!(
            "{}  {}  {}",
            self.pool_id_bech32,
            optional_text(self.meta_url.as_deref()),
            optional_text(self.meta_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolGroupRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub pool_group: Option<String>,
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub adastat_group: Option<String>,
    #[serde(default)]
    pub balanceanalytics_group: Option<String>,
}

impl Row for PoolGroupRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<10} group {:<24} adastat {:<20} balance {}",
            self.pool_id_bech32,
            optional_text(self.ticker.as_deref()),
            optional_text(self.pool_group.as_deref()),
            optional_text(self.adastat_group.as_deref()),
            optional_text(self.balanceanalytics_group.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolPowerRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub amount: Num,
}

impl Row for PoolPowerRow {
    fn line(&self) -> String {
        format!(
            "{}  epoch {:<6} {:>16} ADA",
            self.pool_id_bech32,
            self.epoch_no.as_str(),
            ada_from(&self.amount),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct PoolCalidusRow {
    #[serde(default)]
    pub pool_id_bech32: String,
    #[serde(default)]
    pub pool_status: Option<String>,
    #[serde(default)]
    pub calidus_id_bech32: Option<String>,
    #[serde(default)]
    pub tx_hash: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub block_time: Num,
}

impl Row for PoolCalidusRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<10} key {}  tx {}  epoch {:<6} {}",
            self.pool_id_bech32,
            optional_text(self.pool_status.as_deref()),
            optional_text(self.calidus_id_bech32.as_deref()),
            optional_text(self.tx_hash.as_deref()),
            self.epoch_no.as_str(),
            optional_timestamp(Some(&self.block_time)),
        )
    }
}
