//! A failed task stays flagged until the owner dismisses it, every surface
//! agrees, and a retry opens a linked new task (theseus-7zgsm), through
//! whole cores: the push's execution board, the work board, a restart's
//! seed, the place rule, and the carve.

use std::sync::Arc;

use serde_json::json;
use theseus_kernel::ExecState;
use theseus_protocol::work::{WorkState, WorkView};
use theseus_protocol::{Level, TaskRetryResult};

use crate::approval::{Answerer, Refusal, Surface};
use crate::provider::{FakeProvider, ProviderError, Scripted};
use crate::rpc::Parts;
use crate::test_support::ledger_rows_2k as rows;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, last_user, only_task, parent_session, rig, turn, until,
    Rig, ASK,
};
use crate::Core;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A person in a shared channel nobody bound as private.
fn stranger() -> Answerer {
    Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "9001".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    }
}

/// A conversation that starts one task with a $1.50 limit, whose every try
/// fails with a 400 unless it is a retry (its brief says so), and a retry
/// whose note says `FAILAGAIN` fails too.
fn rig_with_a_failing_task() -> Rig {
    rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD:") {
                let retry = first.contains("This is a retry of task");
                return if retry && !first.contains("FAILAGAIN") {
                    Scripted::text("Counted to three on the second try.")
                } else {
                    Scripted::Fail(ProviderError::InvalidRequest {
                        status: 400,
                        message: "the tool schema was refused".into(),
                    })
                };
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            if last.starts_with("START") {
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
        |_| {},
    )
}

/// Run to the task's failure: the parent's session and the failed task.
async fn failed_task(r: &Rig) -> (String, theseus_kernel::Execution) {
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    until("the task fails", || {
        crate::tests_tasks::exec(&r.core, &task.id).state == ExecState::Failed
    })
    .await;
    let failed = crate::tests_tasks::exec(&r.core, &task.id);
    (parent, failed)
}

/// A restarted daemon's core, over a copy of `c`'s store taken now (the
/// first core still holds the original open).
fn restarted(c: &Core) -> Arc<Core> {
    let from = std::path::Path::new(&c.cfg.server.state_dir).join("store");
    let to = std::path::Path::new(&c.cfg.server.state_dir).join("store-restarted");
    let ok = std::process::Command::new("cp")
        .arg("-r")
        .arg(&from)
        .arg(&to)
        .status()
        .unwrap()
        .success();
    assert!(ok, "copied the store");
    let store = crate::store::Store::open(&to).unwrap();
    let model = Arc::new(FakeProvider::default());
    Core::build(Parts::for_tests((*c.cfg).clone(), model, store)).unwrap()
}

/// The execution board's view of an execution.
fn exec_view(core: &Core, id: &str) -> theseus_protocol::ExecutionView {
    let (_, views, _) = core.push.snapshot(1000);
    views.into_iter().find(|v| v.execution_id == id).unwrap()
}

fn work_view(core: &Core, id: &str) -> WorkView {
    core.push.work_all().remove(id).unwrap()
}

