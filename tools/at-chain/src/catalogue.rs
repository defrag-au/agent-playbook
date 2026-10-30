//! `at-chain`'s verb table. The vocabulary and help rendering live in `at-core`, shared with
//! the other `at-*` binaries, so the catalogue cannot advertise a grammar the parser refuses.

pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb, SUMMARY};

const NETWORK: Flag = Flag {
    name: "--network",
    takes_value: true,
    value: "mainnet|preprod|preview|guild",
    one_line: "Which Koios network; default mainnet",
};
const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Rows to ask for; default 20, ceiling 1000. Clamping is announced",
};
const OFFSET: Flag = Flag {
    name: "--offset",
    takes_value: true,
    value: "N",
    one_line: "Rows to skip; the previous answer's row count is the next offset",
};
const ORDER: Flag = Flag {
    name: "--order",
    takes_value: true,
    value: "COL[.asc|.desc]…",
    one_line: "PostgREST order, e.g. block_time.desc",
};
const SELECT: Flag = Flag {
    name: "--select",
    takes_value: true,
    value: "COL,…",
    one_line: "Columns to ask Koios for, e.g. tx_hash,block_height",
};
const ALL: Flag = Flag {
    name: "--all",
    takes_value: false,
    value: "",
    one_line: "Follow pages at the 1000-row ceiling, up to a 10-page cap",
};
const JSON: Flag = Flag {
    name: "--json",
    takes_value: false,
    value: "",
    one_line: "The response body as JSON, bounded, instead of the summary",
};
const KEYLESS: Flag = Flag {
    name: "--keyless",
    takes_value: false,
    value: "",
    one_line: "Ignore a configured key and use the free tier",
};
const NO_EXTENDED: Flag = Flag {
    name: "--no-extended",
    takes_value: false,
    value: "",
    one_line: "Omit inline datums and script references (a smaller response)",
};
const NO_INPUTS: Flag = Flag {
    name: "--no-inputs",
    takes_value: false,
    value: "",
    one_line: "Omit inputs from /tx_info; the summary says they were not requested",
};
const NO_ASSETS: Flag = Flag {
    name: "--no-assets",
    takes_value: false,
    value: "",
    one_line: "Omit asset lists from /tx_info",
};
const SCRIPTS: Flag = Flag {
    name: "--scripts",
    takes_value: false,
    value: "",
    one_line: "Include scripts in /tx_info",
};
const MINTS: Flag = Flag {
    name: "--mints",
    takes_value: false,
    value: "",
    one_line: "Per-mint rows from /policy_asset_mints instead of asset info",
};
const ADDRESSES: Flag = Flag {
    name: "--addresses",
    takes_value: false,
    value: "",
    one_line: "Holder rows from /policy_asset_addresses instead of asset info",
};
const POST: Flag = Flag {
    name: "--post",
    takes_value: false,
    value: "",
    one_line: "Use POST for `query`; without it the request is a GET",
};
const BODY: Flag = Flag {
    name: "--body",
    takes_value: true,
    value: "JSON",
    one_line: "Inline JSON request body for `query --post`",
};
const BODY_FILE: Flag = Flag {
    name: "--body-file",
    takes_value: true,
    value: "<path>",
    one_line: "Read the JSON request body from a file (max 1 MiB)",
};
const QUERY: Flag = Flag {
    name: "--query",
    takes_value: true,
    value: "key=value",
    one_line: "A query parameter; repeatable",
};
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

const CORE: &[Flag] = &[NETWORK, JSON, KEYLESS, HOW];
const RAW: &[Flag] = &[NETWORK, KEYLESS, HOW];
const AUTH: &[Flag] = &[NETWORK, KEYLESS, HOW];
const PAGED: &[Flag] = &[
    NETWORK, LIMIT, OFFSET, ORDER, SELECT, ALL, SUMMARY, JSON, KEYLESS, HOW,
];
const UTXO: &[Flag] = &[
    NETWORK,
    LIMIT,
    OFFSET,
    ORDER,
    SELECT,
    ALL,
    SUMMARY,
    NO_EXTENDED,
    JSON,
    KEYLESS,
    HOW,
];
const TX_INFO: &[Flag] = &[NETWORK, NO_INPUTS, NO_ASSETS, SCRIPTS, JSON, KEYLESS, HOW];
const POLICY: &[Flag] = &[
    NETWORK, MINTS, ADDRESSES, LIMIT, OFFSET, ORDER, ALL, SUMMARY, JSON, KEYLESS, HOW,
];
const QUERY_FLAGS: &[Flag] = &[
    NETWORK, POST, BODY, BODY_FILE, QUERY, LIMIT, OFFSET, KEYLESS, HOW,
];

