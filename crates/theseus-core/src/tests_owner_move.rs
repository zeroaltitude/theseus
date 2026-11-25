//! Which sessions are the owner's own conversation, for the move of his `theseusd` command into L1
//! (issue 57; `rsi::own_tree::owners_conversation`), through the real producers of each: his
//! conversation and its fork are moved and he is told; a task his turn starts, a routine's run
//! fired by its wake, a fork of either, and a session whose execution is gone all ask as before,
//! whatever authority they inherited, and none is told.
//!
//! The one call each session makes is `sh -c 'true -p theseusd'`: its one tie to the floor is the
//! program name read after `-p`. The unit tests' binary cannot be L1's init, so a moved job fails as
//! it starts; what a test reads is whether the owner was asked (`tool.confirm_requested`), whether a
//! job started (`tool.job_started`, its class), and the notes staged to him.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionForkParams, SessionKind};

use crate::approval::Surface;
use crate::provider::{ProviderRequest, Scripted};
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::test_support::session_rows_1k;
use crate::tests_tasks::{answers_a_call, arranged, first_user, last_user, rig, turn, until, ASK};

const CHILD: &str = "CHILD: run the daemon crate's tests";
const ROUTINE: &str = "ROUTINE: run the daemon crate's tests";

/// The call: a shell line whose one tie to the floor is `theseusd` after `-p`.
fn call() -> Scripted {
    Scripted::tools(
        "Running it.",
        &[(
            "p1",
            "proc_run",
            json!({"argv": ["sh", "-c", "true -p theseusd"]}),
        )],
    )
}

/// The owner's `START:` starts a task; a task's or a run's brief, and every other turn, makes the
/// call; a result ends it.
fn model() -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    |req| {
        if answers_a_call(req) {
            return Scripted::text("Done.");
        }
        // A task's brief comes first in its own transcript and in a fork of it.
        if first_user(req).contains("CHILD:") || first_user(req).contains("ROUTINE:") {
            return call();
        }
        if last_user(req).contains("START:") {
            return Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": CHILD, "arrangement": arranged()}),
                )],
            );
        }
        call()
    }
}

/// What the session's one call met: (asked, the classes of the jobs it started, the notes told of
/// it).
fn met(core: &Core, sid: &str) -> (usize, Vec<String>, usize) {
    let asked = session_rows_1k(core, sid, "tool.confirm_requested");
    assert!(asked.iter().all(|a| a["floor"] == true), "{asked:?}");
    let jobs = session_rows_1k(core, sid, "tool.job_started")
        .iter()
        .map(|j| j["class"].as_str().unwrap_or_default().to_string())
        .collect();
    let short = crate::task::short(sid);
    let told = crate::tests_places::told_notes(core)
        .iter()
        .filter(|t| t.contains(&format!("`{short}`")))
        .count();
    (asked.len(), jobs, told)
}

/// Moved and told: no question, one L1 job, one note.
fn moved(core: &Core, sid: &str, what: &str) {
    assert_eq!(met(core, sid), (0, vec!["l1".to_string()], 1), "{what}");
}

/// Asked as before: one floor question, no job, no note.
fn asked(core: &Core, sid: &str, what: &str) {
    assert_eq!(
        met(core, sid),
        (1, vec![], 0),
        "{what}: {:#?}",
        held(core, sid)
    );
}

/// A conversation the owner opens, as the CLI does: not bound to any place.
fn conversation(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// A fork of `session` at its first message of the person (a task's brief).
fn fork(core: &Core, session: &str) -> String {
    let first = core
        .store
        .session_nodes(session)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .expect("a message of the person")
        .1
        .id;
    core.session_fork(
        SessionForkParams {
            session: session.into(),
            at: first,
            ..Default::default()
        },
        Surface::Cli,
    )
    .unwrap()
    .session_id
}

/// Wait until `sid`'s one call was asked, started a job, or has its result; then, if it was not
/// asked, what the session holds, to say why.
async fn settled(core: &Arc<Core>, sid: &str) {
    let done = || {
        !session_rows_1k(core, sid, "tool.confirm_requested").is_empty()
            || !session_rows_1k(core, sid, "tool.job_started").is_empty()
            || results(core, sid) > 0
    };
    until("the call to be asked, started or answered", done).await;
}

/// How many tool results `sid` holds.
fn results(core: &Core, sid: &str) -> usize {
    core.store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| matches!(n.body, crate::node::Body::ToolResult { .. }))
        .count()
}

/// What `sid` holds, for a failure's words: each node's kind and its first words.
fn held(core: &Core, sid: &str) -> Vec<String> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .map(|(_, n)| {
            let all = serde_json::to_string(&n.body).unwrap_or_default();
            all.chars().take(300).collect()
        })
        .collect()
}

