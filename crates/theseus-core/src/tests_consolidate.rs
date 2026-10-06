//! Consolidation through the whole core (M6 step 31b; §3.2's 31b tests): an
//! invented Kestrel relay's three facts, recalled together by three asking
//! sessions in shadow, become one cited synthesis, checked by the fake Jev;
//! an unsupported sentence rejects it; without Jev it stays unchecked; a
//! cluster with external text is never synthesized; the day's spend stops at
//! its cap; a dry run writes nothing; no frame lands inside a turn; nothing is
//! shown in shadow; and under `+synthesis` a checked one is admitted, an
//! unchecked one dropped with its reason, and a shared place never sees one.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemoryConsolidateParams, MemoryConsolidateResult};
use theseus_protocol::LedgerKind;

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::consolidate::{CitationCheck, Stage};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::provider::Scripted;
use crate::recall::{Ask, AskFuture};
use crate::secrets::{Secret, SecretBoard};
use crate::session::{SessionRecord, TargetRef};
use crate::tests_recall::{recalls, session, turn, Rig, PIER};
use crate::Core;

const FACTS: [&str; 3] = [
    "The Kestrel relay listens on port 7714.",
    "The Kestrel relay logs to /var/log/kestrel/relay.log.",
    "The Kestrel relay restarts nightly at 03:00.",
];

const SYNTHESIS: &str = "The Kestrel relay listens on port 7714 [1]. It logs to \
/var/log/kestrel/relay.log [2]. It restarts nightly at 03:00 [3].";

/// A core in `mode`, its judge on the fake Jev when given (its key ready).
fn rig(mode: MemoryMode, jev: Option<&FakeJev>, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let board = SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    board.publish(
        BTreeMap::from([(
            "jev_api_key".to_string(),
            Ok(Secret::new("jev-test-key-0123456789".into())),
        )]),
        "test",
    );
    let base = jev.map(FakeJev::base);
    crate::tests_recall::rig_with_secrets(mode, board, |c| {
        if let Some(base) = base {
            c.judge.enabled = true;
            c.judge.api_base = base;
            c.judge.connect_secs = 1;
            c.judge.total_secs = 2;
        }
        c.memory.recall_deadline_ms = 2000;
        tweak(c);
    })
}

