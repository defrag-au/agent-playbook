//! Spec tests for the resolution model in `docs/precedence.md`.
//!
//! Two kinds of test live here. The first group runs against the **real** rule tree in this
//! repository, so a rule that does not parse — or a project that silently resolves to
//! nothing — fails `cargo test` rather than being discovered when someone reads a generated
//! file and wonders where a rule went. The second group builds small synthetic playbooks in
//! a temp directory to pin the precedence rules themselves.

use std::fs;
use std::path::{Path, PathBuf};

use playbook::load::{census, has_errors, list_projects, load_rules};
use playbook::model::{Activation, Layer};
use playbook::render;
use playbook::resolve::resolve;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ids(resolved: &playbook::resolve::Resolved) -> Vec<&str> {
    resolved.rules.iter().map(|r| r.id.as_str()).collect()
}

// --- against the real tree -------------------------------------------------

#[test]
fn every_rule_in_the_tree_parses_and_has_a_body() {
    let (rules, diagnostics) = load_rules(&repo_root());
    assert!(
        !has_errors(&diagnostics),
        "the committed rule tree does not parse:\n{}",
        diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        rules.len() >= 20,
        "expected the seeded rule set, found {} rules",
        rules.len()
    );
}

#[test]
fn no_rule_is_unreachable() {
    // A rule that activates nowhere is the failure the eight mis-filed skills had:
    // present, plausible, and never loaded.
    let rows = census(&repo_root()).expect("census");
    let orphans: Vec<&str> = rows
        .iter()
        .filter(|r| r.projects.is_empty() && r.rule.activation != Activation::Manual)
        .map(|r| r.rule.rel.as_str())
        .collect();
    assert!(
        orphans.is_empty(),
        "rules activate for no project: {orphans:?}"
    );
}

#[test]
fn every_known_project_resolves_without_errors() {
    for project in list_projects(&repo_root()) {
        let resolved =
            resolve(&repo_root(), &project, None).unwrap_or_else(|e| panic!("{project}: {e}"));
        let messages: Vec<String> = resolved
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            !resolved.has_errors(),
            "{project} resolved with errors:\n{}",
            messages.join("\n")
        );
        assert!(
            !resolved.rules.is_empty(),
            "{project} resolved to zero rules"
        );
    }
}

#[test]
fn an_empty_override_list_drops_nothing() {
    // Regression guard for the bug that ended the shell implementation: the two-file awk
    // that split kept from drop treated every rule as part of the (empty) drop list, so a
    // project with no overrides resolved to zero rules and reported success. The rule set
    // here has no overrides at all, so every collected rule must survive.
    let resolved = resolve(&repo_root(), "shared-crates", None).expect("resolve");
    assert!(resolved.superseded.is_empty());
    assert!(
        resolved.rules.len() >= 20,
        "a rule set with no overrides lost rules: {} resolved",
        resolved.rules.len()
    );
}

#[test]
fn layer_order_is_core_then_language_then_org_then_project() {
    let resolved = resolve(&repo_root(), "shared-crates", None).expect("resolve");
    let ranks: Vec<u8> = resolved.rules.iter().map(|r| r.layer.rank()).collect();
    let mut sorted = ranks.clone();
    sorted.sort_unstable();
    assert_eq!(ranks, sorted, "rules are not in layer order");

    let first_project = resolved
        .rules
        .iter()
        .position(|r| r.layer == Layer::Project)
        .expect("shared-crates has project rules");
    assert!(
        resolved.rules[..first_project]
            .iter()
            .all(|r| r.layer.rank() < Layer::Project.rank()),
        "a lower layer sorted after the project layer"
    );
}

#[test]
fn org_rules_do_not_leak_into_another_org() {
    // archivist is hodlcroft, not defrag. The D1 and widget rules are not true of it, and
    // a flat rule set would have applied them anyway.
    let resolved = resolve(&repo_root(), "archivist", None).expect("resolve");
    assert!(ids(&resolved).iter().all(|id| !id.starts_with("defrag-")));
    assert!(ids(&resolved).contains(&"rust-devshell-first"));
    assert!(ids(&resolved).contains(&"archivist-devshell-commands"));
}

#[test]
fn project_rules_do_not_leak_between_projects() {
    let shared = resolve(&repo_root(), "shared-crates", None).expect("resolve");
    assert!(ids(&shared).contains(&"shared-crates-storybook-registration"));
    assert!(!ids(&shared).contains(&"cnft-type-placement"));

    let cnft = resolve(&repo_root(), "cnft-dev-workers", None).expect("resolve");
    assert!(ids(&cnft).contains(&"cnft-type-placement"));
    assert!(!ids(&cnft).contains(&"shared-crates-storybook-registration"));
}

