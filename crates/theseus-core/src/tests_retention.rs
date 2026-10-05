//! FSRS-6 retention through whole cores (M6 step 32a's wire-in; design
//! §3.2's 32a row): the memory rows to their events, the projection rebuilt
//! from the record against the one kept as the rows are written, and the
//! `+retention` arm's rank in a search, a live turn, and a live rerank's
//! repack, against a stand-in index whose candidates tie.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_memory::{Access, AccessEvent, Durability, Fsrs6, Grade, Label, Outcome, Retention};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemoryLabelParams, MemorySearchParams, RecallManifest};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};
use tokio::sync::mpsc;

use crate::config::memory::MemoryArm;
use crate::config::{MemoryMode, PackMode};
use crate::ledger::LedgerRow;
use crate::recall::retention::{self, Phase, Projection};
use crate::recall::{Ask, AskFuture};
use crate::tests_rerank::{pack_mode, recalls, rig, sent, session, turn, Rig};
use crate::tests_rerank_live::{answered, ChannelJudge};
use crate::Core;

const DAY: u64 = 86_400_000;

/// Ten days ago, so the rows' times stand well apart from a label's now.
fn t0() -> u64 {
    theseus_protocol::now_unix_ms() - 10 * DAY
}

/// A memory row about `node` at `at`, as the pass or a label writes it.
fn row(kind: LedgerKind, node: &str, at: u64, data: Value) -> NewRecord {
    let mut data = data;
    data["node_id"] = json!(node);
    let row = LedgerRow {
        at_unix_ms: at,
        ..LedgerRow::new(kind, Some("ses_pass"), None, data)
    };
    NewRecord::json(kinds::LEDGER, None, &row).unwrap()
}

fn labeled(node: &str, at: u64, durability: &str) -> NewRecord {
    row(
        LedgerKind::MemoryLabeled,
        node,
        at,
        json!({"durability": durability, "position": 1}),
    )
    .scoped("memory:ses_pass")
}

fn used(node: &str, at: u64, outcome: Option<&str>) -> NewRecord {
    row(
        LedgerKind::MemoryUsed,
        node,
        at,
        json!({"recall_id": "rcl_x", "used": outcome.is_some(), "outcome": outcome}),
    )
    .scoped("recall:ses_pass")
}

/// A frame the memory pass would write: appended, and handed to the
/// projection as the pass hands it.
fn frame(c: &Core, records: &[NewRecord]) -> Vec<u64> {
    let at = c.store.append(records).unwrap();
    c.runner.memory.retention_written(records, &at);
    at
}

