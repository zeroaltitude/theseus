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
    // Health's block is set just after the alert's row is written.
    until("health's alert", || {
        account
            .status()
            .hands
            .is_some_and(|h| h.alerted_hour_unix_ms.is_some())
    })
    .await;
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

// ------------------------------------------------------------ overdue and reaped

/// The heartbeat's reconciler leaves a hand to its own, which asks AWS
/// first: its evidence says a hand still runs, and anything else as before.
#[test]
fn the_heartbeat_leaves_hands_to_their_own_reconciler() {
    use theseus_kernel::{Evidence as _, NoEvidence, Probe};
    let mut a: theseus_kernel::Action = serde_json::from_value(json!({
        "correlation_id": "act_0e", "schema": 2, "execution_id": "exe_0e",
        "session_id": "ses_0e", "tool": "aws.hand", "args_digest": "00",
        "retry_class": {"class": "non_repeatable"}, "state": "dispatched",
        "deadline_at_ms": 1, "planned_at_ms": 0, "reserved_micros": 0,
        "completions_seen": 0}))
    .unwrap();
    let ev = super::overdue::Evidence(&NoEvidence);
    assert_eq!(ev.probe(&a), Probe::StillRunning);
    a.tool = "proc.run".into();
    assert_eq!(ev.probe(&a), NoEvidence.probe(&a));
}

/// Fargate hands past their deadline are asked about with `DescribeTasks`
/// before any is called unknown: one still RUNNING is left (the reaper stops
/// it); one the TTL reaper stopped settles failed with the reaper's reason;
/// one STOPPED otherwise, or one ECS no longer knows, is unknown, saying
/// what ECS said.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_overdue_fargate_hand_is_asked_about_before_it_is_unknown() {
    let r = fargate_rig(json!({"argv": ["true"], "count": 4, "backend": "fargate",
        "ttl_secs": 3600}));
    turn(&r.core, "run four").await;
    let g = group_of(&r.core);
    let rec = record(&r.core, &g);
    let corr = |i: usize| rec.hands[i].correlation_id.clone();
    let task = |i: usize| rec.hands[i].external_op_id.clone().unwrap();
    {
        let mut t = r.state.tasks.lock().unwrap();
        t.insert(
            task(1),
            (
                "STOPPED".into(),
                Some("theseus ttl reaper: theseus:ttl 2026-10-04T10:00:00+00:00 passed".into()),
            ),
        );
        t.insert(
            task(2),
            (
                "STOPPED".into(),
                Some("Essential container in task exited".into()),
            ),
        );
        t.insert(task(3), ("MISSING".into(), None));
    }
    let aws = r.core.tools.aws.clone().unwrap();
    let ctx = super::group::Ctx {
        kernel: &r.core.kernel,
        store: &r.core.store,
        aws: &aws,
    };
    // Not overdue yet: nothing is asked.
    let now = theseus_protocol::now_unix_ms();
    assert!(!super::overdue::pass(&ctx, &rec, now).await.unwrap());
    assert_eq!(r.state.describes.load(Ordering::SeqCst), 0);
    // Two hours on: past every deadline.
    let later = now + 2 * 3_600_000;
    assert!(super::overdue::pass(&ctx, &rec, later).await.unwrap());
    assert_eq!(r.state.describes.load(Ordering::SeqCst), 1);
    assert_eq!(state_of(&r.core, &corr(0)), ActionState::Dispatched);
    assert_eq!(state_of(&r.core, &corr(1)), ActionState::Failed);
    let c = super::group::completion(&r.core.store, &corr(1)).unwrap();
    let why = c.detail.as_ref().unwrap()["error"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        why.starts_with("stopped by the TTL reaper: theseus ttl reaper"),
        "{why}"
    );
    assert_eq!(c.producer, "hands:reaper");
    for (i, says) in [(2, "Essential container"), (3, "no longer knows")] {
        let a = action(&r, &corr(i));
        assert_eq!(a.state, ActionState::OutcomeUnknown, "{a:?}");
        let c = super::group::completion(&r.core.store, &corr(i)).unwrap();
        assert!(c.producer.contains(says), "{}", c.producer);
    }
}