/// The session whose question names it, once one is asked: the first new session of `kind` with a
/// floor question.
async fn asked_in(core: &Arc<Core>, kind: SessionKind, not: &[String]) -> String {
    let found = || -> Option<String> {
        core.store
            .ledger_tail::<crate::ledger::LedgerRow>(1000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "tool.confirm_requested")
            .filter_map(|(_, r)| r.session_id)
            .filter(|s| !not.contains(s))
            .find(|s| {
                core.store
                    .get_session::<SessionRecord>(s)
                    .ok()
                    .flatten()
                    .is_some_and(|r| r.kind == kind)
            })
    };
    until("a floor question", || found().is_some()).await;
    found().unwrap()
}

/// The owner's own conversation is moved and told; so is a fork of it. A task his turn starts, with
/// the operator's authority it inherited, asks; so does a fork of that task.
#[tokio::test(flavor = "multi_thread")]
async fn his_conversation_and_its_fork_are_moved_and_his_task_and_its_fork_ask() {
    let r = rig(model(), |_| {});
    r.core.tools.sandbox.set_l1_verified(true);
    let owner = conversation(&r.core);
    turn(&r.core, &owner, "RUN: the daemon crate's tests").await;
    moved(&r.core, &owner, "his conversation");

    let his_fork = fork(&r.core, &owner);
    turn(&r.core, &his_fork, "RUN: again").await;
    moved(&r.core, &his_fork, "a fork of his conversation");

    let starter = conversation(&r.core);
    turn(&r.core, &starter, ASK).await;
    let exec = crate::tests_tasks::only_task(&r.core, &starter);
    let task = exec.session_id.clone();
    settled(&r.core, &task).await;
    assert_eq!(
        exec.authority.principal,
        crate::turn::OPERATOR,
        "the task inherited his authority"
    );
    asked(&r.core, &task, "a task his turn started");

    let task_fork = fork(&r.core, &task);
    turn(&r.core, &task_fork, "RUN: again").await;
    settled(&r.core, &task_fork).await;
    asked(&r.core, &task_fork, "a fork of his task");
}

/// A routine's run, fired by its own wake, asks, though its routine was opened with the operator's
/// authority; so does a fork of the run. The routine's own session is not his conversation either.
#[tokio::test(flavor = "multi_thread")]
async fn a_routines_fired_run_and_its_fork_ask() {
    let r = rig(model(), |c| c.kernel.min_repeat_minutes = 1);
    r.core.tools.sandbox.set_l1_verified(true);
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let made: Value = crate::tests_routines::rpc(
        &r.core,
        "routine.create",
        json!({"title": "Daemon tests", "schedule": "every 1m", "brief": ROUTINE}),
    )
    .await
    .unwrap();
    let routine_session = made["routine"]["session_id"].as_str().map(str::to_string);
    crate::tests_routines::due_soon(&r.core).await;
    let run = asked_in(&r.core, SessionKind::Task, &[]).await;
    let rec: SessionRecord = r.core.store.get_session(&run).unwrap().unwrap();
    assert!(rec.task.is_some(), "a routine's run is a task: {rec:?}");
    asked(&r.core, &run, "a routine's fired run");

    let run_fork = fork(&r.core, &run);
    turn(&r.core, &run_fork, "RUN: again").await;
    settled(&r.core, &run_fork).await;
    asked(&r.core, &run_fork, "a fork of a routine's run");

    let spec = r.core.runner.tools.routines.all(&r.core.store).unwrap()[0].clone();
    if let Some(s) = routine_session {
        assert_eq!(s, spec.session_id);
    }
    assert!(!crate::rsi::own_tree::owners_conversation(
        &r.core.runner.tools,
        &spec.session_id
    ));
}

/// The owner's conversation is decided from real records and fails closed: a conversation whose
/// execution is gone, one with no execution, and one with no record are not his.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_whose_records_are_missing_is_not_his() {
    let r = rig(model(), |_| {});
    let rt = &r.core.runner.tools;
    let owner = conversation(&r.core);
    turn(&r.core, &owner, "RUN: the daemon crate's tests").await;
    assert!(crate::rsi::own_tree::owners_conversation(rt, &owner));

    let mut gone = SessionRecord::new(SessionKind::Conversation, None);
    gone.execution_id = Some("exe_gone".into());
    r.core.store.put_session(&gone.session_id, &gone).unwrap();
    assert!(!crate::rsi::own_tree::owners_conversation(
        rt,
        &gone.session_id
    ));

    let none = conversation(&r.core);
    assert!(!crate::rsi::own_tree::owners_conversation(rt, &none));
    assert!(!crate::rsi::own_tree::owners_conversation(rt, "ses_never"));
}
