//! The complete Koios surface, as published.
//!
//! The table is transcribed from `https://api.koios.rest/koiosapi.yaml` (fetched 2026-10-31):
//! every path in the spec, its method, and the kind of value it takes. Which *verb* answers
//! an endpoint is derived from the catalogue at run time rather than stored here, so the
//! `endpoints` answer cannot claim a coverage the grammar does not have.

use crate::catalogue::{self, Route};

pub struct EndpointRow {
    /// The path under `/api/v1`, without the slash.
    pub path: &'static str,
    pub method: &'static str,
    /// What the request carries, in the same words the verb tables use.
    pub target: &'static str,
}

pub const SPEC: &str = "https://api.koios.rest/koiosapi.yaml";

pub static ENDPOINTS: &[EndpointRow] = &[
    EndpointRow {
        path: "tip",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "era_summaries",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "genesis",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "totals",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "param_updates",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "cli_protocol_params",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "reserve_withdrawals",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "treasury_withdrawals",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "epoch_info",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "epoch_params",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "epoch_block_protocols",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "blocks",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "block_info",
        method: "POST",
        target: "block hashes",
    },
    EndpointRow {
        path: "block_txs",
        method: "POST",
        target: "block hashes",
    },
    EndpointRow {
        path: "block_tx_cbor",
        method: "POST",
        target: "block hashes",
    },
    EndpointRow {
        path: "block_tx_info",
        method: "POST",
        target: "block hashes",
    },
    EndpointRow {
        path: "utxo_info",
        method: "POST",
        target: "utxo refs",
    },
    EndpointRow {
        path: "tx_cbor",
        method: "POST",
        target: "tx hashes",
    },
    EndpointRow {
        path: "tx_info",
        method: "POST",
        target: "tx hashes",
    },
    EndpointRow {
        path: "tx_metadata",
        method: "POST",
        target: "tx hashes",
    },
    EndpointRow {
        path: "tx_metalabels",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "tx_by_metalabel",
        method: "GET",
        target: "label",
    },
    EndpointRow {
        path: "submittx",
        method: "POST",
        target: "signed tx bytes",
    },
    EndpointRow {
        path: "tx_status",
        method: "POST",
        target: "tx hashes",
    },
    EndpointRow {
        path: "tx_outs_epoch",
        method: "GET",
        target: "epoch",
    },
    EndpointRow {
        path: "tx_utxos",
        method: "POST",
        target: "tx hashes",
    },
    EndpointRow {
        path: "address_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "address_info",
        method: "POST",
        target: "addresses",
    },
    EndpointRow {
        path: "address_utxos",
        method: "POST",
        target: "addresses",
    },
    EndpointRow {
        path: "address_outputs",
        method: "POST",
        target: "addresses",
    },
    EndpointRow {
        path: "credential_utxos",
        method: "POST",
        target: "payment credentials",
    },
    EndpointRow {
        path: "address_txs",
        method: "POST",
        target: "addresses",
    },
    EndpointRow {
        path: "credential_txs",
        method: "POST",
        target: "payment credentials",
    },
    EndpointRow {
        path: "address_assets",
        method: "POST",
        target: "addresses",
    },
    EndpointRow {
        path: "account_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "account_info",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_info_cached",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_utxos",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_txs",
        method: "GET",
        target: "stake address",
    },
    EndpointRow {
        path: "account_rewards",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_reward_history",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_updates",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_update_history",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_addresses",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_assets",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_history",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "account_stake_history",
        method: "POST",
        target: "stake addresses",
    },
    EndpointRow {
        path: "asset_list",
        method: "GET",
        target: "policy?",
    },
    EndpointRow {
        path: "policy_asset_list",
        method: "GET",
        target: "policy",
    },
    EndpointRow {
        path: "asset_token_registry",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "asset_info",
        method: "POST",
        target: "asset units",
    },
    EndpointRow {
        path: "asset_utxos",
        method: "POST",
        target: "asset units",
    },
    EndpointRow {
        path: "asset_history",
        method: "GET",
        target: "asset unit",
    },
    EndpointRow {
        path: "asset_addresses",
        method: "GET",
        target: "asset unit",
    },
    EndpointRow {
        path: "asset_nft_address",
        method: "GET",
        target: "asset unit",
    },
    EndpointRow {
        path: "policy_asset_addresses",
        method: "GET",
        target: "policy",
    },
    EndpointRow {
        path: "policy_asset_info",
        method: "GET",
        target: "policy",
    },
    EndpointRow {
        path: "policy_asset_mints",
        method: "GET",
        target: "policy",
    },
    EndpointRow {
        path: "asset_summary",
        method: "GET",
        target: "asset unit",
    },
    EndpointRow {
        path: "asset_txs",
        method: "GET",
        target: "asset unit",
    },
    EndpointRow {
        path: "drep_epoch_summary",
        method: "GET",
        target: "epoch",
    },
    EndpointRow {
        path: "drep_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "drep_info",
        method: "POST",
        target: "drep ids",
    },
    EndpointRow {
        path: "drep_metadata",
        method: "POST",
        target: "drep ids",
    },
    EndpointRow {
        path: "drep_updates",
        method: "GET",
        target: "drep?",
    },
    EndpointRow {
        path: "drep_history",
        method: "GET",
        target: "drep?, epoch?",
    },
    EndpointRow {
        path: "drep_voting_power_history",
        method: "GET",
        target: "drep?, epoch?",
    },
    EndpointRow {
        path: "drep_votes",
        method: "GET",
        target: "drep",
    },
    EndpointRow {
        path: "drep_delegators",
        method: "GET",
        target: "drep",
    },
    EndpointRow {
        path: "committee_info",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "committee_votes",
        method: "GET",
        target: "member?",
    },
    EndpointRow {
        path: "proposal_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "voter_proposal_list",
        method: "GET",
        target: "voter?",
    },
    EndpointRow {
        path: "proposal_voting_summary",
        method: "GET",
        target: "proposal",
    },
    EndpointRow {
        path: "proposal_votes",
        method: "GET",
        target: "proposal",
    },
    EndpointRow {
        path: "vote_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "pool_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "pool_info",
        method: "POST",
        target: "pool ids",
    },
    EndpointRow {
        path: "pool_stake_snapshot",
        method: "GET",
        target: "pool",
    },
    EndpointRow {
        path: "pool_delegators",
        method: "GET",
        target: "pool",
    },
    EndpointRow {
        path: "pool_delegators_history",
        method: "GET",
        target: "pool, epoch?",
    },
    EndpointRow {
        path: "pool_invalid_delegators",
        method: "GET",
        target: "pool, epoch?",
    },
    EndpointRow {
        path: "pool_blocks",
        method: "GET",
        target: "pool, epoch?",
    },
    EndpointRow {
        path: "pool_owner_history",
        method: "POST",
        target: "pool ids",
    },
    EndpointRow {
        path: "pool_history",
        method: "GET",
        target: "pool, epoch?",
    },
    EndpointRow {
        path: "pool_updates",
        method: "GET",
        target: "pool?",
    },
    EndpointRow {
        path: "pool_registrations",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "pool_retirements",
        method: "GET",
        target: "epoch?",
    },
    EndpointRow {
        path: "pool_relays",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "pool_voting_power_history",
        method: "GET",
        target: "pool?, epoch?",
    },
    EndpointRow {
        path: "pool_votes",
        method: "GET",
        target: "pool",
    },
    EndpointRow {
        path: "pool_groups",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "pool_metadata",
        method: "POST",
        target: "pool ids?",
    },
    EndpointRow {
        path: "pool_calidus_keys",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "script_info",
        method: "POST",
        target: "script hashes",
    },
    EndpointRow {
        path: "native_script_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "plutus_script_list",
        method: "GET",
        target: "—",
    },
    EndpointRow {
        path: "script_redeemers",
        method: "GET",
        target: "script hash",
    },
    EndpointRow {
        path: "reference_script_utxos",
        method: "POST",
        target: "script hashes",
    },
    EndpointRow {
        path: "script_utxos",
        method: "GET",
        target: "script hash",
    },
    EndpointRow {
        path: "datum_info",
        method: "POST",
        target: "datum hashes",
    },
    EndpointRow {
        path: "ogmios",
        method: "POST",
        target: "ogmios request",
    },
];

