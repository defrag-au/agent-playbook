//! The engine: one request path for every catalogue entry.
//!
//! A verb's catalogue entry says what to call and how to print it, so this module has no
//! per-verb branches. What it owns is the part that must not vary: the header, the auth line,
//! paging, the bounds, the `# next:` line, and the exit code that says whether anything was
//! found.

pub mod body;
pub mod local;
pub mod render;
pub mod targets;

use crate::catalogue::{Call, Paging, Param, Route, Verb};
use crate::contract::{self, Exit, Fail, Outcome, Report};
use crate::key;
use crate::koios::{self, Client, Network};
use crate::model;
use crate::scenario;

/// A page is at most 1000 rows on Koios; `--all` follows pages to this ceiling.
const PAGE_CEILING: usize = 1000;
const MAX_PAGES: usize = 10;
/// `query --body-file` is capped so a request body cannot become a file transfer.
pub const MAX_BODY_BYTES: u64 = 1024 * 1024;

pub struct Options {
    pub network: Network,
    /// `--network` was typed, rather than defaulted, so a scenario can be told which wins.
    pub network_explicit: bool,
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
    pub epoch: Option<u64>,
    pub history: bool,
    pub post: bool,
    pub body: Option<String>,
    pub body_file: Option<String>,
    pub query: Vec<(String, String)>,
    /// The file named by `run --scenario`.
    pub scenario: Option<String>,
    /// `run --check`: parse and resolve the scenario, then stop before any request.
    pub check: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            network: Network::Mainnet,
            network_explicit: false,
            json: false,
            keyless: false,
            summary: false,
            all: false,
            limit: 20,
            limit_clamped_from: None,
            limit_explicit: false,
            offset: 0,
            offset_explicit: false,
            order: None,
            select: None,
            extended: true,
            inputs: true,
            assets: true,
            scripts: false,
            epoch: None,
            history: false,
            post: false,
            body: None,
            body_file: None,
            query: Vec::new(),
            scenario: None,
            check: false,
        }
    }
}

pub fn run(verb: &'static Verb, targets: &[String], options: &Options) -> Result<Outcome, Fail> {
    run_with_answer(verb, targets, options).map(|answered| answered.outcome)
}

/// A verb's answer, and the JSON body it was read from when the route made one request. A
/// scenario needs the body to bind a value out of an answer; every other caller takes
/// [`run`] and never sees it.
pub struct Answered {
    pub outcome: Outcome,
    pub answer: Option<Vec<u8>>,
}

impl Answered {
    pub fn without_answer(outcome: Outcome) -> Self {
        Self {
            outcome,
            answer: None,
        }
    }

    pub fn with_answer(outcome: Outcome, answer: Vec<u8>) -> Self {
        Self {
            outcome,
            answer: Some(answer),
        }
    }
}

pub fn run_with_answer(
    verb: &'static Verb,
    targets: &[String],
    options: &Options,
) -> Result<Answered, Fail> {
    match &verb.route {
        Route::Auth => local::auth(options).map(Answered::without_answer),
        Route::Endpoints => local::endpoints(targets, options).map(Answered::without_answer),
        Route::Query => {
            if targets.len() != 1 {
                return Err(Fail::usage(format!(
                    "`query` takes one endpoint · {}",
                    verb.usage
                )));
            }
            local::query(targets, options)
        }
        Route::Scenario => scenario::run(options).map(Answered::without_answer),
        Route::Call(call) => call_verb(verb, call, targets, options),
    }
}

fn call_verb(
    verb: &'static Verb,
    call: &Call,
    targets: &[String],
    options: &Options,
) -> Result<Answered, Fail> {
    let client = Client::new(options.network, key::load(options.keyless)?);
    let targets = if call.param == Param::BlockRef {
        resolve_blocks(&client, targets)?
    } else {
        targets.to_vec()
    };

    let body = body::build(call.body, &targets, options)?;
    let mut params = parameters(call, &targets, options)?;
    if options.history {
        params.push(("_history".to_string(), "true".to_string()));
    }

    let report = report_for(&client, verb, call, &targets);
    let paged = call.paging == Paging::Page;

    if paged {
        let mut base = base_query(options);
        base.extend(params);
        if options.json {
            let bytes = page_bytes(
                &client,
                call,
                &base,
                options.limit,
                options.offset,
                body.as_deref(),
            )?;
            return Ok(Answered::with_answer(
                json_outcome(report, &bytes, options),
                bytes,
            ));
        }
        let (bytes, page) = fetch_pages(&client, call, &base, body.as_deref(), options)?;
        let rendered = (call.render)(&bytes, options)?;
        return Ok(Answered::with_answer(
            rows_outcome(report, verb, options, rendered, Some(&page), &targets),
            bytes,
        ));
    }

    let bytes = call_bytes(&client, call, body.as_deref(), &params)?;
    if options.json {
        return Ok(Answered::with_answer(
            json_outcome(report, &bytes, options),
            bytes,
        ));
    }
    let rendered = (call.render)(&bytes, options)?;
    Ok(Answered::with_answer(
        rows_outcome(report, verb, options, rendered, None, &targets),
        bytes,
    ))
}