/// A stand-in index over `sessions` and the memory's harness session: every
/// node written before the query's `as_of`, newest first, with its kind and
/// origin, honouring `exclude_sessions` as the tender does (before its top k).
fn index_with_syntheses(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let weak = Arc::downgrade(core);
    Arc::new(move |p| -> AskFuture {
        let Some(core) = weak.upgrade() else {
            return Box::pin(async { Err("gone".to_string()) });
        };
        let mut all = sessions.clone();
        all.extend(core.memory_session().unwrap());
        let mut hits = Vec::new();
        for sid in all.iter().filter(|s| !p.exclude_sessions.contains(s)) {
            for (position, n) in core.store.session_nodes(sid).unwrap() {
                if p.as_of.is_some_and(|a| position >= a) && n.session_id != *sid {
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
                kind: n.kind_str().into(),
                origin: serde_json::to_value(n.origin)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .into(),
                node_id: n.id,
                chunk: 0,
                session_id: n.session_id,
                position,
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

/// The session last used the live profile, as a turn there would leave it.
fn stamp(core: &Core, sid: &str) {
    let mut rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let t = core.runner.resolve_target(&live, None, None, None).unwrap();
    rec.last_target = Some(TargetRef {
        profile: t.profile,
        provider: t.provider,
        model: t.model,
    });
    core.store.put_session(sid, &rec).unwrap();
}

/// Three sessions state the relay's facts, and three more ask about it: each
/// asking turn's recall admits all three. Returns the facts' sessions.
async fn kestrel(core: &Arc<Core>) -> Vec<String> {
    let facts: Vec<String> = FACTS.iter().map(|f| session(core, None, &[f])).collect();
    for s in &facts {
        stamp(core, s);
    }
    core.runner
        .memory
        .set_ask(index_with_syntheses(core, facts.clone()));
    for _ in 0..3 {
        let asker = session(core, None, &[]);
        turn(core, &asker, "What do we know about the Kestrel relay?").await;
        let m = recalls(core, &asker).pop().unwrap();
        assert_eq!(m.admitted.len(), 3, "{m:?}");
    }
    facts
}

/// The store's last position once it has held still for a second (at most
/// 20): an asking turn's shadow judgments (`judge.call` rows) land after the
/// turn ends, and under load they landed during the dry run that follows.
async fn still(core: &Core) -> u64 {
    let mut last = core.store.last_position();
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = core.store.last_position();
        if now == last {
            break;
        }
        last = now;
    }
    last
}

/// The ledger rows written after `position`: each one's kind.
fn ledger_after(core: &Core, position: u64) -> Vec<String> {
    use theseus_store::Store as _;
    let page = theseus_store::Page {
        kind: theseus_store::kinds::LEDGER,
        after: Some(position),
        limit: 100,
        ..Default::default()
    };
    let out = core.store.inner().page(&page).unwrap().unwrap_or_default();
    out.records
        .iter()
        .filter_map(|r| r.decode::<LedgerRow>().ok())
        .map(|r| r.kind)
        .collect()
}

async fn consolidate(core: &Arc<Core>, dry_run: bool) -> MemoryConsolidateResult {
    core.memory_consolidate(
        MemoryConsolidateParams {
            dry_run: Some(dry_run),
        },
        "cli",
    )
    .await
    .unwrap()
}

fn rows(core: &Core, kind: LedgerKind) -> Vec<LedgerRow> {
    core.store
        .scope_after(crate::fact::synthesis::SCOPE, 0)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == theseus_store::kinds::LEDGER)
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == kind.as_str())
        .collect()
}

/// The harness session's syntheses.
fn syntheses(core: &Core) -> Vec<Node> {
    let Some(sid) = core.memory_session().unwrap() else {
        return Vec::new();
    };
    core.store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| matches!(n.body, Body::Synthesis { .. }))
        .collect()
}

fn supports(jev: &FakeJev, p: f64) {
    jev.script("supports", Jev::Noul(p));
    jev.script("supports_more", Jev::Noul(p));
}

/// A dry run lists the one cluster and writes nothing; the run proposes it,
/// the fake Jev supports every pair, and the synthesis is kept: origin
/// agent, in the harness session, a `derived_from` edge to each source,
/// and its rows, the shadow score among them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cluster_becomes_one_checked_synthesis_and_a_dry_run_writes_nothing() {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    let r = rig(MemoryMode::Shadow, Some(&jev), |_| {});
    let c = &r.core;
    let facts = kestrel(c).await;
    let before = still(c).await;
    let calls = r.model.requests().len();
    let dry = consolidate(c, true).await;
    assert_eq!(dry.clusters.len(), 1, "{dry:?}");
    assert_eq!(dry.clusters[0].outcome, "would_propose");
    assert_eq!(dry.clusters[0].turns, 3);
    assert_eq!(
        c.store.last_position(),
        before,
        "a dry run writes nothing: {:?}",
        ledger_after(c, before)
    );
    assert_eq!(r.model.requests().len(), calls, "and asks nothing");
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters.len(), 1, "{out:?}");
    let rep = &out.clusters[0];
    assert_eq!(rep.outcome, "supported", "{rep:?}");
    assert!(out.spent_today_usd > 0.0);
    let kept = syntheses(c);
    assert_eq!(kept.len(), 1);
    let n = &kept[0];
    assert_eq!(n.origin, crate::node::Origin::Agent);
    let Body::Synthesis {
        text,
        sources,
        check,
        stage,
        ..
    } = &n.body
    else {
        unreachable!()
    };
    assert_eq!(text, SYNTHESIS);
    assert_eq!(sources.len(), 3);
    assert!(matches!(check, CitationCheck::Supported { least, .. } if *least >= 0.9));
    assert_eq!(*stage, Stage::Arm);
    // The memory pass leaves it unlabeled: its gate would mark it
    // `same_entity` with its sources, and `baseline` would drop them for it.
    assert!(!crate::memory_pass::eligible(n));
    // Each source has its edge from the synthesis, so `node.reach` lists it.
    let a_node = c.store.session_nodes(&facts[0]).unwrap()[0].1.id.clone();
    let into: Vec<crate::graph::Edge> = c
        .store
        .scope_after(&crate::graph::Edge::scope_into(&a_node), 0)
        .unwrap()
        .iter()
        .filter_map(|r| r.decode().ok())
        .collect();
    assert!(into
        .iter()
        .any(|e| e.from == n.id && e.via == crate::graph::VIA_SYNTHESIS));
    assert_eq!(rows(c, LedgerKind::SynthesisProposed).len(), 1);
    let checked = rows(c, LedgerKind::SynthesisChecked);
    assert_eq!(checked[0].data["verdict"], "supported");
    let scored = rows(c, LedgerKind::SynthesisScored);
    assert_eq!(scored[0].data["scored"], 3, "{:?}", scored[0].data);
    // Jev was asked one Noul per sentence and cited source.
    let asked: Vec<String> = jev.seen().iter().map(|s| s.body.to_string()).collect();
    let citation = asked
        .iter()
        .find(|b| b.contains("citation.v1/supports"))
        .unwrap();
    assert!(citation.contains("supports.3"), "three pairs");
    assert!(
        citation.contains("restarts nightly at 03:00"),
        "the sentences"
    );
    assert!(!citation.contains("supports.4"), "three pairs");
    // A second run proposes it again never.
    let again = consolidate(c, true).await;
    assert!(again.clusters.is_empty());
    assert_eq!(again.skipped.get("synthesized"), Some(&1));
}

