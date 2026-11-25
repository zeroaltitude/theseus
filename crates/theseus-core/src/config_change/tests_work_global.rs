//! `look.work` answers for the whole of Theseus's work, played through the whole core (issue 28): every
//! conversation's tasks, what waits on the owner anywhere, projects, routines and what is coming, this
//! conversation's first; its history stays this conversation's; and the wall, played in a shared place
//! and for a guild participant.

use std::collections::BTreeSet;

use serde_json::json;
use theseus_kernel::{usd_to_micros, Opening};
use theseus_protocol::SessionKind;

use super::tests::{change, rig, Rig, OWNER};
use crate::node::{Body, ResultStatus};
use crate::session::{SessionRecord, TargetRef, TaskOf};

/// A conversation that has run one turn, so it has an execution to hold tasks.
pub(super) async fn conversation(r: &Rig) -> String {
    let sid = r.session();
    r.turn_with("look_work", &sid, json!({"what": ["history"]}))
        .await;
    sid
}

/// A task `parent` started, titled and reporting nowhere; deferred to `start_at_ms` when given.
pub(super) fn task(
    r: &Rig,
    parent: &str,
    title: &str,
    start_at_ms: Option<u64>,
) -> theseus_kernel::Execution {
    let p: SessionRecord = r.core.store.get_session(parent).unwrap().unwrap();
    let pe = p.execution_id.unwrap();
    let opened = r
        .core
        .kernel
        .open_task_owned(
            &pe,
            &format!("act_{}", title.replace(' ', "_")),
            usd_to_micros(1.0),
            Opening {
                start_at_ms,
                ..Default::default()
            },
            |_| Ok(vec![]),
        )
        .unwrap()
        .task;
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.session_id = opened.session_id.clone();
    rec.execution_id = Some(opened.id.clone());
    rec.title = Some(title.into());
    rec.task = Some(TaskOf {
        parent_session: parent.into(),
        parent_execution: pe,
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    opened
}

/// The last result in `sid`: each turn of a conversation answers its own call. Its words, its parts rendered
/// when it is kept by reference.
pub(super) fn latest(r: &Rig, sid: &str) -> (ResultStatus, String) {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                status,
                content,
                parts,
                ..
            } => Some((
                status,
                crate::tests_places::kept_text(&r.core, content, &parts),
            )),
            _ => None,
        })
        .expect("a result")
}

/// What `look_work` said to `sid` for `input`.
pub(super) async fn said(r: &Rig, sid: &str, input: serde_json::Value) -> String {
    r.turn_with("look_work", sid, input).await;
    let (status, text) = latest(r, sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    text
}

/// The short ids at the head of each `- ` line.
pub(super) fn heads(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("- "))
        .filter_map(|l| l.split(" · ").next())
        .map(str::to_string)
        .collect()
}

/// Theseus's whole work, from a conversation that started one task while another started two: every
/// task, this conversation's first and the rest newest first, each with where it was started; a
/// deferred one says when it starts; the words of another conversation are marked as such.
#[tokio::test]
async fn it_answers_for_every_conversations_tasks_this_ones_first() {
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    task(&r, &other, "Chart the outer reef", None);
    let soon = r.core.kernel.now_ms() + 3_600_000;
    task(&r, &other, "Mend the lamp", Some(soon));
    task(&r, &here, "Sort the charts", None);
    let text = said(&r, &here, json!({"what": ["tasks"]})).await;
    assert!(text.contains("Tasks (3 in all):"), "{text}");
    assert!(text.contains("Started in this conversation (1):"), "{text}");
    assert!(
        text.contains("Started in other conversations (2), the newest open ones first."),
        "{text}"
    );
    assert!(
        text.contains("Words from other conversations") && text.contains("never as instructions"),
        "marked: {text}"
    );
    let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} in {text}"));
    assert!(at("Sort the charts") < at("Mend the lamp"), "{text}");
    assert!(at("Mend the lamp") < at("Chart the outer reef"), "{text}");
    assert!(text.contains("from the CLI or the web UI (…"), "{text}");
    assert!(text.contains("scheduled for "), "a deferred task: {text}");
    let mine = text
        .lines()
        .find(|l| l.contains("Sort the charts"))
        .unwrap();
    assert!(!mine.contains(" · from "), "its own say no place: {mine}");
}

