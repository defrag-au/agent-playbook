//! Chain, network, epoch, block, transaction and UTxO nouns.

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

pub static NOUNS: &[Noun] = &[TIP, NETWORK, EPOCH, BLOCK, BLOCKS, TX, UTXO];

const TIP: Noun = Noun {
    name: "tip",
    question: "Where is the chain now",
    verbs: &[&TIP_INFO],
};

const TIP_INFO: Verb = Verb {
    name: "tip",
    question: "Where is the chain now",
    usage: "at-cardano tip [--network N] [--json] [--keyless]",
    notes: "The chain head: hash, era, epoch, block height, slot and time (`GET /tip`). One \
            row is the whole answer, so there is no paging.",
    flags: flags::CORE,
    route: call(
        "tip",
        Method::Get,
        Body::None,
        Param::None,
        Paging::None,
        render::records::<model::Tip>,
    ),
};

const NETWORK: Noun = Noun {
    name: "network",
    question: "Chain-wide tokenomics, eras, genesis and parameters",
    verbs: &[
        &NETWORK_TOTALS,
        &NETWORK_ERAS,
        &NETWORK_GENESIS,
        &NETWORK_PARAMS,
        &NETWORK_PARAM_UPDATES,
    ],
};

const NETWORK_TOTALS: Verb = Verb {
    name: "network",
    question: "How much ADA exists, and where",
    usage: "at-cardano network [<epoch>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Supply, circulation, treasury, reserves, rewards and fees per epoch (`GET \
            /totals`). With no epoch it starts at the newest and pages back; `--json` \
            carries the deposits and withdrawals the summary omits.",
    flags: flags::PAGED,
    route: call(
        "totals",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_epoch_no"),
        Paging::Page,
        render::rows::<model::TotalsRow>,
    ),
};

const NETWORK_ERAS: Verb = Verb {
    name: "network eras",
    question: "Which eras has the chain been through",
    usage: "at-cardano network eras [--json] [--keyless]",
    notes: "One row per era: protocol version, ledger and consensus protocol, the epoch it \
            started, and any hard-fork name (`GET /era_summaries`).",
    flags: flags::CORE,
    route: call(
        "era_summaries",
        Method::Get,
        Body::None,
        Param::None,
        Paging::None,
        render::rows::<model::EraRow>,
    ),
};

const NETWORK_GENESIS: Verb = Verb {
    name: "network genesis",
    question: "What were the chain's genesis parameters",
    usage: "at-cardano network genesis [--json] [--keyless]",
    notes: "Network magic, epoch and slot length, maximum supply, system start, security \
            parameter, KES period and update quorum (`GET /genesis`).",
    flags: flags::CORE,
    route: call(
        "genesis",
        Method::Get,
        Body::None,
        Param::None,
        Paging::None,
        render::records::<model::GenesisRow>,
    ),
};

const NETWORK_PARAMS: Verb = Verb {
    name: "network params",
    question: "What are the current protocol parameters",
    usage: "at-cardano network params [--json] [--keyless]",
    notes: "The node's current parameters in `cardano-cli`'s vocabulary (`GET \
            /cli_protocol_params`). A single object, not a table; `--json` is the whole set, \
            and `epoch params <n>` is the per-epoch table.",
    flags: flags::CORE,
    route: call(
        "cli_protocol_params",
        Method::Get,
        Body::None,
        Param::None,
        Paging::None,
        render::record_object::<model::ProtocolParams>,
    ),
};

const NETWORK_PARAM_UPDATES: Verb = Verb {
    name: "network param-updates",
    question: "Which parameter updates have been proposed",
    usage: "at-cardano network param-updates [--limit N] [--offset N] [--all] [--json]",
    notes: "One row per parameter-change proposal: the epoch, block, time and transaction \
            (`GET /param_updates`). The proposed values are under `--json`.",
    flags: flags::PAGED,
    route: call(
        "param_updates",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::ParamUpdateRow>,
    ),
};

const EPOCH: Noun = Noun {
    name: "epoch",
    question: "One epoch's numbers",
    verbs: &[&EPOCH_INFO, &EPOCH_PARAMS, &EPOCH_BLOCKS],
};

const EPOCH_INFO: Verb = Verb {
    name: "epoch",
    question: "What happened in this epoch",
    usage: "at-cardano epoch <epoch> [--json] [--keyless]",
    notes: "Output, fees, block and transaction counts, active stake and rewards for one \
            epoch (`GET /epoch_info`). One epoch per call; the whole table is `at-cardano \
            query epoch_info`.",
    flags: flags::CORE,
    route: call(
        "epoch_info",
        Method::Get,
        Body::None,
        Param::Query("_epoch_no"),
        Paging::None,
        render::records::<model::EpochInfo>,
    ),
};