/// Build the projection and wait, on the runtime's timer, until it is.
async fn built(c: &Arc<Core>) {
    retention::warm(&c.runner.memory, &c.store);
    let t = Instant::now();
    while c.runner.memory.retention().phase() != Phase::Ready {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "{:?}",
            c.runner.memory.retention().phase()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A projection rebuilt from the record alone.
fn rebuilt(c: &Core) -> Projection {
    let p = Projection::default();
    assert!(p.ask());
    assert!(retention::walk(&c.store, &p).unwrap(), "the index answers");
    p.built(Ok(()));
    p
}

fn label(c: &Core, node: &str, word: &str) {
    c.memory_label(
        &MemoryLabelParams {
            node_id: node.into(),
            label: word.into(),
            recall_id: None,
            note: None,
        },
        "cli",
    )
    .unwrap();
}

/// The time the newest label row on `node` was written.
fn label_at(c: &Core, node: &str) -> u64 {
    c.store
        .scope_after(crate::recall::labels::SCOPE, 0)
        .unwrap()
        .iter()
        .filter_map(|r| r.decode::<LedgerRow>().ok())
        .filter(|r| r.data["node_id"] == node)
        .map(|r| r.at_unix_ms)
        .next_back()
        .unwrap()
}

fn live_retention(c: &mut crate::Config) {
    c.memory.mode = MemoryMode::Live;
    c.memory.arm = MemoryArm::Retention;
}

/// The projection kept as the rows are written equals one rebuilt from the
/// record, event for event, whatever order the frames reach it in: the
/// pass's frames, the operator's labels through `memory.label`, and a frame
/// handed over after a later one (the pass and a label racing).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rebuilt_projection_equals_the_incremental_one() {
    let r = rig(None, live_retention);
    let c = &r.core;
    let sid = session(
        c,
        None,
        &["the wren sings at the gate", "the ash leans", "a reed"],
    );
    let ids: Vec<String> = c
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n.id)
        .collect();
    let (wren, ash, reed) = (&ids[0], &ids[1], &ids[2]);
    // Rows written before the build: the walk reads them.
    let t = t0();
    c.store
        .append(&[labeled(wren, t, "high"), labeled(ash, t, "medium")])
        .unwrap();
    built(c).await;
    let p = c.runner.memory.retention();
    assert_eq!(p.shape().events, 2);
    // After it: followed as they are written.
    frame(
        c,
        &[
            labeled(reed, t, "floor"),
            used(wren, t + 2 * DAY, Some("ok")),
        ],
    );
    frame(
        c,
        &[
            used(ash, t + 2 * DAY, None),
            used(reed, t + 3 * DAY, Some("corrected")),
        ],
    );
    label(c, ash, "useful");
    // Two frames, written in order, handed over in the other order.
    let early = [used(wren, t + 4 * DAY, Some("unknown"))];
    let late = [used(wren, t + 6 * DAY, Some("ok"))];
    let at_early = c.store.append(&early).unwrap();
    let at_late = c.store.append(&late).unwrap();
    c.runner.memory.retention_written(&late, &at_late);
    c.runner.memory.retention_written(&early, &at_early);
    // The same frame twice counts once.
    c.runner.memory.retention_written(&late, &at_late);
    label(c, reed, "stale");

    let again = rebuilt(c);
    assert_eq!(p.all(), again.all(), "the rebuild differs");
    assert_eq!(p.shape().events, again.shape().events);
    assert_eq!(p.shape().nodes, 3);
    assert_eq!(p.shape().events, 10);
    // And the wren's is the fold of its events in position order.
    let f = Fsrs6::default();
    let ev = |at, access| AccessEvent { at_ms: at, access };
    let want = f
        .fold(&[
            ev(t, Access::FirstSight(Durability::High)),
            ev(t + 2 * DAY, Access::Used(Outcome::Ok)),
            ev(t + 4 * DAY, Access::Used(Outcome::Unknown)),
            ev(t + 6 * DAY, Access::Used(Outcome::Ok)),
        ])
        .unwrap();
    assert_eq!(p.get(wren), Some(want));
}

/// Exposure without use changes nothing: a `memory.used` row with `used:
/// false` leaves the node's retention as it was, however many there are.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exposure_without_use_changes_nothing() {
    let r = rig(None, live_retention);
    let c = &r.core;
    built(c).await;
    let t = t0();
    frame(c, &[labeled("nod_lark", t, "medium")]);
    let before = c.runner.memory.retention().get("nod_lark").unwrap();
    for d in 1..=5 {
        frame(c, &[used("nod_lark", t + d * DAY, None)]);
    }
    assert_eq!(c.runner.memory.retention().get("nod_lark"), Some(before));
    assert_eq!(rebuilt(c).get("nod_lark"), Some(before));
    // Shown first, before any sight: still no retention.
    frame(c, &[used("nod_kite", t, None)]);
    assert_eq!(c.runner.memory.retention().get("nod_kite"), None);
}

/// First sight by durability: preference or decision Easy, fact or
/// procedure Good, episode Hard, transient Again; and each label's grade
/// through `memory.label`: `useful` and `should_have` Easy, `wrong` and
/// `stale` Again, at the label's time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_sight_by_durability_and_each_labels_grade() {
    let r = rig(None, live_retention);
    let c = &r.core;
    built(c).await;
    let f = Fsrs6::default();
    let t = t0();
    for (word, grade) in [
        ("high", Grade::Easy),
        ("medium", Grade::Good),
        ("low", Grade::Hard),
        ("floor", Grade::Again),
    ] {
        let node = format!("nod_{word}");
        frame(c, &[labeled(&node, t, word)]);
        assert_eq!(
            c.runner.memory.retention().get(&node),
            Some(f.initial(grade, t)),
            "{word}"
        );
    }
    let sid = session(c, None, &["one", "two", "three", "four"]);
    let nodes: Vec<String> = c
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n.id)
        .collect();
    for (node, (word, grade)) in nodes.iter().zip([
        ("useful", Grade::Easy),
        ("should_have", Grade::Easy),
        ("wrong", Grade::Again),
        ("stale", Grade::Again),
    ]) {
        frame(c, &[labeled(node, t, "medium")]);
        label(c, node, word);
        let first = f.initial(Grade::Good, t);
        assert_eq!(
            c.runner.memory.retention().get(node),
            Some(f.review(&first, grade, label_at(c, node))),
            "{word}"
        );
    }
    // The table's label for `should_have` is the core's word's.
    assert_eq!(
        Access::Labeled(Label::ShouldHave).grade(),
        Some(Grade::Easy)
    );
}

