//! The `+rerank` arm in shadow (M6 step 32c; design §3.2's 32c row), through
//! whole cores against the fake Jev and a stand-in index: a fake Jev
//! reorders what would be admitted; a timeout falls back to the fused
//! order; the spend is the judge's, as `purpose: recall`; only candidates
//! that passed the place filter reach Jev, over generated stores; a slow,
//! failing, or rate-limited Jev changes no turn's request bytes, duration,
//! or frames; the day's limit skips a rerank; and mode off calls nothing.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use serde_json::Value;
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemoryRecallsParams, RecallManifest};
use theseus_protocol::{SessionKind, Span, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::{JudgePackConfig, MemoryMode, PackMode};
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::places::BoundPlace;
use crate::provider::{FakeProvider, ProviderRequest};
use crate::recall::{Ask, AskFuture};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The owner on Discord, someone else, and three guild channels: one bound
/// private, two shared.
const OWNER: u64 = 161_803_398_874_989_484;
const ALICE: u64 = 333_333_333_333_333_333;
const DEN: u64 = 271_000_000_000_000_001;
const PIER: u64 = 271_000_000_000_000_002;
const QUAY: u64 = 271_000_000_000_000_003;

pub(crate) struct Rig {
    pub core: Arc<Core>,
    pub model: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

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

pub(crate) fn pack_mode(cfg: &mut Config, pack: &str, mode: PackMode) {
    cfg.judge.packs.insert(
        pack.into(),
        JudgePackConfig {
            mode: Some(mode),
            sample: None,
            notices: None,
        },
    );
}

/// Memory in shadow, admitting one item; the judge on against `jev` when
/// given, with every other wired pack off (`loop.v1`, and the gate's,
/// inbound's, compile's and categorize's points), so every judgment is the
/// rerank's.
pub(crate) fn rig(jev: Option<&FakeJev>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.memory.mode = MemoryMode::Shadow;
    cfg.memory.recall_max_items = 1;
    if let Some(j) = jev {
        cfg.judge.enabled = true;
        cfg.judge.api_base = j.base();
        cfg.judge.connect_secs = 1;
        cfg.judge.total_secs = 5;
        for (pack, _) in crate::judge::WIRED {
            if *pack != crate::judge::rerank::RERANK_PACK {
                pack_mode(&mut cfg, pack, PackMode::Off);
            }
        }
    }
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::default());
    let mut p = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{DEN}"),
        name: "#den".into(),
        private: true,
        ..Default::default()
    }]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

/// A session, posting to `place` when given, saying `said`.
pub(crate) fn session(core: &Core, place: Option<&str>, said: &[&str]) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    for s in said {
        let n = Node::user(&r.session_id, None, "test", s);
        core.store.append(&[n.record().unwrap()]).unwrap();
    }
    r.session_id
}

/// A stand-in index over `sessions`, in the order given (the first is the
/// best), every node written before the query's `as_of`.
pub(crate) fn index_of(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        let mut hits = Vec::new();
        for sid in &sessions {
            for (position, n) in store.session_nodes(sid).unwrap() {
                if p.as_of.is_some_and(|a| position >= a) {
                    continue;
                }
                hits.push((position, n));
            }
        }
        let hits = hits
            .into_iter()
            .enumerate()
            .map(|(i, (position, n))| IndexHit {
                text: crate::recall::text_of(&n),
                node_id: n.id,
                chunk: 0,
                session_id: n.session_id,
                position,
                kind: "user_message".into(),
                origin: "operator".into(),
                author: None,
                place: None,
                tool: None,
                time_ms: 0,
                external: false,
                entities_matched: vec![],
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

pub(crate) async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            prompt: None,
        })
        .await
        .unwrap()
}

pub(crate) fn recalls(core: &Core, sid: &str) -> Vec<RecallManifest> {
    core.memory_recalls(MemoryRecallsParams {
        session_id: sid.into(),
        limit: Some(200),
    })
    .unwrap()
    .recalls
}

/// The rerank's `judge.call` rows.
pub(crate) fn reranks(store: &Store) -> Vec<LedgerRow> {
    store
        .scope_after("judge:rerank", 0)
        .unwrap()
        .iter()
        .map(|r| r.decode().unwrap())
        .collect()
}