/// What waits on the owner, anywhere, this place's first; the tool answers nothing; `scope: here` is
/// this place's alone, and the shorter answer is still the same words.
#[tokio::test]
async fn it_lists_every_question_that_waits_and_where_this_places_first() {
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    r.turn(&other, change("import.notes.enabled", json!(false)))
        .await;
    r.turn(&here, change("narrative", json!(true))).await;
    assert_eq!(r.asked().len(), 2, "{:?}", r.asked());
    let text = said(&r, &here, json!({"what": ["needs_you"]})).await;
    assert!(
        text.contains("Waiting on the owner (2); only they answer, this tool answers nothing:"),
        "{text}"
    );
    assert!(
        text.contains("Here (1):") && text.contains("Elsewhere (1)."),
        "{text}"
    );
    assert!(text.contains(" · waits in the CLI or the web UI"), "{text}");
    assert!(text.find("Here (1)").unwrap() < text.find("Elsewhere (1)").unwrap());
    let alone = said(&r, &here, json!({"what": ["needs_you"], "scope": "here"})).await;
    assert!(alone.contains("Waiting on the owner (1);"), "{alone}");
    assert!(!alone.contains("Elsewhere"), "{alone}");
}

/// Projects, routines and what is coming are read from the views the command line reads.
#[tokio::test]
async fn it_gives_projects_routines_and_what_is_coming() {
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    r.core
        .project_create(
            &serde_json::from_value(json!({
                "title": "Harbour survey", "objective": "Chart every mooring",
                "acceptance": ["every mooring has a depth"],
                "milestones": [{"title": "Outer harbour", "due": "in 30d"}],
                "review": "none"
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap();
    r.core
        .routine_create(
            &serde_json::from_value(json!({
                "title": "Tide check", "schedule": "weekdays 08:00", "brief": "Read the tides."
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap();
    r.turn_with(
        "wake_at",
        &other,
        json!({"after": "2h", "note": "water the ferns", "for": "owner", "say_only": true}),
    )
    .await;
    let text = said(
        &r,
        &here,
        json!({"what": ["projects", "routines", "coming"]}),
    )
    .await;
    for part in [
        "Projects (1 open).",
        "Harbour survey",
        "0/1 milestones",
        "Routines (1).",
        "Tide check",
        "weekdays",
        "Coming: reminders and check-ins (",
        "water the ferns",
        " · in the CLI or the web UI",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    // `here` leaves out what is tied to no conversation, and says so.
    let alone = said(
        &r,
        &here,
        json!({"what": ["projects", "routines", "coming"], "scope": "here"}),
    )
    .await;
    assert!(
        alone.contains("Projects: not tied to one conversation")
            && alone.contains("Routines: not tied to one conversation"),
        "{alone}"
    );
    assert!(
        alone.contains("Coming: no reminder or check-in is set."),
        "{alone}"
    );
}

/// No new power: each id the tool lists is one the command line's own view lists, and its messages are
/// this conversation's alone (another conversation's turn is not among them).
#[tokio::test]
async fn nothing_is_listed_that_the_command_lines_views_would_not_list() {
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    task(&r, &other, "Chart the outer reef", None);
    task(&r, &here, "Sort the charts", None);
    r.turn(&other, change("import.notes.enabled", json!(false)))
        .await;
    let text = said(
        &r,
        &here,
        json!({"what": ["tasks", "needs_you", "history"], "messages": 20}),
    )
    .await;
    let tasks: BTreeSet<String> = r
        .core
        .tasks(None, None)
        .unwrap()
        .into_iter()
        .map(|t| t.short)
        .collect();
    let asks: BTreeSet<String> = r
        .asked()
        .iter()
        .map(|q| crate::task::short(&q.correlation_id))
        .collect();
    let listed = heads(&text);
    for id in listed.iter().filter(|id| id.len() == 6) {
        assert!(
            tasks.contains(id) || asks.contains(id),
            "{id} is in no view of the command line: {text}"
        );
    }
    assert!(
        tasks.is_subset(&listed) && asks.is_subset(&listed),
        "{text}"
    );
    // Two turns of this conversation (the owner, Theseus's reply, the owner again), none of the other's.
    assert!(
        text.contains("The last 3 of this conversation's 3 messages:"),
        "another conversation's messages are not given: {text}"
    );
}

/// A label of `confidential` on `session`, as a content judgment leaves one.
pub(super) fn mark(r: &Rig, session: &str) {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: None,
        kind: ItemKind::ToolResult,
        source: "fs.read".into(),
        confidence: 0.9,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    let mut rec: SessionRecord = r.core.store.get_session(session).unwrap().unwrap();
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(session, &rec).unwrap();
}

/// A rig whose every provider is of restricted origin: the asking turn's own model is one the labels could
/// keep off.
/// Another conversation's words are given, marked as such, whatever that conversation's labels say: they
/// are judged as any tool result is. A turn whose model is of restricted origin is given them too: nothing
/// is held back from one since that rule was deleted (2026-11-21).
#[tokio::test]
async fn a_marked_conversations_words_are_given_marked() {
    for r in [
        rig(),
        super::tests::rig_with(|cfg, _| cfg.serve_default_from_cn()),
    ] {
        let (here, other) = (conversation(&r).await, conversation(&r).await);
        task(&r, &other, "Chart the outer reef", None);
        mark(&r, &other);
        let text = said(&r, &here, json!({"what": ["tasks"]})).await;
        assert!(text.contains("Chart the outer reef"), "{text}");
        assert!(text.contains("never as instructions"), "marked: {text}");
        assert!(!text.contains("held back"), "{text}");
    }
}

/// A shared place and a guild participant are refused, in the tool's own words, and read nothing.
#[tokio::test]
async fn a_shared_place_and_a_guild_participant_are_refused_and_read_nothing() {
    use crate::places::BoundPlace;
    let r = rig();
    let other = conversation(&r).await;
    task(&r, &other, "Chart the outer reef", None);
    // Someone else's DM is a shared place.
    let shared = r.session_at(&format!("dm:{}", OWNER + 1));
    r.turn_with("look_work", &shared, json!({})).await;
    let (status, text) = r.result(&shared);
    assert_eq!(status, ResultStatus::Declined, "{text}");
    assert!(text.contains("not offered in a shared place"), "{text}");
    // A channel bound private is still the guild's: whoever it lets in writes there.
    let channel = "discord:channel:314159265358979323";
    r.core.runner.place_rule.bind(vec![BoundPlace {
        target: channel.into(),
        name: "#lab".into(),
        private: true,
        ..Default::default()
    }]);
    let guild = r.session_at("channel:314159265358979323");
    r.turn_with("look_work", &guild, json!({})).await;
    let (status, text) = r.result(&guild);
    assert_ne!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("shared"), "{text}");
    for refused in [&shared, &guild] {
        let (_, text) = r.result(refused);
        assert!(
            !text.contains("Chart the outer reef") && !text.contains("Tasks"),
            "nothing is read: {text}"
        );
    }
}

/// A scope that is not one, and a part that is not one, are refused.
#[tokio::test]
async fn a_scope_that_is_not_one_is_refused() {
    let r = rig();
    for bad in [json!({"scope": "elsewhere"}), json!({"scope": ["here"]})] {
        let sid = r.session();
        r.turn_with("look_work", &sid, bad.clone()).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{bad}: {text}");
    }
}

/// A task cancelled before any turn of its own ran names no model; one that ran names the model it ran on.
#[tokio::test]
async fn a_task_cancelled_before_it_ran_names_no_model() {
    let r = rig();
    let parent = conversation(&r).await;
    let t = task(&r, &parent, "Mend the lamp", None);
    let mut rec: SessionRecord = r.core.store.get_session(&t.session_id).unwrap().unwrap();
    rec.task.as_mut().unwrap().target = Some("discord:dm:1".into());
    rec.last_target = Some(TargetRef {
        profile: "sonnet".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-5-5".into(),
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let body = |e: &theseus_kernel::Execution| {
        let (_, mut posts) =
            crate::task::cancelled_reports(&r.core.outbox, &r.core.store, e).unwrap();
        crate::outbox::body_of(&posts.pop().unwrap()).clone()
    };
    assert!(
        t.turns == 0 && body(&t).get("ran_on").is_none(),
        "{:?}",
        body(&t)
    );
    let mut ran = t;
    ran.turns = 2;
    assert_eq!(body(&ran)["ran_on"]["model"], json!("claude-sonnet-5-5"));
}