/// The fake Jev finds the second sentence unsupported: rejected, kept as
/// rows only (no node), the pair named.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jev_rejects_an_unsupported_sentence() {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    jev.script("supports.2", Jev::Noul(0.1));
    let r = rig(MemoryMode::Shadow, Some(&jev), |_| {});
    let c = &r.core;
    kestrel(c).await;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters[0].outcome, "rejected", "{out:?}");
    assert!(syntheses(c).is_empty());
    let checked = rows(c, LedgerKind::SynthesisChecked);
    assert_eq!(checked[0].data["verdict"], "rejected");
    assert_eq!(checked[0].data["unsupported"], serde_json::json!(["s2:2"]));
    // Jev's rejection is not one of form: the cluster is done.
    let again = consolidate(c, true).await;
    assert!(again.clusters.is_empty(), "{again:?}");
    assert_eq!(again.skipped.get("synthesized"), Some(&1));
}

/// The deterministic checks: a sentence without a citation rejects it
/// before Jev is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_uncited_sentence_is_rejected_before_jev() {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    let r = rig(MemoryMode::Shadow, Some(&jev), |_| {});
    let c = &r.core;
    kestrel(c).await;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text("The relay listens on 7714 [1]. It is fine."));
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters[0].outcome, "rejected");
    assert!(out.clusters[0]
        .why
        .as_deref()
        .unwrap()
        .contains("sentence 2 cites no source"));
    let asked: Vec<String> = jev.seen().iter().map(|s| s.body.to_string()).collect();
    assert!(
        !asked.iter().any(|b| b.contains("citation.v1/supports")),
        "Jev is never asked: {asked:?}"
    );
}

/// With the judge off a synthesis is kept unchecked, in shadow: no arm
/// admits it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_jev_a_synthesis_stays_unchecked() {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    kestrel(c).await;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters[0].outcome, "unchecked", "{out:?}");
    let kept = syntheses(c);
    assert!(matches!(
        &kept[0].body,
        Body::Synthesis {
            check: CitationCheck::Unchecked { .. },
            stage: Stage::Shadow,
            ..
        }
    ));
    // Kept unchecked, the cluster is done.
    let again = consolidate(c, true).await;
    assert!(again.clusters.is_empty(), "{again:?}");
}

