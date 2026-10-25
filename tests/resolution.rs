//! Spec tests for the resolution model in `docs/precedence.md`.
//!
//! Two kinds of test live here. The first group runs against the **real** rule tree in this
//! repository, so a rule that does not parse — or a project that silently resolves to
//! nothing — fails `cargo test` rather than being discovered when someone reads a generated
//! file and wonders where a rule went. The second group builds small synthetic playbooks in
//! a temp directory to pin the precedence rules themselves.

use std::fs;
use std::path::{Path, PathBuf};

use playbook::init;
use playbook::load::{census, find_manifest, has_errors, load_manifest, load_rules};
use playbook::model::{Activation, Layer};
use playbook::render;
use playbook::resolve::resolve_repo;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ids(resolved: &playbook::resolve::Resolved) -> Vec<&str> {
    resolved.rules.iter().map(|r| r.id.as_str()).collect()
}

// --- against the real tree -------------------------------------------------

/// Every rule must declare its directive explicitly.
///
/// The compiler is not a linter — it compiles whatever it is handed, and a rule with no
/// `## Directive` is emitted whole. That is the right behaviour at compose time (never lose a
/// rule's text) and the wrong thing to leave unnoticed, so the contract lives here, where a
/// failure names every offender at once instead of printing a warning on every command.
///
/// This counted down from 34 as a ratchet while the migration was in progress. The migration
/// is complete, so it is a plain gate. A future change that needs a migration budget again
/// should reintroduce the ratchet shape — with a non-zero budget, or the comparisons that make
/// a ratchet a ratchet become degenerate and clippy says so.
#[test]
fn every_rule_declares_a_directive() {
    let (rules, _) = load_rules(&repo_root());
    let offenders: Vec<&str> = rules
        .iter()
        .filter(|rule| {
            let source = fs::read_to_string(repo_root().join(&rule.rel)).expect("read rule");
            !playbook::model::split_sections(playbook::frontmatter::split_document(&source).body)
                .has_directive
        })
        .map(|rule| rule.rel.as_str())
        .collect();

    assert!(
        offenders.is_empty(),
        "{} rule(s) have no `## Directive` section, so their whole body is compiled. Split \
         each into `## Directive` (the terse direction, emitted) and `## Rationale` (the \
         incident, kept at source) — see docs/rule-format.md.\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}

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
    // present, plausible, and never loaded. There is no project registry to test against any
    // more, so reachable means "a repository could declare this activation": an empty `reach` is
    // a `project:` rule in the shared tree (which belongs in the repository it applies to) or a
    // `manual` rule no target includes.
    let rows = census(&repo_root()).expect("census");
    let orphans: Vec<&str> = rows
        .iter()
        .filter(|r| r.reach.is_empty())
        .map(|r| r.rule.rel.as_str())
        .collect();
    assert!(
        orphans.is_empty(),
        "rules no repository can reach: {orphans:?}"
    );
}

/// A manifest that declares whatever `reach` names, or `None` for an activation a repository
/// cannot declare (`manual`, or `project:` in the shared tree).
fn manifest_declaring(reach: &str) -> Option<String> {
    if reach == "always" {
        return Some("project: probe\n".to_string());
    }
    let (kind, value) = reach.split_once(':')?;
    let key = match kind {
        "language" => "languages",
        "ecosystem" => "ecosystems",
        "org" => "org",
        "renders" => "renders",
        _ => return None,
    };
    Some(format!("project: probe\n{key}: {value}\n"))
}

/// A manifest that reaches every shared rule: all four activation axes.
const EVERYTHING: &str =
    "project: probe\norg: defrag\necosystems: defrag\nrenders: egui, macroquad\nlanguages: rust\n";

#[test]
fn every_shared_activation_resolves_for_a_repository_that_declares_it() {
    // The registry is gone: a repository declares its own identity, so the invariant that
    // replaces "every known project resolves" is that every activation a shared rule can carry
    // is one a manifest can state — and stating it resolves cleanly.
    let rows = census(&repo_root()).expect("census");
    let mut seen: Vec<String> = Vec::new();
    for row in &rows {
        if row.reach.is_empty() || seen.contains(&row.reach) {
            continue;
        }
        let Some(conf) = manifest_declaring(&row.reach) else {
            continue;
        };
        let repo = ManifestedRepo::new(&format!("reach-{}", seen.len()), &conf);
        let resolved = repo.resolve();
        assert!(
            !resolved.has_errors(),
            "`{}` did not resolve:\n{}",
            row.reach,
            resolved
                .diagnostics
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            !resolved.rules.is_empty(),
            "`{}` resolved to zero rules",
            row.reach
        );
        seen.push(row.reach.clone());
    }
    assert!(!seen.is_empty(), "the census found no reachable activation");
}

#[test]
fn the_package_carries_no_repository_registry() {
    // The inversion's own invariant: this package resolves rules for a repository it is told
    // about. A `projects/` directory reappearing means knowledge of consumers is creeping back
    // into the package, which is the thing the manifest model removed.
    assert!(
        !repo_root().join("projects").exists(),
        "the package must not carry a projects/ registry"
    );
}

#[test]
fn an_empty_override_list_drops_nothing() {
    // Regression guard for the bug that ended the shell implementation: the two-file awk
    // that split kept from drop treated every rule as part of the (empty) drop list, so a
    // project with no overrides resolved to zero rules and reported success. The rule set
    // here has no overrides at all, so every collected rule must survive.
    let repo = ManifestedRepo::new("no-overrides", EVERYTHING);
    let resolved = repo.resolve();
    assert!(!resolved.has_errors(), "{:?}", resolved.diagnostics);
    assert!(resolved.superseded.is_empty());
    assert!(
        resolved.rules.len() >= 20,
        "a rule set with no overrides lost rules: {} resolved",
        resolved.rules.len()
    );
}

#[test]
fn layer_order_is_core_then_language_then_org_then_project() {
    // Synthetic so the test does not depend on which real rules carry which layer. The invariant
    // is the sort order, not the fixture.
    let f = LocalFixture::new("layer-order");
    f.write_root(
        "rules/rust/rusty.md",
        &format!(
            "---\n{}---\n\n## Directive\n\nRust rule.\n",
            rule_fm("rusty", "language:rust", "language:rust", "")
        ),
    );
    f.write_root(
        "rules/org/acme/orgy.md",
        &format!(
            "---\n{}---\n\n## Directive\n\nOrg rule.\n",
            rule_fm("orgy", "org", "org:acme", "")
        ),
    );
    f.rule("local", "project: demo");

    let resolved = f.resolve();
    let ranks: Vec<u8> = resolved.rules.iter().map(|r| r.layer.rank()).collect();
    let mut sorted = ranks.clone();
    sorted.sort_unstable();
    assert_eq!(ranks, sorted, "rules are not in layer order");

    let first_project = resolved
        .rules
        .iter()
        .position(|r| r.layer == Layer::Project)
        .expect("the fixture has a project rule");
    assert!(
        resolved.rules[..first_project]
            .iter()
            .all(|r| r.layer.rank() < Layer::Project.rank()),
        "a lower layer sorted after the project layer"
    );
}

#[test]
fn the_personal_target_carries_no_rules_and_the_memory_layer() {
    // Zed's personal instructions file loads for every project the user opens, and an agent
    // reads it *alongside* the project's own file. So it must carry no rules at all: the
    // universal rules ship in each repository's block, because a repo is bootstrapped with
    // them and its `AGENTS.md` is committed and shared. Repeating them here would be
    // duplication with no reader.
    //
    // What it carries is the `memory/` layer — facts about the person and the machine that
    // are true regardless of repository, and that no repo's block should hold.
    //
    // The manifest itself lives in the fleet repository; this fixture is the same shape, so the
    // test does not depend on a checkout outside this one.
    let repo = ManifestedRepo::new(
        "personal",
        "project: personal\norg:\necosystems:\nrenders:\nlanguages:\ndefault_target: zed-personal\n",
    );
    let resolved = repo.resolve_with("zed-personal");
    assert!(!resolved.has_errors(), "{:?}", resolved.diagnostics);
    assert!(
        resolved.rules.is_empty(),
        "the personal file must carry no rules, but resolved: {:?}",
        resolved
            .rules
            .iter()
            .map(|rule| rule.rel.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        !resolved.memory.is_empty(),
        "the personal instructions file would be empty"
    );
}

#[test]
fn an_ecosystem_rule_reaches_a_repo_that_declares_it_and_not_one_that_does_not() {
    // The distinction the selector exists for: a repository operated by one org and built with
    // another's conventions. `demo` declares `org: acme` and `ecosystems: defrag`, so the org rule
    // stays out and the ecosystem rule comes in.
    let f = LocalFixture::new("ecosystem");
    f.write_repo(
        ".playbook/project.conf",
        "project: demo\norg: acme\necosystems: defrag\nlanguages: rust\n",
    );
    f.write_root(
        "rules/org/defrag/service.md",
        &format!(
            "---\n{}---\n\n## Directive\n\nAbout the services.\n",
            rule_fm("service", "org", "org:defrag", "")
        ),
    );
    f.write_root(
        "rules/org/defrag/toolkit.md",
        &format!(
            "---\n{}---\n\n## Directive\n\nAbout the toolchain.\n",
            rule_fm("toolkit", "org", "ecosystem:defrag", "")
        ),
    );

    let resolved = f.resolve();
    assert!(!resolved.has_errors(), "{:?}", resolved.diagnostics);
    assert_eq!(ids(&resolved), vec!["base", "toolkit"]);

    // And the same repo without the declaration gets neither: an ecosystem is a fact a project
    // states, not one inferred from a directory name.
    f.write_repo(
        ".playbook/project.conf",
        "project: demo\norg: acme\nlanguages: rust\n",
    );
    assert_eq!(ids(&f.resolve()), vec!["base"]);
}

#[test]
fn the_zed_target_resolves_for_any_repository() {
    // The zed overlay applies to a harness, not to an org, so it must not require a rule that
    // only one ecosystem carries. It used to emphasise `defrag-agent-tools`, and the composer
    // treats an emphasis that resolves to nothing as an error — correctly, since an unknown id is
    // usually a typo — so `zed` failed for every repository outside the defrag ecosystem. A
    // harness target that only works for some of its consumers is the wrong shape.
    let inside = ManifestedRepo::new(
        "zed-inside",
        "project: inside\norg: defrag\necosystems: defrag\nlanguages: rust\n",
    );
    let resolved = inside.resolve_with("zed");
    assert!(
        !resolved.has_errors(),
        "zed did not resolve inside the ecosystem:\n{}",
        resolved
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );

    let outside = ManifestedRepo::new(
        "zed-outside",
        "project: outside\norg: acme\nlanguages: rust\n",
    );
    let resolved = outside.resolve_with("zed");
    assert!(
        !resolved.has_errors(),
        "zed did not resolve outside the ecosystem:\n{}",
        resolved
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(!resolved.rules.is_empty());
}

#[test]
fn repo_targets_do_not_carry_the_memory_layer() {
    // The two files are complementary: memory is personal and loads everywhere, rules are
    // per-repo. A repo block carrying memory would duplicate it in every project.
    let repo = ManifestedRepo::new("repo-targets", EVERYTHING);
    for target in ["claude-code", "zed", "generic"] {
        let resolved = repo.resolve_with(target);
        assert!(
            resolved.memory.is_empty(),
            "{target} carries the memory layer"
        );
    }
}

#[test]
fn org_rules_do_not_leak_into_another_org() {
    // A hodlcroft repository built with the defrag toolchain: the org layer must not reach it, the
    // ecosystem layer must, and its own rule must.
    //
    // The check is on the **activation**, not on the id, and that distinction is the point of the
    // ecosystem selector: `defrag-agent-tools` is named for the ecosystem it is shared across,
    // not for the org that operates the repository, so an id prefix is not a test of anything.
    // What must not leak is anything scoped to `org:defrag`.
    let repo = ManifestedRepo::new(
        "hodlcroft-viewer",
        "project: viewer\norg: hodlcroft\necosystems: defrag\nlanguages: rust\n",
    )
    .rule("viewer-local", "project: viewer");

    let resolved = repo.resolve();
    let leaked: Vec<&str> = resolved
        .rules
        .iter()
        .filter(|rule| rule.activation == Activation::Org("defrag".into()))
        .map(|rule| rule.id.as_str())
        .collect();
    assert!(
        leaked.is_empty(),
        "org:defrag rules reached viewer: {leaked:?}"
    );
    assert!(ids(&resolved).contains(&"rust-devshell-first"));
    // And a rule that is true of it because of the *toolchain* does arrive — same shell, so the
    // same instruction about what to reach for.
    assert!(
        ids(&resolved).contains(&"defrag-agent-tools"),
        "the repo is on a defrag devshell and should carry the toolkit rule"
    );
    assert!(ids(&resolved).contains(&"viewer-local"));
}

#[test]
fn project_rules_do_not_leak_between_projects() {
    // Every in-workspace rule-bearing repository now owns its rules, so the leak check is on the
    // local model: a rule in one manifest must not load for a sibling.
    let f = LocalFixture::new("no-leak");
    f.rule("only-here", "project: demo");
    assert!(f.resolve().rules.iter().any(|rule| rule.id == "only-here"));

    let sibling = f.sibling("sibling");
    let other = resolve_repo(&f.root, &sibling, Some("t")).expect("resolve sibling");
    assert!(
        !other.rules.iter().any(|rule| rule.id == "only-here"),
        "a local rule leaked into a sibling repository"
    );
}

#[test]
fn emphasis_resolves_to_real_rules_in_overlay_order() {
    let repo = ManifestedRepo::new("emphasis", EVERYTHING);
    for target in ["claude-code", "zed", "generic"] {
        let resolved = repo.resolve_with(target);
        assert!(
            !resolved.has_errors(),
            "{target}: {:?}",
            resolved.diagnostics
        );
        let expected: Vec<&str> = resolved
            .target
            .emphasis
            .iter()
            .map(String::as_str)
            .collect();
        let got: Vec<&str> = resolved.emphasis.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            got, expected,
            "{target}: emphasis order or membership differs"
        );
    }
}

#[test]
fn rendering_is_byte_stable() {
    // `check` diffs the rendered block, so nondeterminism here becomes a false "stale".
    let repo = ManifestedRepo::new("byte-stable", EVERYTHING);
    let resolved = repo.resolve();
    assert_eq!(render::render(&resolved), render::render(&resolved));
}

#[test]
fn the_block_names_every_rule_it_contains() {
    let repo = ManifestedRepo::new("block-names", EVERYTHING);
    let resolved = repo.resolve();
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
    let repo = ManifestedRepo::new("addenda", EVERYTHING);
    let block = render::render(&repo.resolve_with("zed"));
    // The zed addenda carry `#` titles of their own; they must not compete with the
    // document title.
    assert!(block.contains("## Zed mechanics"));
    assert!(!block.contains("\n# Zed mechanics"));
}

// --- synthetic playbooks ---------------------------------------------------

/// A synthetic playbook whose own directory is also the consuming repository.
///
/// The shared tree is `rules/` and the repository's declaration is `.playbook/project.conf`, which
/// is exactly the shape a real checkout has — so the precedence tests exercise the same path the
/// composer takes, without a second directory to keep in step.
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
            ".playbook/project.conf",
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
        resolve_repo(&self.root, &self.root, Some("t")).expect("resolve")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A shared tree plus a repository that owns its own `.playbook/`.
struct LocalFixture {
    base: PathBuf,
    root: PathBuf,
    repo: PathBuf,
}

impl LocalFixture {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "agent-playbook-local-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("playbook");
        let repo = base.join("repo");
        fs::create_dir_all(root.join("rules").join("core")).expect("root rules");
        fs::create_dir_all(root.join("models").join("t")).expect("root models");
        fs::create_dir_all(repo.join(".playbook").join("rules")).expect("repo rules");

        let fixture = LocalFixture { base, root, repo };
        fixture.write_root(
            "rules/core/base.md",
            "---\nid: base\ntitle: base\nlayer: core\nactivation: always\n---\n\n## Directive\n\nBase rule.\n",
        );
        fixture.write_root("models/t/overlay.conf", "target: t\nmodel: m\nharness: h\n");
        fixture.write_repo(
            ".playbook/project.conf",
            "project: demo\norg: acme\nlanguages: rust\n",
        );
        fixture
    }

    fn write_root(&self, rel: &str, content: &str) {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create root parent");
        }
        fs::write(path, content).expect("write root file");
    }

    fn write_repo(&self, rel: &str, content: &str) {
        let path = self.repo.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create repo parent");
        }
        fs::write(path, content).expect("write repo file");
    }

    fn rule(&self, id: &str, activation: &str) {
        self.rule_with_layer(id, "project", activation);
    }

    fn rule_with_layer(&self, id: &str, layer: &str, activation: &str) {
        self.write_repo(
            &format!(".playbook/rules/{id}.md"),
            &format!(
                "---\nid: {id}\ntitle: {id}\nlayer: {layer}\nactivation: {activation}\n---\n\n\
                 ## Directive\n\nA local rule.\n"
            ),
        );
    }

    fn resolve(&self) -> playbook::resolve::Resolved {
        resolve_repo(&self.root, &self.repo, Some("t")).expect("resolve repository")
    }

    /// A second repository under the same base, with a manifest of its own.
    fn sibling(&self, project: &str) -> PathBuf {
        let repo = self.base.join(project);
        fs::create_dir_all(repo.join(".playbook").join("rules")).expect("sibling rules");
        fs::write(
            repo.join(".playbook").join("project.conf"),
            format!("project: {project}\n"),
        )
        .expect("sibling manifest");
        repo
    }
}

