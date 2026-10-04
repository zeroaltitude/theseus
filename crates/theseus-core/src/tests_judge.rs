//! The judge's wire-in (M5 23a; design §3, "23a"): `loop.v1` in shadow at a
//! turn's end, against the fake Jev. Every judgment is recorded (keyed,
//! scoped, its state in a blob), priced from the judge's own budget, and
//! nothing a turn sends or returns changes with it: not when Jev is down,
//! slow, rate-limited, or malformed.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::TurnSubmitResult;
use theseus_store::Record;

use crate::bus::EventSink;
use crate::config::PackMode;
use crate::judge::{JudgeService, LoopEnd};
use crate::ledger::LedgerRow;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::{Core, Parts};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::Config;
use theseus_protocol::SessionKind;

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

/// A board with the Jev key ready, as it is once the secrets settle.
pub(crate) fn board() -> Arc<SecretBoard> {
    let b = SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    b.publish(
        BTreeMap::from([(
            "jev_api_key".to_string(),
            Ok(Secret::new("jev-test-key-0123456789".into())),
        )]),
        "test",
    );
    b
}

pub(crate) fn config(state: &Path, jev: Option<&FakeJev>) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    if let Some(j) = jev {
        cfg.judge.enabled = true;
        cfg.judge.api_base = j.base();
        cfg.judge.connect_secs = 1;
        cfg.judge.total_secs = 1;
    }
    cfg
}

pub(crate) fn rig_with(
    script: Vec<Scripted>,
    jev: Option<&FakeJev>,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), jev);
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = Parts::for_tests(cfg, fake.clone(), store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

pub(crate) fn texts(n: usize) -> Vec<Scripted> {
    (0..n)
        .map(|i| Scripted::text(&format!("Done: answer {i}.")))
        .collect()
}

pub(crate) async fn turn(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// The `judge:loop` scope's rows, decoded.
pub(crate) fn judged(store: &Store) -> Vec<(Record, LedgerRow)> {
    store
        .scope_after("judge:loop", 0)
        .unwrap()
        .into_iter()
        .map(|r| {
            let row: LedgerRow = r.decode().unwrap();
            (r, row)
        })
        .collect()
}

/// Wait (on the runtime's timer) until `n` judgments are recorded.
pub(crate) async fn until_judged(store: &Store, n: usize) -> Vec<(Record, LedgerRow)> {
    let t0 = Instant::now();
    loop {
        let rows = judged(store);
        if rows.len() >= n {
            return rows;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} judgments recorded",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) fn kinds(store: &Store, kind: &str) -> Vec<LedgerRow> {
    store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

/// A turn the baseline ends with no tool calls is judged once by `loop.v1`
/// in shadow: its row is keyed by the judgment's id and scoped
/// `judge:loop`, its state is in a blob written before it, and its cost is
/// the judge's, not the session's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_ends_with_no_tool_calls_is_judged_once_in_shadow() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "complete".into(),
            confidence: 0.95,
        },
    );
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let res = turn(&r.core, None, "Say done.").await;
    assert_eq!(res.stop_reason, "no_tool_calls");
    let rows = until_judged(&r.core.store, 1).await;
    assert_eq!(rows.len(), 1, "one judgment");
    let (rec, row) = &rows[0];
    let d = &row.data;
    assert_eq!(rec.key.as_deref(), d["id"].as_str(), "keyed by its id");
    assert_eq!(row.kind, "judge.call");
    assert_eq!(row.session_id.as_deref(), Some(res.session_id.as_str()));
    assert_eq!(row.turn_id.as_deref(), Some(res.turn_id.as_str()));
    assert_eq!(
        (d["pack"].as_str(), d["version"].as_u64()),
        (Some("loop.v1"), Some(1))
    );
    assert_eq!(
        (d["mode"].as_str(), d["budget"].as_str()),
        (Some("shadow"), Some("shadow"))
    );
    assert_eq!(d["outcome"]["outcome"], "answered", "{d}");
    let work = d["answers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["question"] == "work_state")
        .unwrap();
    assert_eq!(work["answer"]["choice"], "complete");
    assert_eq!(work["band"]["band"], "act");
    let digest = d["state"]["sha256"].as_str().unwrap();
    assert_eq!(d["context"]["blob"].as_str(), Some(digest));
    let state = std::fs::read_to_string(r.core.store.blobs().path(digest)).unwrap();
    let state: Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["ask"], "Say done.");
    assert_eq!(state["final_text"], "Done: answer 0.");
    // Jev saw that state, with the key as a bearer (never recorded).
    let seen = jev.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].bearer_len, Some("jev-test-key-0123456789".len()));
    assert!(!serde_json::to_string(&row.data)
        .unwrap()
        .contains("jev-test-key"));
    // Priced, from the judge's budget: the session's cost is its turn's.
    let cost = d["cost_micros"].as_u64().unwrap();
    assert!(cost > 0);
    let session: SessionRecord = r.core.store.get_session(&res.session_id).unwrap().unwrap();
    assert_eq!(Some(session.cost_usd), res.cost_usd);
    let h = r.core.health().judge.unwrap();
    assert!(h.enabled);
    assert_eq!(h.packs, ["loop.v1: shadow"]);
    assert_eq!((h.calls_today, h.failed_today), (1, 0));
    assert_eq!(h.spend_today_usd, theseus_judge::price::micros_to_usd(cost));
    assert_eq!(h.breaker, "closed");
}

