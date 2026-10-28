//! `task.rehome` (theseus-jid7t.2): a task's report goes to another
//! conversation or thread, the budget stays carved from the original parent,
//! and what the floor of data and places refuses is refused.

use serde_json::json;
use theseus_kernel::ExecState;
use theseus_protocol::ops::{DataMark, TaskRehomeParams};
use theseus_protocol::threads::ThreadNewParams;

use crate::places::BoundPlace;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::test_support::{owner_cli as cli, stranger, turn_in};
use crate::tests_judge::kinds;
use crate::tests_tasks::{
    arranged, exec, first_user, last_user, only_task, parent_exec, parent_session, posts,
    rig as task_rig, turn, until, Rig, ASK,
};
use crate::Config;

const TARGET: &str = "discord:dm:42";
const PRIVATE_CHANNEL: &str = "discord:channel:77";

fn google_on(c: &mut Config) {
    c.google.account = "owner@example.test".into();
    c.secrets.insert(
        "google_client".into(),
        "op://vault/google client/notesPlain".into(),
    );
    c.secrets.insert(
        "google_refresh_token".into(),
        "file:/tmp/unused-google-refresh-token".into(),
    );
}

/// A conversation that starts one task, which waits at the gate for its one
/// model call and then reports.
fn rig_with_a_task() -> Rig {
    let r = task_rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("Counted to three.");
            }
            if last_user(req).starts_with("START") {
                return Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                               "arrangement": arranged()}),
                    )],
                );
            }
            Scripted::text("Fine.")
        },
        google_on,
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    r
}

fn params(task: &str, home: &str) -> TaskRehomeParams {
    TaskRehomeParams {
        task: task.into(),
        home: home.into(),
        author: None,
        discord: None,
    }
}

/// A conversation with no place (the terminal's: private).
fn terminal_home(r: &Rig) -> String {
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// A conversation in a channel bound private, or shared.
fn channel_home(r: &Rig, channel: &str, private: bool) -> String {
    let sid = terminal_home(r);
    r.core.outbox.bind_place(&channel[8..], &sid).unwrap();
    r.core.runner.place_rule.bind_one(BoundPlace {
        target: channel.into(),
        name: "a channel".into(),
        private,
        ..Default::default()
    });
    sid
}

/// Label `sid` `confidential`, as a Google read does.
fn mark(r: &Rig, sid: &str) {
    crate::test_support::label_session(&r.core, sid, crate::content::Flag::Confidential);
}

fn task_of(r: &Rig, sid: &str) -> SessionRecord {
    r.core.store.get_session(sid).unwrap().unwrap()
}

/// Nothing changed: the record, the execution and the ledger.
fn unchanged(r: &Rig, task: &theseus_kernel::Execution) {
    assert_eq!(
        task_of(r, &task.session_id).task.unwrap().target.as_deref(),
        Some(TARGET)
    );
    assert_eq!(exec(&r.core, &task.id).reports_to.as_deref(), Some(TARGET));
    assert!(kinds(&r.core.store, "task.rehomed").is_empty());
}

#[tokio::test]
async fn the_next_report_lands_in_the_new_home_and_the_carve_stays_with_the_parent() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    let home = channel_home(&r, PRIVATE_CHANNEL, true);
    let home_exec = {
        // A conversation that has spoken has an execution and a budget of its own.
        let said = turn_in(&r.core, Some(&home), "hello").await;
        assert_eq!(said.session_id, home);
        parent_exec(&r.core, &home)
    };
    let (reserved, spent) = {
        let p = parent_exec(&r.core, &parent);
        (p.budget.reserved_micros, p.budget.spent_micros)
    };
    assert!(reserved > 0, "the carve is held");
    r.entered.recv().await.unwrap();
    let got = r
        .core
        .task_rehome(&params(&task.id, &home), &cli())
        .unwrap();
    assert_eq!(
        (got.from.as_deref(), got.to.as_deref()),
        (Some(TARGET), Some(PRIVATE_CHANNEL))
    );
    assert!(
        got.note.contains("carved from its original parent")
            && got.note.contains("next turn still reads"),
        "{}",
        got.note
    );
    // Where it reports, in every record that says so.
    let e = exec(&r.core, &task.id);
    assert_eq!(e.reports_to.as_deref(), Some(PRIVATE_CHANNEL));
    assert_eq!(
        task_of(&r, &task.session_id)
            .task
            .unwrap()
            .target
            .as_deref(),
        Some(PRIVATE_CHANNEL)
    );
    assert_eq!(
        r.core.outbox.target(&task.session_id).as_deref(),
        Some(PRIVATE_CHANNEL)
    );
    // The carve did not move: the execution's parent is the same.
    assert_eq!(e.parent, task.parent);
    assert_eq!(
        parent_exec(&r.core, &parent).budget.reserved_micros,
        reserved
    );
    // Its report goes there, and not to the old place.
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let reports = posts(&r.core, "report");
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(crate::outbox::target_of(&reports[0]), PRIVATE_CHANNEL);
    // The spend settled on the original parent, and the new home paid nothing.
    let p = parent_exec(&r.core, &parent);
    assert!(p.budget.spent_micros > spent, "the parent paid");
    assert_eq!(p.budget.reserved_micros, 0, "its carve settled");
    assert!(
        p.reports.contains(&task.id),
        "the parent's next turn still reads it"
    );
    let h = parent_exec(&r.core, &home);
    assert_eq!(
        (h.budget.spent_micros, h.budget.reserved_micros),
        (
            home_exec.budget.spent_micros,
            home_exec.budget.reserved_micros
        )
    );
    // One row, with who and where from and to.
    let rows = kinds(&r.core.store, "task.rehomed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["task"], task.id.as_str());
    assert_eq!(rows[0].data["session_id"], task.session_id.as_str());
    assert_eq!(rows[0].data["from"], TARGET);
    assert_eq!(rows[0].data["to"], PRIVATE_CHANNEL);
    assert_eq!(rows[0].data["by"], "the CLI");
}

#[tokio::test]
async fn a_thread_is_a_home_and_a_conversation_with_no_place_leaves_the_board_alone() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "harbor".into(),
                title: Some("Harbor".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    r.entered.recv().await.unwrap();
    // A thread no place has shown has no place to post at: the report stays on the board.
    let got = r
        .core
        .task_rehome(&params(&task.id, "thread:harbor"), &cli())
        .unwrap();
    assert_eq!((got.from.as_deref(), got.to), (Some(TARGET), None));
    let e = exec(&r.core, &task.id);
    assert_eq!(e.reports_to, None);
    assert_eq!(task_of(&r, &task.session_id).task.unwrap().target, None);
    assert_eq!(
        r.core.outbox.target(&task.session_id),
        None,
        "the place is let go in memory"
    );
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    assert!(
        posts(&r.core, "report").is_empty(),
        "nothing is posted to a place: {:?}",
        posts(&r.core, "report")
            .iter()
            .map(|a| crate::outbox::target_of(a).to_string())
            .collect::<Vec<_>>()
    );
    // The old place is gone for a restarted index too: its record says none
    // (null, which a build that reads a string binds nothing from).
    assert_eq!(
        r.core
            .store
            .get_meta::<Option<String>>(&format!("task.place.{}", task.session_id))
            .unwrap(),
        Some(None)
    );
    assert!(parent_exec(&r.core, &parent).reports.contains(&task.id));
}

#[tokio::test]
async fn an_ended_task_a_missing_home_and_a_shared_place_are_refused() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    let said = |e: anyhow::Error| format!("{e:#}");
    let e = r
        .core
        .task_rehome(&params(&task.id, "ses_missing"), &cli())
        .unwrap_err();
    assert!(said(e).contains("no home ses_missing"));
    let e = r
        .core
        .task_rehome(&params(&task.id, "thread:nowhere"), &cli())
        .unwrap_err();
    assert!(said(e).contains("no thread nowhere"));
    let shared = channel_home(&r, "discord:channel:9001", false);
    let e = r
        .core
        .task_rehome(&params(&task.id, &shared), &cli())
        .unwrap_err();
    assert!(said(e).contains("shared place"));
    let e = r
        .core
        .task_rehome(&params(&task.id, &task.session_id), &cli())
        .unwrap_err();
    assert!(said(e).contains("itself a task"));
    unchanged(&r, &task);
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let home = terminal_home(&r);
    let e = r
        .core
        .task_rehome(&params(&task.id, &home), &cli())
        .unwrap_err();
    assert!(said(e).contains("has ended"));
}

