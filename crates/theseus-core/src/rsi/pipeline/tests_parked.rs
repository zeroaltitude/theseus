//! A build parked on a question to the owner, and a question that outlives its build (issue 20).
//! The rig is the pipeline's own (`tests::rig_with`: a fake builder that opens a real tree, a real
//! session and a real execution), and each test parks the started build the way a turn that ends on
//! a call the floor holds does: the execution takes a turn, plans a call that waits for the owner,
//! and ends the turn waiting on it.

use std::time::Duration;

use serde_json::json;
use theseus_kernel::{ExecState, Proposal, RetryClass, TurnEnd, Wake};
use theseus_protocol::tasks::{TaskEvidence, TaskState};

use super::tests::{rig_with, Rig};
use crate::node::{Body, Node};
use crate::rsi::tree;

/// The build of `item` parked on a question: its session, execution and the question's id.
fn park(r: &Rig, item: &str) -> (String, String, String) {
    let t = crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap();
    let session = t.session.clone().expect("a started item has a session");
    let exec = t
        .evidence
        .iter()
        .find_map(|e| e.identity.strip_prefix("execution:"))
        .expect("and an execution")
        .to_string();
    let k = &r.core.kernel;
    k.wake_input(&exec).unwrap();
    let guard = k.admit(&exec).unwrap();
    let p = Proposal {
        tool: "proc.run".into(),
        args: json!({"argv": ["true"]}),
        resource: None,
        policy_context: serde_json::Value::Null,
    };
    let a = k
        .plan_confirm(&guard, &p, RetryClass::SafeToRepeat, Some(600_000))
        .unwrap();
    // The call's node, which is what a card is made from (`Core::confirm_request`).
    let call = Node::tool_call(
        &session,
        Some("trn_parked"),
        Some(0),
        Body::ToolCall {
            tool_use_id: "tu_parked".into(),
            tool: "proc.run".into(),
            wire_name: "proc_run".into(),
            input: json!({"argv": ["true"]}),
            assistant_node: "asn_parked".into(),
            correlation_id: Some(a.correlation_id.clone()),
            gate: None,
        },
    );
    r.core.store.append(&[call.record().unwrap()]).unwrap();
    let wake = Wake::Confirm {
        confirm_id: a.correlation_id.clone(),
    };
    k.end_turn(guard, TurnEnd::Wait { wake }).unwrap();
    (session, exec, a.correlation_id)
}

/// One started build, parked, and a rig whose grace is over: before this change such a build was
/// read as a turn that had ended with no branch.
async fn parked() -> (Rig, String, String, String, String) {
    let mut r = rig_with(1, 30.0);
    r.grace = Duration::ZERO;
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    let (session, exec, question) = park(&r, &item);
    (r, item, session, exec, question)
}

/// The notices that say a question is still waiting, as posted.
fn notices(r: &Rig) -> Vec<serde_json::Value> {
    r.core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .map(|a| crate::outbox::body_of(&a).clone())
        .filter(|b| b["purpose"] == "question_waiting")
        .collect()
}

/// The execution no longer waits on a confirmation, and nothing queued it.
fn assert_ends_wait(r: &Rig, exec: &str) {
    let e = r.core.kernel.execution(exec).unwrap().unwrap();
    assert!(
        !matches!(e.wake, Some(Wake::Confirm { .. })),
        "{:?} {:?}",
        e.state,
        e.wake
    );
    assert!(
        !matches!(e.state, ExecState::Queued | ExecState::Running),
        "{:?}",
        e.state
    );
}

fn open(r: &Rig, q: &str) -> bool {
    r.core.kernel.action(q).unwrap().unwrap().awaits_confirm()
}

/// KEY: a build whose turn ended on a question is building, waiting on the owner, with its slot,
/// its working copy and its question: it is not failed and nothing is cleared. `theseus self pipeline`
/// says what it waits for and how to answer.
#[tokio::test]
async fn a_build_parked_on_a_question_is_building_and_says_it_waits_on_the_owner() {
    let (r, item, _, _, question) = parked().await;
    for _ in 0..2 {
        r.tick().await;
    }
    assert_eq!(r.state(&item), TaskState::InProgress);
    assert!(tree::find(&r.f.place, &item).is_some(), "its working copy");
    assert!(open(&r, &question), "its question is still wanted");
    assert!(r.rows("self.pipeline.ended").is_empty());
    assert!(r.rows("self.pipeline.cleared").is_empty());
    let v = r.view();
    assert_eq!(v.building.len(), 1, "{:?}", v.building);
    assert_eq!(v.room.building, 1, "it holds its slot");
    let why = v.building[0].why.as_deref().unwrap();
    assert!(why.starts_with("waiting on you: "), "{why}");
    assert!(why.contains("`true`"), "{why}");
    let id = crate::task::short(&question);
    assert!(why.contains(&format!("`theseus answer {id}`")), "{why}");
    // The CLI prints the first 200 characters of it.
    assert!(why.chars().count() <= 200, "{why}");
}

