//! The verbs about the tool itself: `auth`, `endpoints` and `query`.

use crate::contract::{Exit, Fail, Outcome, Report};
use crate::endpoints;
use crate::key;
use crate::koios::{self, Client};
use crate::verbs::render;
use crate::verbs::{Answered, Options};

pub fn auth(options: &Options) -> Result<Outcome, Fail> {
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

pub fn endpoints(targets: &[String], _: &Options) -> Result<Outcome, Fail> {
    if targets.len() > 1 {
        return Err(Fail::usage(
            "`endpoints` takes one filter word · e.g. `at-cardano endpoints pool`",
        ));
    }
    let filter = targets.first().map(|word| word.to_ascii_lowercase());

    let mut report = Report::new();
    report.header(crate::TOOL, "endpoints", "spec", endpoints::SPEC);
    report.content(format!(
        "{:<28} {:<5} {:<24} {}",
        "endpoint", "verb", "takes", "answered by"
    ));

    let mut shown = 0usize;
    let mut hidden = 0usize;
    for row in endpoints::ENDPOINTS {
        let answered = endpoints::answered_by(row.path);
        let keep = match &filter {
            None => true,
            Some(word) => {
                row.path.contains(word.as_str())
                    || answered.to_ascii_lowercase().contains(word.as_str())
                    || row.target.contains(word.as_str())
            }
        };
        if keep {
            report.content(endpoints::line(row));
            shown += 1;
        } else {
            hidden += 1;
        }
    }

    report.bound(format!(
        "{shown} of {} endpoints{} · `query` is the long tail; `/submittx` is refused by name",
        endpoints::ENDPOINTS.len(),
        match &filter {
            Some(word) => format!(" matching `{word}`"),
            None => String::new(),
        }
    ));
    if hidden > 0 {
        report.bound(format!("{hidden} not shown · drop the filter to see them"));
    }
    Ok(Outcome::from_report(report))
}

pub fn query(targets: &[String], options: &Options) -> Result<Answered, Fail> {
    let endpoint = &targets[0];
    if koios::refused_endpoint(endpoint) {
        return Err(Fail::refused(format!(
            "/{endpoint} writes to the chain; at-cardano answers reads only"
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
    let mut report = Report::new();
    report.header(
        crate::TOOL,
        "query",
        client.network().label(),
        &format!("Koios /{endpoint}"),
    );
    report.bound(format!("auth: {}", client.auth().describe()));
    if let Some(caveat) = client.auth().caveat() {
        report.bound(caveat);
    }
    let rendered = render::raw(&bytes, options)?;
    Ok(Answered::with_answer(
        raw_outcome(report, rendered, &bytes),
        bytes,
    ))
}

fn raw_outcome(mut report: Report, rendered: render::Rendered, bytes: &[u8]) -> Outcome {
    report.bound(format!("raw JSON · {} bytes", bytes.len()));
    for bound in rendered.bounds {
        report.bound(bound);
    }
    for line in rendered.lines {
        report.content(line);
    }
    Outcome {
        report,
        exit: if bytes.is_empty() {
            Exit::Nothing
        } else {
            Exit::Results
        },
    }
}

fn request_body(options: &Options) -> Result<Vec<u8>, Fail> {
    if let Some(body) = &options.body {
        return validate_body(body.as_bytes());
    }
    if let Some(path) = &options.body_file {
        let bytes = std::fs::read(path)
            .map_err(|error| Fail::environment(format!("cannot read {path}: {error}")))?;
        if bytes.len() as u64 > crate::verbs::MAX_BODY_BYTES {
            return Err(Fail::usage(format!(
                "{path} is {} bytes; the request body is capped at {}",
                bytes.len(),
                crate::verbs::MAX_BODY_BYTES
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
