//! The flags a verb accepts, and the sets the verb tables share.

use at_core::catalogue::{Flag, SUMMARY};

pub const NETWORK: Flag = Flag {
    name: "--network",
    takes_value: true,
    value: "mainnet|preprod|preview|guild",
    one_line: "Which Koios network; default mainnet",
};
pub const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Rows to ask for; default 20, ceiling 1000. Clamping is announced",
};
pub const OFFSET: Flag = Flag {
    name: "--offset",
    takes_value: true,
    value: "N",
    one_line: "Rows to skip; the previous answer's row count is the next offset",
};
pub const ORDER: Flag = Flag {
    name: "--order",
    takes_value: true,
    value: "COL[.asc|.desc]…",
    one_line: "PostgREST order, e.g. block_time.desc",
};
pub const SELECT: Flag = Flag {
    name: "--select",
    takes_value: true,
    value: "COL,…",
    one_line: "Columns to ask Koios for, e.g. tx_hash,block_height",
};
pub const ALL: Flag = Flag {
    name: "--all",
    takes_value: false,
    value: "",
    one_line: "Follow pages at the 1000-row ceiling, up to a 10-page cap",
};
pub const JSON: Flag = Flag {
    name: "--json",
    takes_value: false,
    value: "",
    one_line: "The response body as JSON, bounded, instead of the summary",
};
pub const KEYLESS: Flag = Flag {
    name: "--keyless",
    takes_value: false,
    value: "",
    one_line: "Ignore a configured key and use the free tier",
};
pub const NO_EXTENDED: Flag = Flag {
    name: "--no-extended",
    takes_value: false,
    value: "",
    one_line: "Omit inline datums and script references (a smaller response)",
};
pub const NO_INPUTS: Flag = Flag {
    name: "--no-inputs",
    takes_value: false,
    value: "",
    one_line: "Omit inputs from /tx_info; the summary says they were not requested",
};
pub const NO_ASSETS: Flag = Flag {
    name: "--no-assets",
    takes_value: false,
    value: "",
    one_line: "Omit asset lists from /tx_info",
};
pub const SCRIPTS: Flag = Flag {
    name: "--scripts",
    takes_value: false,
    value: "",
    one_line: "Include scripts in /tx_info",
};
pub const EPOCH: Flag = Flag {
    name: "--epoch",
    takes_value: true,
    value: "N",
    one_line: "Restrict a history to one epoch (the endpoint's own `_epoch_no` filter)",
};
pub const HISTORY: Flag = Flag {
    name: "--history",
    takes_value: false,
    value: "",
    one_line: "Include every historical transaction, not only the non-empty ones",
};
pub const POST: Flag = Flag {
    name: "--post",
    takes_value: false,
    value: "",
    one_line: "Use POST for `query`; without it the request is a GET",
};
pub const BODY: Flag = Flag {
    name: "--body",
    takes_value: true,
    value: "JSON",
    one_line: "Inline JSON request body for `query --post`",
};
pub const BODY_FILE: Flag = Flag {
    name: "--body-file",
    takes_value: true,
    value: "<path>",
    one_line: "Read the JSON request body from a file (max 1 MiB)",
};
pub const QUERY: Flag = Flag {
    name: "--query",
    takes_value: true,
    value: "key=value",
    one_line: "A query parameter; repeatable",
};
pub const SCENARIO_FILE: Flag = Flag {
    name: "--scenario",
    takes_value: true,
    value: "<file.dsl>",
    one_line: "The scenario file to run",
};
pub const CHECK: Flag = Flag {
    name: "--check",
    takes_value: false,
    value: "",
    one_line: "Validate the file and resolve every reference; opens no socket",
};
pub const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

/// A single fact for one target: no paging.
pub const CORE: &[Flag] = &[NETWORK, JSON, KEYLESS, HOW];
/// A list: paging, ordering and a frame.
pub const PAGED: &[Flag] = &[
    NETWORK, LIMIT, OFFSET, ORDER, SELECT, ALL, SUMMARY, JSON, KEYLESS, HOW,
];
/// A list whose response carries inline data the caller may not want to pay for.
pub const PAGED_EXTENDED: &[Flag] = &[
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
/// A list filtered by an epoch the caller names.
pub const PAGED_EPOCH: &[Flag] = &[
    NETWORK, LIMIT, OFFSET, ORDER, SELECT, ALL, SUMMARY, EPOCH, JSON, KEYLESS, HOW,
];
/// `/tx_info`: the response grows with the parts the caller asks for.
pub const TX_INFO: &[Flag] = &[NETWORK, NO_INPUTS, NO_ASSETS, SCRIPTS, JSON, KEYLESS, HOW];
/// `/asset_txs`: the endpoint's own `_history` switch.
pub const ASSET_TXS: &[Flag] = &[
    NETWORK, LIMIT, OFFSET, ORDER, SELECT, ALL, SUMMARY, HISTORY, JSON, KEYLESS, HOW,
];
/// The escape hatch, which owns the whole request.
pub const QUERY_FLAGS: &[Flag] = &[
    NETWORK, POST, BODY, BODY_FILE, QUERY, LIMIT, OFFSET, KEYLESS, HOW,
];
/// A local verb: no request at all.
pub const LOCAL: &[Flag] = &[NETWORK, KEYLESS, HOW];
/// A scenario: the file names its own steps, so the flags are about the run as a whole.
pub const SCENARIO_FLAGS: &[Flag] = &[SCENARIO_FILE, CHECK, NETWORK, KEYLESS, HOW];
/// A verb that prints a table from this binary's own data.
pub const PLAIN: &[Flag] = &[HOW];
