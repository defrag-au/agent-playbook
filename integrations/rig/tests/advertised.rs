//! The advertised arm: the model is told what each tool is for, and why it beats the shell.
//!
//! A scripted model replies without calling anything. The assertions are on the request the harness
//! built: the tool map is in the system prompt, and each of the seven definitions carries its
//! advertised description rather than the CLI's notes and flags.

use rig_harness::harness_protocol::SessionId;
use rig_harness::prelude::*;
use rig_harness::rig_core::test_utils::MockCompletionModel;
use rig_harness_test_support::{
    app_in, connect, first_agent, log_ended, reply, run_until, scratch_home, Ended,
};
use rig_playbook::AdvertisedPlugin;

const TOOLS: &[&str] = &[
    "at_peek_search",
    "at_peek_slice",
    "at_peek_stat",
    "at_recall_state",
    "at_recall_log",
    "at_recall_diff",
    "at_recall_pr",
];

#[test]
fn the_advertised_arm_puts_the_map_and_the_advantages_in_front_of_the_model() {
    let home = scratch_home("rig-playbook-advertised");
    let dir = home.root().join("project");
    std::fs::create_dir_all(&dir).expect("fixture dir");

    let (mut app, wakes) = app_in(&home, &SessionId::generate());
    app.insert_resource(ProjectRoot(dir))
        .insert_resource(Processes::default())
        .init_resource::<Ended>()
        .add_observer(log_ended)
        .add_plugins((AgentPlugin, SessionLogPlugin, AdvertisedPlugin));
    app.finish();
    app.update();

    let model = MockCompletionModel::from_stream_turns([reply("done")]);
    let world = app.world_mut();
    let agent = first_agent(world).expect("an agent");
    if let Some(connection) = connect(&model) {
        world.entity_mut(agent).insert(connection);
    }
    world.trigger(Deliver::new(agent, "hello", DeliveryMode::Steer));
    run_until(&mut app, &wakes, |world| {
        !world.resource::<Ended>().0.is_empty()
    });

    let requests = model.requests();
    let request = requests.first().expect("a request");
    let prompt = request
        .system_instructions()
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(prompt.contains("<playbook_tools>"), "{prompt}");
    assert!(
        prompt.contains("at_peek_search — every mention"),
        "{prompt}"
    );
    assert!(
        prompt.contains("Prefer the tool when a count, a total or the next range matters"),
        "{prompt}"
    );

    for name in TOOLS {
        let tool = request
            .tools
            .iter()
            .find(|tool| tool.name.as_str() == *name)
            .unwrap_or_else(|| panic!("`{name}` is not in the request"));
        assert!(
            tool.description.contains("Call with"),
            "`{name}`: {}",
            tool.description
        );
        assert!(
            tool.description.contains("Unlike"),
            "`{name}`: {}",
            tool.description
        );
    }

    std::fs::remove_dir_all(home.root()).ok();
}