/// With the judge on, a plain turn still writes 5 frames: the judgment's
/// rows ride in the sink's own frame, after the turn, never in the turn's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judged_turn_keeps_its_frame_budget() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(2), Some(&jev), |_| {});
    let first = turn(&r.core, None, "warm up").await;
    until_judged(&r.core.store, 1).await;
    let before = r.core.store.stats().unwrap().frames_appended;
    let res = turn(&r.core, Some(&first.session_id), "hi").await;
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert_eq!(res.loops, 1);
    assert!(frames <= 5, "a judged plain turn wrote {frames} frames");
    until_judged(&r.core.store, 2).await;
}

/// What each fake mode leaves: its error class on the judgment, and the
/// turn's request and result as they are with the judge off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_jev_is_recorded_by_its_class_and_changes_no_turn() {
    let off = rig_with(texts(1), None, |_| {});
    let base = turn(&off.core, None, "Say done.").await;
    let base_req = serde_json::to_value(&off.fake.requests()[0]).unwrap();
    assert!(off.core.health().judge.is_some_and(|h| !h.enabled));
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
        let r = rig_with(texts(1), Some(&jev), |c| c.judge.total_secs = 5);
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
        let rows = until_judged(&r.core.store, 1).await;
        let d = &rows[0].1.data;
        assert_eq!(d["outcome"]["outcome"], "failed", "{mode:?}: {d}");
        assert_eq!(d["outcome"]["class"], class, "{mode:?}: {d}");
        let h = r.core.health().judge.unwrap();
        assert_eq!((h.calls_today, h.failed_today), (1, 1), "{mode:?}");
    }
}

/// Jev down: five transient failures in a row open the breaker (one
/// `judge.circuit` row), and the judgments after it are skipped at once,
/// with no connection made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_jev_that_is_down_opens_the_breaker_and_later_judgments_skip() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Down);
    let r = rig_with(texts(7), Some(&jev), |_| {});
    let sid = turn(&r.core, None, "one").await.session_id;
    until_judged(&r.core.store, 1).await;
    for i in 2..=7 {
        turn(&r.core, Some(&sid), &format!("turn {i}")).await;
        until_judged(&r.core.store, i).await;
    }
    let rows = judged(&r.core.store);
    let outcomes: Vec<String> = rows
        .iter()
        .map(|(_, r)| {
            let o = &r.data["outcome"];
            o["class"]
                .as_str()
                .or(o["reason"].as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            "network",
            "network",
            "network",
            "network",
            "network",
            "circuit_open",
            "circuit_open"
        ]
    );
    assert_eq!(jev.connections(), 5, "an open breaker sends nothing");
    let circuit = kinds(&r.core.store, "judge.circuit");
    assert_eq!(circuit.len(), 1);
    let t = &circuit[0].data["transition"];
    assert_eq!(
        (t["circuit"].as_str(), t["failures"].as_u64()),
        (Some("opened"), Some(5))
    );
    assert!(r.core.health().judge.unwrap().breaker.starts_with("open"));
}

/// Off makes no call: the judge disabled, or its pack's line off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_judge_or_pack_calls_nothing() {
    let jev = FakeJev::start().unwrap();
    let off = rig_with(texts(1), None, |c| c.judge.api_base = jev.base());
    turn(&off.core, None, "Say done.").await;
    let pack_off = rig_with(texts(1), Some(&jev), |c| {
        c.judge.packs.insert(
            "loop.v1".into(),
            crate::config::JudgePackConfig {
                mode: Some(PackMode::Off),
                sample: None,
            },
        );
    });
    turn(&pack_off.core, None, "Say done.").await;
    let capped = rig_with(texts(1), Some(&jev), |c| c.judge.max_mode = PackMode::Off);
    turn(&capped.core, None, "Say done.").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(jev.connections(), 0);
    for r in [&off, &pack_off, &capped] {
        assert!(judged(&r.core.store).is_empty());
    }
    assert_eq!(
        pack_off.core.health().judge.unwrap().packs,
        ["loop.v1: off"]
    );
}

