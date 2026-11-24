//! Recall in shadow through the whole core (M6 step 30a; §3.2's tests): a
//! turn's recall is recorded with what it would admit and why it dropped the
//! rest (every place, since 2026-11-22: anything goes); a stalled
//! index never holds the turn; and shadow writes no frame and changes no byte
//! of the model's request. The index is a stand-in (`Memory::set_ask`) that
//! answers every node of the sessions it is given, so the filters decide.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemoryRecallsParams, MemorySearchParams, RecallManifest};
use theseus_protocol::SessionKind;

use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::places::BoundPlace;
use crate::provider::FakeProvider;
use crate::recall::{Ask, AskFuture};
use crate::session::SessionRecord;
use crate::store::Store;
pub(crate) use crate::test_support::turn;
use crate::{Config, Core};

/// The owner on Discord, someone else, and three guild channels: one bound
/// private, two shared.
pub(crate) const OWNER: u64 = 161_803_398_874_989_484;
pub(crate) const ALICE: u64 = 333_333_333_333_333_333;
pub(crate) const DEN: u64 = 271_000_000_000_000_001;
pub(crate) const PIER: u64 = 271_000_000_000_000_002;
pub(crate) const QUAY: u64 = 271_000_000_000_000_003;

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) model: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

pub(crate) fn rig(mode: MemoryMode) -> Rig {
    rig_with(mode, |_| {})
}

pub(crate) fn rig_with(mode: MemoryMode, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig_built(mode, tweak, |_| {})
}

/// The same, with `board` as the core's secrets (the judge's key).
pub(crate) fn rig_with_secrets(
    mode: MemoryMode,
    board: Arc<crate::secrets::SecretBoard>,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    rig_built(mode, tweak, |p| p.secrets = board)
}

pub(crate) fn rig_built(
    mode: MemoryMode,
    tweak: impl FnOnce(&mut Config),
    parts: impl FnOnce(&mut crate::rpc::Parts),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.memory.mode = mode;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::default());
    let mut p = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts(&mut p);
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

/// A session, posting to `place` (`channel:<id>`, `dm:<user>`) when given,
/// saying `said`.
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

/// A stand-in index over `sessions`: every node of theirs written before
/// the query's `as_of`, the newest first, as BM25's hits.
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
                // A result's tool, as the index's extractor names it.
                tool: match &n.body {
                    crate::node::Body::ToolResult { tool, .. } => Some(tool.clone()),
                    _ => None,
                },
                time_ms: 0,
                external: false,
                entities_matched: vec![],
                entities: Vec::new(),
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

/// The session's recall rows, as `memory.recalls` reads them.
pub(crate) fn recalls(core: &Core, sid: &str) -> Vec<RecallManifest> {
    core.memory_recalls(MemoryRecallsParams {
        session_id: sid.into(),
        limit: Some(200),
    })
    .unwrap()
    .recalls
}

pub(crate) fn admitted(m: &RecallManifest) -> Vec<&str> {
    m.admitted.iter().map(|a| a.session_id.as_str()).collect()
}

pub(crate) fn dropped_for<'a>(m: &'a RecallManifest, reason: &str) -> Vec<&'a str> {
    m.dropped
        .iter()
        .filter(|d| d.reason == reason)
        .map(|d| d.session_id.as_str())
        .collect()
}

/// A turn in a private place recalls from any session, a shared place's
/// included (theseus-1is6), and drops its own for being in context;
/// its row says so, scoped to the session, and `memory.recalls` gives each
/// admitted item's text.
#[tokio::test]
async fn a_turn_records_what_recall_admits_and_why_it_dropped_the_rest() {
    let r = rig(MemoryMode::Live);
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
    assert_eq!((m.mode.as_str(), m.outcome.as_str()), ("live", "ran"));
    assert_eq!(m.place, "private");
    assert!(m.science.starts_with("baseline@"), "{}", m.science);
    let mut got = admitted(m);
    got.sort_unstable();
    let mut want = [notes.as_str(), pier.as_str()];
    want.sort_unstable();
    assert_eq!(got, want);
    assert!(dropped_for(m, "place").is_empty());
    let item = m.admitted.iter().find(|a| a.session_id == notes).unwrap();
    assert_eq!(
        item.text.as_deref(),
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
    assert_eq!(raw[0].kind, "recall.ran");
    assert!(
        !bytes.contains("weir") && !bytes.contains("heron"),
        "{bytes}"
    );
    assert_eq!(m.query_chars, "where does the heron nest?".len() as u64);
}

/// A stalled index never holds the turn: it answers at its call's pace, and
/// the row says `deadline`. With no tender running, `unavailable`, and why.
/// On the paused clock: the bound on the turn's time is its own virtual time.
#[tokio::test(start_paused = true)]
async fn a_stalled_index_never_holds_the_turn() {
    let r = rig_with(MemoryMode::Live, |c| c.memory.recall_deadline_ms = 50);
    let c = &r.core;
    let here = session(c, None, &[]);
    // Before the stand-in: the core's own tender, which is not running.
    turn(c, &here, "first").await;
    c.runner.memory.set_ask(Arc::new(|_| -> AskFuture {
        Box::pin(std::future::pending())
    }));
    let t0 = tokio::time::Instant::now();
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
    // A private place draws on a shared place's session too (theseus-1is6).
    assert_eq!(admitted(&m), [pier.as_str(), notes.as_str()]);
    assert_eq!(
        m.admitted[1].text.as_deref(),
        Some("the heron nests by the weir")
    );
    let m = c
        .memory_search(MemorySearchParams {
            query: "heron".into(),
            session_id: Some(pier.clone()),
            k: None,
            arm: None,
            as_of: None,
        })
        .await
        .unwrap();
    assert_eq!(dropped_for(&m, "in_context"), [pier.as_str()]);
    assert_eq!(c.store.stats().unwrap().frames_appended, before);
    assert!(c
        .memory_search(MemorySearchParams {
            query: "x".into(),
            session_id: Some("ses_nobody".into()),
            k: None,
            arm: None,
            as_of: None,
        })
        .await
        .is_err());
}
