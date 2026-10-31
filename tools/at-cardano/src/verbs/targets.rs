//! What a verb's targets may be, and the two spellings the tool normalises.
//!
//! The kind comes from the catalogue entry — the same field the request is built from — so a
//! verb cannot validate one kind of value and send another.

use crate::catalogue::{Call, Param, Target};
use crate::contract::Fail;

pub const MAX_BATCH: usize = 100;

pub fn validate(call: &Call, targets: &[String]) -> Result<(), Fail> {
    let kind = call.target();
    let wanted = match call.param {
        // A parameter is one value the endpoint takes once.
        Param::Query(_) => (1, 1),
        Param::OptionalQuery(_) => (0, 1),
        Param::AssetUnit => (1, 1),
        // A body batches, except where the endpoint takes no target at all.
        Param::None | Param::BlockRef => match kind {
            Target::None => (0, 0),
            _ => (1, MAX_BATCH),
        },
    };
    if targets.len() < wanted.0 {
        return Err(Fail::usage(format!(
            "{} needs {} target{}",
            call.endpoint,
            wanted.0,
            if wanted.0 == 1 { "" } else { "s" }
        )));
    }
    if targets.len() > wanted.1 {
        return Err(Fail::usage(format!(
            "{} takes at most {} target{} · got {}",
            call.endpoint,
            wanted.1,
            if wanted.1 == 1 { "" } else { "s" },
            targets.len()
        )));
    }

    for target in targets {
        match kind {
            Target::None | Target::Endpoint => {}
            Target::Address => {
                if !target.starts_with("addr") {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a bech32 address · expected `addr…` or `addr_test…`"
                    )));
                }
            }
            Target::Stake => {
                if !target.starts_with("stake") {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a stake address · expected `stake…` or `stake_test…`"
                    )));
                }
            }
            Target::Hex32 => require_hex(target, 64, "a 32-byte hash")?,
            Target::Hex28 => require_hex(target, 56, "a 28-byte hash")?,
            Target::Pool => {
                if !target.starts_with("pool1") {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a pool id · expected bech32 `pool1…`"
                    )));
                }
            }
            Target::Drep => {
                if !(target.starts_with("drep") || (target.len() == 56 && is_hex(target))) {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a DRep id · expected bech32 `drep…` or a 28-byte \
                         script hash in hex"
                    )));
                }
            }
            Target::Proposal => {
                if !(target.starts_with("gov_action") || (target.len() == 64 && is_hex(target))) {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a governance action id · expected bech32 \
                         `gov_action…` or a 32-byte hash in hex"
                    )));
                }
            }
            Target::Voter => {
                if !(target.starts_with("drep")
                    || target.starts_with("pool1")
                    || target.starts_with("cc_")
                    || target.starts_with("cc1"))
                {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a voter id · expected a DRep, pool or committee \
                         member credential in bech32"
                    )));
                }
            }
            Target::Epoch | Target::Label => {
                if target.is_empty() || !target.chars().all(|character| character.is_ascii_digit())
                {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a number · {} takes {}",
                        call.endpoint,
                        if kind == Target::Epoch {
                            "an epoch"
                        } else {
                            "a label"
                        }
                    )));
                }
            }
            Target::AssetUnit => {
                parse_asset_unit(target)?;
            }
            Target::UtxoRef => {
                parse_utxo_ref(target)?;
            }
            Target::BlockRef => {}
        }
    }

    if kind == Target::BlockRef {
        match block_mode(targets)? {
            BlockMode::Height(_) if targets.len() > 1 => {
                return Err(Fail::usage(
                    "a block height takes one target · batch hashes, or ask for one height",
                ));
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn require_hex(value: &str, length: usize, what: &str) -> Result<(), Fail> {
    if value.len() != length || !is_hex(value) {
        return Err(Fail::usage(format!(
            "`{value}` is not {what} · expected {length} hex characters"
        )));
    }
    Ok(())
}

pub fn is_hex(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_hexdigit())
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `policy.name` or `policy` + hex name. A non-hex name is the UTF-8 spelling of the asset
/// name, which is how a person writes `policy.Aliens` without encoding it by hand.
pub fn parse_asset_unit(raw: &str) -> Result<[String; 2], Fail> {
    let (policy, name) = if let Some((policy, name)) = raw.split_once('.') {
        (policy, name)
    } else if raw.len() > 56 {
        raw.split_at(56)
    } else if raw.len() == 56 {
        (raw, "")
    } else {
        return Err(Fail::usage(format!(
            "`{raw}` is not an asset unit · use `policy.name` or `policy` + hex name"
        )));
    };

    if policy.len() != 56 || !is_hex(policy) {
        return Err(Fail::usage(format!(
            "`{policy}` is not a policy id · expected 56 hex characters"
        )));
    }

    let name = if name.is_empty() {
        String::new()
    } else if name.len().is_multiple_of(2) && is_hex(name) {
        name.to_ascii_lowercase()
    } else {
        to_hex(name.as_bytes())
    };

    Ok([policy.to_ascii_lowercase(), name])
}

pub fn parse_utxo_ref(raw: &str) -> Result<String, Fail> {
    let (hash, index) = raw
        .split_once('#')
        .or_else(|| raw.split_once(':'))
        .ok_or_else(|| {
            Fail::usage(format!(
                "`{raw}` is not a UTxO reference · use `tx_hash#index` or `tx_hash:index`"
            ))
        })?;
    require_hex(hash, 64, "a transaction hash")?;
    let index: u32 = index
        .parse()
        .map_err(|_| Fail::usage(format!("`{index}` is not an output index")))?;
    Ok(format!("{hash}#{index}"))
}

pub enum BlockRef {
    Hash(String),
    Height(u64),
}

#[derive(Debug)]
pub enum BlockMode {
    Hashes(Vec<String>),
    Height(u64),
}

pub fn classify_block(raw: &str) -> Result<BlockRef, Fail> {
    if !raw.is_empty() && raw.chars().all(|character| character.is_ascii_digit()) {
        let height = raw
            .parse::<u64>()
            .map_err(|_| Fail::usage(format!("`{raw}` is not a block height")))?;
        return Ok(BlockRef::Height(height));
    }
    if raw.len() == 64 && is_hex(raw) {
        return Ok(BlockRef::Hash(raw.to_string()));
    }
    Err(Fail::usage(format!(
        "`{raw}` is not a block hash or height · expected a 64-character hash or a number"
    )))
}

pub fn block_mode(targets: &[String]) -> Result<BlockMode, Fail> {
    let mut hashes = Vec::new();
    let mut heights = Vec::new();
    for target in targets {
        match classify_block(target)? {
            BlockRef::Hash(hash) => hashes.push(hash),
            BlockRef::Height(height) => heights.push(height),
        }
    }
    if !hashes.is_empty() && !heights.is_empty() {
        return Err(Fail::usage(
            "a block batch must be all hashes or all heights · split the request",
        ));
    }
    if !heights.is_empty() {
        if heights.len() > 1 {
            return Err(Fail::usage(
                "a block height takes one target · batch hashes, or ask for one height",
            ));
        }
        return Ok(BlockMode::Height(heights[0]));
    }
    Ok(BlockMode::Hashes(hashes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = "c364930bd612f42e14d156e1c5410511e77f64cab8f2367a9df544d1";

    #[test]
    fn asset_units_accept_ascii_and_hex_names() {
        assert_eq!(
            parse_asset_unit(&format!("{POLICY}.Aliens")).expect("ascii"),
            [POLICY.to_string(), to_hex(b"Aliens")]
        );
        assert_eq!(
            parse_asset_unit(&format!("{POLICY}.416c69656e73")).expect("hex"),
            [POLICY.to_string(), "416c69656e73".to_string()]
        );
        assert_eq!(
            parse_asset_unit(POLICY).expect("empty name"),
            [POLICY.to_string(), String::new()]
        );
    }

    #[test]
    fn utxo_refs_normalise_the_separator() {
        let reformatted = parse_utxo_ref(&format!("{}#7", "a".repeat(64))).expect("hash");
        assert_eq!(reformatted, format!("{}#7", "a".repeat(64)));
        let colon = parse_utxo_ref(&format!("{}:7", "b".repeat(64))).expect("colon");
        assert_eq!(colon, format!("{}#7", "b".repeat(64)));
    }

    #[test]
    fn block_refs_are_a_hash_or_a_height() {
        assert!(matches!(
            classify_block(&"a".repeat(64)).expect("hash"),
            BlockRef::Hash(_)
        ));
        assert!(matches!(
            classify_block("14100188").expect("height"),
            BlockRef::Height(14_100_188)
        ));
        assert!(classify_block("not-a-block").is_err());
    }

    #[test]
    fn a_mixed_block_batch_is_refused() {
        let error = block_mode(&["a".repeat(64), "14100188".to_string()]).expect_err("mixed");
        assert_eq!(error.exit.code(), 2);
    }

    #[test]
    fn more_than_one_height_is_refused() {
        let error =
            block_mode(&["14100188".to_string(), "14100189".to_string()]).expect_err("two heights");
        assert_eq!(error.exit.code(), 2);
    }
}