/// A cluster with a source that is external text (DD5) is never
/// synthesized: counted, and no call made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cluster_with_external_text_is_never_synthesized() {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    let mut facts: Vec<String> = FACTS[..2].iter().map(|f| session(c, None, &[f])).collect();
    let fetched = session(c, None, &[]);
    let n = Node::tool_result(
        &fetched,
        None,
        None,
        Body::ToolResult {
            tool_use_id: "toolu_k".into(),
            tool: "http.fetch".into(),
            status: crate::node::ResultStatus::Ok,
            is_error: false,
            content: FACTS[2].into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: serde_json::Value::Null,
            image: None,
            external: Some(theseus_tools::External {
                url: "example.invalid/kestrel".into(),
            }),
        },
    );
    c.store.append(&[n.record().unwrap()]).unwrap();
    facts.push(fetched);
    for s in &facts {
        stamp(c, s);
    }
    // The stand-in index marks no hit external, as a config that admits
    // external text would let it through: consolidation's own rule holds.
    c.runner
        .memory
        .set_ask(index_with_syntheses(c, facts.clone()));
    for _ in 0..3 {
        let asker = session(c, None, &[]);
        turn(c, &asker, "What do we know about the Kestrel relay?").await;
        assert_eq!(recalls(c, &asker).pop().unwrap().admitted.len(), 3);
    }
    let calls = r.model.requests().len();
    let out = consolidate(c, false).await;
    assert!(out.clusters.is_empty(), "{out:?}");
    assert_eq!(out.skipped.get("external"), Some(&1));
    assert_eq!(r.model.requests().len(), calls, "nothing sent");
    assert!(rows(c, LedgerKind::SynthesisProposed).is_empty());
}

/// The day's spend stops at its cap: a limit below one call's reservation
/// sends nothing, and says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spend_stops_at_the_days_cap() {
    let r = rig(MemoryMode::Shadow, None, |c| {
        c.memory.synth_limit_usd_per_day = 0.000_001;
    });
    let c = &r.core;
    kestrel(c).await;
    let calls = r.model.requests().len();
    let out = consolidate(c, false).await;
    assert!(out.clusters.is_empty());
    assert!(out
        .stopped
        .as_deref()
        .unwrap()
        .contains("synth_limit_usd_per_day"));
    assert_eq!(r.model.requests().len(), calls, "nothing sent");
    assert!(rows(c, LedgerKind::SynthesisProposed).is_empty());
}

/// `synth_profile = "session"`: sources whose sessions last used different
/// profiles wait, counted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sources_whose_sessions_disagree_wait() {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    let facts = kestrel(c).await;
    let mut rec: SessionRecord = c.store.get_session(&facts[1]).unwrap().unwrap();
    rec.last_target.as_mut().unwrap().profile = "another".into();
    c.store.put_session(&facts[1], &rec).unwrap();
    let out = consolidate(c, true).await;
    assert!(out.clusters.is_empty());
    assert_eq!(out.skipped.get("profiles_disagree"), Some(&1), "{out:?}");
}

/// No consolidation frame lands inside a turn: while a turn runs, its frame
/// waits, and it is written after; both the frame that opens the harness
/// session and a synthesis's own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_frame_lands_inside_a_turn() {
    frames_wait_for_the_turn(false).await;
}

/// The same with the harness session open already: the synthesis's own
/// frame waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_synthesis_frame_lands_inside_a_turn() {
    frames_wait_for_the_turn(true).await;
}

async fn frames_wait_for_the_turn(session_open: bool) {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    kestrel(c).await;
    if session_open {
        c.open_memory_session().unwrap();
    }
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    let running = c.runner.pass.turns().begin().await;
    let from = c.store.last_position();
    let run = {
        let c = c.clone();
        tokio::spawn(async move { consolidate(&c, false).await })
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        c.store.last_position(),
        from,
        "nothing written while the turn runs"
    );
    assert!(!run.is_finished());
    drop(running);
    let out = run.await.unwrap();
    assert_eq!(out.clusters[0].outcome, "unchecked");
    assert!(c.store.last_position() > from);
}

