//! The plain shell reads, deterministically: each is served in process by the toolkit.
//!
//! A scripted model asks for `cat`, `sed -n`, `head`, `git status`, `git log` and `git diff`. The
//! harness replaces each handler in `Prepare`, so no shell runs and the model sees the verb's
//! bounded answer. The published facts and the redirection name are asserted per call.

use std::fs;
use std::path::Path;
use std::process::Command;

use rig_harness::harness_protocol::SessionId;
use rig_harness::prelude::*;
use rig_harness::rig_core::effect::EffectKind;
use rig_harness::rig_core::message::Message;
use rig_harness::rig_core::test_utils::{MockCompletionModel, MockStreamEvent};
use rig_harness_test_support::{
    app_in, collect, connect, first_agent, log_ended, log_tool_outputs, reply, run_until,
    scratch_home, Collected, Ended, ToolOutputs,
};
use rig_playbook::{
    PlaybookPlugin, PublishedBatch, PublishedDiff, PublishedLog, PublishedSlice, PublishedState,
    Redirection, ServedFact,
};
use serde::Serialize;

#[derive(Serialize)]
struct ShellArgs {
    command: String,
}

fn shell(id: &str, command: &str) -> MockStreamEvent {
    MockStreamEvent::tool_call(
        id,
        "shell",
        serde_json::to_value(ShellArgs {
            command: command.to_owned(),
        })
        .expect("shell arguments"),
    )
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
fn the_plain_shell_reads_are_served_in_process() {
    let home = scratch_home("rig-playbook-redirect-reads");
    let dir = home.root().join("project");
    fs::create_dir_all(&dir).expect("fixture dir");
    fs::write(dir.join("a.rs"), "one\ntwo\nthree\n").expect("fixture file");
    git(&dir, &["init", "--quiet"]);
    git(&dir, &["add", "a.rs"]);
    git(&dir, &["commit", "--quiet", "-m", "initial"]);
    // One uncommitted line, so `git diff` and `git status` have something to answer.
    fs::write(dir.join("a.rs"), "one\ntwo\nthree\nfour\n").expect("working tree");

    let (mut app, wakes) = app_in(&home, &SessionId::generate());
    app.insert_resource(ProjectRoot(dir.clone()))
        .insert_resource(Processes::default())
        .init_resource::<Ended>()
        .add_observer(log_ended)
        .init_resource::<ToolOutputs>()
        .add_observer(log_tool_outputs)
        .add_plugins((
            AgentPlugin,
            SessionLogPlugin,
            rig_basics::coding_tools::ShellTool,
            PlaybookPlugin,
        ));
    collect::<EffectRecorded>(&mut app);
    app.finish();
    app.update();

    let turn = vec![
        shell("cat", "cat a.rs"),
        shell("sed", "sed -n '1,2p' a.rs"),
        shell("head", "head -1 a.rs"),
        shell("status", "git status --short"),
        shell("log", "git log --oneline -1"),
        shell("diff", "git diff"),
        shell(
            "compound",
            "git status --short; git log --oneline -1; git diff",
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

    // The conversation keeps the calls the model made, not the ones that ran.
    let called: Vec<String> = world
        .get::<Conversation>(agent)
        .map(|conversation| conversation.messages())
        .unwrap_or_default()
        .iter()
        .filter_map(|message| match &**message {
            Message::Assistant(assistant) => Some(assistant.tool_calls()),
            _ => None,
        })
        .flatten()
        .map(|call| call.function.raw_arguments())
        .collect();
    assert_eq!(called.len(), 7, "{called:?}");
    assert!(called
        .iter()
        .any(|args| args.contains("git status --short")));

    let outputs = &world.resource::<ToolOutputs>().0;
    for (id, header) in [
        ("cat", "# at-peek slice"),
        ("sed", "# at-peek slice"),
        ("head", "# at-peek slice"),
        ("status", "# at-recall state"),
        ("log", "# at-recall log"),
        ("diff", "# at-recall diff"),
        ("compound", "# at-recall state"),
    ] {
        assert!(outputs[id].starts_with(header), "`{id}`: {}", outputs[id]);
    }
    assert!(outputs["compound"].contains("# at-recall log"));
    assert!(outputs["compound"].contains("# at-recall diff"));

    let records = &world.resource::<Collected<EffectRecorded>>().0;
    let shell_records: Vec<&_> = records
        .iter()
        .filter_map(|EffectRecorded(record)| match &record.kind {
            EffectKind::ToolCall { name, .. } if name == "shell" => Some(record),
            _ => None,
        })
        .collect();

    let mut slices: Vec<PublishedSlice> = shell_records
        .iter()
        .filter_map(|record| {
            record
                .published
                .as_ref()
                .and_then(|published| published.get::<PublishedSlice>().ok().flatten())
        })
        .collect();
    assert_eq!(slices.len(), 3, "one slice per served command");
    assert!(slices.iter().all(|slice| slice.0.targets[0].path == "a.rs"));
    slices.sort_by_key(|slice| slice.0.targets[0].shown);
    assert_eq!(
        slices
            .iter()
            .map(|slice| slice.0.targets[0].shown)
            .collect::<Vec<_>>(),
        [1, 2, 4]
    );

    let state: PublishedState = shell_records
        .iter()
        .find_map(|record| {
            record
                .published
                .as_ref()
                .and_then(|published| published.get::<PublishedState>().ok().flatten())
        })
        .expect("state facts");
    assert_eq!(state.0.branch, "main");
    assert_eq!(state.0.changes.len(), 1);

    let log: PublishedLog = shell_records
        .iter()
        .find_map(|record| {
            record
                .published
                .as_ref()
                .and_then(|published| published.get::<PublishedLog>().ok().flatten())
        })
        .expect("log facts");
    assert_eq!(log.0.commits.as_ref().expect("commits").total, 1);

    let diff: PublishedDiff = shell_records
        .iter()
        .find_map(|record| {
            record
                .published
                .as_ref()
                .and_then(|published| published.get::<PublishedDiff>().ok().flatten())
        })
        .expect("diff facts");
    assert_eq!(diff.0.files.len(), 1);
    assert_eq!(diff.0.files[0].label, "a.rs");

    let batch: PublishedBatch = shell_records
        .iter()
        .find_map(|record| {
            record
                .published
                .as_ref()
                .and_then(|published| published.get::<PublishedBatch>().ok().flatten())
        })
        .expect("batch facts");
    assert_eq!(batch.0.len(), 3);
    assert!(matches!(batch.0[0], ServedFact::State(_)));
    assert!(matches!(batch.0[1], ServedFact::Log(_)));
    assert!(matches!(batch.0[2], ServedFact::Diff(_)));

    let mut tools: Vec<String> = records
        .iter()
        .filter_map(|EffectRecorded(record)| {
            record
                .published
                .as_ref()
                .and_then(|published| published.get::<Redirection>().ok().flatten())
                .map(|redirection| redirection.tool)
        })
        .collect();
    tools.sort();
    assert_eq!(
        tools,
        [
            "at_peek_slice",
            "at_peek_slice",
            "at_peek_slice",
            "at_recall_diff",
            "at_recall_log",
            "at_recall_state",
            "at_recall_state + at_recall_log + at_recall_diff",
        ]
    );

    fs::remove_dir_all(home.root()).ok();
}