/// Wait, on the runtime's timer, for `n` rerank rows.
pub(crate) async fn until_reranked(store: &Store, n: usize) -> Vec<LedgerRow> {
    let t0 = Instant::now();
    loop {
        let rows = reranks(store);
        if rows.len() >= n {
            return rows;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "{} of {n} reranks recorded",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Every `judge` mark in a trace.
pub(crate) fn marks(s: &Span, out: &mut Vec<Value>) {
    // The rerank's marks: 23b marks loop.v1's dispatch at each turn's end.
    if s.name == "judge" && s.kind == "mark" && s.attrs["pack"] == "rerank.v1" {
        out.push(s.attrs.clone());
    }
    for c in &s.children {
        marks(c, out);
    }
}

pub(crate) fn keys_of(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect()
}

/// Three notes, the stand-in index best first: the heron's note third.
pub(crate) fn heron_rig(
    jev: &FakeJev,
    tweak: impl FnOnce(&mut Config),
) -> (Rig, String, Vec<String>) {
    let r = rig(Some(jev), tweak);
    let c = &r.core;
    let kettle = session(c, None, &["the kettle in the shed needs descaling"]);
    let gate = session(c, None, &["the north gate's code changed on monday"]);
    let heron = session(c, None, &["the grey heron nests by the old weir"]);
    let here = session(c, None, &[]);
    let order = vec![kettle, gate, heron];
    c.runner.memory.set_ask(index_of(c, order.clone()));
    (r, here, order)
}

/// A fake Jev reorders: the baseline admits the fused order's first note,
/// and `+rerank` the one Jev says helps. The row is keyed and scoped as
/// every judgment's, names its recall, carries both orders' admitted keys,
/// its latency against the deadline, and its cost, paid by the judge's day
/// budget as `purpose: recall`; the turn's trace is marked with its id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fake_jev_reorders_what_would_be_admitted() {
    let jev = FakeJev::start().unwrap();
    jev.script("helps.1", Jev::Noul(0.10));
    jev.script("helps.2", Jev::Noul(0.30));
    jev.script("helps.3", Jev::Noul(0.97));
    let (r, here, order) = heron_rig(&jev, |_| {});
    let c = &r.core;
    let res = turn(c, &here, "Where does the grey heron nest?").await;
    let rows = until_reranked(&c.store, 1).await;
    assert_eq!(rows.len(), 1, "one rerank a recall");
    let d = &rows[0].data;
    assert_eq!(rows[0].kind, "judge.call");
    assert_eq!(rows[0].session_id.as_deref(), Some(here.as_str()));
    assert_eq!(rows[0].turn_id.as_deref(), Some(res.turn_id.as_str()));
    assert_eq!(
        (d["pack"].as_str(), d["mode"].as_str(), d["budget"].as_str()),
        (Some("rerank.v1"), Some("shadow"), Some("shadow"))
    );
    assert_eq!(d["point"], "recall");
    assert_eq!(d["outcome"]["outcome"], "answered", "{d}");
    let ctx = &d["context"];
    assert_eq!(ctx["purpose"], "recall");
    let m = &recalls(c, &here)[0];
    assert_eq!(ctx["recall"].as_str(), Some(m.recall_id.as_str()));
    let rr = &ctx["rerank"];
    assert_eq!(rr["recall"].as_str(), Some(m.recall_id.as_str()));
    let fused = keys_of(&rr["fused_admitted"]);
    let reranked = keys_of(&rr["reranked_admitted"]);
    let admitted: Vec<String> = m
        .admitted
        .iter()
        .map(|a| format!("{}#{}", a.node_id, a.chunk))
        .collect();
    assert_eq!(fused, admitted, "the baseline's, as its recall row has it");
    assert_eq!(
        m.admitted[0].session_id, order[0],
        "the fused order's first"
    );
    let heron_node = c.store.session_nodes(&order[2]).unwrap()[0].1.id.clone();
    assert_eq!(reranked, [format!("{heron_node}#0")], "Jev's best");
    assert_eq!(rr["changed"], true);
    assert_eq!(rr["fallback"], Value::Null);
    assert_eq!(rr["eligible"], 3);
    assert_eq!(rr["asked"], 3);
    assert_eq!(rr["deadline_ms"], 600);
    assert!(rr["latency_ms"].as_u64().is_some());
    // Jev saw the message and the three notes, numbered in the fused order.
    let seen = jev.seen();
    assert_eq!(seen.len(), 1);
    let state = &seen[0].body["state"];
    assert!(state["message"]
        .as_str()
        .unwrap()
        .starts_with("Where does the grey heron nest?"));
    assert_eq!(
        state["notes"][2]["text"],
        "the grey heron nests by the old weir"
    );
    // Its state is in the blob the row names.
    let digest = d["state"]["sha256"].as_str().unwrap();
    assert_eq!(ctx["blob"].as_str(), Some(digest));
    // Paid by the judge's day budget, never the session's.
    let cost = d["cost_micros"].as_u64().unwrap();
    assert!(cost > 0);
    assert_eq!(rr["cost_micros"].as_u64(), Some(cost));
    let h = c.health().judge.unwrap();
    assert_eq!((h.calls_today, h.failed_today), (1, 0));
    assert_eq!(h.spend_today_usd, theseus_judge::price::micros_to_usd(cost));
    assert!(
        h.packs
            .contains(&"rerank.v1: live (owner: decision of 2026-10-04)".to_string()),
        "{:?}",
        h.packs
    );
    let s: SessionRecord = c.store.get_session(&here).unwrap().unwrap();
    assert_eq!(Some(s.cost_usd), res.cost_usd);
    // The turn's trace is marked at the dispatch, with the row's id.
    let mut found = Vec::new();
    marks(res.trace.as_ref().unwrap(), &mut found);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["pack"], "rerank.v1");
    assert_eq!(found[0]["point"], "recall");
    assert_eq!(found[0]["mode"], "shadow");
    assert_eq!(found[0]["judgment"], d["id"]);
}

/// A Jev slower than the deadline: the call times out at the rerank's own
/// deadline, and the row falls back to the fused order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_timeout_falls_back_to_the_fused_order() {
    let jev = FakeJev::start().unwrap();
    jev.script("helps.3", Jev::Noul(0.97));
    jev.set_mode(FakeMode::Slow(Duration::from_secs(5)));
    let (r, here, _) = heron_rig(&jev, |_| {});
    let c = &r.core;
    turn(c, &here, "Where does the grey heron nest?").await;
    let rows = until_reranked(&c.store, 1).await;
    let d = &rows[0].data;
    assert_eq!(d["outcome"]["outcome"], "failed", "{d}");
    assert_eq!(d["outcome"]["class"], "timeout");
    let rr = &d["context"]["rerank"];
    assert_eq!(rr["fallback"], "timeout");
    assert_eq!(rr["changed"], false);
    assert_eq!(rr["order_changed"], false);
    assert_eq!(rr["fused_admitted"], rr["reranked_admitted"]);
    let took = rr["latency_ms"].as_u64().unwrap();
    assert!((550..3000).contains(&took), "{took} ms against 600");
    assert_eq!(rr["within_deadline"], took <= 600);
}

