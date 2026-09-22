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

/// Whether a file name is secret-shaped, and which rule matched.
///
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