/// A failed task needs you on every surface until it is dismissed; one
/// dismissal moves it out of needs-you on the execution board, the work board,
/// its parent's rollup and the short line, in one frame; and a daemon
/// restarted over the same store, and its fresh seed, still say dismissed.
#[tokio::test]
async fn a_dismissal_moves_a_failure_out_of_needs_you_everywhere_and_survives_a_restart() {
    let r = rig_with_a_failing_task();
    let (parent, failed) = failed_task(&r).await;
    r.core.push.ensure(&r.core).await.unwrap();
    let id = failed.id.clone();

    // Flagged: failed, needs you, on both boards, and in the parent's rollup.
    let v = exec_view(&r.core, &id);
    assert_eq!(v.attention.level, Level::NeedsYou, "{v:?}");
    assert!(
        v.attention.label.starts_with("failed: "),
        "{}",
        v.attention.label
    );
    assert!(!v.dismissed);
    let w = work_view(&r.core, &id);
    assert_eq!(w.attention.level, Level::NeedsYou);
    assert_eq!(w.state, WorkState::Failed);
    let root = work_view(
        &r.core,
        &crate::tests_tasks::parent_exec(&r.core, &parent).id,
    );
    assert_eq!(root.below.unwrap().failed, 1);
    assert_eq!(r.core.push.status_short().failed.len(), 1);
    // Looking at it does not clear it: a second look finds it the same.
    assert_eq!(exec_view(&r.core, &id).attention.level, Level::NeedsYou);

    // One dismissal, one frame.
    let before = theseus_store::frames_written_here();
    let done = r.core.dismiss_failure(&id, &cli()).unwrap();
    assert_eq!(theseus_store::frames_written_here() - before, 1);
    assert!(done.written);
    until("the boards take the dismissal", || {
        exec_view(&r.core, &id).dismissed
    })
    .await;

    let v = exec_view(&r.core, &id);
    assert_eq!(v.attention.level, Level::Idle, "{v:?}");
    assert!(
        v.attention.label.ends_with("· dismissed"),
        "{}",
        v.attention.label
    );
    assert_eq!(v.state, "failed", "the record stays");
    let w = work_view(&r.core, &id);
    assert!(w.dismissed);
    assert_eq!(w.attention.level, Level::Idle);
    assert_eq!(w.state, WorkState::Failed);
    assert_eq!(
        w.report.as_ref().map(|r| r.outcome.as_str()),
        Some("failed"),
        "the view keeps its report"
    );
    assert!(w.report.unwrap().reason.unwrap().contains("400"));
    let root = work_view(
        &r.core,
        &crate::tests_tasks::parent_exec(&r.core, &parent).id,
    );
    assert_eq!(
        root.below.unwrap().failed,
        0,
        "the parent no longer counts it"
    );
    assert!(r.core.push.status_short().failed.is_empty());
    let rows = rows(&r.core, "execution.dismissed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["execution_id"], id.as_str());

    // A restart, and the board's seed from the store: still dismissed.
    let again = restarted(&r.core);
    again.push.ensure(&again).await.unwrap();
    let v = exec_view(&again, &id);
    assert!(v.dismissed, "{v:?}");
    assert_eq!(v.attention.level, Level::Idle);
    let w = work_view(&again, &id);
    assert!(w.dismissed);
    assert_eq!(w.attention.level, Level::Idle);
    assert!(again.push.status_short().failed.is_empty());
    // And the seed equals the replay: the same views, dismissed or not.
    let (a, b) = (
        work_view(&again, &id).attention,
        work_view(&r.core, &id).attention,
    );
    assert_eq!((a.level, a.label), (b.level, b.label));
}

/// A retry opens a new task with the same brief and pieces, linked by
/// `retry_of`, carved no larger than the failed task's; the failed one is
/// dismissed in the same frame; the new one runs and completes; a retry of a
/// retry links to its predecessor.
#[tokio::test]
async fn a_retry_opens_a_linked_task_that_runs_and_a_retry_of_a_retry_links_back() {
    let r = rig_with_a_failing_task();
    let (parent, failed) = failed_task(&r).await;
    r.core.push.ensure(&r.core).await.unwrap();
    let before = theseus_store::frames_written_here();
    let first: TaskRetryResult = r
        .core
        .retry_failed(&failed.id, Some("FAILAGAIN: try a smaller step"), &cli())
        .unwrap();
    assert_eq!(
        theseus_store::frames_written_here() - before,
        1,
        "a retry is one frame"
    );
    assert_eq!(first.retry_of, failed.id);
    assert_eq!(first.limit_usd, 1.5);
    assert_eq!(first.failed_limit_usd, 1.5);
    let one = crate::tests_tasks::exec(&r.core, &first.execution_id);
    assert_eq!(one.retry_of.as_deref(), Some(failed.id.as_str()));
    assert_eq!(one.parent, failed.parent);
    assert_eq!(
        one.budget.limit_micros, failed.budget.limit_micros,
        "no larger"
    );
    assert!(crate::tests_tasks::exec(&r.core, &failed.id)
        .dismissal()
        .is_some());

    // The same brief and the same pieces, and what the owner added.
    let words = |sid: &str| match crate::task::brief(&r.core.store, sid)
        .unwrap()
        .unwrap()
        .body
    {
        crate::node::Body::UserMessage { text, .. } => text,
        b => panic!("{b:?}"),
    };
    let (old, new) = (words(&failed.session_id), words(&first.task_id));
    assert!(old.ends_with("CHILD: count to three"), "{old}");
    assert!(new.contains("CHILD: count to three"), "{new}");
    assert!(new.contains("FAILAGAIN: try a smaller step"), "{new}");
    let pieces = |sid: &str| {
        let rec: crate::session::SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
        crate::task::arrangement_of(&r.core.store, Some(&rec))
            .unwrap()
            .pieces
    };
    assert_eq!(pieces(&first.task_id), pieces(&failed.session_id));
    assert_eq!(
        rows(&r.core, "task.retried")[0].data["retry_of"],
        failed.id.as_str()
    );

    // The work board links it.
    until("the work board links the retry", || {
        r.core
            .push
            .work_all()
            .get(&first.execution_id)
            .is_some_and(|v| v.retry_of.is_some())
    })
    .await;
    let w = work_view(&r.core, &first.execution_id);
    assert_eq!(w.retry_of.as_deref(), Some(failed.id.as_str()));
    assert!(work_view(&r.core, &failed.id).dismissed);

    // That try fails too (its note said so); retrying it links to it, and
    // that one runs to the end.
    until("the first retry fails", || {
        crate::tests_tasks::exec(&r.core, &first.execution_id).state == ExecState::Failed
    })
    .await;
    let second = r
        .core
        .retry_failed(&first.execution_id, None, &cli())
        .unwrap();
    assert_eq!(second.retry_of, first.execution_id);
    let two = crate::tests_tasks::exec(&r.core, &second.execution_id);
    assert_eq!(two.retry_of.as_deref(), Some(first.execution_id.as_str()));
    let brief = words(&second.task_id);
    assert!(brief.contains("CHILD: count to three"), "{brief}");
    assert!(
        !brief.contains("FAILAGAIN"),
        "the first note is not carried: {brief}"
    );
    until("the last retry completes", || {
        crate::tests_tasks::exec(&r.core, &second.execution_id).state == ExecState::Complete
    })
    .await;
    let _ = parent;
}

/// A retry carries the labels of the task it retries (theseus-gyjyo.5): the
/// task started under a parent that read a calendar, a Google label on it, and
/// its retry is no way around the label. The retry's session is made from the
/// old task's record, which used to walk up to its parent to find the mark.
#[tokio::test]
async fn a_retry_carries_the_labels_of_the_task_it_retries() {
    let r = rig_with_a_failing_task();
    let parent = parent_session(&r.core);
    crate::test_support::label_session(&r.core, &parent, crate::content::Flag::Personal);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    until("the task fails", || {
        crate::tests_tasks::exec(&r.core, &task.id).state == ExecState::Failed
    })
    .await;
    let failed = crate::tests_tasks::exec(&r.core, &task.id);
    let labels = |sid: &str| {
        let rec: crate::session::SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
        rec.data.map(|d| d.labels).unwrap_or_default()
    };
    let old = labels(&failed.session_id);
    assert_eq!(old.len(), 1, "the task carries its parent's label: {old:?}");
    let retried: TaskRetryResult = r.core.retry_failed(&failed.id, None, &cli()).unwrap();
    assert_eq!(
        labels(&retried.task_id),
        old,
        "the retry carries the failed task's labels"
    );
}

/// A retry carves no more than the failed task had, though the parent has
/// far more left, and no more than the parent has when it has less.
#[tokio::test]
async fn a_retry_never_carves_more_than_the_failed_task_had() {
    let r = rig_with_a_failing_task();
    let (parent, failed) = failed_task(&r).await;
    let parent_exec = crate::tests_tasks::parent_exec(&r.core, &parent);
    assert!(
        parent_exec.budget.available() > failed.budget.limit_micros * 2,
        "the parent has far more left than the task had"
    );
    let done = r.core.retry_failed(&failed.id, None, &cli()).unwrap();
    assert!(done.limit_usd <= done.failed_limit_usd, "{done:?}");
    assert_eq!(done.limit_usd, 1.5);
}

/// The place rule: a person in a shared place can neither dismiss nor retry;
/// the refusal is ledgered and nothing changes. And a retry of a task whose
/// parent has ended is refused, saying why.
#[tokio::test]
async fn a_shared_place_is_refused_and_a_retry_with_no_parent_says_why() {
    let r = rig_with_a_failing_task();
    let (parent, failed) = failed_task(&r).await;
    let tasks_before = r.core.kernel.tasks(None).unwrap().len();

    let e = r.core.dismiss_failure(&failed.id, &stranger()).unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    let e = r
        .core
        .retry_failed(&failed.id, None, &stranger())
        .unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    assert!(crate::tests_tasks::exec(&r.core, &failed.id)
        .dismissal()
        .is_none());
    assert_eq!(r.core.kernel.tasks(None).unwrap().len(), tasks_before);
    assert_eq!(rows(&r.core, "approval.refused").len(), 2);

    // The conversation was cancelled: nothing to carve from, nowhere to report.
    let parent_exec = crate::tests_tasks::parent_exec(&r.core, &parent);
    r.core
        .kernel
        .cancel_execution(&parent_exec.id, "the owner")
        .unwrap();
    let e = r.core.retry_failed(&failed.id, None, &cli()).unwrap_err();
    assert!(format!("{e:#}").contains("has ended"), "{e:#}");
    assert_eq!(r.core.kernel.tasks(None).unwrap().len(), tasks_before);
    assert!(crate::tests_tasks::exec(&r.core, &failed.id)
        .dismissal()
        .is_none());
}

/// Only a failed task is retried, only a failure dismissed.
#[tokio::test]
async fn only_a_failure_is_dismissed_and_only_a_failed_task_retried() {
    let r = rig_with_a_failing_task();
    let (parent, failed) = failed_task(&r).await;
    let conv = crate::tests_tasks::parent_exec(&r.core, &parent);
    let e = r.core.retry_failed(&conv.id, None, &cli()).unwrap_err();
    assert!(format!("{e:#}").contains("is not a task"), "{e:#}");
    let e = r.core.dismiss_failure(&conv.id, &cli()).unwrap_err();
    assert!(format!("{e:#}").contains("not a failure"), "{e:#}");
    let e = r.core.dismiss_failure("exe_nope", &cli()).unwrap_err();
    assert!(format!("{e:#}").contains("unknown execution"), "{e:#}");
    // A second dismissal answers the first and writes nothing.
    let a = r.core.dismiss_failure(&failed.id, &cli()).unwrap();
    let before = theseus_store::frames_written_here();
    let b = r.core.dismiss_failure(&failed.id, &cli()).unwrap();
    assert_eq!((b.at_ms, b.written), (a.at_ms, false));
    assert_eq!(theseus_store::frames_written_here(), before);
}
