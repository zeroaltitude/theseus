//! `profile.run` played through the whole core (issue 28): "use the haiku profile to DO x" hands
//! the profile a task, started at once, that works with tools; its report speaks in the
//! conversation as the subagent's message, once, in order before the asking turn's reply, whether
//! it came in time or late; and everything the tool can set in motion was already reachable by the
//! same turn (a task started from the conversation, a conversation held to that profile).

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::SubagentAnswer;

use super::tests::{rig, rig_with, Rig};
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::profile_tool::{ask_if_task_asks, RUN};

/// The owner's place: a private DM.
const PLACE: &str = "dm:4100000000000000001";
/// What the agent reports.
const REPORT: &str = "Listed the folder: a.txt, b.txt";
/// The work, in the words the agent is recognised by in the rig.
const WORK: &str = "AGENT: list the files of the folder and report them";

fn work(profile: &str) -> serde_json::Value {
    json!({"profile": profile, "work": WORK})
}

/// The posts a place was written, in order: their kind and their body.
fn posts(r: &Rig) -> Vec<(String, serde_json::Value)> {
    r.core
        .outbox
        .open_for(&format!("discord:{PLACE}"))
        .iter()
        .map(|a| {
            (
                crate::outbox::kind_of(a).to_string(),
                crate::outbox::body_of(a).clone(),
            )
        })
        .collect()
}

/// The tasks of the conversation `sid`.
fn tasks(r: &Rig, sid: &str) -> Vec<theseus_kernel::Execution> {
    let rec: crate::session::SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    r.core.kernel.tasks(rec.execution_id.as_deref()).unwrap()
}

/// The names of the tools the session called.
fn called(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolCall { tool, .. } => Some(tool),
            _ => None,
        })
        .collect()
}

/// "Use haiku to list the folder": ONE call of the new tool (no shell), a task on the haiku
/// profile whose model is given tools, started at once with a task's own limit, its report read
/// by the asking turn as the task's report is, and `profile.ask` not involved.
#[tokio::test]
async fn work_handed_to_a_profile_is_done_by_a_task_that_has_tools() {
    let r = rig();
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("Handed the work to haiku (model claude-haiku-5-5"),
        "{text}"
    );
    assert!(text.contains("works with tools"), "{text}");
    assert_eq!(
        called(&r, &sid),
        [RUN],
        "no shell, and no question to a profile"
    );
    // The agent's model got the work AND tools; the asking turn's own model was a different one.
    let seen = r.seen();
    assert!(
        seen.iter()
            .any(|(m, tools)| m == "claude-haiku-5-5" && *tools > 0),
        "the profile's model is offered tools: {seen:?}"
    );
    // One task, pinned to the profile, with the limit a task's default is.
    let ts = tasks(&r, &sid);
    assert_eq!(ts.len(), 1, "{ts:?}");
    let task_rec: crate::session::SessionRecord = r
        .core
        .store
        .get_session(&ts[0].session_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        task_rec.last_target.as_ref().map(|t| t.profile.as_str()),
        Some("haiku")
    );
    assert!(
        !crate::turn::brief_step::marked(&r.core.store, &ts[0].session_id),
        "pinned"
    );
    let parent = r
        .core
        .kernel
        .execution(ts[0].parent.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        ts[0].budget.limit_micros <= parent.budget.limit_micros / 4,
        "a quarter at most: {} of {}",
        ts[0].budget.limit_micros,
        parent.budget.limit_micros
    );
    // Its report was read in the asking turn, once: the parent has none left to read.
    assert!(parent.reports.is_empty(), "{:?}", parent.reports);
    let read: Vec<String> = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } => text,
            _ => String::new(),
        })
        .collect();
    assert_eq!(read.len(), 1, "{read:?}");
    assert!(read[0].contains(REPORT), "{}", read[0]);
    // And what the agent spent counts against the conversation, as any task's does.
    assert!(
        ts[0].budget.spent_micros > 0,
        "the agent's calls were priced"
    );
    assert!(
        parent.budget.spent_micros >= ts[0].budget.spent_micros,
        "the carry: {} of the conversation's {}",
        ts[0].budget.spent_micros,
        parent.budget.spent_micros
    );
}

/// The report is a message of the conversation in its own right, from the agent: after the call
/// and BEFORE the asking turn's reply, once, with the model that ran in its footer and the profile
/// of the turn that handed the work over. The task's own report post is not a second message here:
/// its place is this one.
#[tokio::test]
async fn the_report_speaks_in_the_conversation_once_before_the_reply() {
    let r = rig();
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let posts = posts(&r);
    let kinds: Vec<&str> = posts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["subagent", "reply"], "{posts:?}");
    let said: SubagentAnswer = serde_json::from_value(posts[0].1["answer"].clone()).unwrap();
    assert_eq!(said.text, REPORT);
    assert_eq!(
        (said.profile.as_str(), said.model.as_str()),
        ("haiku", "claude-haiku-5-5")
    );
    assert_eq!(said.asked_by, "sonnet");
    assert!(said
        .footer()
        .starts_with("haiku · claude-haiku-5-5 · subagent answer, asked by sonnet"));
    assert_eq!(
        said.call,
        tasks(&r, &sid)[0].session_id,
        "keyed to the task: a retry posts it once"
    );
    assert_eq!(posts[1].1["result"]["model"], json!("claude-sonnet-5-5"));
}

