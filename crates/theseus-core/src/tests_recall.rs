//! Recall in shadow through the whole core (M6 step 30a; §3.2's tests): a
//! turn's recall is recorded with what it would admit and why it dropped the
//! rest; the place rule holds over generated stores and places; a stalled
//! index never holds the turn; and shadow writes no frame and changes no byte
//! of the model's request. The index is a stand-in (`Memory::set_ask`) that
//! answers every node of the sessions it is given, so the filters decide.

use std::sync::Arc;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemoryRecallsParams, MemorySearchParams, RecallManifest};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::places::BoundPlace;
use crate::provider::{FakeProvider, ProviderRequest};
use crate::recall::{Ask, AskFuture};
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

struct Rig {
    core: Arc<Core>,
    model: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(mode: MemoryMode) -> Rig {
    rig_with(mode, |_| {})
}

fn rig_with(mode: MemoryMode, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.memory.mode = mode;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{DEN}"),
        name: "#den".into(),
        private: true,
    }]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

/// A session, posting to `place` (`channel:<id>`, `dm:<user>`) when given,
/// saying `said`.
fn session(core: &Core, place: Option<&str>, said: &[&str]) -> String {
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

/// A stand-in index over `sessions`: every node of theirs written before
/// the query's `as_of`, the newest first, as BM25's hits.
fn index_of(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
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
        hits.sort_by_key(|h| std::cmp::Reverse(h.0));
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

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
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
        })
        .await
        .unwrap()
}

/// The session's recall rows, as `memory.recalls` reads them.
fn recalls(core: &Core, sid: &str) -> Vec<RecallManifest> {
    core.memory_recalls(MemoryRecallsParams {
        session_id: sid.into(),
        limit: Some(200),
    })
    .unwrap()
    .recalls
}

fn admitted(m: &RecallManifest) -> Vec<&str> {
    m.admitted.iter().map(|a| a.session_id.as_str()).collect()
}

fn dropped_for<'a>(m: &'a RecallManifest, reason: &str) -> Vec<&'a str> {
    m.dropped
        .iter()
        .filter(|d| d.reason == reason)
        .map(|d| d.session_id.as_str())
        .collect()
}

/// A turn in a private place recalls from private places' sessions, and
/// drops a shared place's for its place and its own for being in context;
/// its row says so, scoped to the session, and `memory.recalls` gives each
/// admitted item's text.
#[tokio::test]
async fn a_turn_records_what_recall_would_admit_and_why_it_dropped_the_rest() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let notes = session(c, None, &["the heron nests by the weir"]);
    let pier = session(
        c,
        Some(&format!("channel:{PIER}")),
        &["the pier's heron is grey"],
    );
    let here = session(c, None, &[]);
    c.runner
        .memory
        .set_ask(index_of(c, vec![notes.clone(), pier.clone(), here.clone()]));
    turn(c, &here, "where does the heron nest?").await;
    turn(c, &here, "and when?").await;
    let rows = recalls(c, &here);
    assert_eq!(rows.len(), 2, "one recall a turn");
    let m = &rows[0];
    assert_eq!((m.mode.as_str(), m.outcome.as_str()), ("shadow", "ran"));
    assert_eq!(m.place, "private");
    assert!(m.science.starts_with("baseline@"), "{}", m.science);
    assert_eq!(admitted(m), [notes.as_str()]);
    assert_eq!(dropped_for(m, "place"), [pier.as_str()]);
    assert_eq!(
        m.admitted[0].text.as_deref(),
        Some("the heron nests by the weir"),
        "memory.recalls reads each item's text from its node"
    );
    // The second turn's own first message, and its first reply, are in its
    // context: the first turn's input is dropped as such.
    assert!(dropped_for(&rows[1], "in_context").contains(&here.as_str()));
    // The row keeps references: no copy of another session's text, and
    // only the query's length and digest.
    let raw: Vec<LedgerRow> = c
        .store
        .scope_after(&crate::fact::recall::scope(&here), 0)
        .unwrap()
        .iter()
        .map(|r| r.decode().unwrap())
        .collect();
    let bytes = serde_json::to_string(&raw[0]).unwrap();
    assert_eq!(raw[0].kind, "recall.shadow");
    assert!(
        !bytes.contains("weir") && !bytes.contains("heron"),
        "{bytes}"
    );
    assert_eq!(m.query_chars, "where does the heron nest?".len() as u64);
}

