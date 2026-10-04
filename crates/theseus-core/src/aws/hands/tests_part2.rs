//! Step 40 part 2 (theseus-mgw.11) through the whole core, on part 1's fake
//! of AWS (`tests_hands.rs`): the cancel lifecycle per backend, `until` met
//! stopping the rest, a cancel or `/stop` of the call stopping its group,
//! and a late envelope after a cancel recorded as late, never settled twice.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::{ActionState, CancelState, VerifiedBy};

use super::envelope::HandSpec;
use super::tests_hands::{
    calls, group_of, late_result, record, rig, rows, signed, state_of, turn, until, Rig,
};

/// A rig whose network stack has its NAT, so Fargate hands launch.
fn fargate_rig(input: Value) -> Rig {
    let r = rig(calls(input));
    r.state.nat.store(true, Ordering::SeqCst);
    r
}

/// The spec each RunTask carried, in launch order.
fn fargate_specs(r: &Rig) -> Vec<HandSpec> {
    r.state
        .ran
        .lock()
        .unwrap()
        .iter()
        .map(|t| {
            serde_json::from_str(
                t["overrides"]["containerOverrides"][0]["environment"][0]["value"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap()
        })
        .collect()
}

fn action(r: &Rig, corr: &str) -> theseus_kernel::Action {
    r.core.kernel.action(corr).unwrap().expect("an action")
}

/// Fargate, `until: first_success`: the first hand's success ends the group,
/// so the two still running are stopped by `StopTask` (each its own task,
/// with Theseus's reason), verified once `DescribeTasks` shows them STOPPED
/// (`verified_by: ecs`), and settled cancelled at the time they ran, not at
/// nothing. The group settles once, met, saying two were cancelled. A late
/// envelope from a stopped hand is recorded as late, and settles nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fargate_until_met_stops_the_running_hands_and_verifies_them() {
    let r = fargate_rig(json!({"argv": ["true"], "count": 3, "backend": "fargate",
        "until": "first_success", "ttl_secs": 3600}));
    r.state.stop_at_once.store(true, Ordering::SeqCst);
    let res = turn(&r.core, "find one that works").await;
    let g = group_of(&r.core);
    let specs = fargate_specs(&r);
    assert_eq!(specs.len(), 3);
    // The first hand's success, and in the same batch a second hand's
    // envelope, sent as its task was being stopped.
    r.state.push(signed(&specs[0], "succeeded", 0));
    r.state.push(signed(&specs[1], "failed", 143));
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    let stops = r.state.stops.lock().unwrap().clone();
    assert_eq!(
        stops.len(),
        2,
        "a StopTask for each running hand: {stops:?}"
    );
    let rec = record(&r.core, &g);
    for s in &specs[1..] {
        let a = action(&r, &s.correlation_id);
        assert_eq!(a.state, ActionState::Cancelled, "{a:?}");
        assert_eq!(a.cancel, Some(CancelState::TerminationVerified));
        assert_eq!(
            a.verdict.as_ref().map(|v| v.verified_by),
            Some(VerifiedBy::Ecs)
        );
        let task = rec
            .hands
            .iter()
            .find(|h| h.correlation_id == s.correlation_id)
            .and_then(|h| h.external_op_id.clone())
            .unwrap();
        assert!(stops
            .iter()
            .any(|st| st["task"] == task.as_str() && st["reason"] == "theseus: cancelled"));
    }
    // Each stopped hand cost the minute its task ran, booked as spent.
    let e = r.core.kernel.execution(&rec.execution_id).unwrap().unwrap();
    assert_eq!(e.budget.held_unknown_micros, 0);
    let s = &rows(&r.core, "aws.hands.settled");
    assert_eq!(s.len(), 1);
    assert_eq!(
        (
            s[0]["met"].as_bool(),
            s[0]["succeeded"].as_u64(),
            s[0]["cancelled"].as_u64()
        ),
        (Some(true), Some(1), Some(2))
    );
    assert_eq!(rows(&r.core, "action.cancel_verified").len(), 2);
    // The stopped hand's envelope came home late: recorded, never settled.
    until("the late row", || {
        !rows(&r.core, "completion.late_after_cancel").is_empty()
    })
    .await;
    assert_eq!(
        state_of(&r.core, &specs[1].correlation_id),
        ActionState::Cancelled
    );
    assert!(rows(&r.core, "action.failed")
        .iter()
        .all(|a| a["correlation_id"] != specs[1].correlation_id));
    assert_eq!(rows(&r.core, "aws.hands.settled").len(), 1);
    r.core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    let (text, _) = late_result(&r.core, &res.session_id);
    assert!(text.contains("1 of 3 succeeded"), "{text}");
    assert!(text.contains("2 cancelled"), "{text}");
    assert!(text.contains("ECS task STOPPED"), "{text}");
}

/// A task ECS has not stopped yet: the cancel is acknowledged, the hand
/// still open, and the group says it is being stopped. The poller's next
/// pass reads `DescribeTasks` and verifies it once STOPPED; another hand's
/// stop is verified by ECS's own state change on the queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fargate_stop_is_verified_later_by_describe_tasks_or_ecs() {
    let r = fargate_rig(json!({"argv": ["true"], "count": 3, "backend": "fargate",
        "until": "first_success"}));
    turn(&r.core, "find one that works").await;
    let g = group_of(&r.core);
    let specs = fargate_specs(&r);
    r.state.push(signed(&specs[0], "succeeded", 0));
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    for s in &specs[1..] {
        let a = action(&r, &s.correlation_id);
        assert_eq!(
            (a.state, a.cancel),
            (ActionState::Dispatched, Some(CancelState::Acknowledged))
        );
    }
    let s = &rows(&r.core, "aws.hands.settled")[0];
    assert_eq!(s["running"].as_u64(), Some(2), "{s}");
    let rec = record(&r.core, &g);
    let task = |i: usize| {
        rec.hands
            .iter()
            .find(|h| h.correlation_id == specs[i].correlation_id)
            .and_then(|h| h.external_op_id.clone())
            .unwrap()
    };
    // ECS stops the first: the poller's pass reads it.
    r.state.tasks.lock().unwrap().insert(
        task(1),
        ("STOPPED".into(), Some("theseus: cancelled".into())),
    );
    until("the first stop verified", || {
        state_of(&r.core, &specs[1].correlation_id) == ActionState::Cancelled
    })
    .await;
    // ECS's state change for the second.
    r.state.push(
        json!({"source": "aws.ecs", "detail-type": "ECS Task State Change",
            "detail": {"lastStatus": "STOPPED", "startedBy": g, "taskArn": task(2),
                "stoppedReason": "theseus: cancelled",
                "containers": [{"name": "hand", "exitCode": 143}]}})
        .to_string(),
    );
    until("the second stop verified", || {
        state_of(&r.core, &specs[2].correlation_id) == ActionState::Cancelled
    })
    .await;
    for s in &specs[1..] {
        let a = action(&r, &s.correlation_id);
        assert_eq!(a.cancel, Some(CancelState::TerminationVerified));
        assert_eq!(
            a.verdict.as_ref().map(|v| v.verified_by),
            Some(VerifiedBy::Ecs)
        );
    }
    // Nothing open: the poller goes quiet.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let n = r.state.receives.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(r.state.receives.load(Ordering::SeqCst), n);
}