/// A Lambda hand past its deadline has nothing to ask (its timeout has
/// passed): unknown, saying no envelope and no failure record came. Its
/// envelope, if it comes, resolves it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_overdue_lambda_hand_is_unknown_until_its_envelope() {
    let r = rig(calls(json!({"argv": ["true"]})));
    turn(&r.core, "run one").await;
    let g = group_of(&r.core);
    let rec = record(&r.core, &g);
    let aws = r.core.tools.aws.clone().unwrap();
    let ctx = super::group::Ctx {
        kernel: &r.core.kernel,
        store: &r.core.store,
        aws: &aws,
    };
    let later = theseus_protocol::now_unix_ms() + 3_600_000;
    assert!(super::overdue::pass(&ctx, &rec, later).await.unwrap());
    let corr = rec.hands[0].correlation_id.clone();
    assert_eq!(state_of(&r.core, &corr), ActionState::OutcomeUnknown);
    let c = super::group::completion(&r.core.store, &corr).unwrap();
    assert!(c.producer.contains("no envelope"), "{}", c.producer);
    r.state.push(signed(&r.state.invoked()[0], "succeeded", 0));
    r.core.poll_hands_after_serving();
    until("the hand resolved", || {
        state_of(&r.core, &corr) == ActionState::Succeeded
    })
    .await;
    assert_eq!(rows(&r.core, "action.resolved").len(), 1);
}

/// ECS's state change for a task the TTL reaper stopped settles its hand
/// failed with the reaper's reason, never unknown; and the reaper's own
/// failure record on the queue is read: an `aws.reaper.failed` row, its
/// message deleted, and health's count.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reaped_hand_fails_with_the_reapers_reason_and_its_failures_are_read() {
    let r = fargate_rig(json!({"argv": ["true"], "count": 2, "backend": "fargate"}));
    turn(&r.core, "run two").await;
    let g = group_of(&r.core);
    let rec = record(&r.core, &g);
    let corr = rec.hands[0].correlation_id.clone();
    r.state.push(
        json!({"source": "aws.ecs", "detail-type": "ECS Task State Change",
            "detail": {"lastStatus": "STOPPED", "startedBy": g,
                "taskArn": rec.hands[0].external_op_id,
                "stoppedReason": "theseus ttl reaper: theseus:ttl 2026-10-04T10:00:00+00:00 passed",
                "containers": [{"name": "hand", "exitCode": 143}]}})
        .to_string(),
    );
    r.state.push(
        json!({"version": "1.0",
            "requestContext": {"requestId": "inv-reaper", "condition": "RetriesExhausted",
                "functionArn": "arn:aws:lambda:us-west-2:111122223333:function:theseus-reaper:$LATEST"},
            "requestPayload": {"source": "aws.events"},
            "responseContext": {"statusCode": 200, "functionError": "Unhandled"},
            "responsePayload": {"errorMessage": "AccessDenied on ecs:StopTask", "errorType": "ClientError"}})
        .to_string(),
    );
    r.core.poll_hands_after_serving();
    until("the reaped hand", || {
        state_of(&r.core, &corr) == ActionState::Failed
    })
    .await;
    let c = super::group::completion(&r.core.store, &corr).unwrap();
    assert!(
        c.detail.as_ref().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("TTL reaper"),
        "{c:?}"
    );
    until("the reaper's failure read", || {
        !rows(&r.core, "aws.reaper.failed").is_empty()
    })
    .await;
    let f = &rows(&r.core, "aws.reaper.failed")[0];
    assert_eq!(f["condition"], "RetriesExhausted");
    assert!(f["error"].as_str().unwrap().contains("AccessDenied"));
    until("both messages deleted", || {
        r.state.deleted.lock().unwrap().len() == 2
    })
    .await;
    let aws = r.core.tools.aws.clone().unwrap();
    until("health's count", || {
        aws.account(None)
            .unwrap()
            .status()
            .hands
            .is_some_and(|h| h.reaper_failures == 1)
    })
    .await;
}

// ------------------------------------------------------------ quotas