impl Drop for LocalFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// A throwaway repository manifest, resolved against the **real** shared tree.
///
/// `LocalFixture` supplies its own two-rule tree so it can pin resolution; this one exists for the
/// activation tests, where the point is which real shared rules a manifest reaches.
struct ManifestedRepo {
    root: PathBuf,
}

impl ManifestedRepo {
    fn new(name: &str, conf: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agent-playbook-manifest-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".playbook").join("rules")).expect("manifest rules");
        fs::write(root.join(".playbook").join("project.conf"), conf).expect("manifest");
        ManifestedRepo { root }
    }

    fn rule(self, id: &str, activation: &str) -> Self {
        fs::write(
            self.root
                .join(".playbook")
                .join("rules")
                .join(format!("{id}.md")),
            format!(
                "---\nid: {id}\ntitle: {id}\nlayer: project\nactivation: {activation}\n---\n\n\
                 ## Directive\n\nA local rule.\n"
            ),
        )
        .expect("local rule");
        self
    }

    fn resolve(&self) -> playbook::resolve::Resolved {
        self.resolve_with("claude-code")
    }

    fn resolve_with(&self, target: &str) -> playbook::resolve::Resolved {
        resolve_repo(&repo_root(), &self.root, Some(target)).expect("resolve manifest")
    }
}

