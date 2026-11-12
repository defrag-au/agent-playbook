//! The read tools, deterministically: each answers in process and publishes its facts.
//!
//! A scripted model calls three tools in one turn. No provider is called, and the assertions are on
//! what the effect log sees: the slice, stat and state answers arrive as typed values under their
//! `agent_playbook.*` keys, not only as the text the model was shown.

use std::fs;
use std::path::Path;
use std::process::Command;

use rig_harness::harness_protocol::SessionId;
use rig_harness::prelude::*;
use rig_harness::rig_core::effect::EffectKind;
use rig_harness::rig_core::test_utils::{MockCompletionModel, MockStreamEvent};
use rig_harness_test_support::{
    app_in, collect, connect, first_agent, log_ended, log_tool_outputs, reply, run_until,
    scratch_home, Collected, Ended, ToolOutputs,
};
use rig_playbook::{
    PlaybookPlugin, PublishedDiff, PublishedLog, PublishedOutline, PublishedPr, PublishedSlice,
    PublishedStat, PublishedState,
};
use serde::Serialize;

#[derive(Serialize)]
struct SliceCall {
    targets: Vec<String>,
}

#[derive(Serialize)]
struct StatCall {
    paths: Vec<String>,
}

#[derive(Serialize)]
struct OutlineCall {
    paths: Vec<String>,
}

#[derive(Serialize)]
struct StateCall {}

#[derive(Serialize)]
struct LogCall {}

#[derive(Serialize)]
struct DiffCall {}

