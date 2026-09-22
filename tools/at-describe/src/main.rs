//! `at-describe` — tier 0 of the agent toolkit.
//!
//! It takes no path, opens no file, and reads nothing. Its dependencies are two siblings in this
//! workspace, linked for their verb tables and nothing else: no member of the toolkit has a write
//! path, so linking one adds no capability here.
//!
//! This is the safest thing in the toolkit to allow, and it is what makes the other grants
//! reviewable — an agent that is unsure asks here instead of composing a shell pipeline to find
//! out, and the answer comes from the same tables the parsers enforce.

use at_peek::catalogue as peek;
use at_recall::catalogue as recall;

const TOOL: &str = "at-describe";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    let Some(first) = args.first() else {
        print!("{TOOL} {VERSION} — the catalogue\n\n{}", toolkit());
        return 0;
    };

    match first.as_str() {
        "-h" | "--help" => {
            print!("{TOOL} {VERSION} — the catalogue\n\n{}", toolkit());
            0
        }
        "--version" => {
            println!(
                "{TOOL} {VERSION} · at-peek {} · at-recall {}",
                at_peek::VERSION,
                at_recall::VERSION
            );
            0
        }
        "at-peek" => {
            print!("{}", peek::help_all());
            0
        }
        "at-recall" => {
            print!("{}", recall::help_all());
            0
        }
        name => {
            let found = peek::find(name)
                .map(peek::help_for)
                .or_else(|| recall::find(name).map(recall::help_for));
            match found {
                Some(help) => {
                    print!("{help}");
                    0
                }
                None => {
                    eprintln!(
                        "{TOOL}: unknown `{name}` · binaries: at-peek, at-recall · verbs: {}, {}",
                        peek::names(),
                        recall::names()
                    );
                    2
                }
            }
        }
    }
}

/// The toolkit view: what is in this build, what each binary promises, and how to read an exit code.
/// Bounded to a screen, because its reader is mid-task.
fn toolkit() -> String {
    let mut out = String::new();
    out.push_str("# binaries in this build: at-peek, at-recall\n");
    out.push_str("# every `at-` binary is read-only: no write path, in any flag or any option\n\n");

    out.push_str(&format!(
        "at-peek {} — read-only inspection of the working tree\n",
        at_peek::VERSION
    ));
    out.push_str(
        "#   No git, no subprocess, no writes, no network, no environment configuration.\n",
    );
    for line in peek::verb_lines() {
        out.push_str(&line);
        out.push('\n');
    }

    out.push_str(&format!(
        "\nat-recall {} — read-only inspection of history and the working tree\n",
        at_recall::VERSION
    ));
    out.push_str(
        "#   One subprocess, git, read verbs only. No writes, no network, and never the reflog.\n",
    );
    for line in recall::verb_lines() {
        out.push_str(&line);
        out.push('\n');
    }

    out.push_str("\nRecipes\n");
    out.push_str(
        "  none in this build · `pr`, `review` and `release` are designed and not written;\n",
    );
    out.push_str("  the shapes are in docs/inspection-tools.md in the playbook\n");

    out.push_str("\nExit codes\n");
    for (code, meaning) in peek::exit_codes() {
        out.push_str(&format!("  {code:<4} {meaning}\n"));
    }

    out.push_str(
        "\n# at-describe knows the binaries it is linked with; one installed separately\n",
    );
    out.push_str("# does not appear here. Ask for detail with `at-describe at-peek`,\n");
    out.push_str("# `at-describe at-recall`, or `at-describe <verb>`.\n");
    out
}