/// A Fargate group bigger than its account's vCPU quota launches in waves:
/// with room for 3 one-vCPU hands, 3 of 5 launch at once, the next as each
/// settles, and all 5 in the end; the quota is read once and kept. It never
/// fails for a quota.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_bigger_than_its_quota_launches_in_waves() {
    let r = fargate_rig(json!({"argv": ["true"], "count": 5, "backend": "fargate"}));
    *r.state.quota.lock().unwrap() = Some(3.0);
    turn(&r.core, "run five").await;
    let g = group_of(&r.core);
    assert_eq!(r.state.ran.lock().unwrap().len(), 3, "the first wave");
    r.core.poll_hands_after_serving();
    for done in 0..5 {
        until("a hand to come home", || fargate_specs(&r).len() > done).await;
        let s = fargate_specs(&r)[done].clone();
        r.state.push(signed(&s, "succeeded", 0));
        until("its settle", || {
            state_of(&r.core, &s.correlation_id) == ActionState::Succeeded
        })
        .await;
        // Never more running than the quota's room.
        let rec = record(&r.core, &g);
        let running = rec
            .hands
            .iter()
            .filter(|h| state_of(&r.core, &h.correlation_id) == ActionState::Dispatched)
            .count();
        assert!(running <= 3, "{running} running");
    }
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    assert_eq!(r.state.ran.lock().unwrap().len(), 5);
    assert_eq!(
        r.state.quota_reads.load(Ordering::SeqCst),
        1,
        "kept an hour"
    );
}

/// Lambda's unreserved concurrency caps a group the same way; a quota
/// smaller than one hand still runs one at a time, never none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lambdas_concurrency_caps_a_group_and_a_tiny_quota_runs_one() {
    let r = rig(calls(json!({"argv": ["true"], "count": 3})));
    *r.state.quota.lock().unwrap() = Some(2.0);
    turn(&r.core, "run three").await;
    assert_eq!(r.state.invoked().len(), 2);
    let f = fargate_rig(json!({"argv": ["true"], "count": 2, "backend": "fargate",
        "vcpu": 2, "memory_mb": 4096}));
    *f.state.quota.lock().unwrap() = Some(1.0);
    turn(&f.core, "run two big ones").await;
    assert_eq!(f.state.ran.lock().unwrap().len(), 1, "one at a time");
    assert_eq!(
        super::quota::hands_that_fit(
            1.0,
            super::launch::Backend::Fargate,
            &super::launch::parse(&json!({"argv": ["x"], "vcpu": 2, "memory_mb": 4096})).unwrap()
        ),
        1
    );
}

// ------------------------------------------------------------ watching

/// `hands.list` reads each group as its surfaces show it: a cell per hand
/// by state, its money against its cap, and its one line. The line goes
/// to the group's place as a `hands` post under the group's key each time
/// it changes, never a line per hand: one while it runs, one per hand that
/// settles, and its last as the group settles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_is_read_as_cells_and_one_line_that_changes_in_place() {
    let r = rig(calls(
        json!({"argv": ["true"], "count": 3, "concurrency": 2, "max_usd": 5}),
    ));
    let res = turn(&r.core, "run three").await;
    let g = group_of(&r.core);
    let list = r
        .core
        .hands_list(&theseus_protocol::HandsListParams::default())
        .unwrap();
    assert_eq!(list.groups.len(), 1);
    let info = &list.groups[0];
    assert_eq!(info.group, g);
    assert_eq!(info.cells, ["running", "running", "waiting"]);
    assert_eq!(info.cap_micros, Some(5_000_000));
    assert!(
        info.reserved_micros > 0 && info.spent_micros == 0,
        "{info:?}"
    );
    assert!(
        info.line
            .starts_with("🖐️ 0/3 done, 0 failed, 2 running, $0.00 of $5"),
        "{}",
        info.line
    );
    // Its place: the line is posted there, and then each change.
    r.core.outbox.bind_place("dm:7", &res.session_id).unwrap();
    let posts = |core: &crate::Core| -> Vec<String> {
        core.outbox
            .open_for("discord:dm:7")
            .iter()
            .filter(|a| crate::outbox::kind_of(a) == "hands")
            .map(|a| {
                let b = crate::outbox::body_of(a);
                assert_eq!(b["group"], g.as_str());
                b["text"].as_str().unwrap().to_string()
            })
            .collect()
    };
    r.core.poll_hands_after_serving();
    until("the first line", || !posts(&r.core).is_empty()).await;
    let mut specs = r.state.invoked();
    specs.sort_by_key(|s| s.index);
    r.state.push(signed(&specs[0], "succeeded", 0));
    until("the third hand's launch", || r.state.invoked().len() == 3).await;
    for s in r.state.invoked().iter().filter(|s| s.index > 0) {
        r.state.push(signed(s, "succeeded", 0));
    }
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    until("the last line", || {
        posts(&r.core)
            .last()
            .is_some_and(|l| l.contains("3/3 done") && l.contains("until met"))
    })
    .await;
    let lines = posts(&r.core);
    assert!(
        lines.len() <= 4,
        "a line per change, not per hand: {lines:?}"
    );
    let mut deduped = lines.clone();
    deduped.dedup();
    assert_eq!(deduped, lines, "a line is posted only when it changed");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(posts(&r.core), lines, "nothing more once it has settled");
    let done = r
        .core
        .hands_list(&theseus_protocol::HandsListParams::default())
        .unwrap();
    assert_eq!(done.groups[0].cells, ["succeeded"; 3]);
    assert_eq!(done.groups[0].settled.as_deref(), Some("met"));
}