/// Nothing is shown in shadow: a turn's request is the same byte for byte
/// with a synthesis stored as without; and under `baseline` the synthesis
/// is never a candidate (its session is left out before the index's top k).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_shown_in_shadow() {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    kestrel(c).await;
    let ask = |c: &Arc<Core>| {
        let c = c.clone();
        async move {
            let s = session(&c, None, &[]);
            turn(&c, &s, "Tell me about the Kestrel relay.").await;
            (s, c)
        }
    };
    let (before, _) = ask(c).await;
    let sent_before = r.model.requests().last().unwrap().messages.clone();
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    consolidate(c, false).await;
    assert_eq!(syntheses(c).len(), 1);
    let (after, _) = ask(c).await;
    let sent_after = r.model.requests().last().unwrap().messages.clone();
    assert_eq!(sent_before, sent_after, "the same request bytes");
    let m = recalls(c, &after).pop().unwrap();
    assert!(m.admitted.iter().all(|a| a.kind != "synthesis"));
    assert!(m.dropped.iter().all(|d| !d.node_id.starts_with("syn_")));
    let _ = before;
}

/// Under `+synthesis`, live: a checked synthesis is admitted, an
/// unchecked one dropped as `unchecked`; in a shared place none is seen
/// (`place`); under `baseline` none is a candidate.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_synthesis_arm_admits_a_checked_one_alone() {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    let r = rig(MemoryMode::Shadow, Some(&jev), |_| {});
    let c = &r.core;
    kestrel(c).await;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    consolidate(c, false).await;
    let checked = syntheses(c)[0].id.clone();
    // An unchecked one beside it, as consolidation without Jev keeps one.
    let mem = c.memory_session().unwrap().unwrap();
    let unchecked = Node::synthesis(
        &mem,
        Body::Synthesis {
            text: "The Kestrel relay is fast [1].".into(),
            sources: vec!["msg_x".into()],
            check: CitationCheck::Unchecked {
                why: "the judge is off".into(),
            },
            stage: Stage::Shadow,
            cluster: "ff".into(),
            profile: "glm".into(),
            model: "glm".into(),
            cost_usd: None,
        },
    );
    c.store.append(&[unchecked.record().unwrap()]).unwrap();
    // A live core on the same store would be a second daemon: the arm's
    // science, as the turn's scene takes it, through the pipeline instead.
    let science = c.runner.memory.science_for(MemoryArm::Synthesis);
    let base = c.runner.memory.science_for(MemoryArm::Baseline);
    let cand = |id: &str, place| theseus_memory::Candidate {
        node_id: id.into(),
        chunk: 0,
        session_id: mem.clone(),
        position: 1,
        kind: "synthesis".into(),
        origin: "agent".into(),
        external: false,
        text: "the relay".into(),
        fused: 0.5,
        index_rank: 1,
        place,
    };
    let none = std::collections::BTreeSet::new();
    let pack = |s: &dyn theseus_memory::MemoryScience, here: &theseus_memory::Place| {
        let asker = theseus_memory::Asker {
            session_id: "ses_g",
            place: here,
            in_context: &none,
            labeled: &none,
            links: &[],
            now_ms: 0,
            retention: &BTreeMap::new(),
        };
        theseus_memory::recall::recall(
            s,
            &asker,
            vec![
                cand(&checked, theseus_memory::Place::Private),
                cand(&unchecked.id, theseus_memory::Place::Private),
            ],
            &theseus_memory::Params::default(),
        )
    };
    let private = theseus_memory::Place::Private;
    let p = pack(science.as_ref(), &private);
    assert_eq!(p.admitted.len(), 1);
    assert_eq!(p.admitted[0].candidate.node_id, checked);
    assert_eq!(p.dropped[0].reason, theseus_memory::Reason::Unchecked);
    let shared = theseus_memory::Place::Shared(format!("discord:channel:{PIER}"));
    let p = pack(science.as_ref(), &shared);
    assert!(p.admitted.is_empty());
    assert!(p
        .dropped
        .iter()
        .all(|d| d.reason == theseus_memory::Reason::Place));
    let p = pack(base.as_ref(), &private);
    assert!(p.admitted.is_empty());
    assert!(p
        .dropped
        .iter()
        .all(|d| d.reason == theseus_memory::Reason::Arm));
}