impl Drop for ManifestedRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn find_manifest_walks_up_to_the_repository_root() {
    let f = LocalFixture::new("manifest-walk");
    let nested = f.repo.join("crates").join("deep");
    fs::create_dir_all(&nested).expect("nested dir");

    let (repo, manifest) = find_manifest(&nested).expect("manifest");
    assert_eq!(repo, fs::canonicalize(&f.repo).expect("canonical repo"));
    assert!(manifest.ends_with("project.conf"));
}

#[test]
fn a_repository_manifest_resolves_shared_and_local_rules() {
    let f = LocalFixture::new("resolve-local");
    f.rule("local", "project: demo");

    let resolved = f.resolve();
    let ids: Vec<&str> = resolved.rules.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"base"), "shared rule missing: {ids:?}");
    assert!(ids.contains(&"local"), "local rule missing: {ids:?}");

    let local = resolved
        .rules
        .iter()
        .find(|rule| rule.id == "local")
        .expect("local rule");
    assert_eq!(local.rel, ".playbook/rules/local.md");
    assert!(!resolved.has_errors());
}

#[test]
fn a_local_rule_naming_another_project_is_an_error() {
    let f = LocalFixture::new("wrong-project");
    f.rule("elsewhere", "project: another-repo");

    let resolved = f.resolve();
    assert!(
        resolved.has_errors(),
        "a rule for another project must be fatal"
    );
    let messages: Vec<String> = resolved
        .diagnostics
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("must activate for `project:demo`")),
        "{messages:#?}"
    );
}