/// Lambda, `until: first_success`: Lambda has no stop, so the hands still
/// running are cancelled `unsupported`, saying their timeout is the bound,
/// and nothing is sent to AWS for them. Their reservation is held until
/// their envelopes come home late: recorded as late, settled never.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lambda_until_met_marks_the_running_hands_unsupported() {
    let r = rig(calls(
        json!({"argv": ["true"], "count": 3, "until": "first_success"}),
    ));
    let res = turn(&r.core, "find one that works").await;
    let g = group_of(&r.core);
    let mut specs = r.state.invoked();
    specs.sort_by_key(|s| s.index);
    r.state.push(signed(&specs[0], "succeeded", 0));
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    assert!(r.state.stops.lock().unwrap().is_empty());
    for s in &specs[1..] {
        let a = action(&r, &s.correlation_id);
        assert_eq!(
            (a.state, a.cancel),
            (ActionState::Cancelled, Some(CancelState::Unsupported))
        );
        let why = a.verdict.as_ref().and_then(|v| v.why.clone()).unwrap();
        assert!(why.contains("timeout"), "{why}");
    }
    assert_eq!(rows(&r.core, "action.cancel_unsupported").len(), 2);
    for s in &specs[1..] {
        r.state.push(signed(s, "succeeded", 0));
    }
    until("both late rows", || {
        rows(&r.core, "completion.late_after_cancel").len() == 2
    })
    .await;
    for s in &specs[1..] {
        assert_eq!(state_of(&r.core, &s.correlation_id), ActionState::Cancelled);
        let ok = rows(&r.core, "action.succeeded")
            .into_iter()
            .filter(|a| a["correlation_id"] == s.correlation_id)
            .count();
        assert_eq!(ok, 0, "never settled twice");
    }
    r.core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    let (text, _) = late_result(&r.core, &res.session_id);
    assert!(text.contains("2 cancelled"), "{text}");
    assert!(text.contains("not verified"), "{text}");
}