/// A Jev model with no price is never called: the judgment is recorded as
/// skipped, `unpriced`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unpriced_jev_model_is_never_called() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), None, |_| {});
    let res = turn(&r.core, None, "Say done.").await;
    let cfg = config(Path::new("/nonexistent"), Some(&jev)).judge;
    let svc = JudgeService::with_parts(
        cfg,
        r.core.store.clone(),
        board(),
        Arc::new(crate::scrub::Scrubber::default()),
        Duration::from_millis(20),
        BTreeMap::new(),
    );
    svc.after_turn(&res, false);
    let rows = until_judged(&r.core.store, 1).await;
    assert_eq!(rows[0].1.data["outcome"]["reason"], "unpriced");
    assert_eq!(jev.connections(), 0);
}

/// A day's limit of $0.0002 pauses shadow: the judgments that fit are made,
/// the next is skipped and counted, never queued, and one `judge.paused`
/// row says so however many are skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tiny_day_limit_pauses_shadow_with_one_row() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(6), Some(&jev), |c| {
        c.judge.shadow_limit_usd_per_day = 0.0002
    });
    let sid = turn(&r.core, None, "one").await.session_id;
    for i in 2..=6 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        turn(&r.core, Some(&sid), &format!("turn {i}")).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let made = jev.connections();
    assert!(
        (1..6).contains(&made),
        "{made} judgments were made under $0.0002"
    );
    let paused = kinds(&r.core.store, "judge.paused");
    assert_eq!(paused.len(), 1, "{paused:?}");
    assert_eq!(paused[0].data["limit_micros"], 200);
    let h = r.core.health().judge.unwrap();
    assert!(h.paused);
    assert_eq!(h.calls_today as usize, made);
    assert_eq!(h.skipped_today as usize, 6 - made);
    assert!(h.spend_today_usd <= 0.0002, "{}", h.spend_today_usd);
}

/// A restart books the rest of today's block as spent at its first
/// judgment, never before: what the first daemon reserved past what it
/// settled (here, a block's rest after a stop) is booked conservatively.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restart_books_the_blocks_rest_at_its_first_judgment() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let build = || {
        let cfg = config(dir.path(), Some(&jev));
        let store = Store::open(&dir.path().join("store")).unwrap();
        let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(1))), store);
        p.secrets = board();
        Core::build(p).unwrap()
    };
    let core = build();
    let sid = turn(&core, None, "one").await.session_id;
    until_judged(&core.store, 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let first = core.health().judge.unwrap().spend_today_usd;
    assert!(first > 0.0 && first < 0.01);
    drop(core);
    let core = build();
    // Before a judgment: the store's record, read; nothing booked.
    assert_eq!(core.health().judge.unwrap().spend_today_usd, first);
    assert!(kinds(&core.store, "judge.block_booked").is_empty());
    turn(&core, Some(&sid), "two").await;
    until_judged(&core.store, 2).await;
    let booked = kinds(&core.store, "judge.block_booked");
    assert_eq!(booked.len(), 1);
    assert_eq!(booked[0].data["reserved_micros"], 10_000);
    let spent = core.health().judge.unwrap().spend_today_usd;
    assert!(
        spent > 0.01,
        "the block's rest booked, then the second call: {spent}"
    );
}

/// The state's builder sees this turn's calls, each with its outcome, and
/// the operator's ask; the class says the turn used tools.
#[test]
fn the_loop_input_reads_the_turns_calls_and_the_ask() {
    use crate::node::Node;
    let sid = "ses_x";
    let mut ask = Node::user(sid, Some("turn_a"), "op", "Read the file.");
    ask.created_at_ms = 0;
    let mut call = Node::user(sid, Some("turn_a"), "op", "");
    call.body = crate::node::Body::ToolCall {
        tool_use_id: "tu1".into(),
        tool: "fs.read".into(),
        wire_name: "fs_read".into(),
        input: serde_json::json!({"path": "a.txt"}),
        assistant_node: "n1".into(),
        correlation_id: None,
        gate: None,
    };
    let end = LoopEnd {
        session_id: sid.into(),
        execution_id: "exe".into(),
        turn_id: "turn_a".into(),
        task: false,
        output: "It says hi.".into(),
        loops: 2,
        cost_usd: Some(0.01),
        tool_calls: 1,
    };
    let i = crate::judge::loop_end::input(&[ask, call], &end, 5 * 60_000);
    assert_eq!(i.ask, "Read the file.");
    assert_eq!(i.minutes_since_ask, 5);
    assert_eq!(i.tool_calls.len(), 1);
    assert_eq!(i.tool_calls[0].tool, "fs.read");
    assert_eq!(end.class(), "tools");
}
