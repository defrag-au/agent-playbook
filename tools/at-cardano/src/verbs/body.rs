//! Typed request bodies. Koios names every body field with a leading underscore, so the
//! serde renames here are the wire contract rather than taste.

use serde::Serialize;

use crate::catalogue::Body;
use crate::contract::Fail;
use crate::koios;
use crate::verbs::targets;
use crate::verbs::Options;

pub fn build(body: Body, targets: &[String], options: &Options) -> Result<Option<Vec<u8>>, Fail> {
    let bytes = match body {
        Body::None => return Ok(None),
        Body::Addresses => encode(&Addresses {
            addresses: targets.to_vec(),
        })?,
        Body::AddressesExtended => encode(&ExtendedAddresses {
            addresses: targets.to_vec(),
            extended: options.extended,
        })?,
        Body::Stake => encode(&StakeAddresses {
            stake_addresses: targets.to_vec(),
        })?,
        Body::StakeExtended => encode(&ExtendedStakeAddresses {
            stake_addresses: targets.to_vec(),
            extended: options.extended,
        })?,
        Body::StakeFirstOnly => encode(&FirstOnlyStakeAddresses {
            stake_addresses: targets.to_vec(),
            first_only: true,
        })?,
        Body::StakeEpoch => encode(&StakeAddressesWithEpoch {
            stake_addresses: targets.to_vec(),
            epoch_no: options.epoch,
        })?,
        Body::TxHashes => encode(&TxHashes {
            tx_hashes: targets.to_vec(),
        })?,
        Body::TxInfo => encode(&TxInfoBody {
            tx_hashes: targets.to_vec(),
            inputs: options.inputs,
            assets: options.assets,
            scripts: options.scripts,
        })?,
        Body::Credentials => encode(&Credentials {
            payment_credentials: targets.to_vec(),
        })?,
        Body::CredentialsExtended => encode(&ExtendedCredentials {
            payment_credentials: targets.to_vec(),
            extended: options.extended,
        })?,
        Body::UtxoRefs => encode(&UtxoRefs {
            utxo_refs: targets
                .iter()
                .map(|target| targets::parse_utxo_ref(target))
                .collect::<Result<Vec<_>, _>>()?,
            extended: true,
        })?,
        Body::DatumHashes => encode(&DatumHashes {
            datum_hashes: targets.to_vec(),
        })?,
        Body::ScriptHashes => encode(&ScriptHashes {
            script_hashes: targets.to_vec(),
        })?,
        Body::AssetList => encode(&AssetList {
            asset_list: asset_units(targets)?,
        })?,
        Body::AssetListExtended => encode(&ExtendedAssetList {
            asset_list: asset_units(targets)?,
            extended: options.extended,
        })?,
        Body::PoolIds => encode(&PoolIds {
            pool_ids: targets.to_vec(),
        })?,
        Body::PoolIdsOptional => encode(&PoolIds {
            pool_ids: targets.to_vec(),
        })?,
        Body::DrepIds => encode(&DrepIds {
            drep_ids: targets.to_vec(),
        })?,
        Body::BlockHashes => encode(&BlockHashes {
            block_hashes: targets.to_vec(),
        })?,
        Body::BlockTxInfo => encode(&BlockTxInfo {
            block_hashes: targets.to_vec(),
            inputs: options.inputs,
            assets: options.assets,
            scripts: options.scripts,
        })?,
    };
    Ok(Some(bytes))
}

fn asset_units(targets: &[String]) -> Result<Vec<[String; 2]>, Fail> {
    targets
        .iter()
        .map(|target| targets::parse_asset_unit(target))
        .collect()
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Fail> {
    koios::json_body(value)
}

#[derive(Serialize)]
struct Addresses {
    #[serde(rename = "_addresses")]
    addresses: Vec<String>,
}

#[derive(Serialize)]
struct ExtendedAddresses {
    #[serde(rename = "_addresses")]
    addresses: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct StakeAddresses {
    #[serde(rename = "_stake_addresses")]
    stake_addresses: Vec<String>,
}

#[derive(Serialize)]
struct ExtendedStakeAddresses {
    #[serde(rename = "_stake_addresses")]
    stake_addresses: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct FirstOnlyStakeAddresses {
    #[serde(rename = "_stake_addresses")]
    stake_addresses: Vec<String>,
    #[serde(rename = "_first_only")]
    first_only: bool,
}

#[derive(Serialize)]
struct StakeAddressesWithEpoch {
    #[serde(rename = "_stake_addresses")]
    stake_addresses: Vec<String>,
    #[serde(rename = "_epoch_no", skip_serializing_if = "Option::is_none")]
    epoch_no: Option<u64>,
}

#[derive(Serialize)]
struct TxHashes {
    #[serde(rename = "_tx_hashes")]
    tx_hashes: Vec<String>,
}

#[derive(Serialize)]
struct TxInfoBody {
    #[serde(rename = "_tx_hashes")]
    tx_hashes: Vec<String>,
    #[serde(rename = "_inputs")]
    inputs: bool,
    #[serde(rename = "_assets")]
    assets: bool,
    #[serde(rename = "_scripts")]
    scripts: bool,
}

#[derive(Serialize)]
struct Credentials {
    #[serde(rename = "_payment_credentials")]
    payment_credentials: Vec<String>,
}

#[derive(Serialize)]
struct ExtendedCredentials {
    #[serde(rename = "_payment_credentials")]
    payment_credentials: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct UtxoRefs {
    #[serde(rename = "_utxo_refs")]
    utxo_refs: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct DatumHashes {
    #[serde(rename = "_datum_hashes")]
    datum_hashes: Vec<String>,
}

#[derive(Serialize)]
struct ScriptHashes {
    #[serde(rename = "_script_hashes")]
    script_hashes: Vec<String>,
}

#[derive(Serialize)]
struct AssetList {
    #[serde(rename = "_asset_list")]
    asset_list: Vec<[String; 2]>,
}

#[derive(Serialize)]
struct ExtendedAssetList {
    #[serde(rename = "_asset_list")]
    asset_list: Vec<[String; 2]>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct PoolIds {
    #[serde(rename = "_pool_bech32_ids")]
    pool_ids: Vec<String>,
}

#[derive(Serialize)]
struct DrepIds {
    #[serde(rename = "_drep_ids")]
    drep_ids: Vec<String>,
}

#[derive(Serialize)]
struct BlockHashes {
    #[serde(rename = "_block_hashes")]
    block_hashes: Vec<String>,
}

#[derive(Serialize)]
struct BlockTxInfo {
    #[serde(rename = "_block_hashes")]
    block_hashes: Vec<String>,
    #[serde(rename = "_inputs")]
    inputs: bool,
    #[serde(rename = "_assets")]
    assets: bool,
    #[serde(rename = "_scripts")]
    scripts: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_carry_the_wire_names() {
        let bytes = build(Body::TxHashes, &["a".repeat(64)], &Options::default()).expect("body");
        let text = String::from_utf8(bytes.expect("some body")).expect("utf8");
        assert!(text.contains("\"_tx_hashes\""));
        assert!(!text.contains("_epoch_no"));
    }

    #[test]
    fn an_epoch_body_omits_the_filter_when_it_is_not_set() {
        let bytes = build(
            Body::StakeEpoch,
            &["stake1".to_string()],
            &Options::default(),
        )
        .expect("body")
        .expect("some body");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("\"_stake_addresses\""));
        assert!(!text.contains("_epoch_no"));
    }
}
