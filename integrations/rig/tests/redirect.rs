//! The redirect, deterministically.
//!
//! A scripted model asks the shell for `rg -n needle .`. The model never asks twice, no provider is
//! called, and the assertions are on what a front and the effect log would see: the conversation
//! keeps the model's own call, the result is the bounded search, and the log names the
//! substitution without a `decided_by`.

use std::fs;

use rig_harness::harness_protocol::SessionId;
use rig_harness::prelude::*;
use rig_harness::rig_core::effect::EffectKind;
use rig_harness::rig_core::message::Message;
use rig_harness::rig_core::test_utils::MockCompletionModel;
use rig_harness_test_support::{
    app_in, calls, collect, connect, first_agent, log_ended, log_tool_outputs, reply, run_until,
    scratch_home, Collected, Ended, ToolOutputs,
};
use rig_playbook::{PlaybookPlugin, PublishedSearch, Redirection};
use serde::Serialize;

/// The shell tool's arguments, typed for the scripted call.
#[derive(Serialize)]
struct ShellArgs {
    command: String,
}

#[test]
fn a_shell_search_is_answered_by_the_tool_without_a_second_model_call() {
    let home = scratch_home("rig-playbook-redirect");
    let dir = home.root().join("project");
    fs::create_dir_all(dir.join("src")).expect("fixture src");
    fs::write(dir.join("src/a.rs"), "fn needle() {}\n").expect("fixture file");

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

    let model = MockCompletionModel::from_stream_turns([
        calls(
            "c1",
            "shell",
            serde_json::to_value(ShellArgs {
                command: "rg -n needle .".to_owned(),
            })
            .expect("the call's arguments"),
        ),
        reply("Found it."),
    ]);

    let world = app.world_mut();
    let agent = first_agent(world).expect("an agent");
    if let Some(connection) = connect(&model) {
        world.entity_mut(agent).insert(connection);
    }
    world.trigger(Deliver::new(agent, "Find the needle.", DeliveryMode::Steer));
    run_until(&mut app, &wakes, |world| {
        !world.resource::<Ended>().0.is_empty()
    });

    let world = app.world();

    // The conversation keeps the call the model made, not the one that ran.
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
    assert_eq!(called, [r#"{"command":"rg -n needle ."}"#]);

    // The result is the search, bounds and all — not a refusal to read.
    let output = &world.resource::<ToolOutputs>().0["c1"];
    assert!(output.starts_with("# at-peek search"), "{output}");
    assert!(output.contains("src/a.rs:1:fn needle() {}"), "{output}");
    assert!(output.contains("# 1 match in 1 file"), "{output}");

    // The log records what answered the call, and no gate decided it.
    let records = &world.resource::<Collected<EffectRecorded>>().0;
    let record = records
        .iter()
        .filter_map(|EffectRecorded(record)| match &record.kind {
            EffectKind::ToolCall { name, .. } if name == "shell" => Some(record),
            _ => None,
        })
        .next()
        .expect("the shell call");

    let published = record.published.as_ref().expect("published facts");
    let redirect: Redirection = published
        .get()
        .expect("a readable redirection")
        .expect("a redirection fact");
    assert_eq!(redirect.tool, "at_peek_search");
    assert_eq!(redirect.command, "rg -n needle .");

    let search: PublishedSearch = published
        .get()
        .expect("readable search facts")
        .expect("a search fact");
    assert_eq!(search.0.total, 1);
    assert!(record.decided_by.is_none(), "a redirect is not a refusal");

    fs::remove_dir_all(home.root()).ok();
}