#[test]
fn a_local_rule_must_be_project_layer() {
    let f = LocalFixture::new("wrong-layer");
    f.rule_with_layer("core-rule", "core", "project: demo");

    let resolved = f.resolve();
    assert!(resolved.has_errors());
    let messages: Vec<String> = resolved
        .diagnostics
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("must declare `layer: project`")),
        "{messages:#?}"
    );
}

#[test]
fn a_project_rule_in_the_shared_tree_is_an_error() {
    // A `project:` rule names a repository, so it belongs in that repository's `.playbook/rules/`.
    // Leaving one in the shared tree is how the package starts carrying knowledge of a consumer
    // again — and it would activate for nobody, because no shared tree knows which repositories
    // exist.
    let f = LocalFixture::new("shared-project-rule");
    f.write_root(
        "rules/core/misplaced.md",
        "---\nid: misplaced\ntitle: misplaced\nlayer: core\nactivation: project:demo\n---\n\n\
         ## Directive\n\nShould not live in the shared tree.\n",
    );

    let resolved = f.resolve();
    assert!(
        resolved.has_errors(),
        "a shared `project:` rule must be fatal"
    );
    let messages: Vec<String> = resolved
        .diagnostics
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("a shared rule cannot activate for a single project")),
        "{messages:#?}"
    );
}

