//! `at-describe` — tier 0 of the agent toolkit.
//!
//! It takes no path, opens no file, and reads nothing. Its one dependency is `at-peek`, linked
//! for that crate's verb table and nothing else: no member of the toolkit has a write path, so
//! linking one adds no capability here.
//!
//! This is the safest thing in the toolkit to allow, and it is what makes the other grants
//! reviewable — an agent that is unsure asks here instead of composing a shell pipeline to
//! find out, and the answer comes from the same table the parser enforces.

use at_peek::catalogue;

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
            println!("{TOOL} {VERSION} · at-peek {}", at_peek::VERSION);
            0
        }
        "at-peek" => {
            print!("{}", catalogue::help_all());
            0
        }
        name => match catalogue::find(name) {
            Some(verb) => {
                print!("{}", catalogue::help_for(verb));
                0
            }
            None => {
                eprintln!(
                    "{TOOL}: unknown `{name}` · binaries: at-peek · verbs: {}",
                    catalogue::names()
                );
                2
            }
        },
    }
}

/// The toolkit view: what is in this build, what the namespace promises, and how to read an
/// exit code. Bounded to a screen, because its reader is mid-task.
fn toolkit() -> String {
    let mut out = String::new();
    out.push_str("# binaries in this build: at-peek\n");
    out.push_str("# every `at-` binary is read-only: no write path, in any flag or any option\n\n");
    out.push_str(&format!(
        "at-peek {} — read-only inspection of the working tree\n",
        at_peek::VERSION
    ));
    for line in catalogue::verb_lines() {
        out.push_str(&line);
        out.push('\n');
    }

    out.push_str("\nRecipes\n");
    out.push_str(
        "  none in this build · the report verbs (`pr`, `review`, `release`) land with at-recall\n",
    );

    out.push_str("\nExit codes\n");
    for (code, meaning) in catalogue::exit_codes() {
        out.push_str(&format!("  {code:<4} {meaning}\n"));
    }

    out.push_str(
        "\n# at-describe knows the binaries it is linked with; one installed separately\n",
    );
    out.push_str("# does not appear here. Ask for detail with `at-describe at-peek` or\n");
    out.push_str("# `at-describe <verb>`.\n");
    out
}
