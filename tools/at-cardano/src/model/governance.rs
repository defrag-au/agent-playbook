//! Governance: DReps, the committee, proposals and votes.

use serde::Deserialize;

use super::{
    ada_amount, ada_from, field, optional_text, optional_value, timestamp, Num, Record, Row,
};

#[derive(Debug, Deserialize)]
pub struct DrepInfo {
    #[serde(default)]
    pub drep_id: String,
    #[serde(default)]
    pub hex: Option<String>,
    #[serde(default)]
    pub has_script: Option<bool>,
    #[serde(default)]
    pub drep_status: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub deposit: Option<Num>,
    #[serde(default)]
    pub amount: Option<Num>,
    #[serde(default)]
    pub expires_epoch_no: Option<Num>,
    #[serde(default)]
    pub live_delegator_count: Option<Num>,
    #[serde(default)]
    pub meta_url: Option<String>,
    #[serde(default)]
    pub meta_hash: Option<String>,
}

impl Record for DrepInfo {
    fn lines(&self) -> Vec<String> {
        vec![
            field("drep", &self.drep_id),
            field("hex", optional_text(self.hex.as_deref())),
            field("status", optional_text(self.drep_status.as_deref())),
            field("script", self.has_script.map(super::yes_no).unwrap_or("-")),
            field(
                "active",
                self.active.map(super::yes_no).unwrap_or("-").to_string(),
            ),
            field(
                "deposit",
                self.deposit
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "voting power",
                self.amount
                    .as_ref()
                    .map(ada_amount)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            field(
                "delegators",
                optional_value(self.live_delegator_count.as_ref()),
            ),
            field("expires", optional_value(self.expires_epoch_no.as_ref())),
            field("meta", optional_text(self.meta_url.as_deref())),
            field("meta hash", optional_text(self.meta_hash.as_deref())),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct DrepListRow {
    #[serde(default)]
    pub drep_id: String,
    #[serde(default)]
    pub hex: Option<String>,
    #[serde(default)]
    pub has_script: Option<bool>,
    #[serde(default)]
    pub drep_status: Option<String>,
}

impl Row for DrepListRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<12} script {:<4} {}",
            self.drep_id,
            optional_text(self.drep_status.as_deref()),
            self.has_script.map(super::yes_no).unwrap_or("-"),
            optional_text(self.hex.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct DrepMetadataRow {
    #[serde(default)]
    pub drep_id: String,
    #[serde(default)]
    pub meta_url: Option<String>,
    #[serde(default)]
    pub meta_hash: Option<String>,
    #[serde(default)]
    pub is_valid: Option<bool>,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

impl Row for DrepMetadataRow {
    fn line(&self) -> String {
        format!(
            "{}  valid {:<4} {:<4} {}  {}",
            self.drep_id,
            self.is_valid.map(super::yes_no).unwrap_or("-"),
            optional_text(self.language.as_deref()),
            optional_text(self.meta_url.as_deref()),
            optional_text(self.meta_hash.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct DrepDelegatorRow {
    #[serde(default)]
    pub stake_address: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub amount: Num,
}

impl Row for DrepDelegatorRow {
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
pub struct DrepPowerRow {
    #[serde(default)]
    pub drep_id: String,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub amount: Num,
}

impl Row for DrepPowerRow {
    fn line(&self) -> String {
        format!(
            "{}  epoch {:<6} {:>16} ADA",
            self.drep_id,
            self.epoch_no.as_str(),
            ada_from(&self.amount),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct DrepUpdateRow {
    #[serde(default)]
    pub drep_id: String,
    #[serde(default)]
    pub update_tx_hash: String,
    #[serde(default)]
    pub cert_index: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub deposit: Option<Num>,
}

impl Row for DrepUpdateRow {
    fn line(&self) -> String {
        format!(
            "{}  {:<12} cert {:<4} {}  {}",
            self.drep_id,
            optional_text(self.action.as_deref()),
            self.cert_index.as_str(),
            timestamp(&self.block_time),
            self.update_tx_hash,
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct DrepEpochSummaryRow {
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub amount: Num,
    #[serde(default)]
    pub dreps: Num,
}

impl Row for DrepEpochSummaryRow {
    fn line(&self) -> String {
        format!(
            "epoch {:<6} {:>16} ADA across {:>6} dreps",
            self.epoch_no.as_str(),
            ada_from(&self.amount),
            self.dreps.as_str(),
        )
    }
}

/// One vote by one voter on one proposal — the shape `/drep_votes`, `/pool_votes` and
/// `/committee_votes` share.
#[derive(Debug, Deserialize)]
pub struct VoteCastRow {
    #[serde(default)]
    pub proposal_id: String,
    #[serde(default)]
    pub proposal_tx_hash: Option<String>,
    #[serde(default)]
    pub proposal_index: Option<Num>,
    #[serde(default)]
    pub vote_tx_hash: String,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub vote: Option<String>,
}

impl Row for VoteCastRow {
    fn line(&self) -> String {
        format!(
            "{}  vote {:<12} {}  {}",
            self.proposal_id,
            optional_text(self.vote.as_deref()),
            timestamp(&self.block_time),
            self.vote_tx_hash,
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct CommitteeMember {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub cc_cold_hex: Option<String>,
    #[serde(default)]
    pub cc_cold_has_script: Option<bool>,
    #[serde(default)]
    pub cc_hot_hex: Option<String>,
    #[serde(default)]
    pub expiration_epoch: Option<Num>,
}

#[derive(Debug, Deserialize)]
pub struct CommitteeInfo {
    #[serde(default)]
    pub proposal_id: Option<String>,
    #[serde(default)]
    pub quorum_numerator: Num,
    #[serde(default)]
    pub quorum_denominator: Num,
    #[serde(default)]
    pub members: Vec<CommitteeMember>,
}

impl Record for CommitteeInfo {
    fn lines(&self) -> Vec<String> {
        let mut out = vec![
            field(
                "quorum",
                format!(
                    "{}/{}",
                    self.quorum_numerator.as_str(),
                    self.quorum_denominator.as_str()
                ),
            ),
            field("members", self.members.len().to_string()),
        ];
        if let Some(proposal) = self.proposal_id.as_deref() {
            if !proposal.is_empty() {
                out.push(field("appointed by", proposal));
            }
        }
        for member in &self.members {
            out.push(field(
                "  member",
                format!(
                    "{:<16} cold {:<58} hot {:<58} expires {}",
                    optional_text(member.status.as_deref()),
                    optional_text(member.cc_cold_hex.as_deref()),
                    optional_text(member.cc_hot_hex.as_deref()),
                    optional_value(member.expiration_epoch.as_ref()),
                ),
            ));
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct ProposalRow {
    #[serde(default)]
    pub proposal_id: String,
    #[serde(default)]
    pub proposal_type: Option<String>,
    #[serde(default)]
    pub proposed_epoch: Num,
    #[serde(default)]
    pub ratified_epoch: Option<Num>,
    #[serde(default)]
    pub enacted_epoch: Option<Num>,
    #[serde(default)]
    pub expired_epoch: Option<Num>,
    #[serde(default)]
    pub dropped_epoch: Option<Num>,
    #[serde(default)]
    pub expiration: Option<Num>,
    #[serde(default)]
    pub deposit: Option<Num>,
    #[serde(default)]
    pub return_address: Option<String>,
    #[serde(default)]
    pub meta_url: Option<String>,
    #[serde(default)]
    pub meta_is_valid: Option<bool>,
}

impl Row for ProposalRow {
    fn line(&self) -> String {
        let mut status = String::new();
        for (label, value) in [
            ("ratified", self.ratified_epoch.as_ref()),
            ("enacted", self.enacted_epoch.as_ref()),
            ("expired", self.expired_epoch.as_ref()),
            ("dropped", self.dropped_epoch.as_ref()),
        ] {
            if let Some(epoch) = value.filter(|num| !num.is_empty()) {
                status.push_str(&format!("  {label} {}", epoch.as_str()));
            }
        }
        format!(
            "{}  {:<22} proposed {:<6} expires {:<6}{status}  {}",
            self.proposal_id,
            optional_text(self.proposal_type.as_deref()),
            self.proposed_epoch.as_str(),
            optional_value(self.expiration.as_ref()),
            optional_text(self.meta_url.as_deref()),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct ProposalSummaryRow {
    #[serde(default)]
    pub proposal_type: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub drep_yes_pct: Num,
    #[serde(default)]
    pub drep_no_pct: Num,
    #[serde(default)]
    pub drep_yes_votes_cast: Num,
    #[serde(default)]
    pub drep_no_votes_cast: Num,
    #[serde(default)]
    pub drep_abstain_votes_cast: Num,
    #[serde(default)]
    pub pool_yes_pct: Num,
    #[serde(default)]
    pub pool_no_pct: Num,
    #[serde(default)]
    pub pool_yes_votes_cast: Num,
    #[serde(default)]
    pub pool_no_votes_cast: Num,
    #[serde(default)]
    pub pool_abstain_votes_cast: Num,
    #[serde(default)]
    pub committee_yes_pct: Num,
    #[serde(default)]
    pub committee_no_pct: Num,
    #[serde(default)]
    pub committee_yes_votes_cast: Num,
    #[serde(default)]
    pub committee_no_votes_cast: Num,
    #[serde(default)]
    pub committee_abstain_votes_cast: Num,
}

impl Row for ProposalSummaryRow {
    fn line(&self) -> String {
        format!(
            "{:<22} epoch {:<6}  drep {:>6}% yes ({}/{}/{})  pool {:>6}% yes ({}/{}/{})  \
             committee {:>6}% yes ({}/{}/{})",
            optional_text(self.proposal_type.as_deref()),
            self.epoch_no.as_str(),
            optional_value(Some(&self.drep_yes_pct)),
            self.drep_yes_votes_cast.as_str(),
            self.drep_no_votes_cast.as_str(),
            self.drep_abstain_votes_cast.as_str(),
            optional_value(Some(&self.pool_yes_pct)),
            self.pool_yes_votes_cast.as_str(),
            self.pool_no_votes_cast.as_str(),
            self.pool_abstain_votes_cast.as_str(),
            optional_value(Some(&self.committee_yes_pct)),
            self.committee_yes_votes_cast.as_str(),
            self.committee_no_votes_cast.as_str(),
            self.committee_abstain_votes_cast.as_str(),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct ProposalVoteRow {
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub voter_role: Option<String>,
    #[serde(default)]
    pub voter_id: String,
    #[serde(default)]
    pub vote: Option<String>,
}

impl Row for ProposalVoteRow {
    fn line(&self) -> String {
        format!(
            "{:<10} {:<64} vote {:<12} {}",
            optional_text(self.voter_role.as_deref()),
            self.voter_id,
            optional_text(self.vote.as_deref()),
            timestamp(&self.block_time),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct VoteRow {
    #[serde(default)]
    pub vote_tx_hash: String,
    #[serde(default)]
    pub voter_role: Option<String>,
    #[serde(default)]
    pub voter_id: String,
    #[serde(default)]
    pub proposal_id: String,
    #[serde(default)]
    pub proposal_type: Option<String>,
    #[serde(default)]
    pub epoch_no: Num,
    #[serde(default)]
    pub block_height: Num,
    #[serde(default)]
    pub block_time: Num,
    #[serde(default)]
    pub vote: Option<String>,
}

impl Row for VoteRow {
    fn line(&self) -> String {
        format!(
            "{:<10} {:<64} {:<22} vote {:<12} epoch {:<6} block {:<10} {}",
            optional_text(self.voter_role.as_deref()),
            self.voter_id,
            optional_text(self.proposal_type.as_deref()),
            optional_text(self.vote.as_deref()),
            self.epoch_no.as_str(),
            self.block_height.as_str(),
            timestamp(&self.block_time),
        )
    }
}
