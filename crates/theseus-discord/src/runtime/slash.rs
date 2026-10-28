//! The slash commands every place answers (moved out of `runtime.rs` for the
//! shape budget's ceiling, theseus-2j2ka): their names, descriptions and
//! options. A child of `runtime`.

use twilight_model::application::command::CommandType;
use twilight_util::builder::command::CommandBuilder;

use super::{
    digest, extensions, halt, horizon_cmds, orders, prompt, remind, task_cmds, thread, thread_ask,
    voice, why,
};

/// The slash commands every place answers, one effect each (the owner's rule):
/// `/stop` halts this session's own work and keeps the conversation (W1),
/// `/new` alone starts a fresh session, `/trust` removes its `injection` label
/// and `/cancel` names a task (DD7) or a pending wake (DD8), by one
/// option, `id`.
pub(super) fn commands() -> Vec<twilight_model::application::command::Command> {
    let mut cmds: Vec<_> = [
        (
            "stop",
            "Stop what Theseus is doing here: its turn, jobs, and queued messages. The conversation goes on",
        ),
        (
            "new",
            "Start a fresh session here; the old one stays in the web UI",
        ),
        (
            "status",
            "This place's session: state, turns, dollars, waiting confirms",
        ),
        (
            "tasks",
            "This place's background tasks: state, spend, and what each waits on",
        ),
        (
            "wakes",
            "This place's pending wakes: when each is due, and its note",
        ),
        (
            "trust",
            "Remove the injection label: this conversation's calls that change things run again",
        ),
    ]
    .into_iter()
    .map(|(n, d)| halt::command(n, d))
    .collect();
    cmds.push(
        CommandBuilder::new(
            "cancel",
            "Stop a background task and its jobs, or cancel a wake; /tasks and /wakes list them",
            CommandType::ChatInput,
        )
        .option(
            twilight_util::builder::command::StringBuilder::new(
                "id",
                "The task's or wake's id, or its last six characters, as /tasks and /wakes show it",
            )
            .required(true)
            .autocomplete(true),
        )
        .build(),
    );
    // `/publish` (the place rule): only the owner, from a private place; the
    // core judges it. The channel first, as Discord wants required options.
    let text = |name: &str, what: &str| {
        twilight_util::builder::command::StringBuilder::new(name, what).required(false)
    };
    cmds.push(
        CommandBuilder::new(
            "publish",
            "Publish a node, a file, or a message into a place (only you, from a private place)",
            CommandType::ChatInput,
        )
        .option(
            twilight_util::builder::command::ChannelBuilder::new("to", "The place to publish into")
                .required(true),
        )
        .option(text("node", "A node's id, as the web UI shows it"))
        .option(text("file", "A file's path, absolute or ~/"))
        .option(text("text", "A message"))
        .option(text("note", "Your words above it"))
        .build(),
    );
    cmds.push(prompt::command());
    cmds.push(extensions::command());
    cmds.extend([why::command(), digest::command()]);
    cmds.push(orders::command());
    cmds.push(thread::command());
    cmds.push(thread_ask::command());
    cmds.push(remind::command());
    cmds.extend(horizon_cmds::commands());
    cmds.extend(voice::commands());
    cmds.extend(task_cmds::commands());
    cmds
}
