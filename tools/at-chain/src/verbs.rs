//! The verbs: one request each, a projection or the raw body, and the bounds that make the
//! answer safe to quote.

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::contract::{self, Exit, Fail, Outcome, Report};
use crate::key;
use crate::koios::{self, Client, Method, Network};
use crate::model::{
    AccountInfo, AddressInfo, AddressTxRow, AssetHoldingRow, AssetInfoRow, BlockInfo, BlockTxRow,
    DatumRow, Record, Row, ScriptInfo, Tip, TxCborRow, TxInfo, TxStatusRow, TxUtxosRow, UtxoRow,
};

pub const MAX_BATCH: usize = 100;
pub const MAX_JSON_OUTPUT: usize = 64 * 1024;
pub const MAX_HEX_OUTPUT: usize = 256 * 1024;
const MAX_BODY_BYTES: u64 = 1024 * 1024;
const PAGE_CEILING: usize = 1000;
const MAX_PAGES: usize = 10;

pub struct Options {
    pub network: Network,
    pub json: bool,
    pub keyless: bool,
    pub summary: bool,
    pub all: bool,
    pub limit: usize,
    pub limit_clamped_from: Option<usize>,
    pub limit_explicit: bool,
    pub offset: usize,
    pub offset_explicit: bool,
    pub order: Option<String>,
    pub select: Option<String>,
    pub extended: bool,
    pub inputs: bool,
    pub assets: bool,
    pub scripts: bool,
    pub mints: bool,
    pub addresses: bool,
    pub post: bool,
    pub body: Option<String>,
    pub body_file: Option<String>,
    pub query: Vec<(String, String)>,
}

