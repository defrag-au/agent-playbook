//! Asset, policy and datum nouns.

use crate::koios::Method;
use crate::model;
use crate::verbs::render::{self, Render};

use super::{flags, Body, Call, Noun, Paging, Param, Route, Verb};

const fn call(
    endpoint: &'static str,
    method: Method,
    body: Body,
    param: Param,
    paging: Paging,
    render: Render,
) -> Route {
    Route::Call(Call {
        endpoint,
        method,
        body,
        param,
        paging,
        epoch: false,
        render,
    })
}

pub static NOUNS: &[Noun] = &[ASSET, ASSETS, POLICY, DATUM];

const ASSET: Noun = Noun {
    name: "asset",
    question: "One native asset",
    verbs: &[
        &ASSET_INFO,
        &ASSET_UTXOS,
        &ASSET_ADDRESSES,
        &ASSET_TXS,
        &ASSET_HISTORY,
        &ASSET_SUMMARY,
        &ASSET_NFT_ADDRESS,
        &ASSET_REGISTRY,
    ],
};

const ASSET_INFO: Verb = Verb {
    name: "asset",
    question: "What is this asset",
    usage: "at-cardano asset <policy.name>… [--json] [--keyless]",
    notes: "Fingerprint, supply, mint and burn counts, creation time and the minting \
            transaction (`POST /asset_info`). The unit is `policy.hexname`, and a printable \
            name may be spelled as text; a policy alone with no name is accepted.",
    flags: flags::CORE,
    route: call(
        "asset_info",
        Method::Post,
        Body::AssetList,
        Param::None,
        Paging::None,
        render::records::<model::AssetInfoRow>,
    ),
};

const ASSET_UTXOS: Verb = Verb {
    name: "asset utxos",
    question: "Where does this asset sit",
    usage: "at-cardano asset utxos <policy.name>… [--limit N] [--offset N] [--all] [--no-extended]",
    notes: "Every unspent output holding the asset (`POST /asset_utxos`), with the holder's \
            address and the asset's quantity on that output.",
    flags: flags::PAGED_EXTENDED,
    route: call(
        "asset_utxos",
        Method::Post,
        Body::AssetListExtended,
        Param::None,
        Paging::Page,
        render::rows::<model::UtxoRow>,
    ),
};

const ASSET_ADDRESSES: Verb = Verb {
    name: "asset addresses",
    question: "Which addresses hold this asset",
    usage: "at-cardano asset addresses <policy.name> [--limit N] [--offset N] [--all] [--json]",
    notes: "Aggregated holders of one asset (`GET /asset_addresses`), with the quantity at \
            each address. One asset per call.",
    flags: flags::PAGED,
    route: call(
        "asset_addresses",
        Method::Get,
        Body::None,
        Param::AssetUnit,
        Paging::Page,
        render::rows::<model::AssetAddressRow>,
    ),
};

const ASSET_TXS: Verb = Verb {
    name: "asset txs",
    question: "Which transactions moved this asset",
    usage: "at-cardano asset txs <policy.name> [--history] [--limit N] [--offset N] [--all]",
    notes: "Transactions touching one asset (`GET /asset_txs`). `--history` includes every \
            historical transaction rather than only the non-empty ones.",
    flags: flags::ASSET_TXS,
    route: call(
        "asset_txs",
        Method::Get,
        Body::None,
        Param::AssetUnit,
        Paging::Page,
        render::rows::<model::AddressTxRow>,
    ),
};

const ASSET_HISTORY: Verb = Verb {
    name: "asset history",
    question: "When was this asset minted or burned",
    usage: "at-cardano asset history <policy.name> [--limit N] [--offset N] [--all] [--json]",
    notes: "One record per asset listing every mint and burn transaction (`GET \
            /asset_history`), with the signed quantity and block time.",
    flags: flags::PAGED,
    route: call(
        "asset_history",
        Method::Get,
        Body::None,
        Param::AssetUnit,
        Paging::Page,
        render::records_capped::<model::AssetHistoryRow>,
    ),
};

const ASSET_SUMMARY: Verb = Verb {
    name: "asset summary",
    question: "How widely is this asset held",
    usage: "at-cardano asset summary <policy.name> [--json] [--keyless]",
    notes: "Transaction count, address count, and how many holders are staked versus \
            unstaked (`GET /asset_summary`).",
    flags: flags::CORE,
    route: call(
        "asset_summary",
        Method::Get,
        Body::None,
        Param::AssetUnit,
        Paging::None,
        render::records::<model::AssetSummaryRow>,
    ),
};