#[test]
fn a_manifest_without_project_is_an_error() {
    let f = LocalFixture::new("no-project");
    f.write_repo(".playbook/project.conf", "org: acme\n");

    let error = load_manifest(&f.repo).expect_err("a nameless manifest");
    assert!(error.contains("`project:` is required"), "{error}");
}

#[test]
fn a_manifest_path_key_is_reported_as_ignored() {
    let f = LocalFixture::new("manifest-path");
    f.write_repo(
        ".playbook/project.conf",
        "project: demo\npath: ~/code/elsewhere\n",
    );

    let (_, diagnostics) = load_manifest(&f.repo).expect("manifest");
    let messages: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert!(
        messages.iter().any(|m| m.contains("`path:` is ignored")),
        "{messages:#?}"
    );
}

#[test]
fn a_repository_block_names_the_manifest_not_the_registry() {
    let f = LocalFixture::new("provenance");
    f.rule("local", "project: demo");

    let block = render::render(&f.resolve());
    assert!(block.contains("`.playbook/project.conf`"), "{block}");
    assert!(!block.contains("projects/demo"), "{block}");
}

#[test]
fn init_scaffolds_a_manifest_and_rules_directory() {
    let f = LocalFixture::new("init");
    let target = f.repo.join("new-project");
    fs::create_dir_all(&target).expect("new repo");

    let created = init::scaffold(&target, "new-project", "zed").expect("scaffold");
    assert!(!created.is_empty());

    let manifest = fs::read_to_string(target.join(".playbook/project.conf")).expect("manifest");
    assert!(manifest.contains("project: new-project"));
    assert!(manifest.contains("default_target: zed"));
    assert!(target.join(".playbook/rules").is_dir());
    assert!(target.join(".playbook/rules/README.md").is_file());

    let error = init::scaffold(&target, "again", "zed").expect_err("refuses to overwrite");
    assert!(error.contains("already exists"), "{error}");
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
        ".playbook/rules/top.md",
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

#[test]
fn a_directory_without_a_manifest_is_not_discovered() {
    let dir = std::env::temp_dir().join(format!("agent-playbook-unclaimed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("nested")).expect("create dir");

    // Nothing at or above the directory declares a repository, so there is nothing to resolve —
    // rather than a project guessed from a directory name.
    assert!(find_manifest(&dir.join("nested")).is_none());

    let _ = fs::remove_dir_all(&dir);
}