// ------------------------------------------------------------ runaway mode

/// A group of two Lambda hands: its worst case, from a rig of its own.
async fn two_hands_reserve() -> u64 {
    let r = rig(calls(json!({"argv": ["true"], "count": 2})));
    turn(&r.core, "run two").await;
    let g = record(&r.core, &group_of(&r.core));
    2 * (g.hand_max_usd * 1e6).ceil() as u64
}

/// A rig whose hour's line puts a two-hand group at `times` the line,
/// scripted to call it `calls_n` times, one turn each.
fn runaway_rig(group: u64, times: f64, calls_n: usize) -> Rig {
    use crate::provider::Scripted;
    let input = json!({"argv": ["true"], "count": 2});
    let mut script = Vec::new();
    for i in 0..calls_n {
        script.push(Scripted::tools(
            "",
            &[(&format!("t{i}"), "aws_hands_run", input.clone())],
        ));
        script.push(Scripted::text("Noted."));
    }
    rig_with(script, move |c| {
        for a in c.aws.accounts.values_mut() {
            a.hourly_alert_usd = (group as f64 / times).floor() / 1e6;
        }
    })
}

/// The call's result text in the session of `res`.
fn result_text(core: &crate::Core, res: &theseus_protocol::TurnSubmitResult) -> String {
    core.store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult { tool, content, .. } if tool == "aws.hands.run" => {
                Some(content)
            }
            _ => None,
        })
        .expect("the call's result")
}

fn notices(core: &crate::Core) -> Vec<String> {
    core.outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .filter_map(|a| crate::outbox::body_of(a)["text"].as_str().map(String::from))
        .collect()
}

/// At 9.9 times the hour's line (the group's own worst case counted), a
/// group runs; the next, at 19.8, is refused with its words, and the
/// account enters runaway mode with one row and one notice; a third is
/// refused and says nothing more. A cancel still runs, the running group
/// settles, health's hands line says it, and the next hour (a later clock)
/// admits a group again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runaway_mode_refuses_at_ten_times_the_hours_line() {
    let group = two_hands_reserve().await;
    let r = runaway_rig(group, 9.9, 3);
    let first = turn(&r.core, "run two").await;
    let g = group_of(&r.core);
    assert!(rows(&r.core, "aws.runaway").is_empty(), "9.9 times runs");
    let second = turn(&r.core, "run two more").await;
    let refused = result_text(&r.core, &second);
    for words in [
        "Not run: AWS runaway mode",
        "this hour",
        "runaway_factor 10",
        "hourly_alert_usd",
        "turns, at",
        "hourly_alert_usd or runaway_factor",
        "restarts the daemon",
    ] {
        assert!(refused.contains(words), "{words:?} in {refused}");
    }
    let row = rows(&r.core, "aws.runaway");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0]["line"], "hour");
    assert_eq!(row[0]["factor"], 10.0);
    let n = notices(&r.core);
    assert_eq!(
        n.iter().filter(|t| t.contains("runaway mode")).count(),
        1,
        "{n:?}"
    );
    let third = turn(&r.core, "and two more").await;
    assert!(result_text(&r.core, &third).contains("Not run: AWS runaway mode"));
    assert_eq!(rows(&r.core, "aws.runaway").len(), 1, "one row");
    assert_eq!(
        notices(&r.core)
            .iter()
            .filter(|t| t.contains("runaway mode"))
            .count(),
        1
    );
    // Health's line, at the poller's pass.
    let aws = r.core.tools.aws.clone().unwrap();
    let account = aws.account(None).unwrap().clone();
    r.core.poll_hands_after_serving();
    until("health's runaway line", || {
        account.status().hands.is_some_and(|h| h.runaway.is_some())
    })
    .await;
    let h = account.status().hands.unwrap();
    assert!(h.runaway.unwrap().contains("refused until"));
    assert!(h.runaway_until_unix_ms.is_some());
    // The running group finishes and settles.
    let mut specs = r.state.invoked();
    specs.sort_by_key(|s| s.index);
    assert_eq!(specs.len(), 2, "only the first group launched");
    for s in &specs {
        r.state.push(signed(s, "succeeded", 0));
    }
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    // The next hour admits a group: the mark does not hold, and the hour's
    // figure starts again.
    let sink = super::runaway::Sink {
        kernel: &r.core.kernel,
        store: &r.core.store,
        outbox: &r.core.outbox,
        rec: r.core.rec(None),
    };
    let now = theseus_protocol::now_unix_ms();
    assert!(sink.admit(&account, group, now).unwrap().is_some());
    let next = super::watch::hour_of(now) + super::watch::HOUR_MS + 1;
    assert_eq!(sink.admit(&account, group, next).unwrap(), None);
    drop(first);
}