/// A turn in a shared place draws only on that place's own sessions (its
/// earlier session, and a task of it): never the CLI's, an owner's DM, a
/// private channel, or another shared place.
#[tokio::test]
async fn a_shared_place_recalls_only_its_own_sessions() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let cli = session(c, None, &["the vault code is 4417"]);
    let dm = session(
        c,
        Some(&format!("dm:{OWNER}")),
        &["my dentist is on tuesday"],
    );
    let den = session(c, Some(&format!("channel:{DEN}")), &["the den's plans"]);
    let quay = session(c, Some(&format!("channel:{QUAY}")), &["the quay's tide"]);
    let task = session(c, None, &["the pier's task found the tide table"]);
    let pier = session(c, Some(&format!("channel:{PIER}")), &["the pier's tide"]);
    c.outbox
        .task_bound(&task, &format!("discord:channel:{PIER}"));
    c.runner.memory.set_ask(index_of(
        c,
        vec![
            cli.clone(),
            dm.clone(),
            den.clone(),
            quay.clone(),
            task.clone(),
        ],
    ));
    turn(c, &pier, "what is the tide?").await;
    let m = &recalls(c, &pier)[0];
    assert_eq!(m.place, format!("shared:discord:channel:{PIER}"));
    assert_eq!(admitted(m), [task.as_str()]);
    let mut dropped = dropped_for(m, "place");
    dropped.sort_unstable();
    let mut want = vec![cli.as_str(), dm.as_str(), den.as_str(), quay.as_str()];
    want.sort_unstable();
    assert_eq!(dropped, want);
}

/// A stalled index never holds the turn: it answers at its call's pace, and
/// the row says `deadline`. With no tender running, `unavailable`, and why.
#[tokio::test]
async fn a_stalled_index_never_holds_the_turn() {
    let r = rig_with(MemoryMode::Shadow, |c| c.memory.recall_deadline_ms = 50);
    let c = &r.core;
    let here = session(c, None, &[]);
    // Before the stand-in: the core's own tender, which is not running.
    turn(c, &here, "first").await;
    c.runner.memory.set_ask(Arc::new(|_| -> AskFuture {
        Box::pin(std::future::pending())
    }));
    let t0 = Instant::now();
    let res = turn(c, &here, "is anyone there?").await;
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    assert_eq!(res.loops, 1);
    let rows = recalls(c, &here);
    assert_eq!(rows[0].outcome, "unavailable");
    assert!(
        rows[0]
            .why
            .as_deref()
            .unwrap_or_default()
            .contains("the index tender is"),
        "{:?}",
        rows[0].why
    );
    assert_eq!(rows[1].outcome, "deadline");
    assert_eq!(rows[1].timings.deadline_ms, 50);
    assert!(rows[1].admitted.is_empty());
}

/// With memory off, a turn asks nothing and records nothing.
#[tokio::test]
async fn memory_off_recalls_nothing() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let notes = session(c, None, &["the heron nests by the weir"]);
    let here = session(c, None, &[]);
    let asked = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let (n, inner) = (asked.clone(), index_of(c, vec![notes]));
    c.runner.memory.set_ask(Arc::new(move |p| {
        n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        inner(p)
    }));
    turn(c, &here, "where does the heron nest?").await;
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(recalls(c, &here).is_empty());
}

/// The requests a core's model got, as bytes.
fn sent(model: &FakeProvider) -> Vec<String> {
    let reqs: Vec<ProviderRequest> = model.requests.lock().unwrap().clone();
    reqs.iter()
        .map(|q| serde_json::to_string(&(&q.system, &q.messages, &q.tools)).unwrap())
        .collect()
}

/// The same two turns, with memory off and in shadow: shadow admits
/// something, and the model gets the same bytes; and a plain turn in shadow
/// stays within the frame budget, its row riding in a frame it writes anyway.
#[tokio::test]
async fn shadow_writes_no_frame_and_changes_no_request_byte() {
    let mut requests = Vec::new();
    for mode in [MemoryMode::Off, MemoryMode::Shadow] {
        let r = rig(mode);
        let c = &r.core;
        let notes = session(c, None, &["the heron nests by the weir"]);
        let here = session(c, None, &[]);
        c.runner.memory.set_ask(index_of(c, vec![notes]));
        turn(c, &here, "warm up").await;
        let before = c.store.stats().unwrap().frames_appended;
        let res = turn(c, &here, "where does the heron nest?").await;
        let frames = c.store.stats().unwrap().frames_appended - before;
        assert!(frames <= 5, "{mode:?}: a plain turn wrote {frames} frames");
        let digests: Vec<String> = c
            .store
            .session_nodes(&here)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                crate::node::Body::AssistantMessage { request_digest, .. } => request_digest,
                _ => None,
            })
            .collect();
        if mode == MemoryMode::Shadow {
            let rows = recalls(c, &here);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1].turn_id.as_deref(), Some(res.turn_id.as_str()));
            assert_eq!(rows[1].admitted.len(), 1, "shadow would admit the note");
        }
        requests.push((sent(&r.model), digests));
    }
    assert_eq!(requests[0].0.len(), 2);
    assert_eq!(requests[0].0, requests[1].0, "shadow changed the request");
    assert_eq!(requests[0].1, requests[1].1, "the request's digest moved");
}

