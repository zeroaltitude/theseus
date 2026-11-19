//! Theseus's own look, run in a shell, answered with its tool (issue 28), through the whole core
//! on the settings rig: the owner's own turn is answered before the command runs; an act, a
//! command that holds anything else, a task, and a daemon with no such tool run as they did.

use serde_json::{json, Value};

use super::tests::{rig_with, Rig};
use crate::node::ResultStatus;

/// The settings rig, whose commands run in its own state directory (the default one is a path of a
/// developer's machine).
fn rig() -> Rig {
    rig_with(|cfg, state| cfg.tools.cwd = Some(state.to_string_lossy().into_owned()))
}

/// What a turn that runs `input` in a shell got, and how many jobs started for it.
async fn shell(r: &Rig, sid: &str, input: Value) -> (ResultStatus, String, usize) {
    r.turn_with("proc_run", sid, input).await;
    let (status, text) = r.result(sid);
    (status, text, r.rows("tool.job_started").len())
}

fn answered(status: ResultStatus, text: &str) -> bool {
    status == ResultStatus::Declined && text.starts_with("Not run: this is `")
}

/// "Look at yourself": `theseus health` in the owner's own turn is answered with `look_theseus`,
/// whatever it is piped into and however it is spelled as argv or as a line; it is not run (no job
/// starts) and no card is made.
#[tokio::test]
async fn the_owners_shell_look_at_theseus_is_answered_and_not_run() {
    for input in [
        json!({"command": "theseus health"}),
        json!({"command": "theseus --json health | head -20"}),
        json!({"argv": ["theseus", "profile"]}),
    ] {
        let r = rig();
        let sid = r.session();
        let (status, text, jobs) = shell(&r, &sid, input.clone()).await;
        assert!(answered(status, &text), "{input}: {status:?} {text}");
        assert!(text.contains("`look_theseus`: use it"), "{text}");
        assert_eq!(jobs, 0, "{input}: it did not run");
        assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    }
    let r = rig();
    let sid = r.session();
    let (_, text, _) = shell(&r, &sid, json!({"command": "theseus config history"})).await;
    assert!(text.contains("`config_history`: use it"), "{text}");
}

/// What the answer does not touch: an act, a look with no tool, a command with anything else in
/// it, and a command that is not Theseus's own run as they did.
#[tokio::test]
async fn an_act_a_look_with_no_tool_and_anything_else_run_as_they_did() {
    for input in [
        json!({"command": "theseus ask hello"}),
        json!({"command": "theseus profile use glm"}),
        json!({"command": "theseus history"}),
        // The command prints a waiting change whole; the tool `config.show` reads settings by name.
        json!({"command": "theseus config show"}),
        json!({"command": "theseus health; echo more"}),
        json!({"command": "theseus health | tee out.txt"}),
        json!({"command": "echo hello"}),
    ] {
        let r = rig();
        let sid = r.session();
        let (status, text, _) = shell(&r, &sid, input.clone()).await;
        assert!(!answered(status, &text), "{input} was answered: {text}");
    }
    // A command that is not Theseus's own is run, and says what it printed.
    let r = rig();
    let sid = r.session();
    let (status, text, jobs) = shell(&r, &sid, json!({"command": "echo hello"})).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("hello"), "{text}");
    assert_eq!(jobs, 1);
}

/// The tool is the owner's own conversation's, so a task's session, which is refused it, keeps the
/// command it has.
#[tokio::test]
async fn a_task_is_not_answered_with_a_tool_it_is_refused() {
    let r = rig();
    let parent = r.session();
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (status, text, _) = shell(&r, &rec.session_id, json!({"command": "theseus health"})).await;
    assert!(!answered(status, &text), "{status:?} {text}");
}

/// A daemon with no such tool (started from no config) answers nothing: the command is the only
/// look it has.
#[tokio::test]
async fn a_daemon_without_the_tool_answers_nothing() {
    let r = rig_with(|cfg, _| cfg.config_file = None);
    let sid = r.session();
    let (status, text, _) = shell(&r, &sid, json!({"command": "theseus health"})).await;
    assert!(!answered(status, &text), "{status:?} {text}");
}