/// A cancel of the execution reaches the group's call: every hand stops
/// (the running ones by `StopTask`, verified; the one never launched,
/// verified: nothing ran), the call settles cancelled with a verified
/// verdict, and the group's record and row say it was cancelled. Its
/// hands' envelopes, if they come, are late.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancel_of_the_call_stops_its_group() {
    let r = fargate_rig(json!({"argv": ["true"], "count": 3, "backend": "fargate",
        "concurrency": 2}));
    r.state.stop_at_once.store(true, Ordering::SeqCst);
    let res = turn(&r.core, "run three").await;
    let exec = res.execution_id.clone().unwrap();
    let g = group_of(&r.core);
    let specs = fargate_specs(&r);
    assert_eq!(specs.len(), 2, "two at a time");
    r.core.cancel_execution(&exec, "the CLI").await.unwrap();
    let call = action(&r, &g);
    assert_eq!(call.state, ActionState::Cancelled);
    assert_eq!(call.cancel, Some(CancelState::TerminationVerified));
    assert_eq!(r.state.stops.lock().unwrap().len(), 2);
    let rec = record(&r.core, &g);
    assert_eq!(rec.settled.as_deref(), Some("cancelled"));
    for h in &rec.hands {
        let a = action(&r, &h.correlation_id);
        assert_eq!(a.state, ActionState::Cancelled, "{a:?}");
    }
    let s = &rows(&r.core, "aws.hands.settled");
    assert_eq!(s.len(), 1);
    assert_eq!(s[0]["met"].as_bool(), Some(false));
    assert_eq!(s[0]["cancelled"].as_u64(), Some(2), "{}", s[0]);
    assert_eq!(s[0]["not_launched"].as_u64(), Some(1), "{}", s[0]);
    assert_eq!(
        rows(&r.core, "action.cancel_verified").len(),
        3,
        "two hands and the call"
    );
}

/// `/stop` of the conversation stops a Lambda group: its running hands are
/// `unsupported`, and so is the call, saying how many run on to their
/// timeout; the conversation stays open.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_of_a_lambda_group_says_its_hands_run_to_their_timeout() {
    let r = rig(calls(json!({"argv": ["true"], "count": 2})));
    let res = turn(&r.core, "run two").await;
    let exec = res.execution_id.clone().unwrap();
    let g = group_of(&r.core);
    let stop = r.core.stop_execution(&exec, "the CLI").await.unwrap();
    assert!(stop.stopped);
    let call = action(&r, &g);
    assert_eq!(
        (call.state, call.cancel),
        (ActionState::Cancelled, Some(CancelState::Unsupported))
    );
    let why = call.verdict.as_ref().and_then(|v| v.why.clone()).unwrap();
    assert!(why.contains("2 Lambda hands"), "{why}");
    let st = stop
        .verdicts
        .iter()
        .find(|v| v.correlation_id == g)
        .expect("the call's verdict");
    assert_eq!(st.state, "unsupported");
    assert!(r.state.stops.lock().unwrap().is_empty());
}