/// The day's limit skips a rerank: nothing reaches Jev, no row, and the
/// skip is counted with the day's `judge.paused` row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_days_limit_skips_a_rerank() {
    let jev = FakeJev::start().unwrap();
    let (r, here, _) = heron_rig(&jev, |c| c.judge.shadow_limit_usd_per_day = 0.0);
    let c = &r.core;
    turn(c, &here, "Where does the grey heron nest?").await;
    // The skip is counted as the reservation answers, and its `judge.paused`
    // row appended just after: wait for both.
    let paused = || {
        c.store
            .ledger_tail::<LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "judge.paused")
            .count()
    };
    let t0 = Instant::now();
    while c.health().judge.unwrap().skipped_today == 0 || paused() == 0 {
        assert!(t0.elapsed() < Duration::from_secs(20), "never skipped");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(paused(), 1);
    assert_eq!(c.health().judge.unwrap().skipped_today, 1);
    assert!(reranks(&c.store).is_empty());
    assert_eq!(jev.connections(), 0);
}

/// `[judge.packs."rerank.v1"] mode = "off"`: a recall calls nothing and
/// marks nothing, while `loop.v1` still judges.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mode_off_calls_nothing() {
    let jev = FakeJev::start().unwrap();
    let (r, here, _) = heron_rig(&jev, |c| {
        pack_mode(c, "rerank.v1", PackMode::Off);
        c.judge.packs.remove("loop.v1");
    });
    let c = &r.core;
    let res = turn(c, &here, "Where does the grey heron nest?").await;
    assert_eq!(recalls(c, &here).len(), 1, "the recall ran");
    // loop.v1's judgment lands after the turn: the rerank would have by then.
    let t0 = Instant::now();
    while c.store.scope_after("judge:loop", 0).unwrap().is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "loop.v1 never judged"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(reranks(&c.store).is_empty());
    for s in jev.seen() {
        let qs = s.body["questions"].as_object().unwrap();
        assert!(qs.keys().all(|k| k.starts_with("loop.v1/")), "{qs:?}");
    }
    let mut found = Vec::new();
    marks(res.trace.as_ref().unwrap(), &mut found);
    assert!(found.is_empty(), "{found:?}");
    assert!(c
        .health()
        .judge
        .unwrap()
        .packs
        .contains(&"rerank.v1: off (the config's ceiling; on the ladder: live (owner: decision of 2026-10-04))".to_string()));
}