#[derive(Serialize)]
struct PrCall {
    base: String,
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

#[test]
fn the_read_tools_answer_in_process_and_publish_their_facts() {
    let home = scratch_home("rig-playbook-tools");
    let dir = home.root().join("project");
    fs::create_dir_all(&dir).expect("fixture dir");
    fs::write(dir.join("a.rs"), "one\ntwo\nthree\n").expect("fixture file");
    fs::write(dir.join("doc.md"), "# One\n\n## Two\n").expect("fixture outline");
    git(&dir, &["init", "--quiet"]);
    git(&dir, &["add", "a.rs", "doc.md"]);
    git(&dir, &["commit", "--quiet", "-m", "initial"]);
    fs::write(dir.join("a.rs"), "one\ntwo\nthree\nfour\n").expect("second revision");
    git(&dir, &["add", "a.rs"]);
    git(&dir, &["commit", "--quiet", "-m", "second"]);
    // One uncommitted change, so `diff` has a working-tree comparison to answer.
    fs::write(dir.join("a.rs"), "one\ntwo\nthree\nfour\nfive\n").expect("working tree");

    let (mut app, wakes) = app_in(&home, &SessionId::generate());
    app.insert_resource(ProjectRoot(dir.clone()))
        .insert_resource(Processes::default())
        .init_resource::<Ended>()
        .add_observer(log_ended)
        .init_resource::<ToolOutputs>()
        .add_observer(log_tool_outputs)
        .add_plugins((AgentPlugin, SessionLogPlugin, PlaybookPlugin));
    collect::<EffectRecorded>(&mut app);
    app.finish();
    app.update();

    let turn = vec![
        MockStreamEvent::tool_call(
            "s1",
            "at_peek_slice",
            serde_json::to_value(SliceCall {
                targets: vec!["a.rs:2-3".to_owned()],
            })
            .expect("slice arguments"),
        ),
        MockStreamEvent::tool_call(
            "t1",
            "at_peek_stat",
            serde_json::to_value(StatCall {
                paths: vec!["a.rs".to_owned()],
            })
            .expect("stat arguments"),
        ),
        MockStreamEvent::tool_call(
            "o1",
            "at_peek_outline",
            serde_json::to_value(OutlineCall {
                paths: vec!["doc.md".to_owned()],
            })
            .expect("outline arguments"),
        ),
        MockStreamEvent::tool_call(
            "r1",
            "at_recall_state",
            serde_json::to_value(StateCall {}).expect("state arguments"),
        ),
        MockStreamEvent::tool_call(
            "l1",
            "at_recall_log",
            serde_json::to_value(LogCall {}).expect("log arguments"),
        ),
        MockStreamEvent::tool_call(
            "d1",
            "at_recall_diff",
            serde_json::to_value(DiffCall {}).expect("diff arguments"),
        ),
        MockStreamEvent::tool_call(
            "p1",
            "at_recall_pr",
            serde_json::to_value(PrCall {
                base: "HEAD~1".to_owned(),
            })
            .expect("pr arguments"),
        ),
        MockStreamEvent::final_response_with_default_usage(),
    ];
    let model = MockCompletionModel::from_stream_turns([turn, reply("done")]);

    let world = app.world_mut();
    let agent = first_agent(world).expect("an agent");
    if let Some(connection) = connect(&model) {
        world.entity_mut(agent).insert(connection);
    }
    world.trigger(Deliver::new(
        agent,
        "Read the fixture.",
        DeliveryMode::Steer,
    ));
    run_until(&mut app, &wakes, |world| {
        !world.resource::<Ended>().0.is_empty()
    });

    let world = app.world();
    let outputs = &world.resource::<ToolOutputs>().0;
    assert!(
        outputs["s1"].contains("# at-peek slice"),
        "{}",
        outputs["s1"]
    );
    assert!(
        outputs["t1"].contains("# at-peek stat"),
        "{}",
        outputs["t1"]
    );
    assert!(
        outputs["o1"].contains("# at-peek outline"),
        "{}",
        outputs["o1"]
    );
    assert!(
        outputs["r1"].contains("# at-recall state"),
        "{}",
        outputs["r1"]
    );
    assert!(
        outputs["l1"].contains("# at-recall log"),
        "{}",
        outputs["l1"]
    );
    assert!(
        outputs["d1"].contains("# at-recall diff"),
        "{}",
        outputs["d1"]
    );
    assert!(
        outputs["p1"].contains("# at-recall pr"),
        "{}",
        outputs["p1"]
    );

    let records = &world.resource::<Collected<EffectRecorded>>().0;
    let record = |tool: &str| {
        records
            .iter()
            .filter_map(|EffectRecorded(record)| match &record.kind {
                EffectKind::ToolCall { name, .. } if name == tool => Some(record),
                _ => None,
            })
            .next()
            .unwrap_or_else(|| panic!("the `{tool}` call"))
    };

    let slice: PublishedSlice = record("at_peek_slice")
        .published
        .as_ref()
        .expect("published slice facts")
        .get()
        .expect("readable slice facts")
        .expect("slice facts");
    assert_eq!(slice.0.targets[0].path, "a.rs");
    assert_eq!(slice.0.targets[0].shown, 2);

    let stat: PublishedStat = record("at_peek_stat")
        .published
        .as_ref()
        .expect("published stat facts")
        .get()
        .expect("readable stat facts")
        .expect("stat facts");
    assert_eq!(stat.0.rows[0].path, "a.rs");
    assert_eq!(stat.0.rows[0].lines, Some(5));

    let outline: PublishedOutline = record("at_peek_outline")
        .published
        .as_ref()
        .expect("published outline facts")
        .get()
        .expect("readable outline facts")
        .expect("outline facts");
    assert_eq!(outline.0.targets[0].path, "doc.md");
    assert_eq!(outline.0.targets[0].headings_total, 2);
    assert_eq!(outline.0.targets[0].headings[1].depth, 2);

    let state: PublishedState = record("at_recall_state")
        .published
        .as_ref()
        .expect("published state facts")
        .get()
        .expect("readable state facts")
        .expect("state facts");
    assert_eq!(state.0.branch, "main");
    assert_eq!(state.0.changes.len(), 1);

    let log: PublishedLog = record("at_recall_log")
        .published
        .as_ref()
        .expect("published log facts")
        .get()
        .expect("readable log facts")
        .expect("log facts");
    assert_eq!(log.0.commits.as_ref().expect("commits").total, 2);

    let diff: PublishedDiff = record("at_recall_diff")
        .published
        .as_ref()
        .expect("published diff facts")
        .get()
        .expect("readable diff facts")
        .expect("diff facts");
    assert_eq!(diff.0.rev.as_deref(), Some("HEAD"));
    assert_eq!(diff.0.files.len(), 1);
    assert_eq!(diff.0.files[0].label, "a.rs");

    let pr: PublishedPr = record("at_recall_pr")
        .published
        .as_ref()
        .expect("published pr facts")
        .get()
        .expect("readable pr facts")
        .expect("pr facts");
    assert_eq!(pr.0.base, "HEAD~1");
    assert_eq!(pr.0.commits.as_ref().expect("commits").total, 1);
    assert_eq!(pr.0.files.as_ref().expect("files").len(), 1);

    fs::remove_dir_all(home.root()).ok();
}