/// A stand-in index answering `sessions`' nodes, every one at the same
/// fused score: only the science can order them.
fn tied_index(c: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let store = c.store.clone();
    Arc::new(move |p| -> AskFuture {
        let mut hits = Vec::new();
        for sid in &sessions {
            for (position, n) in store.session_nodes(sid).unwrap() {
                if p.as_of.is_some_and(|a| position >= a) {
                    continue;
                }
                hits.push(IndexHit {
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
                            rank: 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: 0.02,
                });
            }
        }
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

/// Two notes the index ties: the one whose key sorts first (`first`, which
/// the baseline admits) was used and corrected, and the other (`second`)
/// used `ok`, a week ago. Returns the rig, the turn's session, and both.
async fn tied(
    jev: Option<&FakeJev>,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, String, String, String) {
    let r = rig(jev, tweak);
    let c = &r.core;
    let a = session(c, None, &["the osprey build caches to the blue bucket"]);
    let b = session(c, None, &["the osprey build runs on the larch runner"]);
    let here = session(c, None, &[]);
    let node = |s: &str| c.store.session_nodes(s).unwrap()[0].1.id.clone();
    let mut two = [node(&a), node(&b)];
    two.sort();
    let [first, second] = two;
    c.runner.memory.set_ask(tied_index(c, vec![a, b]));
    let t = t0();
    frame(
        c,
        &[
            labeled(&first, t, "medium"),
            labeled(&second, t, "medium"),
            used(&first, t + 3 * DAY, Some("corrected")),
            used(&second, t + 3 * DAY, Some("ok")),
        ],
    );
    built(c).await;
    (r, here, first, second)
}

fn search(arm: Option<&str>) -> MemorySearchParams {
    MemorySearchParams {
        query: "the osprey build".into(),
        arm: arm.map(str::to_string),
        ..Default::default()
    }
}

/// Under `+retention`, two candidates with equal fused scores order by
/// retention: a search admits the one used `ok` (with its retrievability,
/// stability, difficulty and last review), where `baseline` admits the
/// other by its key; and a live turn does the same, its row and request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn equal_fused_scores_order_by_retention() {
    let (r, here, first, second) = tied(None, live_retention).await;
    let c = &r.core;
    let base = c.memory_search(search(None)).await.unwrap();
    assert!(base.science.starts_with("baseline@"), "{}", base.science);
    assert_eq!(base.admitted[0].node_id, first);
    assert_eq!(
        (base.retention.as_deref(), &base.admitted[0].retention),
        (None, &None)
    );
    let m = c.memory_search(search(Some("+retention"))).await.unwrap();
    assert!(m.science.starts_with("retention@"), "{}", m.science);
    assert_eq!(m.retention.as_deref(), Some("ready"));
    assert_eq!(m.admitted[0].node_id, second, "{m:?}");
    let rr = m.admitted[0].retention.as_ref().expect("its retention");
    let p: Retention = c.runner.memory.retention().get(&second).unwrap();
    assert_eq!(
        (rr.stability, rr.difficulty, rr.last_review_ms),
        (p.stability, p.difficulty, p.last_review_ms)
    );
    assert!(rr.retrievability > 0.0 && rr.retrievability < 1.0, "{rr:?}");
    // A live turn under the arm admits the same, and says so.
    turn(c, &here, "Tell me about the osprey build.").await;
    let row: &RecallManifest = &recalls(c, &here)[0];
    assert_eq!(row.arm.as_deref(), Some("+retention"));
    assert!(row.science.starts_with("retention@"), "{}", row.science);
    assert_eq!(row.admitted[0].node_id, second);
    assert!(row.admitted[0].retention.is_some());
    let req = sent(&r.model).pop().unwrap();
    assert!(
        req.contains("larch runner") != req.contains("blue bucket"),
        "{req}"
    );
    let second_text = c.store.session_nodes(&row.admitted[0].session_id).unwrap()[0]
        .1
        .clone();
    assert!(req.contains(&crate::recall::text_of(&second_text)), "{req}");
    // Labeled stale: a same-day Again lowers its stability, and the search
    // drops it as `labeled_wrong`, its retention on the drop.
    label(c, &second, "stale");
    let m = c.memory_search(search(Some("+retention"))).await.unwrap();
    let d = m.dropped.iter().find(|d| d.node_id == second).unwrap();
    assert_eq!(d.reason, "labeled_wrong");
    let after = d.retention.as_ref().expect("the drop's retention");
    assert!(after.stability < rr.stability, "{after:?} after {rr:?}");
    assert_eq!(m.admitted[0].node_id, first);
}

/// The repack in Jev's order keeps the arm's science: Jev answers both
/// notes alike, so its order keeps the science's, and the live rerank's
/// pack admits the one retention ranks first, as the recall did. On tokio's
/// paused clock, with Jev a channel that answers at once, so a loaded
/// machine never runs the wait out.
#[tokio::test(start_paused = true)]
async fn the_repack_in_jevs_order_keeps_the_arms_science() {
    let jev = FakeJev::start().unwrap();
    let (r, here, _, second) = tied(Some(&jev), |c| {
        live_retention(c);
        pack_mode(c, crate::judge::rerank::RERANK_PACK, PackMode::Live);
    })
    .await;
    let c = r.core.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    c.runner.judge.rerank_with(Arc::new(ChannelJudge(tx)));
    let turned = {
        let (c, here) = (c.clone(), here.clone());
        tokio::spawn(async move { turn(&c, &here, "Tell me about the osprey build.").await })
    };
    let (point, reply) = rx.recv().await.unwrap();
    let _ = reply.send(answered(&point, |_| 0.6));
    turned.await.unwrap();
    let m = &recalls(&c, &here)[0];
    let rr = m.rerank.as_ref().expect("a live rerank");
    assert!(rr.applied, "{rr:?}");
    assert!(m.science.starts_with("retention@"), "{}", m.science);
    assert_eq!(m.admitted[0].node_id, second, "{m:?}");
    assert!(
        m.admitted[0].retention.is_some(),
        "the repack's items keep it"
    );
}

/// A recall before the projection is built ranks without it and says so;
/// once built, the same search reads it. Health names its state and nodes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recall_before_the_build_ranks_without_it_and_says_so() {
    let r = rig(None, |c| c.memory.mode = MemoryMode::Shadow);
    let c = &r.core;
    assert_eq!(c.health().memory.unwrap().retention, "unbuilt");
    let a = session(c, None, &["the osprey build caches to the blue bucket"]);
    c.runner.memory.set_ask(tied_index(c, vec![a]));
    frame(c, &[labeled("nod_unseen", t0(), "medium")]);
    // A shadow daemon's baseline never builds it.
    c.memory_search(search(None)).await.unwrap();
    assert_eq!(c.runner.memory.retention().phase(), Phase::Unasked);
    let m = c.memory_search(search(Some("+retention"))).await.unwrap();
    assert!(
        matches!(m.retention.as_deref(), Some("building" | "ready")),
        "{:?}",
        m.retention
    );
    if m.retention.as_deref() == Some("building") {
        assert!(m.admitted.iter().all(|i| i.retention.is_none()));
    }
    let t = Instant::now();
    while c.runner.memory.retention().phase() != Phase::Ready {
        assert!(t.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let m = c.memory_search(search(Some("+retention"))).await.unwrap();
    assert_eq!(m.retention.as_deref(), Some("ready"));
    let h = c.health().memory.unwrap();
    assert_eq!((h.retention.as_str(), h.nodes, h.events), ("ready", 1, 1));
    assert!(c.memory_search(search(Some("none"))).await.is_err());
}

/// A shadow turn under the arm's config changes no request byte: shadow
/// assigns no arm, and nothing of recall reaches the model.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shadow_turn_under_the_arm_changes_no_request_byte() {
    let mut requests = Vec::new();
    for mode in [MemoryMode::Off, MemoryMode::Shadow] {
        let r = rig(None, |c| {
            c.memory.mode = mode;
            c.memory.arm = MemoryArm::Retention;
        });
        let c = &r.core;
        let notes = session(c, None, &["the osprey build caches to the blue bucket"]);
        let here = session(c, None, &[]);
        c.runner.memory.set_ask(tied_index(c, vec![notes]));
        turn(c, &here, "where does the osprey build cache?").await;
        requests.push(sent(&r.model));
    }
    assert_eq!(requests[0], requests[1], "shadow changed the request");
}