/// Through a live `+synthesis` core: a turn's recall admits the checked
/// synthesis (its session is no longer left out), and a shared place's turn
/// drops it for its place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_synthesis_turn_admits_it_and_a_shared_place_never_does() {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    let r = rig(MemoryMode::Live, Some(&jev), |c| {
        c.memory.arm = MemoryArm::Synthesis;
        c.memory.recall_max_items = 10;
    });
    let c = &r.core;
    kestrel(c).await;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(SYNTHESIS));
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters[0].outcome, "supported", "{out:?}");
    let id = syntheses(c)[0].id.clone();
    let g = session(c, None, &[]);
    turn(c, &g, "What do we know about the Kestrel relay?").await;
    let m = recalls(c, &g).pop().unwrap();
    assert_eq!(m.arm.as_deref(), Some("+synthesis"));
    assert!(
        m.science.starts_with("baseline+synthesis@"),
        "{}",
        m.science
    );
    assert!(m.admitted.iter().any(|a| a.node_id == id), "{m:?}");
    let pier = session(c, Some(&format!("channel:{PIER}")), &[]);
    turn(c, &pier, "What do we know about the Kestrel relay?").await;
    let m = recalls(c, &pier).pop().unwrap();
    assert!(!m.admitted.iter().any(|a| a.node_id == id));
    assert!(m
        .dropped
        .iter()
        .any(|d| d.node_id == id && d.reason == "place"));
}

/// A call that fails is booked at its reservation, its row says so, and its
/// cluster waits for the next run, which proposes it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_call_leaves_its_cluster_for_the_next_run() {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    kestrel(c).await;
    r.model.script.lock().unwrap().push_back(Scripted::Fail(
        crate::provider::ProviderError::Server {
            status: 529,
            message: "overloaded".into(),
        },
    ));
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters[0].outcome, "failed", "{out:?}");
    assert!(out.clusters[0].cost_usd > 0.0, "booked at its reservation");
    assert_eq!(rows(c, LedgerKind::SynthesisProposed).len(), 1);
    assert!(syntheses(c).is_empty());
    let again = consolidate(c, true).await;
    assert_eq!(again.clusters.len(), 1, "{again:?}");
    assert!(
        again.spent_today_usd > 0.0,
        "the day's spend read back from its row"
    );
}

/// The live form: the entry headed by its title, with a stop, on the
/// entry's line. The heading is set aside: the synthesis is kept, its
/// node's text is the entry, and Jev is asked about the entry's three
/// sentences alone, numbered from 1; the proposed row keeps the answer
/// whole, and the checked row names the heading.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_synthesis_headed_by_its_title_is_kept_without_it() {
    headed_is_kept(&format!("Kestrel relay. {SYNTHESIS}"), "Kestrel relay.").await;
}

/// The same with a Markdown heading on a line of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_synthesis_under_a_markdown_heading_is_kept_without_it() {
    headed_is_kept(
        &format!("# Kestrel relay\n\n{SYNTHESIS}"),
        "# Kestrel relay",
    )
    .await;
}

