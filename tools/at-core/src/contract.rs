//! The output contract — the invariants that make the toolkit cheap to read and safe to
//! allowlist.
//!
//! These are asserted from outside, by running the binary, in `tests/contract.rs`. A unit
//! test cannot honestly check "no output exceeds the cap" or "every truncation is
//! announced", because both are properties of what a caller sees, not of a function.

use std::time::{SystemTime, UNIX_EPOCH};

/// Lines one invocation may print before it has to announce the cut.
pub const DEFAULT_LIMIT: usize = 200;

/// The ceiling `--limit` cannot raise. A caller can never ask for an unbounded read.
pub const MAX_LIMIT: usize = 2000;

/// Files a walk may consider before it stops and says so. Higher than any sane repository,
/// low enough that pointing the tool at the wrong root cannot become a ten-minute read.
pub const DEFAULT_MAX_FILES: usize = 20_000;
/// The ceiling `--max-files` cannot raise.
pub const MAX_FILES: usize = 200_000;

/// Characters a single output line may carry before it is truncated and flagged. Long lines
/// are the real token cost — one minified file would otherwise be a whole context window.
pub const MAX_LINE_WIDTH: usize = 500;

/// Exit codes. `Nothing` and `Refused` are the two an agent needs to distinguish a fact from
/// a guess: "there is nothing there" is not "that failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Something was found and printed.
    Results = 0,
    /// Read successfully, nothing to show.
    Nothing = 1,
    /// The command was not one this tool accepts.
    Usage = 2,
    /// The request was fine; the environment could not satisfy it.
    Environment = 3,
    /// The tool declined: a path outside the root, or a secret-shaped path.
    Refused = 4,
}

impl Exit {
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// A failure that carries the exit code it should produce. Every one of these is printed to
/// stderr with the tool's name in front, so a transcript says which tool declined and why.
#[derive(Debug)]
pub struct Fail {
    pub exit: Exit,
    pub message: String,
}

impl Fail {
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::Usage,
            message: message.into(),
        }
    }

    pub fn environment(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::Environment,
            message: message.into(),
        }
    }

    pub fn refused(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::Refused,
            message: message.into(),
        }
    }
}

/// Output as it will be printed.
///
/// A line beginning `# ` is metadata: a header naming what was read, or a bound saying what
/// was not. Everything else is content, and content lines carry their own `path:line` so a
/// transcript line can be cited without re-deriving where it came from.
#[derive(Default)]
pub struct Report {
    lines: Vec<String>,
    content: usize,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    /// `# at-peek slice · root · target` — names the root, the verb and what was read.
    pub fn header(&mut self, tool: &str, verb: &str, root: &str, target: &str) {
        self.lines
            .push(format!("# {tool} {verb} · {root} · {target}"));
    }

    pub fn content(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
        self.content += 1;
    }