#[test]
fn emphasis_resolves_to_real_rules_in_overlay_order() {
    for project in list_projects(&repo_root()) {
        let resolved = resolve(&repo_root(), &project, Some("claude-code")).expect("resolve");
        let expected: Vec<&str> = resolved
            .target
            .emphasis
            .iter()
            .map(String::as_str)
            .collect();
        let got: Vec<&str> = resolved.emphasis.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            got, expected,
            "{project}: emphasis order or membership differs"
        );
    }
}

#[test]
fn rendering_is_byte_stable() {
    // `check` diffs the rendered block, so nondeterminism here becomes a false "stale".
    let resolved = resolve(&repo_root(), "shared-crates", None).expect("resolve");
    assert_eq!(render::render(&resolved), render::render(&resolved));
}

#[test]
fn the_block_names_every_rule_it_contains() {
    let resolved = resolve(&repo_root(), "shared-crates", None).expect("resolve");
    let block = render::render(&resolved);
    for rule in &resolved.rules {
        let marker = format!("<!-- rule: {} -->", rule.rel.trim_end_matches(".md"));
        assert!(block.contains(&marker), "block is missing {marker}");
        assert!(
            block.contains(&format!("## {}", rule.title)),
            "block is missing the heading for {}",
            rule.id
        );
    }
    assert!(block.starts_with(render::BEGIN_PREFIX));
    assert!(block.trim_end().ends_with(render::END_MARKER));
}

#[test]
fn addenda_are_demoted_below_the_rules() {
    let resolved = resolve(&repo_root(), "shared-crates", Some("zed")).expect("resolve");
    let block = render::render(&resolved);
    // The zed addenda carry `#` titles of their own; they must not compete with the
    // document title.
    assert!(block.contains("## Zed mechanics"));
    assert!(!block.contains("\n# Zed mechanics"));
}

// --- synthetic playbooks ---------------------------------------------------

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("agent-playbook-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture root");
        let fixture = Fixture { root };
        fixture.write(
            "projects/demo/project.conf",
            "project: demo\norg: acme\nlanguages: rust\n",
        );
        fixture.write("models/t/overlay.conf", "target: t\nmodel: m\nharness: h\n");
        fixture
    }

    fn write(&self, rel: &str, content: &str) -> &Self {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write fixture file");
        self
    }

    fn rule(&self, rel: &str, frontmatter: &str, body: &str) -> &Self {
        self.write(
            &format!("rules/{rel}"),
            &format!("---\n{frontmatter}---\n\n{body}\n"),
        )
    }

    fn resolve(&self) -> playbook::resolve::Resolved {
        resolve(&self.root, "demo", Some("t")).expect("resolve")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn rule_fm(id: &str, layer: &str, activation: &str, extra: &str) -> String {
    format!(
        "id: {id}\ntitle: {id}\nlayer: {layer}\nactivation: {activation}\npriority: 50\n{extra}"
    )
}

#[test]
fn a_higher_layer_overrides_a_lower_one() {
    // Each layer overrides the one below it: project beats org beats core.
    let f = Fixture::new("override");
    f.rule(
        "core/base.md",
        &rule_fm("base", "core", "always", ""),
        "Base rule.",
    )
    .rule(
        "org/replacement.md",
        &rule_fm("replacement", "org", "always", "overrides: base\n"),
        "Replacement rule.",
    )
    .write(
        "projects/demo/rules/top.md",
        &format!(
            "---\n{}---\n\nProject rule that wins.\n",
            rule_fm("top", "project", "project:demo", "overrides: replacement\n")
        ),
    );

    let resolved = f.resolve();
    assert!(!resolved.has_errors(), "{:?}", resolved.diagnostics);
    assert_eq!(ids(&resolved), vec!["top"]);
    assert_eq!(resolved.superseded, vec!["base", "replacement"]);
}

#[test]
fn overriding_upward_is_an_error() {
    // A core rule may not supersede an org rule. Allowing it would let the lowest layer
    // quietly delete a constraint, which is the thing the precedence model exists to
    // prevent.
    let f = Fixture::new("upward");
    f.rule(
        "org/base.md",
        &rule_fm("base", "org", "always", ""),
        "Base.",
    )
    .rule(
        "core/naughty.md",
        &rule_fm("naughty", "core", "always", "overrides: base\n"),
        "Cannot override upward.",
    );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("lower layer")),
        "expected a layer-ordering error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn same_layer_override_is_an_error() {
    // Not merely ambiguous: two rules at one layer disagreeing about which one wins has
    // no defined answer, and the shell implementation resolved it by accident of sort order.
    let f = Fixture::new("same-layer");
    f.rule("core/a.md", &rule_fm("a", "core", "always", ""), "A.")
        .rule(
            "core/b.md",
            &rule_fm("b", "core", "always", "overrides: a\n"),
            "B.",
        );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("lower layer")),
        "expected a layer-ordering error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn mutual_override_is_an_error() {
    let f = Fixture::new("mutual");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", "overrides: b\n"),
        "A.",
    )
    .rule(
        "org/b.md",
        &rule_fm("b", "org", "always", "overrides: a\n"),
        "B.",
    );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("override each other")),
        "expected a mutual-override error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn an_override_naming_a_missing_rule_warns() {
    let f = Fixture::new("missing-override");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", "overrides: ghost\n"),
        "A.",
    );

    let resolved = f.resolve();
    assert!(!resolved.has_errors());
    assert_eq!(ids(&resolved), vec!["a"]);
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("not in this rule set")),
        "expected a warning, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn duplicate_ids_are_fatal() {
    let f = Fixture::new("duplicate");
    f.rule("core/a.md", &rule_fm("same", "core", "always", ""), "A.")
        .rule("org/b.md", &rule_fm("same", "org", "always", ""), "B.");

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("duplicate rule id")),
        "expected a duplicate-id error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn an_unknown_emphasis_id_is_fatal() {
    let f = Fixture::new("bad-emphasis");
    f.rule("core/a.md", &rule_fm("a", "core", "always", ""), "A.")
        .write(
            "models/t/overlay.conf",
            "target: t\nemphasis: a, not-a-rule\n",
        );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("not-a-rule")),
        "expected an emphasis error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn an_unknown_activation_is_fatal() {
    let f = Fixture::new("bad-activation");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "langauge:rust", ""),
        "A.",
    );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
}