/// A height is not a hash, and every block endpoint takes hashes: resolve first so the
/// request shape stays one POST.
fn resolve_blocks(client: &Client, targets: &[String]) -> Result<Vec<String>, Fail> {
    match targets::block_mode(targets)? {
        targets::BlockMode::Hashes(hashes) => Ok(hashes),
        targets::BlockMode::Height(height) => {
            let bytes = client.get(
                "blocks",
                &[("block_height".to_string(), format!("eq.{height}"))],
            )?;
            let blocks: Vec<model::BlockInfo> = koios::parse_rows(&bytes, "blocks")?;
            match blocks.as_slice() {
                [] => Err(Fail::environment(format!(
                    "Koios returned no block at height {height}; the height may be ahead of the \
                     indexer"
                ))),
                [block] => Ok(vec![block.hash.clone()]),
                _ => Err(Fail::environment(
                    "Koios returned more than one block for a block height",
                )),
            }
        }
    }
}

fn parameters(
    call: &Call,
    targets: &[String],
    options: &Options,
) -> Result<Vec<(String, String)>, Fail> {
    let mut params = Vec::new();
    if call.epoch {
        if let Some(epoch) = options.epoch {
            params.push(("_epoch_no".to_string(), epoch.to_string()));
        }
    }
    match call.param {
        Param::None | Param::BlockRef => {}
        Param::Query(name) => params.push((name.to_string(), targets[0].clone())),
        Param::OptionalQuery(name) => {
            if let Some(target) = targets.first() {
                params.push((name.to_string(), target.clone()));
            }
        }
        Param::AssetUnit => {
            let [policy, name] = targets::parse_asset_unit(&targets[0])?;
            params.push(("_asset_policy".to_string(), policy));
            params.push(("_asset_name".to_string(), name));
        }
    }
    Ok(params)
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

fn call_bytes(
    client: &Client,
    call: &Call,
    body: Option<&[u8]>,
    query: &[(String, String)],
) -> Result<Vec<u8>, Fail> {
    match call.method {
        koios::Method::Get => client.get(call.endpoint, query),
        koios::Method::Post => client.post(call.endpoint, query, body.unwrap_or(&b"{}"[..])),
    }
}

fn page_bytes(
    client: &Client,
    call: &Call,
    base: &[(String, String)],
    limit: usize,
    offset: usize,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, Fail> {
    let mut query = base.to_vec();
    query.push(("limit".to_string(), limit.to_string()));
    query.push(("offset".to_string(), offset.to_string()));
    call_bytes(client, call, body, &query)
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

    fn next_flags(&self, verb: &Verb, options: &Options) -> Vec<String> {
        let mut flags = Vec::new();
        if has(verb, "--network") && options.network != Network::Mainnet {
            flags.push(format!("--network {}", options.network.label()));
        }
        if has(verb, "--json") && options.json {
            flags.push("--json".to_string());
        }
        if has(verb, "--keyless") && options.keyless {
            flags.push("--keyless".to_string());
        }
        if has(verb, "--summary") && options.summary {
            flags.push("--summary".to_string());
        }
        if has(verb, "--all") && options.all {
            flags.push("--all".to_string());
        } else {
            flags.push(format!("--limit {}", self.limit));
        }
        if let Some(order) = &options.order {
            if has(verb, "--order") {
                flags.push(format!("--order {order}"));
            }
        }
        if let Some(select) = &options.select {
            if has(verb, "--select") {
                flags.push(format!("--select {select}"));
            }
        }
        if has(verb, "--no-extended") && !options.extended {
            flags.push("--no-extended".to_string());
        }
        if has(verb, "--epoch") {
            if let Some(epoch) = options.epoch {
                flags.push(format!("--epoch {epoch}"));
            }
        }
        if has(verb, "--history") && options.history {
            flags.push("--history".to_string());
        }
        flags.push(format!("--offset {}", self.next_offset()));
        flags
    }
}

fn has(verb: &Verb, flag: &str) -> bool {
    verb.flags.iter().any(|candidate| candidate.name == flag)
}

fn fetch_pages(
    client: &Client,
    call: &Call,
    base: &[(String, String)],
    body: Option<&[u8]>,
    options: &Options,
) -> Result<(Vec<u8>, PageInfo), Fail> {
    let limit = if options.all {
        PAGE_CEILING
    } else {
        options.limit
    };
    let mut offset = options.offset;
    let mut pages = 0;
    let mut collected: Vec<Vec<u8>> = Vec::new();
    let mut rows = 0;

    loop {
        let bytes = page_bytes(client, call, base, limit, offset, body)?;
        let page_len: Vec<serde::de::IgnoredAny> = koios::parse_rows(&bytes, call.endpoint)?;
        let page_len = page_len.len();
        collected.push(bytes);
        pages += 1;
        rows += page_len;
        offset += page_len;

        if !options.all || page_len < PAGE_CEILING || pages >= MAX_PAGES {
            let capped = options.all && pages >= MAX_PAGES && page_len == PAGE_CEILING;
            return Ok((
                concat_json_arrays(&collected),
                PageInfo {
                    pages,
                    rows,
                    limit,
                    start_offset: options.offset,
                    last_page_len: page_len,
                    capped,
                },
            ));
        }
    }
}

/// Join page bodies into one array so a projection sees every row at once: a record renderer
/// then separates records correctly across a page boundary, which concatenating its output
/// could not.
fn concat_json_arrays(pages: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::from(b"[" as &[u8]);
    let mut first = true;
    for page in pages {
        let text = String::from_utf8_lossy(page);
        let trimmed = text.trim();
        let inner = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .unwrap_or(trimmed);
        if inner.trim().is_empty() {
            continue;
        }
        if !first {
            out.push(b',');
        }
        out.extend_from_slice(inner.as_bytes());
        first = false;
    }
    out.push(b']');
    out
}

fn report_for(client: &Client, verb: &Verb, call: &Call, targets: &[String]) -> Report {
    let mut report = Report::new();
    let target = if targets.is_empty() {
        format!("Koios /{}", call.endpoint)
    } else {
        format!("Koios /{} · {}", call.endpoint, target_label(targets))
    };
    report.header(crate::TOOL, verb.name, client.network().label(), &target);
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

fn json_outcome(mut report: Report, bytes: &[u8], options: &Options) -> Outcome {
    report.bound(format!("raw JSON · {} bytes", bytes.len()));
    let rendered = render::raw(bytes, options).unwrap_or_else(|_| render::Rendered::default());
    for bound in rendered.bounds {
        report.bound(bound);
    }
    for line in rendered.lines {
        report.content(line);
    }
    Outcome {
        report,
        exit: if bytes.len() <= 2 {
            Exit::Nothing
        } else {
            Exit::Results
        },
    }
}

fn rows_outcome(
    mut report: Report,
    verb: &Verb,
    options: &Options,
    rendered: render::Rendered,
    page: Option<&PageInfo>,
    targets: &[String],
) -> Outcome {
    if let Some(asked) = options.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {}", options.limit));
    }
    if let Some(page) = page {
        // The row count is the response's, not the content lines': a record renderer prints
        // several lines per row.
        report.bound(format!(
            "{} rows · limit {} · offset {}{}",
            page.rows,
            page.limit,
            page.start_offset,
            if page.pages > 1 {
                format!(" · {} pages", page.pages)
            } else {
                String::new()
            }
        ));
    }
    for bound in rendered.bounds {
        report.bound(bound);
    }

    let found = !rendered.lines.is_empty();
    let exit = if found { Exit::Results } else { Exit::Nothing };

    if options.summary {
        report.bound(if found {
            "rows not shown (--summary)"
        } else {
            "nothing to show"
        });
        return Outcome { report, exit };
    }

    for line in rendered.lines {
        if line.is_empty() {
            report.note("");
        } else {
            report.content(line);
        }
    }
    if !found {
        report.bound(
            "Koios returned no rows; the identifier may be unknown, or the indexer may not have \
             caught up",
        );
    }

    if let Some(page) = page {
        if page.has_more(options) {
            report.next(
                contract::again(
                    crate::TOOL,
                    verb.name,
                    targets,
                    &page.next_flags(verb, options),
                    None,
                ),
                "the next page",
            );
        }
        if page.capped {
            report.bound(format!(
                "stopped at the {MAX_PAGES}-page cap · {} rows read",
                page.rows
            ));
            report.next(
                contract::again(
                    crate::TOOL,
                    verb.name,
                    targets,
                    &page.next_flags(verb, options),
                    None,
                ),
                "continue past the page cap",
            );
        }
    }

    Outcome { report, exit }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_join_into_one_array() {
        let joined =
            concat_json_arrays(&[br#"[{"a":1},{"b":2}]"#.to_vec(), br#"[{"c":3}]"#.to_vec()]);
        assert_eq!(
            String::from_utf8(joined).expect("utf8"),
            r#"[{"a":1},{"b":2},{"c":3}]"#
        );
    }

    #[test]
    fn an_empty_page_contributes_nothing() {
        let joined = concat_json_arrays(&[b"[]".to_vec(), br#"[{"a":1}]"#.to_vec()]);
        assert_eq!(String::from_utf8(joined).expect("utf8"), r#"[{"a":1}]"#);
        let empty = concat_json_arrays(&[]);
        assert_eq!(String::from_utf8(empty).expect("utf8"), "[]");
    }
}
