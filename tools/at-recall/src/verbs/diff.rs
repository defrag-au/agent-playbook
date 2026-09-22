//! `at-recall diff` — what is the difference.
//!
//! The verb behind the single most common read in a working session, and the one the toolkit's
//! bounding matters for most: `git diff` prints without a ceiling, and an agent that reads a whole
//! diff has spent its context on the least of what it needed. The default answer is therefore a
//! per-file table with the totals, and `--patch` is the caller asking for the hunks as well.
//!
//! Two decisions are worth stating because they are where a diff lies to a reader:
//!
//! * Nothing is reported for a path git does not track, and *nothing* looks exactly like
//!   "unchanged". An empty answer with paths named is therefore explained — untracked, or not a
//!   path at all — rather than left to read as agreement.
//! * A diff prints content, so `--patch` withholds the files the deny-list refuses and names them.
//!   The table does not: a file's name and its line counts are metadata, and a reader who cannot
//!   see that a file changed cannot ask about it.

use std::collections::BTreeSet;

use at_core::contract::{secret_shaped, Fail, Report, MAX_LIMIT};

use crate::git::{rev, with_paths, Noun};
use crate::status::Status;
use crate::verbs::{again, plural, Budget, Opts, Outcome};
use crate::TOOL;

/// What every diff run carries, whatever the caller asked for. `--no-color` because a repository
/// can configure colour on and this output is read by a machine; `--no-ext-diff` and `--no-textconv`
/// because both names a program for git to run, and this tool runs one program only.
const NEUTRAL: &[&str] = &["--no-color", "--no-ext-diff", "--no-textconv"];

pub fn run(args: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }

    let (rev, paths) = split(args, opts)?;
    let rev = default_revision(rev, opts);
    refuse_filtered(opts, &rev, &paths)?;
    report.header(TOOL, "diff", opts.root.name(), &comparison(&rev));

    let files = numstat(opts, &rev, &paths)?;
    if files.is_empty() {
        report.bound("no differences".to_string());
        explain_empty(&mut report, opts, &paths)?;
        return Ok(Outcome::from_report(report));
    }

    let mut budget = Budget::new(opts.limit);
    let width = files
        .iter()
        .map(|file| file.counts().len())
        .max()
        .unwrap_or(0);
    for file in &files {
        budget.push(
            &mut report,
            format!("{:<width$}  {}", file.counts(), file.label),
        );
    }

    let totals = totals(&files);
    let total = files.len();
    let shown = total - budget.dropped();
    report.bound(if shown == total {
        format!("{}, {totals}", plural(total, "file"))
    } else {
        format!(
            "{}, {totals} · {shown} shown · --limit {}",
            plural(total, "file"),
            opts.limit
        )
    });

    if opts.patch {
        patch(
            &mut report,
            &mut budget,
            opts,
            &rev,
            &paths,
            &withheld(&files, opts),
        )?;
    }

    let emitted = report.content_lines();
    let dropped = budget.dropped();
    if dropped > 0 {
        report.bound(format!(
            "{emitted} of {} lines · --limit {} reached",
            emitted + dropped,
            opts.limit
        ));
    }
    budget.width_caveat(&mut report);
    exits(
        &mut report,
        opts,
        &rev,
        &paths,
        &withheld(&files, opts),
        emitted,
        dropped,
    );

    Ok(Outcome::from_report(report))
}