/// A question is withdrawn when its working copy is gone: the question closes, its card says so in
/// plain words, its reminders stop, and the execution it parked is stopped.
#[tokio::test]
async fn a_question_is_withdrawn_when_its_working_copy_is_removed() {
    let (r, item, _, exec, question) = parked().await;
    // A card for it, as the owner's DM would have it (a question that waited past its first ping).
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let at = theseus_protocol::now_unix_ms() + 3_600_000;
    assert_eq!(r.core.escalate_questions(at), 1, "its card is posted");
    // The working copy goes (the way a closed or removed tree does).
    let t = tree::find(&r.f.place, &item).unwrap();
    tree::close(&r.f.place, &t).unwrap();
    r.tick().await;
    let a = r.core.kernel.action(&question).unwrap().unwrap();
    assert!(!a.awaits_confirm(), "{a:?}");
    let how = crate::outbox::Closed::of(&a).unwrap();
    assert_eq!(how.how, "withdrawn");
    assert_eq!(
        how.note.as_deref(),
        Some("no longer needed: the build it belonged to has ended")
    );
    // Its card is edited, in those words.
    let settles: Vec<_> = r
        .core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "settle")
        .map(|a| crate::outbox::body_of(&a).clone())
        .collect();
    assert_eq!(settles.len(), 1, "{settles:?}");
    assert_eq!(settles[0]["closed"]["how"], "withdrawn");
    assert!(settles[0]["closed"]["note"]
        .as_str()
        .unwrap()
        .contains("no longer needed: the build it belonged to has ended"));
    // The parked execution was stopped: it waits on nothing, so nothing the owner says wakes it.
    assert_ends_wait(&r, &exec);
    // Its reminders stop: no notice for it any more, however long it would have waited.
    let before = notices(&r).len();
    r.core.expire_questions(at + 10 * 3_600_000);
    assert_eq!(notices(&r).len(), before);
}