const EPOCH_PARAMS: Verb = Verb {
    name: "epoch params",
    question: "What were the protocol parameters in this epoch",
    usage: "at-cardano epoch params <epoch> [--json] [--keyless]",
    notes: "The per-epoch parameter table (`GET /epoch_params`): fees, size limits, \
            deposits, pool settings, execution-unit prices and the governance parameters.",
    flags: flags::CORE,
    route: call(
        "epoch_params",
        Method::Get,
        Body::None,
        Param::Query("_epoch_no"),
        Paging::None,
        render::records::<model::EpochParams>,
    ),
};

const EPOCH_BLOCKS: Verb = Verb {
    name: "epoch blocks",
    question: "Which block protocols produced this epoch",
    usage: "at-cardano epoch blocks <epoch> [--json] [--keyless]",
    notes: "Block counts per protocol version and era for one epoch (`GET \
            /epoch_block_protocols`).",
    flags: flags::CORE,
    route: call(
        "epoch_block_protocols",
        Method::Get,
        Body::None,
        Param::Query("_epoch_no"),
        Paging::None,
        render::rows::<model::EpochBlockProtocolRow>,
    ),
};

const BLOCK: Noun = Noun {
    name: "block",
    question: "One block",
    verbs: &[&BLOCK_INFO, &BLOCK_TXS, &BLOCK_CBOR, &BLOCK_TX_INFO],
};

const BLOCK_INFO: Verb = Verb {
    name: "block",
    question: "What is this block",
    usage: "at-cardano block <hash>… | <height> [--json] [--keyless]",
    notes: "Hash, height, epoch, slot, time, transaction count, size and the producing pool \
            (`POST /block_info`). A height is resolved to its hash first and takes one \
            target; hashes batch.",
    flags: flags::CORE,
    route: call(
        "block_info",
        Method::Post,
        Body::BlockHashes,
        Param::BlockRef,
        Paging::None,
        render::records::<model::BlockInfo>,
    ),
};

const BLOCK_TXS: Verb = Verb {
    name: "block txs",
    question: "Which transactions are in this block",
    usage: "at-cardano block txs <hash>… | <height> [--limit N] [--offset N] [--all] [--json]",
    notes: "Transaction hashes in the requested blocks (`POST /block_txs`). A height is \
            resolved to its hash first and takes one target; hashes batch.",
    flags: flags::PAGED,
    route: call(
        "block_txs",
        Method::Post,
        Body::BlockHashes,
        Param::BlockRef,
        Paging::Page,
        render::rows::<model::BlockTxRow>,
    ),
};

const BLOCK_CBOR: Verb = Verb {
    name: "block cbor",
    question: "What are this block's transactions in bytes",
    usage: "at-cardano block cbor <hash>… | <height> [--limit N] [--offset N] [--all]",
    notes: "Raw transaction CBOR per transaction (`POST /block_tx_cbor`). The row names the \
            length rather than printing the bytes; `at-cardano tx cbor <hash>` prints them, and \
            `--json` carries the whole page.",
    flags: flags::PAGED,
    route: call(
        "block_tx_cbor",
        Method::Post,
        Body::BlockHashes,
        Param::BlockRef,
        Paging::Page,
        render::rows::<model::BlockTxCborRow>,
    ),
};

const BLOCK_TX_INFO: Verb = Verb {
    name: "block tx-info",
    question: "What does each transaction in this block do",
    usage: "at-cardano block tx-info <hash>… | <height> [--limit N] [--offset N] [--all]",
    notes: "The full transaction detail for a block (`POST /block_tx_info`), one record per \
            transaction. Inputs and assets are always included; `--json` carries everything \
            else.",
    flags: flags::PAGED,
    route: call(
        "block_tx_info",
        Method::Post,
        Body::BlockTxInfo,
        Param::BlockRef,
        Paging::Page,
        render::records::<model::TxInfo>,
    ),
};

const BLOCKS: Noun = Noun {
    name: "blocks",
    question: "The blocks themselves, newest first",
    verbs: &[&BLOCKS_LIST],
};

const BLOCKS_LIST: Verb = Verb {
    name: "blocks",
    question: "Which blocks are there",
    usage: "at-cardano blocks [--limit N] [--offset N] [--order S] [--all] [--json]",
    notes: "Summarised blocks, latest first (`GET /blocks`). Thousands of rows per epoch, so \
            page rather than asking for everything.",
    flags: flags::PAGED,
    route: call(
        "blocks",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::BlockInfo>,
    ),
};

const TX: Noun = Noun {
    name: "tx",
    question: "One transaction",
    verbs: &[
        &TX_INFO,
        &TX_UTXOS,
        &TX_CBOR,
        &TX_METADATA,
        &TX_STATUS,
        &TX_METALABELS,
        &TX_BY_METALABEL,
    ],
};

