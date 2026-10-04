//! The inbound point (M5 25a; design §3, "25a"): `classify.v1` and
//! `role.v1` about each message a person sends, in shadow, against the fake
//! Jev. One request per message, two rows that share its state's blob with
//! the cost split by question count, each judgment marked on the turn's
//! trace; none for a slash command, a task's first turn, a wake's turn, or
//! a report's turn; and nothing a turn sends or returns changes when Jev is
//! down, slow, rate-limited, or malformed.

use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::Span;
use theseus_store::Record;

use crate::ledger::LedgerRow;
use crate::store::Store;
use crate::tests_judge::{board, kinds, off, rig_on, texts, turn};

/// A scope's rows (`judge:classify`, `judge:role`), decoded.
fn scoped(store: &Store, scope: &str) -> Vec<(Record, LedgerRow)> {
    store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .map(|r| {
            let row: LedgerRow = r.decode().unwrap();
            (r, row)
        })
        .collect()
}

/// Wait (on the runtime's timer) until each inbound pack has `n` rows.
async fn until_inbound(store: &Store, n: usize) -> (Vec<LedgerRow>, Vec<LedgerRow>) {
    let t0 = Instant::now();
    loop {
        let c = scoped(store, "judge:classify");
        let r = scoped(store, "judge:role");
        if c.len() >= n && r.len() >= n {
            let rows = |v: Vec<(Record, LedgerRow)>| v.into_iter().map(|(_, r)| r).collect();
            return (rows(c), rows(r));
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} and {} of {n} inbound judgments recorded",
            c.len(),
            r.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The turn's `judge` marks, from its trace's root.
fn marks(trace: &Option<Span>) -> Vec<Value> {
    trace
        .as_ref()
        .unwrap()
        .children
        .iter()
        .filter(|s| s.name == "judge" && s.kind == "mark")
        .inspect(|s| assert_eq!(s.start_us, s.end_us.unwrap(), "zero-length"))
        .map(|s| s.attrs.clone())
        .collect()
}

/// `loop.v1` off, so the inbound point's call is the only one.
fn inbound_only(c: &mut crate::Config) {
    c.judge.packs.insert("loop.v1".into(), off());
}

/// A person's message is judged by both packs in one request: the fake
/// counts one call, the two rows (scoped `judge:classify` and `judge:role`,
/// each keyed by its own id) share the state's blob and the call, and the
/// call's cost is split by question count. The turn's trace marks each
/// judgment with the id its row carries.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_message_is_judged_by_both_packs_in_one_request() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "kind",
        Jev::Choice {
            option: "new_ask".into(),
            confidence: 0.93,
        },
    );
    jev.script(
        "role",
        Jev::Choice {
            option: "coder".into(),
            confidence: 0.91,
        },
    );
    let r = rig_on(texts(1), Some(&jev), inbound_only);
    let res = turn(&r.core, None, "Write a function that adds two numbers.").await;
    let (classify, role) = until_inbound(&r.core.store, 1).await;
    assert_eq!((classify.len(), role.len()), (1, 1));
    assert_eq!(jev.connections(), 1, "one request for the message");
    let seen = jev.seen();
    assert_eq!(seen.len(), 1);
    let q = &seen[0].body["questions"];
    let ids: Vec<&String> = q.as_object().unwrap().keys().collect();
    assert!(ids.iter().any(|i| *i == "classify.v1/kind"), "{ids:?}");
    // role.v1's options are the twelve seed roles and its no-match option.
    let roles = q["role.v1/role"]["criteria"].as_object().unwrap();
    assert_eq!(roles.len(), 13, "{roles:?}");
    assert!(roles.contains_key("coder") && roles.contains_key("other"));
    // No live tasks: `addressed_task` is not asked.
    assert!(!ids.iter().any(|i| i.ends_with("/addressed_task")));
    let (c, ro) = (&classify[0].data, &role[0].data);
    for (row, pack) in [(&classify[0], "classify.v1"), (&role[0], "role.v1")] {
        assert_eq!(row.kind, "judge.call");
        assert_eq!(row.data["pack"], pack);
        assert_eq!(row.data["mode"], "shadow");
        assert_eq!(row.data["point"], "inbound");
        assert_eq!(row.data["outcome"]["outcome"], "answered", "{}", row.data);
        assert_eq!(row.session_id.as_deref(), Some(res.session_id.as_str()));
        assert_eq!(row.turn_id.as_deref(), Some(res.turn_id.as_str()));
    }
    assert_ne!(c["id"], ro["id"], "each judgment its own id");
    let answer = |d: &Value, q: &str| {
        d["answers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["question"] == q)
            .unwrap()["answer"]["choice"]
            .clone()
    };
    assert_eq!(answer(c, "kind"), "new_ask");
    assert_eq!(answer(ro, "role"), "coder");
    // One blob, one call.
    let digest = c["state"]["sha256"].as_str().unwrap();
    assert_eq!(ro["state"]["sha256"].as_str(), Some(digest));
    assert_eq!(c["context"]["blob"].as_str(), Some(digest));
    assert_eq!(ro["context"]["blob"].as_str(), Some(digest));
    assert_eq!(c["call"]["id"], ro["call"]["id"]);
    assert_eq!(c["call"]["packs"], 2);
    let (qc, qr) = (
        c["questions"].as_u64().unwrap() as usize,
        ro["questions"].as_u64().unwrap() as usize,
    );
    assert_eq!((qc, qr), (5, 1));
    assert_eq!(c["call"]["questions"].as_u64(), Some((qc + qr) as u64));
    // The call's cost, split by question count.
    let (cc, cr) = (
        c["cost_micros"].as_u64().unwrap(),
        ro["cost_micros"].as_u64().unwrap(),
    );
    assert!(cc > cr && cr > 0, "{cc} {cr}");
    assert_eq!(theseus_judge::batch::shares(cc + cr, &[qc, qr]), [cc, cr]);
    // The state: the message, from a private place's operator, no role yet.
    let state = std::fs::read_to_string(r.core.store.blobs().path(digest)).unwrap();
    let state: Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["message"], "Write a function that adds two numbers.");
    assert_eq!(state["author"], "operator");
    assert_eq!(state["place_kind"], "cli (private)");
    assert_eq!(state["current_role"], "none");
    // Each judgment marked on the turn's trace with its row's id.
    let marked = marks(&res.trace);
    assert_eq!(marked.len(), 2, "{marked:?}");
    for (m, d) in marked.iter().zip([c, ro]) {
        assert_eq!(m["pack"], d["pack"]);
        assert_eq!(m["judgment"], d["id"]);
        assert_eq!(
            (m["point"].as_str(), m["mode"].as_str()),
            (Some("inbound"), Some("shadow"))
        );
    }
    // One call, from the judge's budget.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let h = r.core.health().judge.unwrap();
    assert_eq!(
        h.packs,
        ["loop.v1: off", "classify.v1: shadow", "role.v1: shadow"]
    );
    assert_eq!((h.calls_today, h.failed_today), (1, 0));
    assert_eq!(
        h.spend_today_usd,
        theseus_judge::price::micros_to_usd(cc + cr)
    );
}

