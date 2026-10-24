//! `at-crate installed` — every extracted registry copy, bounded and counted.
//!
//! The question this answers is the one `find ~/.cargo/registry/src -name 'foo-*'` answers
//! badly: not only which versions exist, but which registry directory each came from. With a
//! crate name it is the disambiguation step before `--version`; without one it is a map of the
//! store, which is useful when the crate name itself is the unknown.

use at_core::contract::Fail;

use crate::store::{self, Installed};
use crate::verbs::{self, Options};

pub fn run(args: &[String], opts: &Options) -> Result<verbs::Outcome, Fail> {
    if args.len() > 1 {
        return Err(Fail::usage(
            "`installed` takes one crate name, or none for the whole store",
        ));
    }
    let query = match args.first().map(String::as_str) {
        Some(query) => Some(verbs::bare_crate(query, "installed")?),
        None => None,
    };
    let store = opts.store()?;
    let listing = store.installed()?;

    let entries: Vec<&Installed> = match query {
        Some(query) => listing
            .entries
            .iter()
            .filter(|entry| {
                entry.name == query || verbs::normalised(&entry.name) == verbs::normalised(query)
            })
            .collect(),
        None => listing.entries.iter().collect(),
    };

    let total = entries.len();
    let shown = total.min(opts.limit);
    let root = query
        .map(str::to_string)
        .unwrap_or_else(|| "the store".to_string());
    let mut report = verbs::report_for(opts, "installed", &root, "extracted copies");
    for entry in entries.iter().take(shown) {
        report.content(store::installed_row(entry));
    }
    report.bound(store::listing_bound(&listing, total, shown, opts.limit));

    if let Some(query) = query {
        if total == 1 {
            report.bound(format!("{query} has one extracted version"));
            report.next(
                opts.again("where", &[query.to_string()], &[]),
                "resolve it, version and all",
            );
        } else if total > 1 {
            report.bound(
                "pass --version <ver> to `where` to choose one; this verb lists, it does not choose",
            );
        }
    }

    Ok(verbs::Outcome::from_report(report))
}