// ------------------------------------------------------------ reservations

/// A rig whose config `f` changes first.
fn rig_with(script: Vec<crate::provider::Scripted>, f: impl FnOnce(&mut crate::Config)) -> Rig {
    use super::tests_hands::{answer, config, State};
    let state = std::sync::Arc::new(State::default());
    let s = state.clone();
    let fake = crate::aws::tests::Fake::start(move |seen, n| answer(&s, seen, n));
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("THESEUS_HAND_DIR", dir.path());
    let mut cfg = config(dir.path(), &fake.url);
    f(&mut cfg);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = std::sync::Arc::new(crate::provider::FakeProvider::scripted(script));
    let core = crate::Core::build(crate::rpc::Parts {
        secrets: crate::aws::tests::board(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    Rig {
        core,
        state,
        fake,
        dir,
    }
}

fn budget(r: &Rig, exec: &str) -> theseus_kernel::Budget {
    r.core.kernel.execution(exec).unwrap().unwrap().budget
}

/// Each hand reserves its worst case (its TTL at its size's rate) in the
/// group's frame, and settles at its real cost: the envelope's duration at
/// the same rate. After the group, nothing of it is reserved or held, and
/// what it spent is exactly its hands' costs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_hand_reserves_its_worst_case_and_settles_at_its_real_cost() {
    let r = rig(calls(json!({"argv": ["true"], "count": 2})));
    let res = turn(&r.core, "run two").await;
    let exec = res.execution_id.clone().unwrap();
    let g = group_of(&r.core);
    let rec = record(&r.core, &g);
    let each = (rec.hand_max_usd * 1e6).ceil() as u64;
    assert!(each > 10_000, "a 600 s Lambda hand at 2 GB: {each}");
    let b = budget(&r, &exec);
    for h in &rec.hands {
        let a = action(&r, &h.correlation_id);
        assert_eq!(a.reserved_micros, each, "{a:?}");
        let id = a.reservation_id.clone().expect("a reservation");
        assert_eq!(b.reservations.get(&id), Some(&each));
    }
    let spent_before = b.spent_micros;
    let reserved_before = b.reserved_micros;
    assert!(reserved_before >= 2 * each);
    for s in r.state.invoked() {
        r.state.push(signed(&s, "succeeded", 0));
    }
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    let b = budget(&r, &exec);
    let real: u64 = rec
        .hands
        .iter()
        .map(|h| {
            super::group::completion(&r.core.store, &h.correlation_id)
                .and_then(|c| c.cost_micros)
                .unwrap()
        })
        .sum();
    assert!(real > 0 && real < 2 * each, "{real}");
    assert_eq!(b.reserved_micros, reserved_before - 2 * each);
    assert_eq!(b.spent_micros, spent_before + real);
    assert_eq!(b.held_unknown_micros, 0);
}

/// A group whose worst case passes what the session has left is not run:
/// nothing is planned or launched, its result says why with the figures,
/// and its turn asks the session's budget question, as a model call over
/// the limit does, and waits on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_over_the_sessions_budget_meets_its_question() {
    let r = rig_with(
        calls(json!({"argv": ["true"], "count": 20, "backend": "fargate",
            "vcpu": 4, "memory_mb": 8192, "ttl_secs": 3600})),
        |c| c.kernel.spend_limit_usd = 1.40,
    );
    r.state.nat.store(true, Ordering::SeqCst);
    let res = turn(&r.core, "run twenty big ones").await;
    let exec = res.execution_id.clone().unwrap();
    assert!(r.state.ran.lock().unwrap().is_empty(), "nothing launched");
    assert!(rows(&r.core, "aws.hands.launched").is_empty());
    let text = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult { tool, content, .. } if tool == "aws.hands.run" => {
                Some(content.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(text.contains("over the session's budget"), "{text}");
    assert!(text.contains("20 hands"), "{text}");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    let q = e.budget.question.clone().expect("the budget question");
    assert!(
        matches!(&e.wake, Some(theseus_kernel::Wake::Budget { correlation_id }) if *correlation_id == q),
        "{:?}",
        e.wake
    );
    let asked = rows(&r.core, "budget.asked");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0]["needed_usd"].as_f64().unwrap() > 3.0, "{asked:?}");
    assert!(r
        .core
        .kernel
        .actions_by(&[theseus_kernel::terms::prefix("s:")])
        .unwrap()
        .iter()
        .all(|a| a.tool != "aws.hand"));
}

// ------------------------------------------------------------ the hour

/// The hour's meter: with the line lowered to a cent, a group of two
/// Lambda hands (each reserving about two cents) passes it, and the poller's
/// pass alerts once: an `aws.hour.alert` row, the hour's mark, a notice
/// where approvals go, and health's hands block saying so with the running
/// hands and what they hold. A second group in the same hour says nothing
/// more.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hours_alert_fires_once_an_hour() {
    use crate::provider::Scripted;
    let input = json!({"argv": ["true"], "count": 2});
    let r = rig_with(
        vec![
            Scripted::tools("", &[("t1", "aws_hands_run", input.clone())]),
            Scripted::text("Running."),
            Scripted::tools("", &[("t2", "aws_hands_run", input)]),
            Scripted::text("Running again."),
        ],
        |c| {
            for a in c.aws.accounts.values_mut() {
                a.hourly_alert_usd = 0.01;
            }
        },
    );
    let hour = super::watch::hour_of(theseus_protocol::now_unix_ms());
    turn(&r.core, "run two").await;
    r.core.poll_hands_after_serving();
    until("the hour's alert", || {
        !rows(&r.core, "aws.hour.alert").is_empty()
    })
    .await;
    let alert = &rows(&r.core, "aws.hour.alert")[0];
    assert_eq!(alert["line_usd"].as_f64(), Some(0.01));
    assert!(alert["usd"].as_f64().unwrap() > 0.03, "{alert}");
    let aws = r.core.tools.aws.clone().unwrap();
    let account = aws.account(None).unwrap();
    let h = account.status().hands.expect("health's hands block");
    assert_eq!((h.running_lambda, h.running_fargate), (2, 0));
    assert_eq!(h.groups_open, 1);
    assert!(h.oldest_unix_ms.is_some());
    assert!(h.reserved_micros > 30_000, "{h:?}");
    assert_eq!(h.hour_line_micros, 10_000);
    assert_eq!(h.alerted_hour_unix_ms, Some(hour));
    let mark: Option<u64> = r
        .core
        .store
        .get_meta(&format!("{}{}", super::watch::ALERTED, account.id))
        .unwrap();
    assert_eq!(mark, Some(hour));
    let notices: Vec<String> = r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .filter_map(|a| crate::outbox::body_of(a)["text"].as_str().map(String::from))
        .collect();
    assert!(
        notices.iter().any(|n| n.contains("past its $0.01 line")),
        "{notices:?}"
    );
    // Another group in the same hour: the meter grows, the alert does not.
    turn(&r.core, "run two more").await;
    until("the second group in health", || {
        account
            .status()
            .hands
            .is_some_and(|h| h.running_lambda == 4)
    })
    .await;
    if super::watch::hour_of(theseus_protocol::now_unix_ms()) == hour {
        assert_eq!(rows(&r.core, "aws.hour.alert").len(), 1);
    }
}
