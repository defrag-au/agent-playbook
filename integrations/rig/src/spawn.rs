//! Running one `at-*` invocation.
//!
//! No shell: the arguments are a slice, so a caller cannot inject one. The tools print their
//! report to stdout, which is why the plugin spawns them rather than calling the library — a
//! tool's stdout would otherwise land in the agent's own, which is the protocol channel in every
//! non-terminal front.
//!
//! The binary is found through `PATH`, unless `AT_PEEK_BIN` or `AT_RECALL_BIN` names it.

use std::path::PathBuf;
use std::process::Command;

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The variable that overrides where `binary` is found: `at-peek` → `AT_PEEK_BIN`.
fn override_variable(binary: &str) -> String {
    let stem: String = binary
        .chars()
        .map(|c| match c {
            'a'..='z' => c.to_ascii_uppercase(),
            c => c,
        })
        .collect();
    format!("{}_BIN", stem.replace('-', "_"))
}

/// Run `binary` with `args`, capturing both streams. The verb is `args[0]`.
pub fn run(binary: &str, args: &[String]) -> std::io::Result<Outcome> {
    let program = std::env::var_os(override_variable(binary))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(binary));

    let output = Command::new(&program).args(args).output()?;

    Ok(Outcome {
        // A process killed by a signal has no code; `-1` is still not zero, which is what the
        // caller acts on.
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}
