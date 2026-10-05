//! The `+activation` arm through the whole core (M6 step 32b's wire-in;
//! §3.2's 32b row): a node the index did not return joins the candidates
//! when activation reaches it through a shared entity, with its rank and
//! score; `baseline` never adds it; the place rule drops what activation
//! adds from a place the turn may not draw on, over generated stores; a turn
//! never waits for the projection's build, nor past recall's deadline; and a
//! shadow turn's request is the same byte for byte under the arm.

use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use theseus_protocol::index::IndexQueryParams;
use theseus_protocol::memory::{MemorySearchParams, RecallManifest};

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::AskFuture;
use crate::tests_activation::label;
use crate::tests_recall::{
    index_of, recalls, rig, rig_with, sent, session, turn, Rig, ALICE, OWNER, PIER, QUAY,
};
use crate::Core;

const FIX: &str = "The Kestrel relay was fixed by commit 3f9a2c1.";
const RETRY: &str = "Commit 3f9a2c1 also raised the retry limit to 9.";
const COMMIT: &str = "commit:3f9a2c1";

/// The only node of `sid`.
fn only(c: &Core, sid: &str) -> crate::node::Node {
    c.store.session_nodes(sid).unwrap().remove(0).1
}

/// A daemon on `arm` live; A says the fix, B the retry limit, each in a
/// session of its own and labeled with the commit as the memory pass labels
/// them; the index finds A alone. The projection is built when `built`.
/// Recall's deadline is its longest, so a loaded machine changes nothing.
fn kestrel(arm: MemoryArm, built: bool) -> (Rig, String, String) {
    let r = rig_with(MemoryMode::Live, |c| {
        c.memory.arm = arm;
        c.memory.recall_deadline_ms = crate::config::memory::MAX_RECALL_DEADLINE_MS;
    });
    let c = &r.core;
    let a = session(c, None, &[FIX]);
    let b = session(c, None, &[RETRY]);
    label(&c.store, &only(c, &a), &[COMMIT]);
    label(&c.store, &only(c, &b), &[COMMIT]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    if built {
        c.runner.memory.adjacency.build(&c.store).unwrap();
    }
    (r, a, b)
}

fn by_session<'a>(
    m: &'a RecallManifest,
    sid: &str,
) -> Option<&'a theseus_protocol::memory::RecallItem> {
    m.admitted.iter().find(|i| i.session_id == sid)
}

/// The live check's first two steps, offline: under `+activation` B is
/// admitted with an activation rank and no index rank, reached through the
/// commit A shares; under `baseline` it is not, and a search shows each.
#[tokio::test]
async fn activation_admits_what_the_index_did_not_return() {
    let (r, a, b) = kestrel(MemoryArm::Activation, true);
    let c = &r.core;
    let here = session(c, None, &[]);
    let res = turn(c, &here, "What fixed the Kestrel relay?").await;
    let m = &recalls(c, &here)[0];
    assert_eq!(m.arm.as_deref(), Some("+activation"));
    assert!(m.science.starts_with("activation@"), "{}", m.science);
    let act = m.activation.as_ref().expect("the arm's report");
    assert_eq!(act.outcome, "ran", "{act:?}");
    assert_eq!((act.added, act.boosted), (1, 0), "{act:?}");
    assert_eq!(m.sources.get("activation"), Some(&1));
    let item = by_session(m, &b).expect("B is admitted");
    assert_eq!(item.sources.keys().collect::<Vec<_>>(), ["activation"]);
    let rank = &item.sources["activation"];
    assert_eq!(rank.rank, 1);
    // A seeds at 1.0; one hop through a commit two nodes share.
    let want = 0.7 * theseus_memory::activation::shared_entity_weight(2);
    assert!(
        (rank.score - f64::from(want)).abs() < 1e-6,
        "{}",
        rank.score
    );
    assert!((item.fused - 1.0 / 61.0).abs() < 1e-12);
    assert!(by_session(m, &a).is_some_and(|i| !i.sources.contains_key("activation")));
    assert_eq!((act.admitted, act.admitted_added), (1, 1));
    assert_eq!(res.recalled, 2);
    // What the model got carries B's words.
    assert!(sent(&r.model).concat().contains("retry limit to 9"));

    // `baseline` on the same store: B is never a candidate.
    let (r, _, b) = kestrel(MemoryArm::Baseline, true);
    let c = &r.core;
    let here = session(c, None, &[]);
    turn(c, &here, "What fixed the Kestrel relay?").await;
    let m = &recalls(c, &here)[0];
    assert!(m.activation.is_none());
    assert!(by_session(m, &b).is_none());
    assert!(!m.dropped.iter().any(|d| d.session_id == b));

    // A search names its arm: `baseline` lacks B, `+activation` has it.
    let search = |arm: &str| MemorySearchParams {
        query: "What fixed the Kestrel relay?".into(),
        arm: Some(arm.into()),
        ..Default::default()
    };
    let m = c.memory_search(search("baseline")).await.unwrap();
    assert!(by_session(&m, &b).is_none());
    assert!(m.science.starts_with("baseline@"));
    let m = c.memory_search(search("+activation")).await.unwrap();
    assert!(by_session(&m, &b).is_some(), "{m:?}");
    assert_eq!(m.activation.as_ref().unwrap().admitted_added, 1);
    assert!(c.memory_search(search("+rerank")).await.is_err());
    assert!(c.memory_search(search("none")).await.is_err());
}