#[test]
fn a_rule_with_no_directive_is_fatal() {
    let f = Fixture::new("empty-body");
    f.write(
        "rules/core/a.md",
        "---\nid: a\ntitle: a\nlayer: core\nactivation: always\n---\n",
    );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no directive")),
        "expected a directive error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn a_rule_whose_directive_section_is_empty_is_fatal() {
    // The heading is present but carries no text. That is the same failure as no body at all:
    // a heading that constrains nothing.
    let f = Fixture::new("empty-directive");
    f.write(
        "rules/core/a.md",
        "---\nid: a\ntitle: a\nlayer: core\nactivation: always\n---\n\n## Directive\n\n## Rationale\n\nWhy it matters.\n",
    );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no directive")),
        "expected a directive error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn manual_rules_need_an_explicit_include() {
    let f = Fixture::new("manual");
    f.rule("core/a.md", &rule_fm("a", "core", "manual", ""), "A.")
        .rule("core/b.md", &rule_fm("b", "core", "always", ""), "B.");

    let without = f.resolve();
    assert_eq!(ids(&without), vec!["b"]);

    f.write(
        "models/t/overlay.conf",
        "target: t\ninclude: rules/core/a.md\n",
    );
    let with = f.resolve();
    assert_eq!(ids(&with), vec!["a", "b"]);
}

#[test]
fn targets_restrict_a_rule_to_named_targets() {
    let f = Fixture::new("targets");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", "targets: other\n"),
        "A.",
    )
    .rule("core/b.md", &rule_fm("b", "core", "always", ""), "B.");

    assert_eq!(ids(&f.resolve()), vec!["b"]);
}

#[test]
fn exclude_accepts_an_id_or_a_path_prefix() {
    let f = Fixture::new("exclude");
    f.rule("core/a.md", &rule_fm("a", "core", "always", ""), "A.")
        .rule("org/b.md", &rule_fm("b", "org", "always", ""), "B.")
        .write(
            "models/t/overlay.conf",
            "target: t\nexclude: a, rules/org\n",
        );

    assert!(ids(&f.resolve()).is_empty());
}

#[test]
fn sorting_is_priority_desc_within_a_layer_then_id() {
    let f = Fixture::new("sorting");
    f.rule(
        "core/low.md",
        &rule_fm("z-low", "core", "always", "priority: 10\n"),
        "Low.",
    )
    .rule(
        "core/high.md",
        &rule_fm("a-high", "core", "always", "priority: 90\n"),
        "High.",
    )
    .rule(
        "core/mid.md",
        &rule_fm("m-mid", "core", "always", ""),
        "Mid.",
    );

    assert_eq!(ids(&f.resolve()), vec!["a-high", "m-mid", "z-low"]);
}