    pub fn note(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// A bound or a caveat. Prefixed so a reader can separate it from content, and never
    /// optional: every output that was cut, clamped or widened says so through here.
    pub fn bound(&mut self, line: impl Into<String>) {
        self.lines.push(format!("# {}", line.into()));
    }

    /// The next question, printed as the command that asks it.
    ///
    /// The third kind of line: not content, not a bound. A bound says what was *not* shown; an exit
    /// says what to ask next, and says it as something the reader can run rather than as something
    /// it has to compose. Four rules make it a contract rather than a hint:
    ///
    /// * **It is literal and complete.** An approval over the grammar covers it, and no state
    ///   crosses between invocations — an exit is an *address*, never a handle. A token that
    ///   carried state would be unreadable to whoever reviews the invocation, which is the same
    ///   objection that keeps this toolkit stateless.
    /// * **It continues the question just asked.** The same binary, the same comparison, widened or
    ///   deepened. A command that switched to a different tool would be a suggestion, and
    ///   suggestions belong in the rule where they can be argued with.
    /// * **It is built from the parts of the invocation that produced it**, so it cannot name a
    ///   flag the parser would refuse, and cannot quietly point somewhere else — an exit drops the
    ///   caller's `--root` only by being wrong.
    /// * **There are at most two, in one order:** widen an answer that was cut, then read the part
    ///   that was held back. An answer with nothing cut and nothing further to read offers none.
    pub fn next(&mut self, command: impl Into<String>, why: impl Into<String>) {
        self.lines
            .push(format!("# next: {} · {}", command.into(), why.into()));
    }

    /// How many content lines were produced. Zero means "nothing found", which is an exit
    /// code rather than an empty success.
    pub fn content_lines(&self) -> usize {
        self.content
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

/// A verb's answer, and the exit code it implies.
///
/// Every verb returns one of these rather than printing, so that "nothing was found" is a fact
/// the caller can branch on instead of an empty success — and so the code and the output cannot
/// disagree, which they could if a verb printed and then chose a code separately.
pub struct Outcome {
    pub report: Report,
    pub exit: Exit,
}

impl Outcome {
    /// Exit 0 when something was printed, 1 when the read succeeded and found nothing. A verb
    /// that needs a different code — a refusal, an environment failure — sets `exit` itself.
    pub fn from_report(report: Report) -> Outcome {
        let exit = if report.content_lines() > 0 {
            Exit::Results
        } else {
            Exit::Nothing
        };
        Outcome { report, exit }
    }

    /// For a verb whose answer is its frame, because the body was withheld on request.
    ///
    /// A summary prints no content lines by construction, so [`Outcome::from_report`] would call a
    /// survey that found six changed files "nothing to show" — and the exit code is the cheapest
    /// reading of a survey there is. Reaching for this is a claim that the frame itself is the
    /// answer, which is only true for a verb that says so in its own help.
    pub fn from_frame(report: Report) -> Outcome {
        Outcome {
            report,
            exit: Exit::Results,
        }
    }
}

/// `1 path` / `3 paths`, because "1 path(s)" reads like a tool that is not sure.
///
/// A noun the `+s` rule gets wrong passes its own plural to [`plural_of`]: this toolkit has one,
/// and `2 directorys` was printed by it before the second form existed.
pub fn plural(count: usize, noun: &str) -> String {
    plural_of(count, noun, &format!("{noun}s"))
}

/// The same, for a noun whose plural is not `+s`.
pub fn plural_of(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// Whether a file name is secret-shaped, and which rule matched.
/// This is a guard rail, not a boundary: it stops an accidental `slice .env` and an
/// accidental sweep over a key file, and `--include-secret-paths` reads one anyway. The
/// committed-placeholder case is deliberately excluded — `.env.example` is a template, and
/// the playbook's own convention is that it is committed.
pub fn secret_shaped(file_name: &str) -> Option<&'static str> {
    const EXACT: &[&str] = &[".env", ".netrc", ".git-credentials"];
    const PREFIX: &[&str] = &["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"];
    const SUFFIX: &[&str] = &[".pem", ".key", ".p12", ".pfx"];
    // A committed template is not a secret: `.env.example` is the shape the playbook's own
    // rule tells you to commit, so matching it would make the guard rail an obstacle.
    const TEMPLATE: &[&str] = &[".env.example", ".env.sample", ".env.template"];

    if TEMPLATE.contains(&file_name) {
        return None;
    }
    if EXACT.contains(&file_name) {
        return Some("a secret-shaped name");
    }
    if PREFIX.iter().any(|p| file_name.starts_with(p)) {
        return Some("a private-key name");
    }
    if let Some(suffix) = SUFFIX.iter().find(|s| file_name.ends_with(**s)) {
        return Some(suffix);
    }
    if file_name.starts_with(".env.") {
        return Some("a dotenv file that is not a template");
    }
    let lower = file_name.to_lowercase();
    if lower.contains("secret") {
        return Some("a name containing `secret`");
    }
    if lower.contains("credential") {
        return Some("a name containing `credential`");
    }
    None
}

/// Cut a line at the width cap and say so, rather than silently shortening it.
pub fn truncate(line: &str) -> (String, bool) {
    let mut out = String::new();
    for (index, character) in line.chars().enumerate() {
        if index == MAX_LINE_WIDTH {
            let extra = line.chars().count() - index;
            out.push_str(&format!(" ...(+{extra} characters)"));
            return (out, true);
        }
        out.push(character);
    }
    (out, false)
}

/// UTC, ISO-8601, seconds resolution.
///
/// Hand-rolled rather than pulled from a date crate: this output has to be byte-stable, and
/// twelve lines that can be read here are easier to trust than a version range in a lockfile.
pub fn iso8601_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since the Unix epoch to a civil date. Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn iso8601_renders_the_epoch() {
        assert_eq!(iso8601_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso8601_renders_a_known_timestamp() {
        assert_eq!(
            iso8601_utc(UNIX_EPOCH + Duration::from_secs(1_000_000_000)),
            "2001-09-09T01:46:40Z"
        );
    }

    #[test]
    fn plural_agrees_with_itself_on_one() {
        assert_eq!(plural(1, "path"), "1 path");
        assert_eq!(plural(0, "path"), "0 paths");
        assert_eq!(plural(3, "path"), "3 paths");
    }

    #[test]
    fn a_noun_with_its_own_plural_uses_it() {
        assert_eq!(
            plural_of(1, "untracked directory", "untracked directories"),
            "1 untracked directory"
        );
        assert_eq!(
            plural_of(2, "untracked directory", "untracked directories"),
            "2 untracked directories"
        );
    }

    #[test]
    fn an_empty_report_is_nothing_rather_than_success() {
        let outcome = Outcome::from_report(Report::new());
        assert_eq!(outcome.exit, Exit::Nothing);
    }

    #[test]
    fn a_report_with_content_is_results() {
        let mut report = Report::new();
        report.content("a line");
        assert_eq!(Outcome::from_report(report).exit, Exit::Results);
    }

    #[test]
    fn a_header_alone_is_not_content() {
        // The failure this guards: a verb that prints only bounds and exits 0, so a caller reads
        // "there is something here" out of an empty answer.
        let mut report = Report::new();
        report.header("at-peek", "stat", "root", "1 path");
        report.bound("1 path");
        assert_eq!(Outcome::from_report(report).exit, Exit::Nothing);
    }

    #[test]
    fn a_frame_that_found_something_is_results() {
        let mut report = Report::new();
        report.header("at-recall", "diff", "repo", "working tree against HEAD");
        report.bound("6 files, +98 -38, not shown");
        assert_eq!(Outcome::from_frame(report).exit, Exit::Results);
    }

    #[test]
    fn an_exit_is_one_line_and_is_not_content() {
        // Content is what the exit code counts, and an exit is not an answer — a verb that found
        // nothing must not exit 0 because it suggested what to ask instead.
        let mut report = Report::new();
        report.next("at-recall diff --patch", "the hunks");
        assert_eq!(report.content_lines(), 0);
        assert_eq!(
            report.render(),
            "# next: at-recall diff --patch · the hunks\n"
        );
    }

    #[test]
    fn env_template_is_not_secret_shaped() {
        assert_eq!(secret_shaped(".env.example"), None);
        assert!(secret_shaped(".env").is_some());
        assert!(secret_shaped(".env.local").is_some());
        assert!(secret_shaped("id_ed25519").is_some());
        assert!(secret_shaped("relay.pem").is_some());
    }

    #[test]
    fn ordinary_source_names_are_not_secret_shaped() {
        for name in ["cache.rs", "Cargo.lock", "flake.nix", "main.rs", "ci.yml"] {
            assert_eq!(secret_shaped(name), None, "{name} must not be refused");
        }
    }

    #[test]
    fn substring_matches_are_deliberately_broad() {
        // The accepted false positives: any name containing the words, including source files
        // that merely talk about secrets. A guard rail that misses is worse than one that makes
        // you pass a flag, and the refusal names the flag to pass.
        for name in ["secrets.md", "credentials.rs", "credentials.toml"] {
            assert!(
                secret_shaped(name).is_some(),
                "{name} is refused on purpose"
            );
        }
    }
}