/// Not in time: the tool says the agent is still working and the turn goes on; when the agent
/// ends, its report is delivered to the conversation as the same subagent's message, once, and
/// the conversation's next turn reads it as a task's report.
#[tokio::test(start_paused = true)]
async fn a_late_report_is_delivered_to_the_conversation_when_it_comes() {
    let r = rig();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    r.agents_report(REPORT, Some(gate.clone()));
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("still working"), "{text}");
    let before: Vec<String> = posts(&r).into_iter().map(|(k, _)| k).collect();
    assert_eq!(before, ["reply"], "nothing from the agent yet: {before:?}");
    gate.add_permits(1);
    let mut spoke = vec![];
    for _ in 0..200 {
        spoke = posts(&r)
            .into_iter()
            .filter(|(k, _)| k == "subagent")
            .collect();
        if !spoke.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(spoke.len(), 1, "{:?}", posts(&r));
    assert_eq!(spoke[0].1["answer"]["text"], json!(REPORT));
    assert_eq!(spoke[0].1["answer"]["model"], json!("claude-haiku-5-5"));
    // The parent has it to read at its next turn, as it has any task's report.
    let ts = tasks(&r, &sid);
    let parent = r
        .core
        .kernel
        .execution(ts[0].parent.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(parent.reports, [ts[0].id.clone()]);
}

/// A name that is not a profile is told in the layer's words, and no task opens and no model is called.
#[tokio::test]
async fn an_unknown_profile_opens_nothing() {
    let r = rig();
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("nope"))]).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("`nope` is not a configured profile"),
        "{text}"
    );
    assert!(tasks(&r, &sid).is_empty());
    assert!(
        r.seen().iter().all(|(m, _)| m != "claude-haiku-5-5"),
        "{:?}",
        r.seen()
    );
}

/// A conversation whose labels keep a model of restricted origin off it may not hand work to one:
/// the floor's refusal is the one a turn on that profile meets, said before any task opens.
#[tokio::test]
async fn a_profile_the_labels_forbid_is_refused_before_a_task_opens() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    let r = rig_with(|cfg, _| cfg.vouch_zai(crate::catalog::ModelOrigin::Cn));
    r.agents_report(REPORT, None);
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: Some("nod_earlier".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    let sid = r.session_at(PLACE);
    let mut rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&sid, &rec).unwrap();
    r.steps(&sid, vec![("profile_run", work("glm"))]).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("keeps a model of restricted origin off it"),
        "{text}"
    );
    assert!(tasks(&r, &sid).is_empty());
}

/// What a model may not set is not there to set: a budget, a wake, a start time, a profile of
/// its own under another name, the tools: each is refused as an unknown field, and no task opens.
#[tokio::test]
async fn nothing_but_the_profile_and_the_work_can_be_named() {
    let r = rig();
    r.agents_report(REPORT, None);
    for extra in [
        json!({"budget_usd": 50.0}),
        json!({"wake_parent": true}),
        json!({"start_after": "1h"}),
        json!({"check_of": "x"}),
        json!({"tools": ["proc_run"]}),
        json!({"arrangement": {"pieces": []}}),
    ] {
        let sid = r.session_at(PLACE);
        let mut input = work("haiku");
        for (k, v) in extra.as_object().unwrap() {
            input[k] = v.clone();
        }
        r.steps(&sid, vec![("profile_run", input.clone())]).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{input}: {text}");
        assert!(tasks(&r, &sid).is_empty(), "{input}");
    }
    assert!(r.seen().iter().all(|(m, _)| m != "claude-haiku-5-5"));
}

/// A task's session may not hand work to a profile: no agent starts agents through this tool.
#[tokio::test]
async fn a_task_may_not_hand_work_to_a_profile() {
    let r = rig();
    r.agents_report(REPORT, None);
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
    let sid = rec.session_id;
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.starts_with("profile.run is not for a task's session"),
        "{text}"
    );
    assert!(r.seen().iter().all(|(m, _)| m != "claude-haiku-5-5"));
}

/// Where starting a task is held to ask first, handing work to a profile asks too: the owner's
/// setting for `task.create` is not gone round by a tool of its own name. Nothing starts until
/// the owner says yes.
#[tokio::test]
async fn where_a_task_asks_first_so_does_the_hand_over() {
    let r = rig_with(|cfg, _| {
        cfg.policy
            .tools
            .insert("task.create".into(), Posture::Approve);
        cfg.policy.tools.insert(RUN.into(), Posture::Open);
    });
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let asked = r.asked();
    assert_eq!(asked.len(), 1, "one card: {asked:?}");
    assert!(tasks(&r, &sid).is_empty(), "nothing started before the yes");
    assert!(r.seen().iter().all(|(m, _)| m != "claude-haiku-5-5"));
}

/// The gate's hook: only `profile.run`, only where `task.create` asks, and an authority already
/// there is the one that stands.
#[test]
fn the_hook_touches_only_the_hand_over_and_only_where_a_task_asks() {
    let plan = || theseus_tools::Plan {
        summary: "x".into(),
        ..Default::default()
    };
    assert!(ask_if_task_asks(RUN, true, plan()).authority.is_some());
    assert!(ask_if_task_asks(RUN, false, plan()).authority.is_none());
    for other in ["profile.ask", "profile.hold", "task.create", "proc.run"] {
        assert!(
            ask_if_task_asks(other, true, plan()).authority.is_none(),
            "{other}"
        );
    }
    let mut held = plan();
    held.authority = Some("outside text".into());
    assert_eq!(
        ask_if_task_asks(RUN, true, held).authority.as_deref(),
        Some("outside text")
    );
}