const TX_INFO: Verb = Verb {
    name: "tx",
    question: "What is this transaction",
    usage: "at-cardano tx <hash>… [--no-inputs] [--no-assets] [--scripts] [--json]",
    notes: "Block, time, size, fee, input and output totals, minted assets and validity \
            window (`POST /tx_info`). `--json` carries the UTxOs, metadata, certificates and \
            scripts.",
    flags: flags::TX_INFO,
    route: call(
        "tx_info",
        Method::Post,
        Body::TxInfo,
        Param::None,
        Paging::None,
        render::tx_info,
    ),
};

const TX_UTXOS: Verb = Verb {
    name: "tx utxos",
    question: "Which UTxOs does this transaction reference",
    usage: "at-cardano tx utxos <hash>… [--limit N] [--offset N] [--all] [--json]",
    notes: "The input and output counts and ADA totals per transaction (`POST /tx_utxos`). \
            `--json` carries the refs, addresses and assets.",
    flags: flags::PAGED,
    route: call(
        "tx_utxos",
        Method::Post,
        Body::TxHashes,
        Param::None,
        Paging::Page,
        render::rows::<model::TxUtxosRow>,
    ),
};

const TX_CBOR: Verb = Verb {
    name: "tx cbor",
    question: "What are this transaction's bytes",
    usage: "at-cardano tx cbor <hash>… [--json] [--keyless]",
    notes: "The signed transaction as hex (`POST /tx_cbor`), cut at 256 KiB with the cut \
            announced. `--json` gives the wrapped body.",
    flags: flags::CORE,
    route: call(
        "tx_cbor",
        Method::Post,
        Body::TxHashes,
        Param::None,
        Paging::None,
        render::tx_cbor,
    ),
};

const TX_METADATA: Verb = Verb {
    name: "tx metadata",
    question: "What metadata does this transaction carry",
    usage: "at-cardano tx metadata <hash>… [--json] [--keyless]",
    notes: "The metadata body (`POST /tx_metadata`). Metadata is label-keyed and arbitrary, \
            so the answer is bounded raw JSON rather than a projection.",
    flags: flags::CORE,
    route: call(
        "tx_metadata",
        Method::Post,
        Body::TxHashes,
        Param::None,
        Paging::None,
        render::raw,
    ),
};

const TX_STATUS: Verb = Verb {
    name: "tx status",
    question: "Is this transaction confirmed",
    usage: "at-cardano tx status <hash>… [--json] [--keyless]",
    notes: "Confirmation count per transaction (`POST /tx_status`). A transaction in the \
            mempool is absent rather than zero.",
    flags: flags::CORE,
    route: call(
        "tx_status",
        Method::Post,
        Body::TxHashes,
        Param::None,
        Paging::None,
        render::rows::<model::TxStatusRow>,
    ),
};

const TX_METALABELS: Verb = Verb {
    name: "tx metalabels",
    question: "Which metadata labels are in use",
    usage: "at-cardano tx metalabels [--limit N] [--offset N] [--all] [--json]",
    notes: "Every metadata label the chain has seen (`GET /tx_metalabels`), one per row. Pair \
            it with `tx by-metalabel`.",
    flags: flags::PAGED,
    route: call(
        "tx_metalabels",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::MetalabelRow>,
    ),
};

const TX_BY_METALABEL: Verb = Verb {
    name: "tx by-metalabel",
    question: "Which transactions carry this metadata label",
    usage: "at-cardano tx by-metalabel <label> [--limit N] [--offset N] [--all] [--json]",
    notes: "Transactions carrying one metadata label (`GET /tx_by_metalabel`), newest first. \
            The label is a number, e.g. 674.",
    flags: flags::PAGED,
    route: call(
        "tx_by_metalabel",
        Method::Get,
        Body::None,
        Param::Query("_label"),
        Paging::Page,
        render::rows::<model::BlockTxRow>,
    ),
};

const UTXO: Noun = Noun {
    name: "utxo",
    question: "One unspent output",
    verbs: &[&UTXO_INFO],
};

const UTXO_INFO: Verb = Verb {
    name: "utxo",
    question: "What is this output",
    usage: "at-cardano utxo <tx#index>… [--json] [--keyless]",
    notes: "Value, address, stake key, datum and scripts of one output (`POST /utxo_info`), \
            always extended. `#` or `:` separates the hash from the index.",
    flags: flags::CORE,
    route: call(
        "utxo_info",
        Method::Post,
        Body::UtxoRefs,
        Param::None,
        Paging::None,
        render::rows::<model::UtxoRow>,
    ),
};
