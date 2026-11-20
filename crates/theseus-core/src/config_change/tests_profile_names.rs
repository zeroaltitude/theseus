//! The line that names the configured profiles (issue 37), played through the whole core: a first
//! turn "use <profile> to ..." in the owner's own private place is sent the profiles' names, and a
//! place, a task or a setup with one profile that the tools refuse or have no use for is not.

use super::tests::{rig, rig_with, Rig, OWNER};
use crate::profile_tool::{ASK, RUN};
use crate::session::{SessionRecord, TaskOf};

/// The owner's place: a private DM.
const PLACE: &str = "dm:4100000000000000001";
const LEAD: &str = "Model profiles configured here: ";

/// Everything the turn's first request says, as text.
fn first_request(r: &Rig) -> String {
    let sent = r.sent();
    let first = sent.first().expect("the turn asked the model");
    serde_json::to_string(&first.messages).unwrap()
}

/// What `r`'s core would name: every configured profile, by name.
fn names(r: &Rig) -> Vec<String> {
    r.core.cfg.all_profiles().keys().cloned().collect()
}

/// The first request of a turn in `place` that begins "use haiku to ...".
async fn played(r: &Rig, place: &str) -> String {
    r.says("use haiku to say hello");
    let sid = r.session_at(place);
    r.turn_with("fs_list", &sid, serde_json::json!({"path": "."}))
        .await;
    first_request(r)
}

/// The owner's private place is told the names, in the settings' own order, and the stand-in model
/// is sent them in the turn that says "use haiku to ...".
#[tokio::test]
async fn the_owners_private_place_is_sent_the_names() {
    let r = rig();
    let names = names(&r);
    assert!(names.len() > 1, "{names:?}");
    let sent = played(&r, PLACE).await;
    let line = format!(
        "{LEAD}{}. You can hand one a piece of work or ask it one question (search the tools for `profile`).",
        names.join(", ")
    );
    assert!(sent.contains(&line), "{sent}");
    assert!(sent.contains("haiku"), "the profile the owner named");
    assert_eq!(sent.matches(LEAD).count(), 1, "once");
}

/// Where the tools are refused, no line: another person's direct message, a channel the guild
/// writes in, and a task's session.
#[tokio::test]
async fn a_shared_place_and_a_task_are_not_told() {
    use crate::places::BoundPlace;
    let r = rig();
    let shared = played(&r, &format!("dm:{}", OWNER + 1)).await;
    assert!(!shared.contains(LEAD), "{shared}");
    r.core.runner.place_rule.bind(vec![BoundPlace {
        target: "discord:channel:314159265358979323".into(),
        name: "#lab".into(),
        private: true,
        ..Default::default()
    }]);
    let guild = played(&r, "channel:314159265358979323").await;
    assert!(!guild.contains(LEAD), "{guild}");

    let parent = r.session();
    let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.task = Some(TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    r.turn_with("fs_list", &rec.session_id, serde_json::json!({"path": "."}))
        .await;
    let task = first_request(&r);
    assert!(!task.contains(LEAD), "{task}");
}

/// With one profile there is nothing to choose between, and with the tools gone from the place's
/// offer there is no way to use one: no line.
#[tokio::test]
async fn one_profile_or_none_is_not_told() {
    let r = rig_with(|cfg, _| {
        for n in cfg.all_profiles().keys().cloned().collect::<Vec<_>>() {
            if n != "haiku" {
                cfg.remove_profile(&n);
            }
        }
    });
    assert_eq!(names(&r), ["haiku"]);
    let sent = played(&r, PLACE).await;
    assert!(!sent.contains(LEAD), "{sent}");
}

/// A profile added or removed is in the line, as it is in the tools' own list.
#[tokio::test]
async fn the_line_follows_the_settings() {
    let r = rig_with(|cfg, _| {
        cfg.copy_profile("mythos", "haiku");
        cfg.remove_profile("opus");
    });
    let sent = played(&r, PLACE).await;
    assert!(sent.contains("mythos"), "{sent}");
    assert!(!sent.contains(", opus"), "{sent}");
    assert_eq!(
        r.core.tools.profiles.keys().cloned().collect::<Vec<_>>(),
        names(&r),
        "the tools' own list is the same set"
    );
}

/// The tools the line speaks of are the ones the wall gates: the line names the same family.
#[test]
fn the_line_names_the_family_the_tools_are_found_under() {
    assert!(ASK.starts_with("profile.") && RUN.starts_with("profile."));
    let line = crate::profile_tool::names::line(["a", "b"].into_iter()).unwrap();
    assert!(line.contains("`profile`"), "{line}");
}