pub fn run(verb: &str, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    if verb == "auth" {
        return auth(options);
    }
    if verb == "query" {
        return query(targets, options);
    }

    let client = Client::new(options.network, key::load(options.keyless)?);
    match verb {
        "tip" => tip(&client, options),
        "address" => records::<AddressInfo>(
            &client,
            "address",
            "address_info",
            Method::Post,
            json_body(&AddressesBody {
                addresses: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "utxos" => paged::<UtxoRow>(
            &client,
            "utxos",
            "address_utxos",
            Method::Post,
            json_body(&ExtendedAddressesBody {
                addresses: targets.to_vec(),
                extended: options.extended,
            })?,
            options,
            targets,
        ),
        "txs" => paged::<AddressTxRow>(
            &client,
            "txs",
            "address_txs",
            Method::Post,
            json_body(&AddressesBody {
                addresses: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "assets" => paged::<AssetHoldingRow>(
            &client,
            "assets",
            "address_assets",
            Method::Post,
            json_body(&AddressesBody {
                addresses: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "credential-utxos" => paged::<UtxoRow>(
            &client,
            "credential-utxos",
            "credential_utxos",
            Method::Post,
            json_body(&CredentialUtxosBody {
                payment_credentials: targets.to_vec(),
                extended: options.extended,
            })?,
            options,
            targets,
        ),
        "tx" => tx(&client, targets, options),
        "tx-utxos" => paged::<TxUtxosRow>(
            &client,
            "tx-utxos",
            "tx_utxos",
            Method::Post,
            json_body(&TxHashesBody {
                tx_hashes: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "tx-cbor" => tx_cbor(&client, targets, options),
        "tx-metadata" => raw_verb(
            &client,
            "tx-metadata",
            "tx_metadata",
            json_body(&TxHashesBody {
                tx_hashes: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "tx-status" => rows_once::<TxStatusRow>(
            &client,
            "tx-status",
            "tx_status",
            Method::Post,
            json_body(&TxHashesBody {
                tx_hashes: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "utxo" => rows_once::<UtxoRow>(
            &client,
            "utxo",
            "utxo_info",
            Method::Post,
            json_body(&UtxoRefsBody {
                utxo_refs: targets
                    .iter()
                    .map(|target| parse_utxo_ref(target))
                    .collect::<Result<Vec<_>, _>>()?,
                extended: true,
            })?,
            options,
            targets,
        ),
        "datum" => rows_once::<DatumRow>(
            &client,
            "datum",
            "datum_info",
            Method::Post,
            json_body(&DatumHashesBody {
                datum_hashes: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "asset" => records::<AssetInfoRow>(
            &client,
            "asset",
            "asset_info",
            Method::Post,
            json_body(&AssetListBody {
                asset_list: targets
                    .iter()
                    .map(|target| parse_asset_unit(target))
                    .collect::<Result<Vec<_>, _>>()?,
            })?,
            options,
            targets,
        ),
        "policy" => policy(&client, targets, options),
        "account" => records::<AccountInfo>(
            &client,
            "account",
            "account_info",
            Method::Post,
            json_body(&StakeAddressesBody {
                stake_addresses: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "account-utxos" => paged::<UtxoRow>(
            &client,
            "account-utxos",
            "account_utxos",
            Method::Post,
            json_body(&ExtendedStakeAddressesBody {
                stake_addresses: targets.to_vec(),
                extended: options.extended,
            })?,
            options,
            targets,
        ),
        "account-assets" => paged::<AssetHoldingRow>(
            &client,
            "account-assets",
            "account_assets",
            Method::Post,
            json_body(&StakeAddressesBody {
                stake_addresses: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        "block" => block(&client, targets, options),
        "block-txs" => block_txs(&client, targets, options),
        "script" => records::<ScriptInfo>(
            &client,
            "script",
            "script_info",
            Method::Post,
            json_body(&ScriptHashesBody {
                script_hashes: targets.to_vec(),
            })?,
            options,
            targets,
        ),
        other => Err(Fail::usage(format!(
            "`{other}` is catalogued but not implemented"
        ))),
    }
}

fn auth(options: &Options) -> Result<Outcome, Fail> {
    let auth = key::load(options.keyless)?;
    let mut report = Report::new();
    report.header(crate::TOOL, "auth", options.network.label(), "Koios");
    report.content(format!("{:<10} {}", "endpoint", options.network.base_url()));
    report.content(format!("{:<10} {}", "key", auth.describe()));
    report.content(format!(
        "{:<10} {}",
        "env",
        if std::env::var_os("KOIOS_API_KEY").is_some() {
            "KOIOS_API_KEY is set"
        } else {
            "KOIOS_API_KEY is unset"
        }
    ));
    if let Some(caveat) = auth.caveat() {
        report.bound(caveat);
    }
    report.bound("no request was made");
    report.next(
        format!("{} tip", crate::TOOL),
        "confirm the endpoint answers",
    );
    Ok(Outcome::from_report(report))
}

fn query(targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    let endpoint = &targets[0];
    if koios::refused_endpoint(endpoint) {
        return Err(Fail::refused(format!(
            "/{endpoint} writes to the chain; at-chain answers reads only"
        )));
    }
    if !koios::valid_endpoint(endpoint) {
        return Err(Fail::usage(format!(
            "`{endpoint}` is not a Koios endpoint name · use lowercase letters, digits and `_`"
        )));
    }

    let client = Client::new(options.network, key::load(options.keyless)?);
    let mut params = options.query.clone();
    if options.limit_explicit && !params.iter().any(|(key, _)| key == "limit") {
        params.push(("limit".to_string(), options.limit.to_string()));
    }
    if options.offset_explicit && !params.iter().any(|(key, _)| key == "offset") {
        params.push(("offset".to_string(), options.offset.to_string()));
    }

    let body = request_body(options)?;
    let bytes = if options.post {
        client.post(endpoint, &params, &body)?
    } else {
        client.get(endpoint, &params)?
    };
    let report = report_for(&client, "query", endpoint, &[]);
    Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT))
}

fn tip(client: &Client, options: &Options) -> Result<Outcome, Fail> {
    let bytes = client.get("tip", &[])?;
    let mut report = report_for(client, "tip", "tip", &[]);
    if options.json {
        return Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT));
    }
    let rows: Vec<Tip> = koios::parse_rows(&bytes, "tip")?;
    if rows.is_empty() {
        report.bound("Koios returned no tip; the indexer has not caught up");
        return Ok(Outcome {
            report,
            exit: Exit::Nothing,
        });
    }
    for row in &rows {
        for line in row.lines() {
            report.content(line);
        }
    }
    Ok(Outcome::from_report(report))
}

fn tx(client: &Client, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    let body = json_body(&TxInfoBody {
        tx_hashes: targets.to_vec(),
        inputs: options.inputs,
        assets: options.assets,
        scripts: options.scripts,
    })?;
    let bytes = client.post("tx_info", &[], &body)?;
    let report = report_for(client, "tx", "tx_info", targets);
    if options.json {
        return Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT));
    }
    let rows: Vec<TxInfo> = koios::parse_rows(&bytes, "tx_info")?;
    Ok(records_with(report, &rows, |row| {
        row.lines_with(options.inputs, options.assets)
    }))
}

fn tx_cbor(client: &Client, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    let body = json_body(&TxHashesBody {
        tx_hashes: targets.to_vec(),
    })?;
    let bytes = client.post("tx_cbor", &[], &body)?;
    let mut report = report_for(client, "tx-cbor", "tx_cbor", targets);
    if options.json {
        return Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT));
    }
    let rows: Vec<TxCborRow> = koios::parse_rows(&bytes, "tx_cbor")?;
    let exit = if rows.is_empty() {
        Exit::Nothing
    } else {
        Exit::Results
    };
    let mut printed = 0usize;
    for row in &rows {
        report.content(format!("{}  {} hex chars", row.tx_hash, row.cbor.len()));
        if printed >= MAX_HEX_OUTPUT {
            continue;
        }
        let room = MAX_HEX_OUTPUT - printed;
        let take = row.cbor.len().min(room);
        for chunk in row.cbor.as_bytes()[..take].chunks(120) {
            report.content(String::from_utf8_lossy(chunk));
        }
        printed += take;
        if take < row.cbor.len() {
            report.bound(format!(
                "cut at {MAX_HEX_OUTPUT} hex chars; {} remain · narrow the batch",
                row.cbor.len() - take
            ));
        }
    }
    if rows.is_empty() {
        report.bound(
            "Koios returned an empty array; the identifier may be unknown, or the indexer may \
             not have caught up",
        );
    }
    Ok(Outcome { report, exit })
}

fn policy(client: &Client, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    if targets.len() != 1 {
        return Err(Fail::usage(
            "`policy` takes one policy · the per-policy endpoints take one `_asset_policy` each",
        ));
    }
    let params = vec![("_asset_policy".to_string(), targets[0].clone())];
    if options.mints {
        return paged_with_params::<AssetInfoRow>(
            client,
            "policy",
            "policy_asset_mints",
            Method::Get,
            Vec::new(),
            params,
            options,
            targets,
        );
    }
    if options.addresses {
        return paged_with_params::<AssetHoldingRow>(
            client,
            "policy",
            "policy_asset_addresses",
            Method::Get,
            Vec::new(),
            params,
            options,
            targets,
        );
    }
    paged_with_params::<AssetInfoRow>(
        client,
        "policy",
        "policy_asset_info",
        Method::Get,
        Vec::new(),
        params,
        options,
        targets,
    )
}

fn block(client: &Client, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    match block_mode(targets)? {
        BlockMode::Hashes(hashes) => records::<BlockInfo>(
            client,
            "block",
            "block_info",
            Method::Post,
            json_body(&BlockHashesBody {
                block_hashes: hashes,
            })?,
            options,
            targets,
        ),
        BlockMode::Height(height) => records_query::<BlockInfo>(
            client,
            "block",
            "blocks",
            Method::Get,
            Vec::new(),
            &[("block_height".to_string(), format!("eq.{height}"))],
            options,
            targets,
        ),
    }
}

fn block_txs(client: &Client, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    let hashes = match block_mode(targets)? {
        BlockMode::Hashes(hashes) => hashes,
        BlockMode::Height(height) => {
            let bytes = client.get(
                "blocks",
                &[("block_height".to_string(), format!("eq.{height}"))],
            )?;
            let blocks: Vec<BlockInfo> = koios::parse_rows(&bytes, "blocks")?;
            match blocks.as_slice() {
                [] => {
                    let mut report = report_for(client, "block-txs", "block_txs", targets);
                    report.bound("0 rows · no block at that height");
                    return Ok(Outcome {
                        report,
                        exit: Exit::Nothing,
                    });
                }
                [block] => vec![block.hash.clone()],
                _ => {
                    return Err(Fail::environment(
                        "Koios returned more than one block for a block height",
                    ));
                }
            }
        }
    };
    paged::<BlockTxRow>(
        client,
        "block-txs",
        "block_txs",
        Method::Post,
        json_body(&BlockHashesBody {
            block_hashes: hashes,
        })?,
        options,
        targets,
    )
}

fn records<T: Record + DeserializeOwned>(
    client: &Client,
    verb: &str,
    endpoint: &str,
    method: Method,
    body: Vec<u8>,
    options: &Options,
    targets: &[String],
) -> Result<Outcome, Fail> {
    records_query::<T>(client, verb, endpoint, method, body, &[], options, targets)
}

// The fixed query parameters are the endpoint's own; the caller's `--order`/`--select` are
// added by the paged helpers, and a non-paged endpoint that needs one passes it here.
#[allow(clippy::too_many_arguments)]
fn records_query<T: Record + DeserializeOwned>(
    client: &Client,
    verb: &str,
    endpoint: &str,
    method: Method,
    body: Vec<u8>,
    query: &[(String, String)],
    options: &Options,
    targets: &[String],
) -> Result<Outcome, Fail> {
    let bytes = call_bytes(client, endpoint, method, Some(&body), query)?;
    let report = report_for(client, verb, endpoint, targets);
    if options.json {
        return Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT));
    }
    let rows: Vec<T> = koios::parse_rows(&bytes, endpoint)?;
    Ok(records_with(report, &rows, |row| row.lines()))
}

fn rows_once<T: Row + DeserializeOwned>(
    client: &Client,
    verb: &str,
    endpoint: &str,
    method: Method,
    body: Vec<u8>,
    options: &Options,
    targets: &[String],
) -> Result<Outcome, Fail> {
    let bytes = call_bytes(client, endpoint, method, Some(&body), &[])?;
    let mut report = report_for(client, verb, endpoint, targets);
    if options.json {
        return Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT));
    }
    let rows: Vec<T> = koios::parse_rows(&bytes, endpoint)?;
    let exit = if rows.is_empty() {
        Exit::Nothing
    } else {
        Exit::Results
    };
    for row in &rows {
        report.content(row.line());
    }
    if rows.is_empty() {
        report.bound(
            "Koios returned an empty array; the identifier may be unknown, or the indexer may \
             not have caught up",
        );
    }
    Ok(Outcome { report, exit })
}

fn records_with<T>(mut report: Report, rows: &[T], lines: impl Fn(&T) -> Vec<String>) -> Outcome {
    let exit = if rows.is_empty() {
        Exit::Nothing
    } else {
        Exit::Results
    };
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            report.note("");
        }
        for line in lines(row) {
            report.content(line);
        }
    }
    if rows.is_empty() {
        report.bound(
            "Koios returned an empty array; the identifier may be unknown, or the indexer may \
             not have caught up",
        );
    }
    Outcome { report, exit }
}

fn paged<T: Row + DeserializeOwned>(
    client: &Client,
    verb: &str,
    endpoint: &str,
    method: Method,
    body: Vec<u8>,
    options: &Options,
    targets: &[String],
) -> Result<Outcome, Fail> {
    paged_with_params::<T>(
        client,
        verb,
        endpoint,
        method,
        body,
        Vec::new(),
        options,
        targets,
    )
}

// The parameter list is the call: verb, endpoint, method, body and the endpoint's fixed
// query parameters, beside the caller's options and targets. A struct here would only move
// the same fields one line down.
#[allow(clippy::too_many_arguments)]
fn paged_with_params<T: Row + DeserializeOwned>(
    client: &Client,
    verb: &str,
    endpoint: &str,
    method: Method,
    body: Vec<u8>,
    params: Vec<(String, String)>,
    options: &Options,
    targets: &[String],
) -> Result<Outcome, Fail> {
    let mut base = base_query(options);
    base.extend(params);
    let report = report_for(client, verb, endpoint, targets);
    if options.json {
        let bytes = page_bytes(
            client,
            endpoint,
            method,
            Some(&body),
            &base,
            options.limit,
            options.offset,
        )?;
        return Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT));
    }

    let (rows, page) = fetch_pages::<T>(client, endpoint, method, Some(&body), &base, options)?;
    Ok(rows_outcome(report, verb, targets, options, &rows, &page))
}

fn raw_verb(
    client: &Client,
    verb: &str,
    endpoint: &str,
    body: Vec<u8>,
    _options: &Options,
    targets: &[String],
) -> Result<Outcome, Fail> {
    let bytes = call_bytes(client, endpoint, Method::Post, Some(&body), &[])?;
    let report = report_for(client, verb, endpoint, targets);
    Ok(raw_json(report, &bytes, MAX_JSON_OUTPUT))
}

fn call_bytes(
    client: &Client,
    endpoint: &str,
    method: Method,
    body: Option<&[u8]>,
    query: &[(String, String)],
) -> Result<Vec<u8>, Fail> {
    match method {
        Method::Get => client.get(endpoint, query),
        Method::Post => client.post(endpoint, query, body.unwrap_or(&b"{}"[..])),
    }
}

fn page_bytes(
    client: &Client,
    endpoint: &str,
    method: Method,
    body: Option<&[u8]>,
    base: &[(String, String)],
    limit: usize,
    offset: usize,
) -> Result<Vec<u8>, Fail> {
    let mut query = base.to_vec();
    query.push(("limit".to_string(), limit.to_string()));
    query.push(("offset".to_string(), offset.to_string()));
    call_bytes(client, endpoint, method, body, &query)
}

struct PageInfo {
    pages: usize,
    rows: usize,
    limit: usize,
    start_offset: usize,
    last_page_len: usize,
    capped: bool,
}

impl PageInfo {
    fn has_more(&self, options: &Options) -> bool {
        !options.all && self.last_page_len == self.limit
    }

    fn next_offset(&self) -> usize {
        self.start_offset + self.rows
    }

    fn next_flags(&self, options: &Options) -> Vec<String> {
        let mut flags = Vec::new();
        if options.network != Network::Mainnet {
            flags.push(format!("--network {}", options.network.label()));
        }
        if options.json {
            flags.push("--json".to_string());
        }
        if options.keyless {
            flags.push("--keyless".to_string());
        }
        if options.summary {
            flags.push("--summary".to_string());
        }
        if options.all {
            flags.push("--all".to_string());
        } else {
            flags.push(format!("--limit {}", self.limit));
        }
        if let Some(order) = &options.order {
            flags.push(format!("--order {order}"));
        }
        if let Some(select) = &options.select {
            flags.push(format!("--select {select}"));
        }
        if !options.extended {
            flags.push("--no-extended".to_string());
        }
        if options.mints {
            flags.push("--mints".to_string());
        }
        if options.addresses {
            flags.push("--addresses".to_string());
        }
        flags.push(format!("--offset {}", self.next_offset()));
        flags
    }
}

fn fetch_pages<T: DeserializeOwned>(
    client: &Client,
    endpoint: &str,
    method: Method,
    body: Option<&[u8]>,
    base: &[(String, String)],
    options: &Options,
) -> Result<(Vec<T>, PageInfo), Fail> {
    let limit = if options.all {
        PAGE_CEILING
    } else {
        options.limit
    };
    let mut offset = options.offset;
    let mut pages = 0;
    let mut rows = Vec::new();

    loop {
        let bytes = page_bytes(client, endpoint, method, body, base, limit, offset)?;
        let page: Vec<T> = koios::parse_rows(&bytes, endpoint)?;
        let page_len = page.len();
        rows.extend(page);
        pages += 1;
        offset += page_len;

        if !options.all || page_len < PAGE_CEILING || pages >= MAX_PAGES {
            let capped = options.all && pages >= MAX_PAGES && page_len == PAGE_CEILING;
            return Ok((
                rows,
                PageInfo {
                    pages,
                    rows: offset - options.offset,
                    limit,
                    start_offset: options.offset,
                    last_page_len: page_len,
                    capped,
                },
            ));
        }
    }
}

fn rows_outcome<T: Row>(
    mut report: Report,
    verb: &str,
    targets: &[String],
    options: &Options,
    rows: &[T],
    page: &PageInfo,
) -> Outcome {
    if let Some(asked) = options.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {}", options.limit));
    }
    report.bound(format!(
        "{} rows · limit {} · offset {}{}",
        rows.len(),
        page.limit,
        page.start_offset,
        if page.pages > 1 {
            format!(" · {} pages", page.pages)
        } else {
            String::new()
        }
    ));
    let exit = if rows.is_empty() {
        Exit::Nothing
    } else {
        Exit::Results
    };

    if options.summary {
        report.bound(if rows.is_empty() {
            "nothing to show"
        } else {
            "rows not shown (--summary)"
        });
        return Outcome { report, exit };
    }

    for row in rows {
        report.content(row.line());
    }
    if rows.is_empty() {
        report.bound(
            "Koios returned an empty array; the identifier may be unknown, or the indexer may \
             not have caught up",
        );
    }
    if page.has_more(options) {
        report.next(
            contract::again(crate::TOOL, verb, targets, &page.next_flags(options), None),
            "the next page",
        );
    }
    if page.capped {
        report.bound(format!(
            "stopped at the {MAX_PAGES}-page cap · {} rows read",
            page.rows
        ));
        report.next(
            contract::again(crate::TOOL, verb, targets, &page.next_flags(options), None),
            "continue past the page cap",
        );
    }
    Outcome { report, exit }
}

fn report_for(client: &Client, verb: &str, endpoint: &str, targets: &[String]) -> Report {
    let mut report = Report::new();
    let target = if targets.is_empty() {
        format!("Koios /{endpoint}")
    } else {
        format!("Koios /{endpoint} · {}", target_label(targets))
    };
    report.header(crate::TOOL, verb, client.network().label(), &target);
    report.bound(format!("auth: {}", client.auth().describe()));
    if let Some(caveat) = client.auth().caveat() {
        report.bound(caveat);
    }
    report
}

fn target_label(targets: &[String]) -> String {
    match targets {
        [] => "no target".to_string(),
        [only] => only.clone(),
        [first, ..] => format!("{first} +{} more", targets.len() - 1),
    }
}

fn base_query(options: &Options) -> Vec<(String, String)> {
    let mut query = Vec::new();
    if let Some(order) = &options.order {
        query.push(("order".to_string(), order.clone()));
    }
    if let Some(select) = &options.select {
        query.push(("select".to_string(), select.clone()));
    }
    query
}

fn raw_json(mut report: Report, bytes: &[u8], cap: usize) -> Outcome {
    report.bound(format!("raw JSON · {} bytes", bytes.len()));
    let shown = &bytes[..bytes.len().min(cap)];
    if shown.len() < bytes.len() {
        report.bound(format!(
            "cut at {cap} bytes of {} · narrow with --select/--limit, page with --offset, or \
             ask for the exact shape with --json on a typed verb",
            bytes.len()
        ));
    }
    for line in json_lines(shown) {
        report.content(line);
    }
    let exit = if bytes.is_empty() {
        Exit::Nothing
    } else {
        Exit::Results
    };
    Outcome { report, exit }
}

/// Break JSON at commas and opening delimiters outside strings, so a compact body becomes
/// readable lines without changing a byte inside a value. No `Value` tree is built.
fn json_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for character in text.chars() {
        current.push(character);
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' | ',' => {
                current.push('\n');
                let done = std::mem::take(&mut current);
                if !done.trim().is_empty() {
                    lines.push(done);
                }
            }
            _ => {}
        }
        if current.chars().count() >= 200 {
            current.push('\n');
            let done = std::mem::take(&mut current);
            if !done.trim().is_empty() {
                lines.push(done);
            }
        }
    }

    if !current.trim().is_empty() {
        lines.push(current);
    }
    lines
}

fn request_body(options: &Options) -> Result<Vec<u8>, Fail> {
    if let Some(body) = &options.body {
        return validate_body(body.as_bytes());
    }
    if let Some(path) = &options.body_file {
        let bytes = std::fs::read(path)
            .map_err(|error| Fail::environment(format!("cannot read {path}: {error}")))?;
        if bytes.len() as u64 > MAX_BODY_BYTES {
            return Err(Fail::usage(format!(
                "{path} is {} bytes; the request body is capped at {MAX_BODY_BYTES}",
                bytes.len()
            )));
        }
        return validate_body(&bytes);
    }
    Ok(b"{}".to_vec())
}

fn validate_body(bytes: &[u8]) -> Result<Vec<u8>, Fail> {
    serde_json::from_slice::<serde::de::IgnoredAny>(bytes)
        .map_err(|error| Fail::usage(format!("the request body is not JSON: {error}")))?;
    Ok(bytes.to_vec())
}

pub fn validate_targets(verb: &str, targets: &[String]) -> Result<(), Fail> {
    match verb {
        "tip" | "auth" => {
            if !targets.is_empty() {
                return Err(Fail::usage(format!("`{verb}` takes no target")));
            }
        }
        "query" => {
            let endpoint = &targets[0];
            if koios::refused_endpoint(endpoint) {
                return Err(Fail::refused(format!(
                    "/{endpoint} writes to the chain; at-chain answers reads only"
                )));
            }
            if !koios::valid_endpoint(endpoint) {
                return Err(Fail::usage(format!(
                    "`{endpoint}` is not a Koios endpoint name · use lowercase letters, digits and `_`"
                )));
            }
        }
        "address" | "utxos" | "txs" | "assets" => {
            for target in targets {
                if !target.starts_with("addr") {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a bech32 address · these verbs take `addr…` or `addr_test…`"
                    )));
                }
            }
        }
        "credential-utxos" => {
            for target in targets {
                require_hex(target, 56, "a payment credential")?;
            }
        }
        "tx" | "tx-utxos" | "tx-cbor" | "tx-metadata" | "tx-status" | "datum" => {
            for target in targets {
                require_hex(target, 64, "a 32-byte hash")?;
            }
        }
        "utxo" => {
            for target in targets {
                parse_utxo_ref(target)?;
            }
        }
        "asset" => {
            for target in targets {
                parse_asset_unit(target)?;
            }
        }
        "policy" | "script" => {
            for target in targets {
                require_hex(target, 56, "a 28-byte hash")?;
            }
        }
        "account" | "account-utxos" | "account-assets" => {
            for target in targets {
                if !target.starts_with("stake") {
                    return Err(Fail::usage(format!(
                        "`{target}` is not a stake address · these verbs take `stake…` or `stake_test…`"
                    )));
                }
            }
        }
        "block" | "block-txs" => {
            let mut kind: Option<bool> = None;
            for target in targets {
                let is_hash = match classify_block(target)? {
                    BlockRef::Hash(_) => true,
                    BlockRef::Height(_) => false,
                };
                match kind {
                    Some(previous) if previous != is_hash => {
                        return Err(Fail::usage(
                            "a block batch must be all hashes or all heights · split the request",
                        ));
                    }
                    _ => kind = Some(is_hash),
                }
            }
            if kind == Some(false) && targets.len() > 1 {
                return Err(Fail::usage(
                    "a block height takes one target · batch hashes, or ask for one height",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn require_hex(value: &str, length: usize, what: &str) -> Result<(), Fail> {
    if value.len() != length || !is_hex(value) {
        return Err(Fail::usage(format!(
            "`{value}` is not {what} · expected {length} hex characters"
        )));
    }
    Ok(())
}

fn is_hex(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `policy.name` or `policy` + hex name. A non-hex name is the UTF-8 spelling of the asset
/// name, which is how a person writes `policy.Aliens` without encoding it by hand.
fn parse_asset_unit(raw: &str) -> Result<[String; 2], Fail> {
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

fn parse_utxo_ref(raw: &str) -> Result<String, Fail> {
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

enum BlockRef {
    Hash(String),
    Height(u64),
}

fn classify_block(raw: &str) -> Result<BlockRef, Fail> {
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

#[derive(Debug)]
enum BlockMode {
    Hashes(Vec<String>),
    Height(u64),
}

fn block_mode(targets: &[String]) -> Result<BlockMode, Fail> {
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

// ---- typed request bodies ------------------------------------------------------------------

#[derive(Serialize)]
struct AddressesBody {
    #[serde(rename = "_addresses")]
    addresses: Vec<String>,
}

#[derive(Serialize)]
struct ExtendedAddressesBody {
    #[serde(rename = "_addresses")]
    addresses: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct StakeAddressesBody {
    #[serde(rename = "_stake_addresses")]
    stake_addresses: Vec<String>,
}

#[derive(Serialize)]
struct ExtendedStakeAddressesBody {
    #[serde(rename = "_stake_addresses")]
    stake_addresses: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct TxHashesBody {
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
struct CredentialUtxosBody {
    #[serde(rename = "_payment_credentials")]
    payment_credentials: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct UtxoRefsBody {
    #[serde(rename = "_utxo_refs")]
    utxo_refs: Vec<String>,
    #[serde(rename = "_extended")]
    extended: bool,
}

#[derive(Serialize)]
struct DatumHashesBody {
    #[serde(rename = "_datum_hashes")]
    datum_hashes: Vec<String>,
}

#[derive(Serialize)]
struct ScriptHashesBody {
    #[serde(rename = "_script_hashes")]
    script_hashes: Vec<String>,
}

#[derive(Serialize)]
struct AssetListBody {
    #[serde(rename = "_asset_list")]
    asset_list: Vec<[String; 2]>,
}

#[derive(Serialize)]
struct BlockHashesBody {
    #[serde(rename = "_block_hashes")]
    block_hashes: Vec<String>,
}

fn json_body<T: Serialize>(value: &T) -> Result<Vec<u8>, Fail> {
    koios::json_body(value)
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

    #[test]
    fn json_lines_break_outside_strings_only() {
        let lines = json_lines(br#"[{"tx_hash":"a,b","n":1},{"tx_hash":"c"}]"#);
        assert!(lines.iter().any(|line| line.contains("\"a,b\"")));
        assert!(lines.len() > 2);
    }
}