pub static VERBS: &[Verb] = &[
    Verb {
        name: "tip",
        question: "Where is the chain now",
        usage: "at-chain tip [--network N] [--json] [--keyless]",
        notes: "The chain head: hash, epoch, block height, slot and time. One row is the whole \
                answer, so there is no paging.",
        flags: CORE,
    },
    Verb {
        name: "auth",
        question: "Which key and endpoint will be used",
        usage: "at-chain auth [--network N] [--keyless]",
        notes: "Prints the endpoint, the key source and whether KOIOS_API_KEY is set. It makes \
                no request and never prints the key; it is how a run that is unexpectedly \
                keyless is diagnosed.",
        flags: AUTH,
    },
    Verb {
        name: "address",
        question: "What does this address hold",
        usage: "at-chain address <addr>… [--network N] [--json] [--keyless]",
        notes: "Balance, stake address and script hash per address (`POST /address_info`). \
                Up to 100 addresses in one request.",
        flags: CORE,
    },
    Verb {
        name: "utxos",
        question: "Which UTxOs sit at these addresses",
        usage: "at-chain utxos <addr>… [--limit N] [--offset N] [--all] [--no-extended] [--json]",
        notes: "UTxOs at up to 100 addresses (`POST /address_utxos`, `_extended` on by \
                default). The row names the out-ref, lovelace, asset count, datum marker and \
                address; `--json` carries the inline datum and every asset.",
        flags: UTXO,
    },
    Verb {
        name: "txs",
        question: "Which transactions touched these addresses",
        usage: "at-chain txs <addr>… [--limit N] [--offset N] [--order S] [--all] [--json]",
        notes: "Transaction hashes for up to 100 addresses (`POST /address_txs`), newest first \
                unless --order says otherwise.",
        flags: PAGED,
    },
    Verb {
        name: "assets",
        question: "Which assets do these addresses hold",
        usage: "at-chain assets <addr>… [--limit N] [--offset N] [--all] [--json]",
        notes: "Aggregated native-asset holdings per address (`POST /address_assets`). ADA is \
                not a native asset and does not appear here.",
        flags: PAGED,
    },
    Verb {
        name: "credential-utxos",
        question: "Which UTxOs sit at these payment credentials",
        usage:
            "at-chain credential-utxos <hex-credential>… [--limit N] [--offset N] [--all] [--json]",
        notes: "Extended UTxOs for 28-byte hex payment credentials (`POST /credential_utxos`). \
                This is the script-credential form of `utxos`; it accepts no bech32 address.",
        flags: UTXO,
    },
    Verb {
        name: "tx",
        question: "What is this transaction",
        usage: "at-chain tx <hash>… [--no-inputs] [--no-assets] [--scripts] [--json]",
        notes: "Block, time, fee, size, input and output counts and totals, and the minted-asset \
                count (`POST /tx_info`). Inputs and assets are requested by default because the \
                summary uses them; `--no-inputs`/`--no-assets` shrink the response.",
        flags: TX_INFO,
    },
    Verb {
        name: "tx-utxos",
        question: "Which UTxOs does this transaction reference",
        usage: "at-chain tx-utxos <hash>… [--limit N] [--offset N] [--all] [--json]",
        notes: "The input and output counts and ADA totals per transaction (`POST /tx_utxos`). \
                `--json` carries the refs, addresses and assets.",
        flags: PAGED,
    },
    Verb {
        name: "tx-cbor",
        question: "What are this transaction's bytes",
        usage: "at-chain tx-cbor <hash>… [--json] [--keyless]",
        notes: "The CBOR hex (`POST /tx_cbor`), wrapped at 120 characters. Hex lines are \
                contiguous; join them before decoding. The answer is capped at 256 KiB.",
        flags: CORE,
    },
    Verb {
        name: "tx-metadata",
        question: "What metadata does this transaction carry",
        usage: "at-chain tx-metadata <hash>… [--network N] [--keyless]",
        notes: "The metadata JSON (`POST /tx_metadata`). Metadata is label-keyed and arbitrary, \
                so the answer is raw JSON rather than a projection.",
        flags: RAW,
    },
    Verb {
        name: "tx-status",
        question: "Is this transaction confirmed",
        usage: "at-chain tx-status <hash>… [--json] [--keyless]",
        notes: "Confirmations, block height and block time (`POST /tx_status`). An unknown hash \
                is an empty array, which exits 1 rather than pretending the transaction is \
                unconfirmed.",
        flags: CORE,
    },
    Verb {
        name: "utxo",
        question: "What is this output",
        usage: "at-chain utxo <tx#index>… [--json] [--keyless]",
        notes: "One UTxO per out-ref (`POST /utxo_info`). `tx:index` is accepted and printed as \
                the Koios `tx#index` form.",
        flags: CORE,
    },
    Verb {
        name: "datum",
        question: "What is this datum",
        usage: "at-chain datum <hash>… [--json] [--keyless]",
        notes: "Datum CBOR and its length (`POST /datum_info`). `--json` carries Koios's Plutus \
                JSON rendering, which is arbitrary and is not projected.",
        flags: CORE,
    },
    Verb {
        name: "asset",
        question: "What is this asset",
        usage: "at-chain asset <policy.name>… [--json] [--keyless]",
        notes: "Supply, mint/burn counts, creation time and fingerprint (`POST /asset_info`). \
                The unit is `policy.name`; a name that is not hex is encoded as its UTF-8 hex, \
                so `policy.Aliens` and `policy.416c69656e73` are the same request.",
        flags: CORE,
    },
    Verb {
        name: "policy",
        question: "What is in this policy",
        usage: "at-chain policy <policy> [--mints | --addresses] [--limit N] [--offset N] [--all]",
        notes: "Asset details for one policy (`GET /policy_asset_info`). `--mints` switches to \
                per-asset mint counts (`GET /policy_asset_mints`) and `--addresses` to holder \
                rows (`GET /policy_asset_addresses`); all three take one policy. A large policy's \
                holder walk can exceed the request timeout — narrow it with --limit or --offset.",
        flags: POLICY,
    },
    Verb {
        name: "account",
        question: "What does this stake key hold",
        usage: "at-chain account <stake>… [--json] [--keyless]",
        notes: "Balance, UTxO balance, rewards, delegation and status (`POST /account_info`). \
                Up to 100 stake addresses.",
        flags: CORE,
    },
    Verb {
        name: "account-utxos",
        question: "Which UTxOs sit under these stake keys",
        usage: "at-chain account-utxos <stake>… [--limit N] [--offset N] [--all] [--json]",
        notes: "Extended UTxOs for up to 100 stake addresses (`POST /account_utxos`).",
        flags: UTXO,
    },
    Verb {
        name: "account-assets",
        question: "Which assets sit under these stake keys",
        usage: "at-chain account-assets <stake>… [--limit N] [--offset N] [--all] [--json]",
        notes: "Aggregated native-asset holdings per stake address (`POST /account_assets`).",
        flags: PAGED,
    },
    Verb {
        name: "block",
        question: "What is this block",
        usage: "at-chain block <hash>… | <height> [--json] [--keyless]",
        notes: "Hash, height, epoch, slot, time, transaction count and size (`POST /block_info` \
                for hashes, `GET /blocks` for a height). A height takes one target; hashes batch.",
        flags: CORE,
    },
    Verb {
        name: "block-txs",
        question: "Which transactions are in this block",
        usage: "at-chain block-txs <hash>… | <height> [--limit N] [--offset N] [--all] [--json]",
        notes: "Transaction hashes in the requested blocks (`POST /block_txs`). A height is \
                resolved to its hash first and takes one target; hashes batch.",
        flags: PAGED,
    },
    Verb {
        name: "script",
        question: "What is this script",
        usage: "at-chain script <hash>… [--json] [--keyless]",
        notes: "Script type and serialised size (`POST /script_info`).",
        flags: CORE,
    },
    Verb {
        name: "query",
        question: "Anything else Koios answers",
        usage: "at-chain query <endpoint> [--post] [--body JSON | --body-file PATH] [--query k=v]…",
        notes: "The long tail: one Koios endpoint name, GET by default. It cannot choose a host, \
                add a header or follow a redirect, and /submittx is refused. The answer is raw \
                JSON, bounded to 64 KiB; narrow it at the source with --select or --limit.",
        flags: QUERY_FLAGS,
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "read-only Cardano chain enquiry through Koios",
    promise: "Read-only: HTTPS GET/POST to Koios only, /submittx refused, no writes, no \
              subprocess, and the key is never printed.",
    verbs: VERBS,
};

pub fn find(name: &str) -> Option<&'static Verb> {
    at_core::catalogue::find(VERBS, name)
}

pub fn names() -> String {
    at_core::catalogue::names(VERBS)
}

pub fn verb_lines() -> Vec<String> {
    at_core::catalogue::verb_lines(VERBS)
}

pub fn help_all() -> String {
    at_core::catalogue::help_all(&SPEC)
}

pub fn help_for(verb: &Verb) -> String {
    at_core::catalogue::help_for(&SPEC, verb)
}