/// `memory.search` runs the pipeline for a query, writing nothing: as the
/// CLI's place with no session, and as a session's place with one.
#[tokio::test]
async fn memory_search_runs_the_pipeline_and_writes_nothing() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let notes = session(c, None, &["the heron nests by the weir"]);
    let pier = session(c, Some(&format!("channel:{PIER}")), &["the pier's heron"]);
    c.runner
        .memory
        .set_ask(index_of(c, vec![notes.clone(), pier.clone()]));
    let before = c.store.stats().unwrap().frames_appended;
    let m = c
        .memory_search(MemorySearchParams {
            query: "heron".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!((m.mode.as_str(), m.place.as_str()), ("search", "private"));
    assert_eq!(admitted(&m), [notes.as_str()]);
    assert_eq!(
        m.admitted[0].text.as_deref(),
        Some("the heron nests by the weir")
    );
    let m = c
        .memory_search(MemorySearchParams {
            query: "heron".into(),
            session_id: Some(pier.clone()),
            k: None,
        })
        .await
        .unwrap();
    assert!(m.admitted.is_empty());
    assert_eq!(dropped_for(&m, "place"), [notes.as_str()]);
    assert_eq!(dropped_for(&m, "in_context"), [pier.as_str()]);
    assert_eq!(c.store.stats().unwrap().frames_appended, before);
    assert!(c
        .memory_search(MemorySearchParams {
            query: "x".into(),
            session_id: Some("ses_nobody".into()),
            k: None,
        })
        .await
        .is_err());
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

    /// Private, or the shared place it speaks in: the place rule, written
    /// out by hand for the test.
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
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// The place property test (§3.2's 30a, as the place rule makes it): over
    /// generated stores and places, nothing from a private place's session is
    /// a candidate in a shared place, nothing from one shared place is one in
    /// another, and nothing of a shared place is one in a private place: each
    /// such hit is dropped for its place, and every other survives that
    /// filter.
    #[test]
    fn the_place_rule_holds_over_generated_stores(
        asker in spot(),
        spots in proptest::collection::vec(spot(), 1..12),
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async {
            let r = rig(MemoryMode::Shadow);
            let c = &r.core;
            let mut made = Vec::new();
            for (i, s) in spots.iter().enumerate() {
                let sid = session(c, None, &[&format!("note {i}")]);
                // Only a place's latest session speaks there.
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
            // A session a later one replaced in its place speaks nowhere now:
            // the CLI's, as the rule reads it.
            let spoken: Vec<(String, Spot)> = made
                .iter()
                .map(|(sid, s)| {
                    let current = s.place().is_none_or(|p| {
                        c.outbox.place_session(&p).unwrap().as_deref() == Some(sid.as_str())
                    });
                    (sid.clone(), if current { *s } else { Spot::Cli })
                })
                .collect();
            c.runner.memory.set_ask(index_of(c, made.iter().map(|(s, _)| s.clone()).collect()));
            turn(c, &me, "the notes?").await;
            let m = &recalls(c, &me)[0];
            prop_assert_eq!(m.candidates as usize, spots.len());
            let here = asker.class();
            for (sid, s) in &spoken {
                let may = match (&here, s.class()) {
                    (Ok(()), Ok(())) => true,
                    (Err(a), Err(b)) => *a == b,
                    _ => false,
                };
                let by_place = m.dropped.iter().any(|d| &d.session_id == sid && d.reason == "place");
                prop_assert_eq!(!may, by_place, "{:?} asked of {:?} ({})", asker, s, sid);
                if !may {
                    prop_assert!(!m.admitted.iter().any(|a| &a.session_id == sid));
                }
            }
            Ok(())
        })?;
    }
}