/// The requests a core's model got, as bytes.
pub(crate) fn sent(model: &FakeProvider) -> Vec<String> {
    let reqs: Vec<ProviderRequest> = model.requests.lock().unwrap().clone();
    reqs.iter()
        .map(|q| serde_json::to_string(&(&q.system, &q.messages, &q.tools)).unwrap())
        .collect()
}

/// A slow, failing, or rate-limited Jev changes no turn: the model gets the
/// same bytes as with the judge off, and the turn never waits for the
/// rerank, whose deadline here is 5 s (a turn that waited would take it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_or_failing_jev_changes_no_turn() {
    let off = rig(None, |_| {});
    let notes = session(&off.core, None, &["the grey heron nests by the old weir"]);
    let here = session(&off.core, None, &[]);
    off.core
        .runner
        .memory
        .set_ask(index_of(&off.core, vec![notes]));
    let base = turn(&off.core, &here, "Where does the grey heron nest?").await;
    let base_req = sent(&off.model);
    for mode in [
        FakeMode::Slow(Duration::from_secs(10)),
        FakeMode::Down,
        FakeMode::RateLimited {
            retry_after_secs: 7,
        },
        FakeMode::Malformed,
    ] {
        let jev = FakeJev::start().unwrap();
        jev.set_mode(mode.clone());
        let r = rig(Some(&jev), |_| {});
        let c = &r.core;
        c.runner.judge.set_rerank_deadline(Duration::from_secs(5));
        let notes = session(c, None, &["the grey heron nests by the old weir"]);
        let here = session(c, None, &[]);
        c.runner.memory.set_ask(index_of(c, vec![notes]));
        let t0 = Instant::now();
        let res = turn(c, &here, "Where does the grey heron nest?").await;
        let took = t0.elapsed();
        assert!(
            took < Duration::from_secs(3),
            "{mode:?}: the turn took {took:?}"
        );
        assert_eq!(
            (&res.output, &res.stop_reason, res.loops, res.cost_usd),
            (&base.output, &base.stop_reason, base.loops, base.cost_usd),
            "{mode:?}: the turn's result"
        );
        assert_eq!(sent(&r.model), base_req, "{mode:?}: the turn's request");
        let rows = until_reranked(&c.store, 1).await;
        let rr = &rows[0].data["context"]["rerank"];
        assert!(rr["fallback"].is_string(), "{mode:?}: {rr}");
        assert_eq!(rr["changed"], false, "{mode:?}");
    }
}

/// A reranked turn writes the frames it writes with the judge off: the
/// rerank's reservation draws on a block written before, and its row rides
/// in the sink's own frame, after the turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reranked_turn_keeps_its_frame_budget() {
    let mut counts = Vec::new();
    for judged in [false, true] {
        let jev = FakeJev::start().unwrap();
        let r = rig(judged.then_some(&jev), |_| {});
        let c = &r.core;
        let notes = session(c, None, &["the grey heron nests by the old weir"]);
        let here = session(c, None, &[]);
        c.runner.memory.set_ask(index_of(c, vec![notes]));
        turn(c, &here, "warm up").await;
        if judged {
            // The first rerank reserved the day's first block, and is written.
            until_reranked(&c.store, 1).await;
            // The next waits on Jev past the turn and the sink's window.
            jev.set_mode(FakeMode::Slow(Duration::from_secs(10)));
            c.runner.judge.set_rerank_deadline(Duration::from_secs(5));
        }
        let before = c.store.stats().unwrap().frames_appended;
        let res = turn(c, &here, "Where does the grey heron nest?").await;
        let frames = c.store.stats().unwrap().frames_appended - before;
        assert_eq!(res.loops, 1);
        assert!(frames <= 5, "judged {judged}: a plain turn wrote {frames}");
        counts.push(frames);
    }
    assert_eq!(counts[0], counts[1], "the rerank changed the turn's frames");
}

