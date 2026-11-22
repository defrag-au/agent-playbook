//! The rendered output, pinned byte for byte.
//!
//! `tests/contract.rs` asserts properties of stdout; these assert the exact bytes. A refactor that
//! moves rendering between functions — the toolkit's typed core does exactly that — cannot change a
//! character without a failure that names the verb and the case.
//!
//! The fixture is built by this file, so a golden is a fact about the tool rather than a snapshot of
//! a repository that moves. The root's own path is normalised to `<root>`, because the exit lines
//! echo it and a temp directory is not part of the answer.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use at_peek::paths::Root;
use at_peek::verbs::search::{self, Mode};
use at_peek::verbs::Opts;

const BIN: &str = env!("CARGO_BIN_EXE_at-peek");

/// A small tree, fixed in content and order.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    /// `case` in the name keeps two tests running at once from sharing a directory.
    fn new(case: &str) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("at-peek-golden-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).expect("fixture src");
        fs::create_dir_all(dir.join("docs")).expect("fixture docs");
        fs::create_dir_all(dir.join(".git")).expect("fixture .git");
        fs::write(dir.join("src/a.rs"), "fn alpha() {}\nfn beta() {}\n").expect("a.rs");
        fs::write(dir.join("src/b.rs"), "fn alpha() {}\n").expect("b.rs");
        fs::write(dir.join("docs/readme.md"), "alpha beta\n").expect("readme");
        fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").expect("HEAD");
        // Canonical, because `--root` is canonicalised and a macOS temp directory is a link.
        Fixture {
            dir: fs::canonicalize(&dir).expect("canonical fixture"),
        }
    }

    /// `text` with the fixture's path and name replaced, so a golden is about the answer.
    fn normalise(&self, text: &str) -> String {
        let path = self.dir.display().to_string();
        let name = self
            .dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        text.replace(&path, "<root>").replace(&name, "<root>")
    }

    fn golden(&self, case: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(format!("search-{case}.txt"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

struct Case {
    name: &'static str,
    args: &'static [&'static str],
    code: i32,
}

/// Each mode, the bound a limit adds, the frame without rows, and nothing found.
const CASES: &[Case] = &[
    Case {
        name: "matches",
        args: &["search", "alpha"],
        code: 0,
    },
    Case {
        name: "count",
        args: &["search", "alpha", "--count"],
        code: 0,
    },
    Case {
        name: "files-only",
        args: &["search", "alpha", "--files-only"],
        code: 0,
    },
    Case {
        name: "limit",
        args: &["search", "alpha", "--limit", "1"],
        code: 0,
    },
    Case {
        name: "summary",
        args: &["search", "alpha|beta", "--summary"],
        code: 0,
    },
    Case {
        name: "nothing",
        args: &["search", "nomatch"],
        code: 1,
    },
];

#[test]
fn rendered_output_is_byte_for_byte_what_it_was() {
    let fixture = Fixture::new("cli");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();

    for case in CASES {
        let output = Command::new(BIN)
            .args(case.args)
            .arg("--root")
            .arg(&fixture.dir)
            .output()
            .expect("run at-peek");

        assert_eq!(
            output.status.code(),
            Some(case.code),
            "{} exited with the wrong code",
            case.name
        );

        let actual = fixture.normalise(&String::from_utf8_lossy(&output.stdout));
        let path = fixture.golden(case.name);
        if update {
            fs::create_dir_all(path.parent().expect("golden directory")).expect("golden directory");
            fs::write(&path, &actual).expect("write golden");
        } else {
            let expected = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(actual, expected, "{}", case.name);
        }
    }
}

/// Library mode is the same read, not a second one: the facts a harness collects render to the
/// bytes the CLI prints, including the exit a limit adds.
#[test]
fn the_library_renders_what_the_cli_prints() {
    let fixture = Fixture::new("lib");
    let opts = Opts::resolve(
        Root::at(&fixture.dir).expect("the fixture is a root"),
        true,
        Some(1),
        None,
        false,
        false,
    )
    .expect("valid options");

    let facts = search::collect(&["alpha".to_owned()], &opts, Mode::Matches).expect("a search");
    let actual = fixture.normalise(&facts.report(&opts).render());
    let expected = fs::read_to_string(fixture.golden("limit")).expect("the golden");

    assert_eq!(actual, expected);
}