const ASSET_NFT_ADDRESS: Verb = Verb {
    name: "asset nft-address",
    question: "Which address holds this NFT",
    usage: "at-cardano asset nft-address <policy.name> [--json] [--keyless]",
    notes: "The payment and stake address currently holding an NFT (`GET \
            /asset_nft_address`). A policy and name with no NFTs is an empty answer rather \
            than an error.",
    flags: flags::CORE,
    route: call(
        "asset_nft_address",
        Method::Get,
        Body::None,
        Param::AssetUnit,
        Paging::None,
        render::records::<model::NftAddressRow>,
    ),
};

const ASSET_REGISTRY: Verb = Verb {
    name: "asset registry",
    question: "What is in the token registry",
    usage: "at-cardano asset registry [--limit N] [--offset N] [--all] [--json]",
    notes: "The off-chain token registry (`GET /asset_token_registry`): ticker, description, \
            URL and decimals per registered asset. A curated list, not the whole chain.",
    flags: flags::PAGED,
    route: call(
        "asset_token_registry",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::TokenRegistryRow>,
    ),
};

const ASSETS: Noun = Noun {
    name: "assets",
    question: "The assets themselves",
    verbs: &[&ASSETS_LIST],
};

const ASSETS_LIST: Verb = Verb {
    name: "assets",
    question: "Which assets are there",
    usage: "at-cardano assets [<policy>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Every asset the chain has seen (`GET /asset_list`), or every asset under one \
            policy when a policy id is given. Millions of rows: page it, or ask `policy <id>` \
            for the supply-bearing view.",
    flags: flags::PAGED,
    route: call(
        "asset_list",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_asset_policy"),
        Paging::Page,
        render::rows::<model::AssetListRow>,
    ),
};

const POLICY: Noun = Noun {
    name: "policy",
    question: "One minting policy",
    verbs: &[
        &POLICY_INFO,
        &POLICY_MINTS,
        &POLICY_ADDRESSES,
        &POLICY_ASSETS,
    ],
};

const POLICY_INFO: Verb = Verb {
    name: "policy",
    question: "What is in this policy",
    usage: "at-cardano policy <policy> [--limit N] [--offset N] [--all] [--json]",
    notes: "Per-asset supply, mint/burn counts, creation time and fingerprint for one policy \
            (`GET /policy_asset_info`). One policy per call; a large collection should be \
            paged.",
    flags: flags::PAGED,
    route: call(
        "policy_asset_info",
        Method::Get,
        Body::None,
        Param::Query("_asset_policy"),
        Paging::Page,
        render::rows::<model::AssetInfoRow>,
    ),
};

const POLICY_MINTS: Verb = Verb {
    name: "policy mints",
    question: "Which assets in this policy were minted",
    usage: "at-cardano policy mints <policy> [--limit N] [--offset N] [--all] [--json]",
    notes: "Per-mint rows from `GET /policy_asset_mints` — the supply view with the minting \
            transaction and decimals for each asset.",
    flags: flags::PAGED,
    route: call(
        "policy_asset_mints",
        Method::Get,
        Body::None,
        Param::Query("_asset_policy"),
        Paging::Page,
        render::rows::<model::AssetInfoRow>,
    ),
};

const POLICY_ADDRESSES: Verb = Verb {
    name: "policy addresses",
    question: "Who holds this policy",
    usage: "at-cardano policy addresses <policy> [--limit N] [--offset N] [--all] [--json]",
    notes: "Holder rows from `GET /policy_asset_addresses`, naming the asset, the address and \
            the quantity. On a 10k-asset collection this can exceed the timeout: page it, or \
            use `asset addresses` for one asset.",
    flags: flags::PAGED,
    route: call(
        "policy_asset_addresses",
        Method::Get,
        Body::None,
        Param::Query("_asset_policy"),
        Paging::Page,
        render::rows::<model::AssetAddressRow>,
    ),
};

const POLICY_ASSETS: Verb = Verb {
    name: "policy assets",
    question: "Which assets does this policy define",
    usage: "at-cardano policy assets <policy> [--limit N] [--offset N] [--all] [--json]",
    notes: "The lighter listing (`GET /policy_asset_list`): name, fingerprint, supply and \
            decimals, without the minting transaction.",
    flags: flags::PAGED,
    route: call(
        "policy_asset_list",
        Method::Get,
        Body::None,
        Param::Query("_asset_policy"),
        Paging::Page,
        render::rows::<model::PolicyAssetRow>,
    ),
};

const DATUM: Noun = Noun {
    name: "datum",
    question: "One datum",
    verbs: &[&DATUM_INFO],
};

const DATUM_INFO: Verb = Verb {
    name: "datum",
    question: "What is this datum",
    usage: "at-cardano datum <hash>… [--json] [--keyless]",
    notes: "The datum's CBOR and the transaction that created it (`POST /datum_info`). The \
            decoded Plutus value is arbitrary JSON and is under `--json`.",
    flags: flags::CORE,
    route: call(
        "datum_info",
        Method::Post,
        Body::DatumHashes,
        Param::None,
        Paging::None,
        render::rows::<model::DatumRow>,
    ),
};