/// The verb that answers a path, or the way the caller reaches it instead.
pub fn answered_by(path: &str) -> &'static str {
    if path == "submittx" {
        return "refused";
    }
    if let Some(verb) = catalogue::verbs().find(|verb| match &verb.route {
        Route::Call(call) => call.endpoint == path,
        _ => false,
    }) {
        return verb.name;
    }
    "query"
}

/// Rows whose answer is a command rather than a word: `refused` and `query` explain themselves.
pub fn line(row: &EndpointRow) -> String {
    format!(
        "{:<28} {:<5} {:<24} {}",
        row.path,
        row.method,
        row.target,
        answered_by(row.path)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_the_published_spec() {
        // 102 paths: 100 reads, the one write, and the Ogmios proxy.
        assert_eq!(ENDPOINTS.len(), 102);
        let writes: Vec<&str> = ENDPOINTS
            .iter()
            .filter(|row| row.path == "submittx")
            .map(|row| row.path)
            .collect();
        assert_eq!(writes, vec!["submittx"]);
    }

    #[test]
    fn only_the_documented_long_tail_is_left_to_query() {
        let mut unanswerable: Vec<&str> = ENDPOINTS
            .iter()
            .filter(|row| row.path != "submittx")
            .filter(|row| answered_by(row.path) == "query")
            .map(|row| row.path)
            .collect();
        unanswerable.sort_unstable();
        // `ogmios` is a proxy protocol, not this REST API. The other three are full-chain scans
        // that Koios either answers with a 504 after two minutes (`/tx_outs_epoch`) or refuses on
        // mainnet outright (`/reserve_withdrawals`, `/treasury_withdrawals`), so a verb would only
        // wrap a failure — they stay reachable through `query`.
        assert_eq!(
            unanswerable,
            vec![
                "ogmios",
                "reserve_withdrawals",
                "treasury_withdrawals",
                "tx_outs_epoch"
            ]
        );
    }

    #[test]
    fn the_only_refusal_is_the_write() {
        for row in ENDPOINTS {
            let answered = answered_by(row.path);
            if answered == "refused" {
                assert_eq!(row.path, "submittx");
            }
        }
    }
}