/// Exactly ten times the line refuses the first group, and a cancel in
/// runaway mode still runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_times_refuses_and_a_cancel_still_runs() {
    let group = two_hands_reserve().await;
    let r = runaway_rig(group, 10.0, 1);
    let res = turn(&r.core, "run two").await;
    assert!(result_text(&r.core, &res).contains("Not run: AWS runaway mode"));
    assert_eq!(rows(&r.core, "aws.runaway").len(), 1);
    assert!(r.state.invoked().is_empty(), "nothing launched");

    let r = runaway_rig(group, 9.9, 2);
    let res = turn(&r.core, "run two").await;
    let g = group_of(&r.core);
    let refused = turn(&r.core, "run two more").await;
    assert!(result_text(&r.core, &refused).contains("Not run: AWS runaway mode"));
    let stop = r
        .core
        .stop_execution(res.execution_id.as_deref().unwrap(), "the CLI")
        .await
        .unwrap();
    assert!(stop.stopped, "a cancel runs in runaway mode");
    assert!(state_of(&r.core, &g).is_settled());
}

/// The day's line trips runaway mode the same way, at `runaway_factor`
/// times `daily_budget_usd` within the local day, and the next day (a later
/// clock) admits again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_days_line_trips_runaway_mode_the_same_way() {
    let r = rig_with(vec![], |c| {
        for a in c.aws.accounts.values_mut() {
            a.hourly_alert_usd = 100.0;
            a.daily_budget_usd = Some(1);
            a.runaway_factor = 2.0;
        }
    });
    let aws = r.core.tools.aws.clone().unwrap();
    let account = aws.account(None).unwrap().clone();
    let sink = super::runaway::Sink {
        kernel: &r.core.kernel,
        store: &r.core.store,
        outbox: &r.core.outbox,
        rec: r.core.rec(None),
    };
    let now = theseus_protocol::now_unix_ms();
    assert_eq!(
        sink.admit(&account, 1_990_000, now).unwrap(),
        None,
        "1.99 times"
    );
    let why = sink
        .admit(&account, 2_000_000, now)
        .unwrap()
        .expect("refused");
    assert!(
        why.contains("today") && why.contains("daily_budget_usd"),
        "{why}"
    );
    let row = rows(&r.core, "aws.runaway");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0]["line"], "day");
    assert!(
        sink.admit(&account, 1, now).unwrap().is_some(),
        "in runaway mode"
    );
    let tomorrow = super::runaway::day_of(now) + 25 * super::watch::HOUR_MS;
    assert_eq!(sink.admit(&account, 1, tomorrow).unwrap(), None);
    assert_eq!(
        sink.admit(&account, 0, now).unwrap(),
        None,
        "nothing reserved"
    );
    // The factor is at least 2, and 10 unless set.
    let mut cfg = crate::Config::example();
    cfg.aws = super::tests_hands::account("http://127.0.0.1:9");
    assert_eq!(
        cfg.aws.accounts[crate::aws::tests::ACCOUNT].runaway_factor,
        10.0
    );
    cfg.validate().unwrap();
    for a in cfg.aws.accounts.values_mut() {
        a.runaway_factor = 1.5;
    }
    let e = cfg.validate().unwrap_err().to_string();
    assert!(e.contains("runaway_factor is 1.5"), "{e}");
}