/// An answer after the withdrawal is late: it is refused and recorded, binds nothing, runs nothing,
/// spends nothing and wakes nothing.
#[tokio::test]
async fn an_approval_after_the_withdrawal_runs_nothing_spends_nothing_and_is_recorded_as_late() {
    let (r, item, session, exec, question) = parked().await;
    let t = tree::find(&r.f.place, &item).unwrap();
    tree::close(&r.f.place, &t).unwrap();
    r.tick().await;
    let spent = |r: &Rig| {
        r.core
            .kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .budget
            .spent_micros
    };
    let (before, nodes, turns) = (
        spent(&r),
        r.core.store.session_nodes(&session).unwrap().len(),
        r.rows("turn.started").len(),
    );
    let err = r
        .core
        .confirm_action(&question, true, None, "test")
        .expect_err("a late answer does nothing");
    let said = format!("{err:#}");
    assert!(said.contains("late"), "{said}");
    assert!(said.contains("no longer needed"), "{said}");
    // Recorded as a refused act, once.
    let refused = r.rows("approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    // Changed nothing: the action, the execution, the spend, the transcript and the turns.
    let a = r.core.kernel.action(&question).unwrap().unwrap();
    assert!(a.confirm.is_none() && !a.awaits_confirm(), "{a:?}");
    assert_eq!(spent(&r), before);
    assert_eq!(r.core.store.session_nodes(&session).unwrap().len(), nodes);
    assert_eq!(r.rows("turn.started").len(), turns);
    assert_ends_wait(&r, &exec);
}

/// Every way a build ends or is replaced withdraws its question: its item failed, set aside,
/// dropped or sent back to be built again, and a newer start of the same item, which supersedes its
/// session. A question that is still wanted (its build parked, tree open) is not.
#[tokio::test]
async fn every_way_a_build_ends_withdraws_its_question() {
    type End = fn(&mut theseus_protocol::tasks::TaskRecord);
    let ends: [(&str, End); 5] = [
        ("failed", |t| t.state = TaskState::Failed),
        ("set aside", |t| t.state = TaskState::WaitingHuman),
        ("dropped", |t| t.state = TaskState::Abandoned),
        ("retried", |t| t.state = TaskState::Proposed),
        ("superseded", |t| {
            t.evidence.push(TaskEvidence {
                node: None,
                identity: "session:ses_a_newer_start".into(),
                note: None,
                by: "self-backlog".into(),
                at_ms: theseus_protocol::now_unix_ms() + 1,
            });
        }),
    ];
    for (what, end) in ends {
        let (r, item, _, _, question) = parked().await;
        assert!(open(&r, &question), "{what}: wanted while it builds");
        let mut t = crate::task_graph::get(&r.core.store, &item)
            .unwrap()
            .unwrap();
        end(&mut t);
        t.version += 1;
        r.core
            .store
            .append(&[crate::task_graph::record(&t).unwrap()])
            .unwrap();
        r.tick().await;
        let a = r.core.kernel.action(&question).unwrap().unwrap();
        assert!(!a.awaits_confirm(), "{what}: {a:?}");
        assert_eq!(
            crate::outbox::Closed::of(&a).unwrap().how,
            "withdrawn",
            "{what}"
        );
    }
}

/// A question of a session that is no self build's is not read: a task of any other principal never
/// names it.
#[test]
fn only_a_self_items_session_names_a_question() {
    let mut t = theseus_protocol::tasks::TaskRecord {
        session: Some("ses_x".into()),
        ..Default::default()
    };
    t.origin.principal = "operator".into();
    assert!(super::parked::item_of(&[t.clone()], "ses_x").is_none());
    t.origin.principal = "self".into();
    assert!(super::parked::item_of(&[t], "ses_x").is_some());
}

/// A restart with a parked build: a daemon over the same store reads it as parked again, with its
/// working copy and its question kept, and says so; when the working copy went before the restart,
/// the first tick of the new daemon withdraws the question instead. Never a live question for a dead
/// build.
#[tokio::test]
async fn a_restart_with_a_parked_build_keeps_the_wait_or_ends_the_question() {
    let (r, item, _, _, question) = parked().await;
    let again = crate::Core::build(crate::rpc::Parts::for_tests(
        (*r.core.cfg).clone(),
        std::sync::Arc::new(crate::provider::FakeProvider::default()),
        r.core.store.clone(),
    ))
    .unwrap();
    again.pipeline_tick(&r.seams()).await.unwrap();
    assert!(open(&r, &question), "a parked build waits across a restart");
    let v = again
        .self_pipeline_with(
            &theseus_protocol::rsi::SelfPipelineParams::default(),
            &r.seams(),
        )
        .unwrap();
    assert_eq!(v.building.len(), 1);
    assert!(v.building[0]
        .why
        .as_deref()
        .unwrap()
        .starts_with("waiting on you: "));
    assert_eq!(r.state(&item), TaskState::InProgress);
    // The working copy goes while the daemon is down; the next start withdraws what was asked.
    let t = tree::find(&r.f.place, &item).unwrap();
    tree::close(&r.f.place, &t).unwrap();
    let after = crate::Core::build(crate::rpc::Parts::for_tests(
        (*r.core.cfg).clone(),
        std::sync::Arc::new(crate::provider::FakeProvider::default()),
        r.core.store.clone(),
    ))
    .unwrap();
    after.pipeline_tick(&r.seams()).await.unwrap();
    assert!(!open(&r, &question), "no live question for a dead build");
}

/// A self build's question is told at a life, at two, and then once an hour; each says what builds,
/// what it wants and how to answer. The plain question's cadence (a life, every life) is the other
/// tests' (`tests_m3::question_waits`).
#[tokio::test]
async fn a_self_builds_reminders_come_twice_a_life_apart_and_then_once_an_hour() {
    let (r, item, _, _, question) = parked().await;
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let planned = r
        .core
        .kernel
        .action(&question)
        .unwrap()
        .unwrap()
        .planned_at_ms;
    assert_eq!(r.core.kernel.config().confirm_ttl_ms, 15 * 60_000, "a life");
    let told = |minute: u64| {
        r.core.expire_questions(planned + minute * 60_000);
        notices(&r).len()
    };
    for (minute, count) in [
        (14, 0),
        (15, 1),
        (29, 1),
        (30, 2),
        (45, 2),
        (89, 2),
        (90, 3),
        (149, 3),
        (150, 4),
    ] {
        assert_eq!(told(minute), count, "after {minute} minutes");
    }
    let title = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap()
        .title;
    let said = notices(&r);
    let text = |i: usize| said[i]["text"].as_str().unwrap().to_string();
    for i in 0..4 {
        let t = text(i);
        assert!(t.contains(&title), "{t}");
        assert!(t.contains("is waiting on you"), "{t}");
        assert!(t.contains("it wants to "), "{t}");
        assert!(t.contains("`true`"), "{t}");
        assert!(
            t.contains(&format!(
                "`theseus answer {}`",
                crate::task::short(&question)
            )),
            "{t}"
        );
    }
    assert!(
        text(0).contains("once more in 15 minutes, then once an hour"),
        "{}",
        text(0)
    );
    assert!(text(3).contains("once an hour from here"), "{}", text(3));
    assert!(text(3).contains("2 hours 30 minutes"), "{}", text(3));
}

/// KEY: the one way the pipeline removes a working copy (`Core::close_item_trees`) ends the item's
/// session and withdraws its question as part of the same call, not as a later sweep's work: the call
/// alone leaves the tree gone, the session ended, the question withdrawn, and nothing of it able to run.
/// (Before this, the tree went first and the session's next call failed at launch: three rows of the
/// evidence for "proc.run is failing".)
#[tokio::test]
async fn closing_an_items_trees_ends_its_session_and_withdraws_its_question() {
    let (r, item, _, exec, question) = parked().await;
    assert!(tree::find(&r.f.place, &item).is_some());
    let mut w = super::Write {
        mark: super::Mark::read(&r.core.store).unwrap(),
        frame: vec![],
        expected: vec![],
        said: vec![],
        dirty: false,
        told: vec![],
    };
    let entry = super::queues::Entry {
        item: item.clone(),
        ..Default::default()
    };
    r.core
        .close_item_trees(&r.f.place, &entry, "closed", &mut w)
        .await
        .unwrap();
    assert!(tree::find(&r.f.place, &item).is_none(), "the tree is gone");
    let a = r.core.kernel.action(&question).unwrap().unwrap();
    assert_eq!(crate::outbox::Closed::of(&a).unwrap().how, "withdrawn");
    assert_ends_wait(&r, &exec);
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert!(e.state.is_terminal(), "its session is ended: {:?}", e.state);
    // Nothing of it runs afterwards: the answer is late, and no turn starts.
    let turns = r.rows("turn.started").len();
    assert!(r
        .core
        .confirm_action(&question, true, None, "test")
        .is_err());
    assert_eq!(r.rows("turn.started").len(), turns);
}

/// And through a tick: an item that failed with its working copy open has its tree removed and its
/// session ended and its question withdrawn by the same tick.
#[tokio::test]
async fn a_failed_items_tick_ends_its_session_and_removes_its_tree() {
    let (r, item, _, exec, question) = parked().await;
    let mut t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    t.state = TaskState::Failed;
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    r.tick().await;
    assert!(tree::find(&r.f.place, &item).is_none());
    assert!(!open(&r, &question));
    assert!(r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .state
        .is_terminal());
}

/// A ready item's earlier session is ended even when it has no question: an idle session of a build
/// the owner restarted must not wake onto the tree a new start reuses.
#[tokio::test]
async fn a_restarted_items_idle_session_is_ended() {
    let mut r = rig_with(1, 30.0);
    r.grace = Duration::ZERO;
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    let t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    let exec = t
        .evidence
        .iter()
        .find_map(|e| e.identity.strip_prefix("execution:"))
        .unwrap()
        .to_string();
    assert!(!r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .state
        .is_terminal());
    let mut t = t;
    t.state = TaskState::Proposed;
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    r.tick().await;
    assert!(r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .state
        .is_terminal());
}

/// Every path that removes a working copy ends the item's sessions first. The builder's own
/// `self.propose_change` spares the one session making the call, and ends any other.
#[tokio::test]
async fn ending_an_items_sessions_spares_only_the_one_making_the_call() {
    let (r, item, session, exec, _) = parked().await;
    let live = |r: &Rig| {
        !r.core
            .kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .state
            .is_terminal()
    };
    assert_eq!(
        r.core
            .end_item_sessions(&item, Some(&session))
            .await
            .unwrap(),
        0
    );
    assert!(live(&r), "the spared session is untouched");
    assert_eq!(r.core.end_item_sessions(&item, None).await.unwrap(), 1);
    assert!(!live(&r));
    assert_eq!(
        r.core.end_item_sessions(&item, None).await.unwrap(),
        0,
        "once"
    );
}

/// An item built again after a restart is not failed for the earlier start's ended session: the
/// backlog's sweep reads the item's LATEST execution.
#[tokio::test]
async fn a_restarted_item_is_not_swept_failed_for_its_earlier_ended_session() {
    let (r, item, _, exec, question) = parked().await;
    // The owner retries it: back to proposed. The next tick ends the old session and starts it anew.
    let mut t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    t.state = TaskState::Proposed;
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    r.tick().await;
    assert!(r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .state
        .is_terminal());
    assert!(!open(&r, &question));
    assert_eq!(r.fake.starts.lock().unwrap().len(), 2, "built again");
    assert_eq!(r.state(&item), TaskState::InProgress);
    r.core
        .backlog_night(theseus_protocol::now_unix_ms())
        .unwrap();
    assert_eq!(
        r.state(&item),
        TaskState::InProgress,
        "the new build is not judged by the old session's end"
    );
}
