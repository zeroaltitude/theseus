//! A request bound for a model of restricted origin carries only its own
//! session's content (theseus-9ymz0, the review's B1). A label belongs to the
//! session it was raised in; what that session wrote reaches another one
//! through recall, the task view, the project slice, the place's digest and
//! the standing records kept from a message. So a request to a restricted
//! profile leaves all of them out, and the same request on a Claude profile
//! carries every one. The rig is `tests_dm_rig`'s, `dsflash` the restricted
//! profile, with live recall for every session and a judge that finds nothing.

use std::sync::Arc;

use serde_json::json;
use theseus_judge::band::Band;
use theseus_protocol::standing::{StandingAddParams, StandingKind, StandingScope};

use crate::config::MemoryMode;
use crate::content::{ContentFlags, Flag, ItemKind, Label};
use crate::provider::FakeProvider;
use crate::tests_dm_rig::{cn_session, jev, judging, rig_with, say, text, OPEN_A};
use crate::tests_recall::{index_of, session};
use crate::tests_route::{call, mode, Rig};

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";
const ORDER: &str = "Always cc the Tidewater survey lead";
const TITLE: &str = "Tidewater harmonics";
const ASK: &str = "What was it we noted about nesting?";
/// What only A's note says: the asks never name it.
const NOTE: &str = "Millbrook";

fn rig() -> (Rig, Arc<FakeProvider>, tempfile::TempDir) {
    let jev = jev(&[]);
    mode(&jev, "chat", 0.2);
    let (r, zai, dir) = rig_with(&jev, None, |c| {
        c.memory.mode = MemoryMode::Canary;
        c.memory.canary_fraction = 1.0;
    });
    judging(&r);
    (r, zai, dir)
}

/// Session A: it read a confidential line, so it carries the label, and it
/// said what the others will draw; and a standing order kept from its words.
fn confidential_session(r: &Rig) -> String {
    let a = session(&r.core, None, &[HERON]);
    let mut rec = crate::tests_dm_rig::record(&r.core, &a);
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: Some("nod_raised".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&a, &rec).unwrap();
    let stored = crate::content_wait::flags_of(&r.core.store, &a).unwrap();
    assert!(
        stored.disqualifies_cn().is_some(),
        "A is off the restricted models"
    );
    let node = r.core.store.transcript(&a).unwrap()[0].1.id.clone();
    r.core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: ORDER.into(),
                scope: StandingScope::Everywhere,
                source: Some(format!("capture:{node}")),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    a
}

/// A project and a task, in B's scope, both titled by another session.
fn views_for(r: &Rig, b: &str) {
    let p = r
        .core
        .project_create(
            &serde_json::from_value(json!({
                "title": TITLE,
                "milestones": [{"title": TITLE}],
                "review": "none",
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap();
    r.core
        .project_attach(
            &theseus_protocol::projects::ProjectAttachParams {
                session: b.into(),
                project: Some(p.project.id),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let task = crate::task_graph::NewTask {
        id: "tsk_tidewater".into(),
        title: TITLE,
        objective: String::new(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: theseus_protocol::tasks::TaskOrigin {
            session: b.into(),
            principal: "agent".into(),
            by_model: true,
            ..Default::default()
        },
        state: theseus_protocol::tasks::TaskState::Accepted,
    }
    .build(r.core.kernel.now_ms());
    r.core
        .store
        .append(&[crate::task_graph::record(&task).unwrap()])
        .unwrap();
}

/// The key test. B is clean and every item of it was judged, so it may use
/// `dsflash`; but what its recall, its task view and its project slice carry
/// was written by A, which holds `confidential`. None of it reaches
/// `dsflash`. The same session on Claude carries every one, so nothing was
/// lost for a model that may see it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_request_carries_nothing_another_session_wrote() {
    let (r, zai, _dir) = rig();
    let a = confidential_session(&r);
    let b = cn_session(&r).await;
    views_for(&r, &b);
    r.core
        .runner
        .memory
        .set_ask(index_of(&r.core, vec![a.clone()]));

    let res = say(&r.core, &b, ASK, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    assert_eq!(res.recalled, 1, "A's note was drawn for B");
    let body = text(&zai.requests()[0]);
    for (what, left_out) in [
        ("the recall", NOTE),
        ("the task view", crate::task_graph::view::HEAD),
        ("the project slice", crate::project::slice::HEAD),
        ("a record kept from A's words", ORDER),
        ("A's titles", TITLE),
    ] {
        assert!(!body.contains(left_out), "{what} reached dsflash: {body}");
    }

    call(&r.core, "profile.use", json!({"name": "default"})).await;
    let before = r.claude.requests().len();
    say(&r.core, &b, "And does it nest there every year?", None).await;
    let sent = r.claude.requests();
    assert!(sent.len() > before);
    let body = text(sent.last().unwrap());
    for (what, kept) in [
        ("the recall", NOTE),
        ("the task view", crate::task_graph::view::HEAD),
        ("the project slice", crate::project::slice::HEAD),
        ("a record kept from A's words", ORDER),
    ] {
        assert!(body.contains(kept), "{what} is missing on Claude: {body}");
    }
}

/// The recall note B drew on a Claude turn stays in B's transcript, and a
/// later turn of B on `dsflash` does not carry it: the cut is made when the
/// request is compiled, not when the note is drawn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_drawn_on_a_claude_turn_stays_out_of_the_next_restricted_one() {
    let (r, zai, _dir) = rig();
    let a = confidential_session(&r);
    let b = session(&r.core, None, &[]);
    r.core
        .runner
        .memory
        .set_ask(index_of(&r.core, vec![a.clone()]));
    call(&r.core, "profile.use", json!({"name": "default"})).await;
    let res = say(&r.core, &b, ASK, None).await;
    assert_eq!(res.recalled, 1);
    assert!(text(r.claude.requests().last().unwrap()).contains(NOTE));

    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    let res = say(&r.core, &b, OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(&zai.requests()[0]);
    assert!(!body.contains(NOTE), "{body}");
    assert!(body.contains(OPEN_A), "its own message is there: {body}");
}