/// The second message's state reads the conversation so far: the previous
/// message, the last reply, and the minutes between.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_next_messages_state_reads_the_previous_message_and_the_reply() {
    let jev = FakeJev::start().unwrap();
    let r = rig_on(texts(2), Some(&jev), inbound_only);
    let first = turn(&r.core, None, "Run the unit tests for the store.").await;
    until_inbound(&r.core.store, 1).await;
    turn(
        &r.core,
        Some(&first.session_id),
        "and the integration tests too",
    )
    .await;
    let (classify, _) = until_inbound(&r.core.store, 2).await;
    let digest = classify[1].data["state"]["sha256"].as_str().unwrap();
    let state = std::fs::read_to_string(r.core.store.blobs().path(digest)).unwrap();
    let state: Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["message"], "and the integration tests too");
    assert_eq!(
        state["previous_human_message"],
        "Run the unit tests for the store."
    );
    assert_eq!(state["last_reply"], "Done: answer 0.");
    assert_eq!(state["minutes_since_last_message"], 0);
}

/// A slash command is no person's message to judge: it makes no call, and
/// the message after it makes one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slash_command_is_not_judged() {
    let jev = FakeJev::start().unwrap();
    let r = rig_on(texts(2), Some(&jev), inbound_only);
    let slash = turn(&r.core, None, "/status").await;
    assert!(marks(&slash.trace).is_empty());
    let next = turn(&r.core, Some(&slash.session_id), "What changed today?").await;
    let (classify, role) = until_inbound(&r.core.store, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(jev.connections(), 1);
    assert_eq!((classify.len(), role.len()), (1, 1));
    assert_eq!(classify[0].turn_id.as_deref(), Some(next.turn_id.as_str()));
}

/// A task's first turn (its brief, which its parent wrote), the wake's turn
/// that continues it, and the parent's turn its report starts are no
/// person's messages: only the parent's own message is judged.
#[tokio::test]
async fn a_tasks_first_turn_a_wakes_turn_and_a_reports_turn_are_not_judged() {
    use crate::tests_task_wakes::{exec, life_with, parked, until};
    use theseus_kernel::ExecState;
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let l = life_with(dir.path(), |p| {
        p.cfg.judge.enabled = true;
        p.cfg.judge.api_base = jev.base();
        p.cfg.judge.packs.insert("loop.v1".into(), off());
        p.secrets = board();
    });
    let (parent, task) = parked(&l.core, "CHILD AGAIN 3s: check the build").await;
    let pid = task.parent.clone().unwrap();
    until("the task's report and the parent's turn", 30, || {
        let p = exec(&l.core, &pid);
        exec(&l.core, &task.id).state == ExecState::Complete
            && p.turns == 2
            && p.state == ExecState::Waiting
    })
    .await;
    assert_eq!(
        exec(&l.core, &task.id).turns,
        2,
        "the first turn, and the wake's"
    );
    assert_eq!(
        crate::tests_task_wakes::rows(&l.core, "wake.fired").len(),
        1
    );
    // The sink's window, and then some.
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    let classify = scoped(&l.core.store, "judge:classify");
    let role = scoped(&l.core.store, "judge:role");
    assert_eq!(
        (classify.len(), role.len()),
        (1, 1),
        "the parent's message alone"
    );
    assert_eq!(classify[0].1.session_id.as_deref(), Some(parent.as_str()));
    assert_eq!(jev.connections(), 1);
    l.stop().await;
}