/// The names a patch will not print, sorted and deduplicated.
fn withheld(files: &[FileStat], opts: &Opts) -> Vec<String> {
    if !opts.patch || opts.include_secret_paths {
        return Vec::new();
    }
    let mut names: Vec<String> = files
        .iter()
        .flat_map(|file| file.names.iter())
        .filter(|name| secret_shaped_path(name))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The next questions this answer implies, in the order the contract fixes: widen what was cut,
/// then read what was held back.
///
/// `--patch` comes last and only on its own, because it is the one exit that is not a consequence of
/// something the answer already said — the other two are what a cut bound and a withheld file are
/// *for*. At most two lines, and none at all when the diff was complete and the hunks were already
/// asked for.
fn exits(
    report: &mut Report,
    opts: &Opts,
    rev: &Option<String>,
    paths: &[String],
    withheld: &[String],
    emitted: usize,
    dropped: usize,
) {
    let total = emitted + dropped;
    if dropped > 0 {
        // `--patch` is carried, not assumed away: the exit has to ask the same question wider, and
        // a widen that returned the table where the caller had asked for hunks would answer
        // something else.
        let mut flags: Vec<String> = Vec::new();
        if opts.patch {
            flags.push("--patch".to_string());
        }
        flags.push(format!("--limit {}", total.min(MAX_LIMIT)));
        report.next(
            again(opts, "diff", rev.as_deref(), paths, &flags),
            format!("all {total} lines"),
        );
    }
    if !withheld.is_empty() {
        report.next(
            again(
                opts,
                "diff",
                rev.as_deref(),
                paths,
                &["--include-secret-paths".to_string()],
            ),
            "the hunks of those files",
        );
    }
    if dropped == 0 && withheld.is_empty() && !opts.patch {
        report.next(
            again(
                opts,
                "diff",
                rev.as_deref(),
                paths,
                &["--patch".to_string()],
            ),
            "the hunks",
        );
    }
}

/// Refuse a worktree comparison when the repository routes any of its paths through a filter
/// driver.
///
/// A worktree diff asks git to turn a working-tree file into the blob it would commit, and a
/// repository can choose the program that does that conversion: `filter.<driver>.clean`, selected
/// by `.gitattributes`. This tool runs one program — git — so a change set that includes such a
/// path is refused before anything reads the worktree. The alternative is worse both ways: diffing
/// *with* the filter runs a program a repository named, as the reader, on a tool that exists to be
/// allowlisted once; diffing *without* it prints raw bytes where the reader expects the filtered
/// form of the file, and a diff that is quietly not the diff git would show is the failure this
/// toolkit exists to remove.
///
/// The change set is gathered from the two sources that read no worktree content — the index
/// against the revision, and the status against the index — and narrowed by the caller's own paths
/// rather than by matching them here. Their union is every tracked path the comparison could name:
/// a path that differs between the revision and the worktree differs either before the index or
/// after it.
fn refuse_filtered(opts: &Opts, rev: &Option<String>, paths: &[String]) -> Result<(), Fail> {
    if matches!(rev, Some(rev) if rev.contains("..")) {
        // Two commits and no worktree: no file is converted, so no filter can run.
        return Ok(());
    }

    let mut changed: BTreeSet<String> = BTreeSet::new();
    changed.extend(index_paths(opts, rev, paths)?);

    let status = Status::read_paths(&opts.git, paths)?;
    if status.unparsed() > 0 {
        return Err(Fail::environment(
            "the working tree state did not parse, so this diff cannot be checked against the \
             filters a repository may configure"
                .to_string(),
        ));
    }
    changed.extend(status.paths());

    if changed.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = changed.into_iter().collect();

    // `unspecified` is "no pattern matched" and `unset` is a repository saying `-filter`; neither
    // names a driver, so neither can run one.
    let offenders: Vec<String> = opts
        .git
        .check_filter_attribute(&names)?
        .into_iter()
        .filter(|(_, value)| value != "unspecified" && value != "unset")
        .map(|(path, value)| format!("{path} (filter={value})"))
        .collect();
    if offenders.is_empty() {
        return Ok(());
    }
    Err(Fail::refused(format!(
        "{} · a filter driver is a program the repository names, and this tool runs git only · \
         `git diff` shows the filtered form",
        offenders.join(", ")
    )))
}

/// The paths the index differs on, from the one diff that converts no working-tree file.
fn index_paths(opts: &Opts, rev: &Option<String>, paths: &[String]) -> Result<Vec<String>, Fail> {
    let mut args: Vec<String> = ["--numstat", "--cached"]
        .iter()
        .map(|flag| (*flag).to_string())
        .collect();
    if let Some(rev) = rev {
        args.push(rev.clone());
    }
    with_paths(&mut args, paths);
    let raw = opts.git.run(Noun::Diff, &borrowed(&args))?;
    Ok(raw
        .lines()
        .filter_map(parse_numstat)
        .flat_map(|file| file.names)
        .collect())
}

/// The hunks, minus the files the deny-list refuses.
///
/// The exclusion is a pathspec rather than a filter over the patch text: git decides which file a
/// hunk belongs to, so a file named with a space or a quote cannot make the two disagree. The names
/// come from git's own listing, which is what makes them safe to pass back as pathspecs.
fn patch(
    report: &mut Report,
    budget: &mut Budget,
    opts: &Opts,
    rev: &Option<String>,
    paths: &[String],
    withheld: &[String],
) -> Result<(), Fail> {
    if !withheld.is_empty() {
        report.bound(format!(
            "{} withheld from the hunks: {}",
            plural(withheld.len(), "secret-shaped path"),
            withheld.join(", ")
        ));
    }

    let mut pathspecs = paths.to_vec();
    for name in withheld {
        pathspecs.push(format!(":(exclude,literal){name}"));
    }

    let args = diff_args(rev, &pathspecs, &["--patch"]);
    let hunks = opts.git.run(Noun::Diff, &borrowed(&args))?;
    for line in hunks.lines() {
        budget.push(report, line);
    }
    Ok(())
}

/// What a comparison with no revision given is against.
///
/// HEAD, stated rather than left to git's default — plain `git diff` compares the index with the
/// working tree, which is a different answer to the same question, and the one a reader who asked
/// "what have I changed" is not asking. On a branch with no commits there is no HEAD to name, and
/// the index is the only thing in front of the worktree.
fn default_revision(rev: Option<String>, opts: &Opts) -> Option<String> {
    match rev {
        Some(rev) => Some(rev),
        None if opts.git.head_exists() => Some("HEAD".to_string()),
        None => None,
    }
}

/// The comparison, in the words the caller would use for it. A range names its own comparison, so
/// it is printed as typed rather than paraphrased.
fn comparison(rev: &Option<String>) -> String {
    match rev {
        Some(rev) if rev.contains("..") => rev.clone(),
        Some(rev) => format!("working tree against {rev}"),
        None => "working tree against the index (no commits yet)".to_string(),
    }
}

/// `git diff` reports nothing for a path it does not track, and nothing reads as "unchanged". Say
/// which of the three this is, and fail on a path that is not there at all.
fn explain_empty(report: &mut Report, opts: &Opts, paths: &[String]) -> Result<(), Fail> {
    for path in paths {
        // A pattern that matches nothing is indistinguishable from a path that changed nothing, so
        // the check is limited to the plain names it can actually answer for.
        if path.contains(['*', '?', '[', ']']) || path.starts_with(':') {
            continue;
        }
        let probe = opts
            .git
            .probe(
                Noun::Status,
                &[
                    "--porcelain=v2",
                    "-z",
                    "--untracked-files=all",
                    "--ignored=matching",
                    "--",
                    path,
                ],
            )
            .unwrap_or_default();
        // Both of these are paths git knows about and a diff between commits still cannot show,
        // which is the reading a bare "no differences" invites.
        if probe.split('\0').any(|record| record.starts_with("? ")) {
            report.bound(format!(
                "caveat: {path} is untracked, and a diff between commits cannot show it"
            ));
        } else if probe.split('\0').any(|record| record.starts_with("! ")) {
            report.bound(format!(
                "caveat: {path} is ignored by this repository, so no diff shows it"
            ));
        } else if std::fs::metadata(opts.root.dir().join(path)).is_err() {
            return Err(Fail::environment(format!(
                "{path} is neither a revision nor a path in {}, and no difference mentions it",
                opts.root.name()
            )));
        }
    }
    Ok(())
}

/// The first argument is a revision when it names one and a path otherwise — the rule git itself
/// uses, so `at-recall diff main` and `at-recall diff src/main.rs` both mean what they look like.
///
/// A name that git can resolve but this tool will not pass — the reflog above all — is refused
/// here rather than quietly demoted to a path, because "that is a reflog reference and this tool
/// does not read the reflog" is the answer the caller needs.
fn split(args: &[String], opts: &Opts) -> Result<(Option<String>, Vec<String>), Fail> {
    match args.split_first() {
        Some((first, rest)) => {
            // Refused before git is asked anything: whether the reference resolves has nothing to do
            // with the fact that this tool does not read the reflog, and a `HEAD@{1}` that happens
            // not to exist must not be quietly read as a path.
            crate::git::refuse_reflog(first)?;
            if opts.git.names_a_revision(first) {
                return Ok((Some(rev(first)?), rest.to_vec()));
            }
            // A token with a range in it is meant as one, whatever it resolves to. Reading it as a
            // path instead would answer "nothing changed" about a range that does not exist.
            if first.contains("..") {
                return Err(Fail::environment(format!(
                    "`{first}` is not a revision range in {} · a range needs both of its ends to \
                     resolve",
                    opts.root.name()
                )));
            }
            Ok((None, args.to_vec()))
        }
        None => Ok((None, Vec::new())),
    }
}

fn diff_args(rev: &Option<String>, paths: &[String], mode: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = NEUTRAL
        .iter()
        .chain(mode.iter())
        .map(|flag| (*flag).to_string())
        .collect();
    if let Some(rev) = rev {
        args.push(rev.clone());
    }
    with_paths(&mut args, paths);
    args
}

fn borrowed(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// One line of `--numstat`.
struct FileStat {
    /// `None` for a binary file: git prints `-` where a count would be, and a line count for a
    /// binary file is a number that means nothing.
    added: Option<u64>,
    deleted: Option<u64>,
    /// The path as git wrote it, which for a rename is the factored `old => new` form.
    label: String,
    /// The one or two real names behind `label`, because a rename is one line with two paths and
    /// the deny-list has to see both.
    names: Vec<String>,
}

impl FileStat {
    /// The first column: the counts, or the word for a file that has none.
    fn counts(&self) -> String {
        match (self.added, self.deleted) {
            (Some(added), Some(deleted)) => format!("+{added} -{deleted}"),
            _ => "binary".to_string(),
        }
    }
}

fn numstat(opts: &Opts, rev: &Option<String>, paths: &[String]) -> Result<Vec<FileStat>, Fail> {
    let args = diff_args(rev, paths, &["--numstat"]);
    let raw = opts.git.run(Noun::Diff, &borrowed(&args))?;
    Ok(raw
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(parse_numstat)
        .collect())
}

fn parse_numstat(line: &str) -> Option<FileStat> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?;
    let deleted = parts.next()?;
    let label = parts.next()?;
    Some(FileStat {
        added: added.parse().ok(),
        deleted: deleted.parse().ok(),
        label: label.to_string(),
        names: names_of(label),
    })
}

/// `src/{old.rs => new.rs}` and `old.rs => new.rs` are git's two factored rename forms, and the
/// brace form factors out a common prefix or suffix — `{old => new}/mod.rs` for a directory that
/// moved. Neither name is on a line of its own, so both are rebuilt here.
///
/// A path containing the literal text ` => ` would be read as a rename. That is accepted: the
/// misreading costs a broader refusal, never a file printed that the deny-list meant to withhold.
fn names_of(label: &str) -> Vec<String> {
    if let (Some(open), Some(close)) = (label.find('{'), label.find('}')) {
        if open < close {
            if let Some((old, new)) = label[open + 1..close].split_once(" => ") {
                let prefix = &label[..open];
                let suffix = &label[close + 1..];
                return vec![
                    format!("{prefix}{old}{suffix}"),
                    format!("{prefix}{new}{suffix}"),
                ];
            }
        }
    }
    match label.split_once(" => ") {
        Some((old, new)) => vec![old.to_string(), new.to_string()],
        None => vec![label.to_string()],
    }
}

/// Whether a path is one the deny-list refuses. Both the name and the whole path are checked: the
/// name catches `.env` in any directory, and the path catches a file inside a directory that is
/// itself named for secrets.
fn secret_shaped_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    secret_shaped(name).is_some() || secret_shaped(path).is_some()
}