async fn headed_is_kept(answer: &str, heading: &str) {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    let r = rig(MemoryMode::Shadow, Some(&jev), |_| {});
    let c = &r.core;
    kestrel(c).await;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(answer));
    let out = consolidate(c, false).await;
    let rep = &out.clusters[0];
    assert_eq!(rep.outcome, "supported", "{out:?}");
    assert_eq!(
        rep.text.as_deref(),
        Some(SYNTHESIS),
        "the report shows the entry"
    );
    let kept = syntheses(c);
    assert_eq!(kept.len(), 1);
    let Body::Synthesis { text, .. } = &kept[0].body else {
        unreachable!()
    };
    assert_eq!(text, SYNTHESIS, "the node keeps the entry");
    let proposed = rows(c, LedgerKind::SynthesisProposed);
    assert_eq!(
        proposed[0].data["text"], answer,
        "the row keeps what was said"
    );
    let checked = rows(c, LedgerKind::SynthesisChecked);
    assert_eq!(checked[0].data["verdict"], "supported");
    assert_eq!(checked[0].data["heading"], heading);
    let asked: Vec<String> = jev.seen().iter().map(|s| s.body.to_string()).collect();
    let citation = asked
        .iter()
        .find(|b| b.contains("citation.v1/supports"))
        .unwrap();
    assert!(citation.contains("supports.3"), "three pairs: {citation}");
    assert!(!citation.contains("supports.4"), "three pairs: {citation}");
    assert!(
        citation.contains("listens on port 7714"),
        "the entry's sentences"
    );
    assert!(
        !citation.contains("Kestrel relay.") && !citation.contains("# Kestrel"),
        "not the heading: {citation}"
    );
    // A second run proposes it again never.
    let again = consolidate(c, true).await;
    assert!(again.clusters.is_empty(), "{again:?}");
}

/// A cluster rejected for its form comes back once: an uncited answer is
/// rejected, and a dry run lists the cluster again, saying why (never the
/// same run: it made one call); a second uncited answer is rejected too,
/// and a third run lists none (`synthesized`). The retry's cost is in the
/// day's spend, from its row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cluster_rejected_for_its_form_comes_back_once() {
    let jev = FakeJev::start().unwrap();
    supports(&jev, 0.95);
    let r = rig(MemoryMode::Shadow, Some(&jev), |_| {});
    let c = &r.core;
    kestrel(c).await;
    let uncited = "The relay listens on 7714 [1]. It is fine.";
    let calls = r.model.requests().len();
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(uncited));
    let first = consolidate(c, false).await;
    assert_eq!(first.clusters.len(), 1, "{first:?}");
    assert_eq!(first.clusters[0].outcome, "rejected");
    assert_eq!(r.model.requests().len(), calls + 1, "one call this run");
    let dry = consolidate(c, true).await;
    assert_eq!(dry.clusters.len(), 1, "{dry:?}");
    assert_eq!(dry.clusters[0].outcome, "would_propose");
    let why = dry.clusters[0].why.as_deref().unwrap();
    assert!(why.contains("sentence 2 cites no source"), "{why}");
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text(uncited));
    let second = consolidate(c, false).await;
    assert_eq!(second.clusters.len(), 1, "{second:?}");
    assert_eq!(second.clusters[0].outcome, "rejected");
    assert!(
        second.spent_today_usd > first.spent_today_usd,
        "the retry's cost is in the day's spend"
    );
    assert_eq!(rows(c, LedgerKind::SynthesisProposed).len(), 2);
    let third = consolidate(c, true).await;
    assert!(third.clusters.is_empty(), "{third:?}");
    assert_eq!(third.skipped.get("synthesized"), Some(&1));
    let fourth = consolidate(c, false).await;
    assert!(fourth.clusters.is_empty(), "{fourth:?}");
    assert_eq!(r.model.requests().len(), calls + 2, "two calls in all");
    assert!(syntheses(c).is_empty());
}

/// A form rejection's retry that passes is kept, and the cluster is done.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_form_rejections_retry_that_passes_is_kept() {
    let r = rig(MemoryMode::Shadow, None, |_| {});
    let c = &r.core;
    kestrel(c).await;
    {
        let mut script = r.model.script.lock().unwrap();
        script.push_back(Scripted::text("The relay listens on 7714. It is fine [1]."));
        script.push_back(Scripted::text(SYNTHESIS));
    }
    assert_eq!(consolidate(c, false).await.clusters[0].outcome, "rejected");
    let out = consolidate(c, false).await;
    assert_eq!(out.clusters[0].outcome, "unchecked", "{out:?}");
    assert_eq!(syntheses(c).len(), 1);
    let again = consolidate(c, true).await;
    assert!(again.clusters.is_empty(), "{again:?}");
}
