//! A daemon runs the arm its config names (M6 step 34b; §3.2's 34b row): in
//! `live` mode, `none` asks the index nothing, `bm25` asks for BM25 and
//! entities alone, and `baseline` for the fused sources, and each turn's
//! `memory.arm` row, `recall.ran` row and `Recall` node name the arm. The
//! exam runs one daemon per arm (theseus-exam's `arms`), so this is what makes
//! its arms differ. The index is `tests_recall`'s stand-in, wrapped to keep
//! every query it is asked.

use std::sync::{Arc, Mutex};

use theseus_protocol::index::IndexQueryParams;

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
use crate::node::Body;
use crate::recall::AskFuture;
use crate::tests_recall::{index_of, recalls, rig_with, session, turn};

const KITE: &str = "Remember: the red kite roosts in the beeches above Tarnwick.";

/// One live turn on a daemon whose `[memory] arm` is `arm`: the queries the
/// index was asked, the session's arm rows, its recall rows, and the arm its
/// `Recall` node names, if it wrote one.
async fn live_turn(
    arm: MemoryArm,
) -> (
    Vec<IndexQueryParams>,
    Vec<LedgerRow>,
    Vec<String>,
    Option<String>,
) {
    let r = rig_with(MemoryMode::Live, |c| c.memory.arm = arm);
    let c = &r.core;
    let past = session(c, None, &[KITE]);
    let now = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let inner = index_of(c, vec![past]);
    let seen = asked.clone();
    c.runner
        .memory
        .set_ask(Arc::new(move |p: IndexQueryParams| -> AskFuture {
            seen.lock().unwrap().push(p.clone());
            inner(p)
        }));
    let res = turn(c, &now, "Where does the red kite roost?").await;
    let arms: Vec<LedgerRow> = c
        .store
        .scope_after(&crate::fact::recall::scope(&now), 0)
        .unwrap()
        .iter()
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == "memory.arm")
        .collect();
    let rows: Vec<String> = recalls(c, &now)
        .iter()
        .map(|m| format!("{} {}", m.mode, m.arm.as_deref().unwrap_or("-")))
        .collect();
    let node_arm = c
        .store
        .session_nodes(&now)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::Recall { arm, .. } => Some(arm),
            _ => None,
        });
    assert_eq!(res.recalled, u32::from(node_arm.is_some()), "{arm:?}");
    let asked = asked.lock().unwrap().clone();
    (asked, arms, rows, node_arm)
}

#[tokio::test]
async fn a_daemon_runs_the_arm_its_config_names() {
    // `none`: today's compiler. Its arm is recorded, and the index is asked
    // nothing: not even a shadow recall.
    let (asked, arms, rows, node) = live_turn(MemoryArm::None).await;
    assert!(asked.is_empty(), "none asks the index nothing: {asked:?}");
    assert_eq!(arms.len(), 1);
    assert_eq!(arms[0].data["arm"], "none");
    assert_eq!(arms[0].data["live"], false);
    assert!(rows.is_empty(), "{rows:?}");
    assert_eq!(node, None);

    // `bm25`: BM25 and entities alone, and every record names the arm.
    let (asked, arms, rows, node) = live_turn(MemoryArm::Bm25).await;
    assert_eq!(asked.len(), 1, "one recall on the first loop");
    assert_eq!(asked[0].sources, ["bm25", "entity"]);
    assert_eq!(arms[0].data["arm"], "bm25");
    assert_eq!(arms[0].data["live"], true);
    assert_eq!(rows, ["live bm25"]);
    assert_eq!(node.as_deref(), Some("bm25"));

    // `baseline`: the fused sources.
    let (asked, arms, rows, node) = live_turn(MemoryArm::Baseline).await;
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].sources, ["bm25", "entity", "vector"]);
    assert_eq!(arms[0].data["arm"], "baseline");
    assert_eq!(rows, ["live baseline"]);
    assert_eq!(node.as_deref(), Some("baseline"));
}

/// Shadow, and a canary's control, ask for `baseline`'s sources whatever
/// `arm` says: the control's diagnostics measure the default pipeline.
#[tokio::test]
async fn shadow_and_a_canarys_control_ask_for_baselines_sources() {
    for (mode, fraction) in [(MemoryMode::Shadow, 1.0), (MemoryMode::Canary, 0.0)] {
        let r = rig_with(mode, |c| {
            c.memory.arm = MemoryArm::Bm25;
            c.memory.canary_fraction = fraction;
        });
        let c = &r.core;
        let past = session(c, None, &[KITE]);
        let now = session(c, None, &[]);
        let asked = Arc::new(Mutex::new(Vec::new()));
        let inner = index_of(c, vec![past]);
        let seen = asked.clone();
        c.runner
            .memory
            .set_ask(Arc::new(move |p: IndexQueryParams| -> AskFuture {
                seen.lock().unwrap().push(p.sources.clone());
                inner(p)
            }));
        let res = turn(c, &now, "Where does the red kite roost?").await;
        assert_eq!(res.recalled, 0, "{mode:?}");
        assert_eq!(
            *asked.lock().unwrap(),
            [["bm25", "entity", "vector"]],
            "{mode:?}"
        );
    }
}