#[tokio::test]
async fn a_private_task_never_reports_into_an_open_home() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    // The task read Google after it started: it carries the label.
    mark(&r, &task.session_id);
    let (open, private) = (terminal_home(&r), terminal_home(&r));
    mark(&r, &private);
    for (home, mark) in [(&open, DataMark::Open), (&private, DataMark::Private)] {
        assert_eq!(crate::content_wait::mark_of(&r.core.store, home), mark);
    }
    let e = r
        .core
        .task_rehome(&params(&task.id, &open), &cli())
        .unwrap_err();
    let said = format!("{e:#}");
    assert!(
        said.contains("a report never goes to a weaker home"),
        "{said}"
    );
    unchanged(&r, &task);
    // An equally private home takes it.
    r.core
        .task_rehome(&params(&task.id, &private), &cli())
        .unwrap();
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    // An open task may report into a private home.
}

#[tokio::test]
async fn only_the_owner_from_a_private_place_moves_a_report() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    let home = terminal_home(&r);
    let e = r
        .core
        .task_rehome(&params(&task.id, &home), &stranger())
        .unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains("owner") || said.contains("private"), "{said}");
    unchanged(&r, &task);
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
}

/// No verb posts to a place: the outbox holds what it held.
#[tokio::test]
async fn moving_a_report_posts_nothing() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap();
    let home = channel_home(&r, PRIVATE_CHANNEL, true);
    let before = r.core.kernel.outbox_actions().unwrap().len();
    r.core
        .task_rehome(&params(&task.id, &home), &cli())
        .unwrap();
    assert_eq!(r.core.kernel.outbox_actions().unwrap().len(), before);
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == ExecState::Complete
    })
    .await;
    let _ = turn_in;
}

#[tokio::test]
async fn a_rehome_to_nowhere_writes_a_null_place_a_rollback_reads_as_none() {
    let mut r = rig_with_a_task();
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "harbor".into(),
                title: Some("Harbor".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    r.entered.recv().await.unwrap();
    let got = r
        .core
        .task_rehome(&params(&task.id, "thread:harbor"), &cli())
        .unwrap();
    assert_eq!(got.to, None);
    let key = format!("task.place.{}", task.session_id);
    let record = r
        .core
        .store
        .get_meta::<serde_json::Value>(&key)
        .unwrap()
        .expect("the record is there");
    assert!(record.is_null(), "{record}");
    // A build that reads the record as a string (1.0.1's rebuild) binds nothing.
    assert!(serde_json::from_value::<String>(record).is_err());
    // This build's rebuild: no target for the task.
    let again = crate::outbox::Outbox::new(r.core.store.clone(), r.core.kernel.clone());
    assert_eq!(again.target(&task.session_id), None);
    r.model.gate.add_permits(1);
}