/// `+12 -35`, and the binary count when there is one to state.
fn totals(files: &[FileStat]) -> String {
    let added: u64 = files.iter().filter_map(|file| file.added).sum();
    let deleted: u64 = files.iter().filter_map(|file| file.deleted).sum();
    let binary = files.iter().filter(|file| file.added.is_none()).count();
    if binary == 0 {
        format!("+{added} -{deleted}")
    } else {
        format!("+{added} -{deleted} ({binary} binary)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_its_own_name() {
        assert_eq!(names_of("src/main.rs"), vec!["src/main.rs"]);
    }

    #[test]
    fn a_factored_rename_yields_both_names() {
        assert_eq!(
            names_of("src/{old.rs => new.rs}"),
            vec!["src/old.rs", "src/new.rs"]
        );
        assert_eq!(
            names_of("src/{old => new}/mod.rs"),
            vec!["src/old/mod.rs", "src/new/mod.rs"]
        );
    }

    #[test]
    fn an_unfactored_rename_yields_both_names() {
        assert_eq!(names_of("a.rs => b/c.rs"), vec!["a.rs", "b/c.rs"]);
    }

    #[test]
    fn a_binary_file_has_no_counts() {
        let file = parse_numstat("-\t-\tassets/logo.png").expect("a numstat line");
        assert_eq!(file.counts(), "binary");
        assert_eq!(file.added, None);
    }

    #[test]
    fn counts_and_path_survive_a_path_with_a_tab_after_it() {
        let file = parse_numstat("12\t3\tsrc/a weird name.rs").expect("a numstat line");
        assert_eq!(file.counts(), "+12 -3");
        assert_eq!(file.label, "src/a weird name.rs");
    }

    #[test]
    fn the_deny_list_sees_a_secret_name_in_any_directory() {
        assert!(secret_shaped_path("config/.env"));
        assert!(secret_shaped_path("deploy/relay.pem"));
        assert!(secret_shaped_path("secrets/notes.md"));
        assert!(!secret_shaped_path("src/cache.rs"));
        assert!(!secret_shaped_path(".env.example"));
    }

    #[test]
    fn totals_state_the_binary_count() {
        let files = vec![
            FileStat {
                added: Some(12),
                deleted: Some(3),
                label: "a.rs".to_string(),
                names: vec!["a.rs".to_string()],
            },
            FileStat {
                added: None,
                deleted: None,
                label: "b.png".to_string(),
                names: vec!["b.png".to_string()],
            },
        ];
        assert_eq!(totals(&files), "+12 -3 (1 binary)");
    }
}