/// With the judge on, a judged plain turn still counts 5 frames of its own
/// (its trace's `frames`), and the WAL holds no more for it: the judgments'
/// rows ride in the sink's own frame.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judged_turn_keeps_its_frame_budget() {
    let jev = FakeJev::start().unwrap();
    let r = rig_on(texts(2), Some(&jev), |_| {});
    let first = turn(&r.core, None, "warm up").await;
    until_inbound(&r.core.store, 1).await;
    // The first turn's loop.v1 judgment, and the sink's frame after it.
    let t0 = Instant::now();
    while scoped(&r.core.store, "judge:loop").is_empty() {
        assert!(t0.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let before = r.core.store.stats().unwrap().frames_appended;
    let res = turn(&r.core, Some(&first.session_id), "hi").await;
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert_eq!(res.loops, 1);
    assert_eq!(marks(&res.trace).len(), 2, "judged");
    let own = res.trace.as_ref().unwrap().attrs["frames"].as_u64();
    assert_eq!(own, Some(5), "the turn's own frames");
    assert!(frames <= 5, "a judged plain turn wrote {frames} frames");
    until_inbound(&r.core.store, 2).await;
}

/// What each fake mode leaves on both judgments: its error class, and the
/// turn's request and result as they are with the judge off. A turn waits
/// on none of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_jev_is_recorded_by_its_class_and_changes_no_turn() {
    let off_rig = rig_on(texts(1), None, |_| {});
    let base = turn(&off_rig.core, None, "Say done.").await;
    let base_req = serde_json::to_value(&off_rig.fake.requests()[0]).unwrap();
    assert!(marks(&base.trace).is_empty(), "the judge off marks nothing");
    for (mode, class) in [
        (FakeMode::Down, "network"),
        (FakeMode::Slow(Duration::from_secs(10)), "timeout"),
        (
            FakeMode::RateLimited {
                retry_after_secs: 7,
            },
            "rate_limited",
        ),
        (FakeMode::Malformed, "malformed"),
    ] {
        let jev = FakeJev::start().unwrap();
        jev.set_mode(mode.clone());
        // A turn that waited on a slow Jev would wait out the whole call, 5 s;
        // one that does not takes its own time, which load can stretch past a
        // second, never to 3 s.
        let r = rig_on(texts(1), Some(&jev), |c| {
            inbound_only(c);
            c.judge.total_secs = 5;
        });
        let t0 = Instant::now();
        let res = turn(&r.core, None, "Say done.").await;
        let took = t0.elapsed();
        assert!(
            took < Duration::from_secs(3),
            "{mode:?}: the turn took {took:?}"
        );
        assert_eq!(
            (
                &res.output,
                &res.stop_reason,
                res.loops,
                res.usage.clone(),
                res.cost_usd
            ),
            (
                &base.output,
                &base.stop_reason,
                base.loops,
                base.usage.clone(),
                base.cost_usd
            ),
            "{mode:?}: the turn's result"
        );
        let req = serde_json::to_value(&r.fake.requests()[0]).unwrap();
        assert_eq!(req, base_req, "{mode:?}: the turn's request");
        let (classify, role) = until_inbound(&r.core.store, 1).await;
        for d in [&classify[0].data, &role[0].data] {
            assert_eq!(d["outcome"]["outcome"], "failed", "{mode:?}: {d}");
            assert_eq!(d["outcome"]["class"], class, "{mode:?}: {d}");
        }
        assert_eq!(jev.connections(), 1, "{mode:?}: one call");
        let h = r.core.health().judge.unwrap();
        assert_eq!((h.calls_today, h.failed_today), (1, 1), "{mode:?}");
        assert!(kinds(&r.core.store, "judge.circuit").is_empty());
    }
}

/// The inbound packs off: no call, and nothing marked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_inbound_packs_off_call_nothing() {
    let jev = FakeJev::start().unwrap();
    let r = rig_on(texts(1), Some(&jev), |c| {
        inbound_only(c);
        for p in crate::judge::inbound::PACKS {
            c.judge.packs.insert(p.into(), off());
        }
    });
    let res = turn(&r.core, None, "Say done.").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(marks(&res.trace).is_empty());
    assert_eq!(jev.connections(), 0);
    assert!(scoped(&r.core.store, "judge:classify").is_empty());
}