#[test]
fn a_rule_with_an_unterminated_header_is_reported_not_guessed_at() {
    let f = Fixture::new("unterminated");
    f.write(
        "rules/core/a.md",
        "---\nid: a\ntitle: a\n\nBody with no closing marker.\n",
    );

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("is required")),
        "expected a required-key error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn install_round_trip_leaves_hand_written_content_alone() {
    let f = Fixture::new("install");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", ""),
        "Rule A body.",
    );

    let repo = f.root.join("repo");
    fs::create_dir_all(&repo).expect("repo");
    fs::write(
        repo.join("AGENTS.md"),
        "# My notes\n\nHand written above.\n",
    )
    .expect("seed");

    let resolved = f.resolve();
    let block = render::render(&resolved);

    let created = playbook::install::write(&repo, "AGENTS.md", &block, false).expect("install");
    assert!(matches!(created, playbook::install::Outcome::Updated(_)));

    let text = fs::read_to_string(repo.join("AGENTS.md")).expect("read");
    assert!(text.starts_with("# My notes\n\nHand written above.\n"));
    assert!(text.contains("Rule A body."));

    // Idempotent: a second install must not move the file.
    let again = playbook::install::write(&repo, "AGENTS.md", &block, false).expect("install");
    assert!(matches!(again, playbook::install::Outcome::Unchanged(_)));

    // And check agrees.
    let checked = playbook::install::write(&repo, "AGENTS.md", &block, true).expect("check");
    assert!(matches!(checked, playbook::install::Outcome::UpToDate(_)));
}

#[test]
fn check_reports_stale_when_a_rule_changed() {
    let f = Fixture::new("stale");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", ""),
        "First body.",
    );

    let repo = f.root.join("repo");
    fs::create_dir_all(&repo).expect("repo");
    let block = render::render(&f.resolve());
    playbook::install::write(&repo, "AGENTS.md", &block, false).expect("install");

    // The rule changes; the file on disk does not.
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", ""),
        "Second body.",
    );
    let newer = render::render(&f.resolve());

    match playbook::install::write(&repo, "AGENTS.md", &newer, true).expect("check") {
        playbook::install::Outcome::Stale(_, diff) => {
            assert!(diff.contains("First body.") || diff.contains("Second body."));
        }
        _ => panic!("expected Stale"),
    }
}

#[test]
fn check_reports_a_missing_file_and_a_missing_block() {
    let f = Fixture::new("check-missing");
    f.rule("core/a.md", &rule_fm("a", "core", "always", ""), "A.");
    let repo = f.root.join("repo");
    fs::create_dir_all(&repo).expect("repo");
    let block = render::render(&f.resolve());

    let missing = playbook::install::write(&repo, "AGENTS.md", &block, true).expect("check");
    assert!(matches!(missing, playbook::install::Outcome::Missing(_)));

    fs::write(repo.join("AGENTS.md"), "Prose with no block.\n").expect("seed");
    let no_block = playbook::install::write(&repo, "AGENTS.md", &block, true).expect("check");
    assert!(matches!(no_block, playbook::install::Outcome::NoBlock(_)));
}

#[test]
fn a_target_with_a_broken_addendum_is_fatal() {
    let f = Fixture::new("bad-addendum");
    f.rule("core/a.md", &rule_fm("a", "core", "always", ""), "A.")
        .write("models/t/overlay.conf", "target: t\naddenda: nope.md\n");

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("nope.md")),
        "expected an addendum error, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn an_unknown_key_is_a_warning_so_typos_are_visible() {
    let f = Fixture::new("unknown-key");
    f.rule(
        "core/a.md",
        &rule_fm("a", "core", "always", "overide: b\n"),
        "A.",
    );

    let resolved = f.resolve();
    assert!(!resolved.has_errors(), "an unknown key must not be fatal");
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.contains("overide")),
        "expected an unknown-key warning, got {:?}",
        resolved.diagnostics
    );
}

#[test]
fn root_discovery_accepts_an_explicit_path_and_rejects_a_non_root() {
    let root = playbook::load::find_root(Some(Path::new(env!("CARGO_MANIFEST_DIR"))));
    assert!(root.is_ok());
    let not_a_root = playbook::load::find_root(Some(Path::new("/")));
    assert!(not_a_root.is_err());
}