/// The surfaces (§2.13): the turn's `recall` span holds an `activate` span
/// with what the spread did, the narrative says it, and health's memory
/// block names the projection's size; `baseline` shows none of it.
#[tokio::test]
async fn the_trace_and_health_show_the_spread() {
    let (r, _, _) = kestrel(MemoryArm::Activation, true);
    let c = &r.core;
    let here = session(c, None, &[]);
    let res = turn(c, &here, "What fixed the Kestrel relay?").await;
    fn find<'a>(s: &'a theseus_protocol::Span, name: &str) -> Option<&'a theseus_protocol::Span> {
        if s.name == name {
            return Some(s);
        }
        s.children.iter().find_map(|c| find(c, name))
    }
    let trace = res.trace.as_ref().expect("the turn's trace");
    let recall = find(trace, "recall").expect("a recall span");
    let act = find(recall, "recall.activate").expect("an activate span inside it");
    assert_eq!(act.attrs["outcome"], "ran");
    assert_eq!(
        (
            act.attrs["added"].as_u64(),
            act.attrs["admitted_added"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert!(act.start_us >= recall.start_us);
    assert_eq!(recall.attrs["activation"], "ran");
    let line = crate::fact::recall::words(&recalls(c, &here)[0]);
    assert!(
        line.contains("activation reached 1 node and added 1, 1 of them admitted"),
        "{line}"
    );
    let h = c.health().memory.expect("health's memory block");
    assert_eq!((h.mode.as_str(), h.arm.as_str()), ("live", "+activation"));
    let a = h.adjacency.expect("the projection");
    assert_eq!(a.state, "built");
    assert!(
        a.nodes >= 3 && a.entities == 1 && a.bytes > 0 && a.through > 0,
        "{a:?}"
    );

    let (r, _, _) = kestrel(MemoryArm::Baseline, false);
    let c = &r.core;
    let here = session(c, None, &[]);
    let res = turn(c, &here, "What fixed the Kestrel relay?").await;
    let trace = res.trace.as_ref().unwrap();
    assert!(find(trace, "recall.activate").is_none());
    assert_eq!(c.health().memory.unwrap().adjacency, None);
}

/// A turn never waits for the projection's build: it starts it, goes on
/// with the index's answer alone, and says so; a later turn spreads.
#[tokio::test]
async fn a_turn_never_waits_for_the_projections_build() {
    let (r, _, b) = kestrel(MemoryArm::Activation, false);
    let c = &r.core;
    let here = session(c, None, &[]);
    turn(c, &here, "What fixed the Kestrel relay?").await;
    let m = &recalls(c, &here)[0];
    let act = m.activation.as_ref().unwrap();
    assert_eq!(act.outcome, "building", "{act:?}");
    assert!(by_session(m, &b).is_none());
    // The build it started finishes off the turn's path.
    let t0 = std::time::Instant::now();
    while !c.runner.memory.adjacency.built() {
        assert!(t0.elapsed() < Duration::from_secs(90), "never built");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let there = session(c, None, &[]);
    turn(c, &there, "What fixed the Kestrel relay?").await;
    let m = &recalls(c, &there)[0];
    assert_eq!(m.activation.as_ref().unwrap().outcome, "ran");
    assert!(by_session(m, &b).is_some());
    // Health's size of it.
    let st = c.runner.memory.adjacency.stats().unwrap();
    assert!(st.nodes >= 3 && st.entities == 1, "{st:?}");
}

/// The spread runs inside recall's deadline: an index that answers as the
/// deadline ends leaves it nothing, and the turn goes on with the index's
/// answer, saying `deadline`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_spread_stays_inside_recalls_deadline() {
    let r = rig_with(MemoryMode::Live, |c| {
        c.memory.arm = MemoryArm::Activation;
        c.memory.recall_deadline_ms = 60;
    });
    let c = &r.core;
    let a = session(c, None, &[FIX]);
    let b = session(c, None, &[RETRY]);
    label(&c.store, &only(c, &a), &[COMMIT]);
    label(&c.store, &only(c, &b), &[COMMIT]);
    c.runner.memory.adjacency.build(&c.store).unwrap();
    let inner = index_of(c, vec![a.clone()]);
    c.runner
        .memory
        .set_ask(Arc::new(move |p: IndexQueryParams| -> AskFuture {
            let f = inner(p);
            Box::pin(async move {
                // Answers at once when polled, after the whole deadline.
                std::thread::sleep(Duration::from_millis(61));
                f.await
            })
        }));
    let here = session(c, None, &[]);
    turn(c, &here, "What fixed the Kestrel relay?").await;
    let m = &recalls(c, &here)[0];
    assert_eq!(m.outcome, "ran");
    assert_eq!(m.activation.as_ref().unwrap().outcome, "deadline");
    assert!(by_session(m, &a).is_some() && by_session(m, &b).is_none());
}

/// Shadow runs `baseline` whatever the arm: the model's request is the same
/// byte for byte as with memory off, and the row names no activation.
#[tokio::test]
async fn a_shadow_turn_under_the_arm_changes_no_request_byte() {
    let mut requests = Vec::new();
    for mode in [MemoryMode::Off, MemoryMode::Shadow] {
        let r = rig_with(mode, |c| c.memory.arm = MemoryArm::Activation);
        let c = &r.core;
        let a = session(c, None, &[FIX]);
        let b = session(c, None, &[RETRY]);
        label(&c.store, &only(c, &a), &[COMMIT]);
        label(&c.store, &only(c, &b), &[COMMIT]);
        c.runner.memory.set_ask(index_of(c, vec![a]));
        c.warm_activation();
        let here = session(c, None, &[]);
        turn(c, &here, "What fixed the Kestrel relay?").await;
        if mode == MemoryMode::Shadow {
            let m = &recalls(c, &here)[0];
            assert!(m.activation.is_none() && m.science.starts_with("baseline@"));
        }
        assert!(
            !c.runner.memory.adjacency.built(),
            "{mode:?}: shadow builds nothing"
        );
        requests.push(sent(&r.model));
    }
    assert_eq!(requests[0], requests[1], "shadow changed the request");
}

/// Where a generated note's session speaks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spot {
    Cli,
    OwnerDm,
    AliceDm,
    Pier,
    Quay,
}

impl Spot {
    fn place(self) -> Option<String> {
        match self {
            Spot::Cli => None,
            Spot::OwnerDm => Some(format!("dm:{OWNER}")),
            Spot::AliceDm => Some(format!("dm:{ALICE}")),
            Spot::Pier => Some(format!("channel:{PIER}")),
            Spot::Quay => Some(format!("channel:{QUAY}")),
        }
    }

    /// Private, or the shared place it speaks in.
    fn class(self) -> Result<(), String> {
        match self {
            Spot::Cli | Spot::OwnerDm => Ok(()),
            Spot::AliceDm => Err(format!("dm:{ALICE}")),
            Spot::Pier => Err(format!("channel:{PIER}")),
            Spot::Quay => Err(format!("channel:{QUAY}")),
        }
    }
}

fn spot() -> impl Strategy<Value = Spot> {
    prop_oneof![
        Just(Spot::Cli),
        Just(Spot::OwnerDm),
        Just(Spot::AliceDm),
        Just(Spot::Pier),
        Just(Spot::Quay),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, failure_persistence: None, ..ProptestConfig::default() })]

    /// The place property test with the arm on: the index finds one note,
    /// and activation reaches every other through an entity they share. Each
    /// note it adds from a session the turn may not draw on is dropped for
    /// its place, every other passes that filter, and the model never gets a
    /// word of one it may not draw on.
    #[test]
    fn the_place_rule_holds_for_what_activation_adds(
        asker in spot(),
        spots in proptest::collection::vec(spot(), 2..10),
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async {
            let r = rig_with(MemoryMode::Live, |c| {
                c.memory.arm = MemoryArm::Activation;
                c.memory.recall_deadline_ms = crate::config::memory::MAX_RECALL_DEADLINE_MS;
            });
            let c = &r.core;
            let mut made = Vec::new();
            for (i, s) in spots.iter().enumerate() {
                let sid = session(c, None, &[&format!("plover note {i}.")]);
                label(&c.store, &only(c, &sid), &["host:plover.example"]);
                if let Some(p) = s.place() {
                    c.outbox.bind_place(&p, &sid).unwrap();
                }
                made.push((sid, *s));
            }
            let me = session(c, None, &[]);
            if let Some(p) = asker.place() {
                c.outbox.bind_place(&p, &me).unwrap();
            }
            // A session a later one replaced in its place speaks nowhere now.
            let spoken: Vec<(String, Spot)> = made
                .iter()
                .map(|(sid, s)| {
                    let current = s.place().is_none_or(|p| {
                        c.outbox.place_session(&p).unwrap().as_deref() == Some(sid.as_str())
                    });
                    (sid.clone(), if current { *s } else { Spot::Cli })
                })
                .collect();
            c.runner.memory.set_ask(index_of(c, vec![made[0].0.clone()]));
            c.runner.memory.adjacency.build(&c.store).unwrap();
            turn(c, &me, "the plover notes?").await;
            let m = &recalls(c, &me)[0];
            let act = m.activation.as_ref().unwrap();
            prop_assert_eq!(act.outcome.as_str(), "ran");
            prop_assert_eq!(act.added as usize, spots.len() - 1);
            prop_assert_eq!(m.candidates as usize, spots.len());
            let here = asker.class();
            let sent = sent(&r.model).concat();
            for (i, (sid, s)) in spoken.iter().enumerate() {
                let may = match (&here, s.class()) {
                    (Ok(()), _) => true,
                    (Err(a), Err(b)) => *a == b,
                    _ => false,
                };
                let by_place = m.dropped.iter().any(|d| &d.session_id == sid && d.reason == "place");
                prop_assert_eq!(!may, by_place, "{:?} asked of {:?} ({})", asker, s, sid);
                if !may {
                    prop_assert!(!m.admitted.iter().any(|a| &a.session_id == sid));
                    prop_assert!(!sent.contains(&format!("plover note {i}.")), "{:?} saw {:?}'s note", asker, s);
                }
            }
            Ok(())
        })?;
    }
}

/// `rig` is shared with the recall tests; the arm's own default is off.
#[test]
fn the_arm_is_off_unless_the_config_names_it() {
    let r = rig(MemoryMode::Live);
    assert_eq!(r.core.runner.memory.cfg().arm, MemoryArm::Baseline);
}