/// Where a generated session speaks.
#[derive(Clone, Copy, Debug)]
enum Spot {
    Cli,
    OwnerDm,
    AliceDm,
    Den,
    Pier,
    Quay,
    /// A task of the pier's.
    PierTask,
}

impl Spot {
    fn place(self) -> Option<String> {
        match self {
            Spot::Cli | Spot::PierTask => None,
            Spot::OwnerDm => Some(format!("dm:{OWNER}")),
            Spot::AliceDm => Some(format!("dm:{ALICE}")),
            Spot::Den => Some(format!("channel:{DEN}")),
            Spot::Pier => Some(format!("channel:{PIER}")),
            Spot::Quay => Some(format!("channel:{QUAY}")),
        }
    }

    /// Private, or the shared place it speaks in, written out by hand.
    fn class(self) -> Result<(), String> {
        match self {
            Spot::Cli | Spot::OwnerDm | Spot::Den => Ok(()),
            Spot::AliceDm => Err(format!("dm:{ALICE}")),
            Spot::Pier | Spot::PierTask => Err(format!("channel:{PIER}")),
            Spot::Quay => Err(format!("channel:{QUAY}")),
        }
    }
}

fn spot() -> impl Strategy<Value = Spot> {
    prop_oneof![
        Just(Spot::Cli),
        Just(Spot::OwnerDm),
        Just(Spot::AliceDm),
        Just(Spot::Den),
        Just(Spot::Pier),
        Just(Spot::Quay),
        Just(Spot::PierTask),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, ..ProptestConfig::default() })]

    /// The place property test through the rerank, over generated stores and
    /// places: the state Jev is sent holds no note of a session the place
    /// filter dropped, and holds every note the filters passed.
    #[test]
    fn a_reranks_state_holds_nothing_the_place_filter_dropped(
        asker in spot(),
        spots in proptest::collection::vec(spot(), 1..12),
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async {
            let jev = FakeJev::start().unwrap();
            let r = rig(Some(&jev), |c| c.memory.recall_max_items = 40);
            let c = &r.core;
            let mut made = Vec::new();
            for (i, s) in spots.iter().enumerate() {
                let sid = session(c, None, &[&format!("note number {i}")]);
                if let Some(p) = s.place() {
                    c.outbox.bind_place(&p, &sid).unwrap();
                }
                if matches!(s, Spot::PierTask) {
                    c.outbox.task_bound(&sid, &format!("discord:channel:{PIER}"));
                }
                made.push((sid, *s));
            }
            let me = session(c, None, &[]);
            if let Some(p) = asker.place() {
                c.outbox.bind_place(&p, &me).unwrap();
            }
            if matches!(asker, Spot::PierTask) {
                c.outbox.task_bound(&me, &format!("discord:channel:{PIER}"));
            }
            // A session a later one replaced in its place speaks nowhere now.
            let spoken: Vec<(usize, Spot)> = made
                .iter()
                .enumerate()
                .map(|(i, (sid, s))| {
                    let current = s.place().is_none_or(|p| {
                        c.outbox.place_session(&p).unwrap().as_deref() == Some(sid.as_str())
                    });
                    (i, if current { *s } else { Spot::Cli })
                })
                .collect();
            c.runner.memory.set_ask(index_of(c, made.iter().map(|(s, _)| s.clone()).collect()));
            turn(c, &me, "the notes?").await;
            let here = asker.class();
            let may: Vec<usize> = spoken
                .iter()
                .filter(|(_, s)| match (&here, s.class()) {
                    (Ok(()), Ok(())) => true,
                    (Err(a), Err(b)) => *a == b,
                    _ => false,
                })
                .map(|(i, _)| *i)
                .collect();
            let notes: Vec<String> = if may.is_empty() {
                // Nothing passed the filters: nothing is sent.
                tokio::time::sleep(Duration::from_millis(100)).await;
                prop_assert!(jev.seen().is_empty(), "a rerank with nothing eligible");
                Vec::new()
            } else {
                until_reranked(&c.store, 1).await;
                let seen = jev.seen();
                prop_assert_eq!(seen.len(), 1);
                seen[0].body["state"]["notes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n["text"].as_str().unwrap().to_string())
                    .collect()
            };
            let mut want: Vec<String> = may.iter().map(|i| format!("note number {i}")).collect();
            let mut got = notes;
            want.sort();
            got.sort();
            prop_assert_eq!(got, want, "{:?} asked of {:?}", asker, spoken);
            Ok(())
        })?;
    }
}
