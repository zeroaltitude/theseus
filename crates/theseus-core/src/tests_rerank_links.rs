//! The memory pass's links in a rerank's repack (M6 32c, 32d; theseus-daiz):
//! two notes on one subject, the newer a correction linked `same_entity` to
//! the older, as the memory pass writes its edges, and a science that
//! prefers the newer. Jev's order puts the older first, yet the repack,
//! live (`Memory::refill`) and in shadow (`Recalled.links`), reads the link
//! as the recall did and admits the newer alone.

use std::sync::Arc;

use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::config::MemoryMode;
use crate::graph::EdgeKind;
use crate::rpc::Core;
use crate::tests_activation::edge;
use crate::tests_rerank::{
    index_of, keys_of, recalls, rig, sent, session, turn, until_reranked, Rig,
};

const OLD: &str = "the grey heron nests by the old weir";
const NEW: &str = "correction: the grey heron now nests by the new mill";

/// A rig whose index ranks the older note first, and Jev too; the newer
/// linked to the older. Returns the rig, the asking session, and the two
/// notes' ids, older first.
fn rig_with_a_correction(
    jev: &FakeJev,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, String, [String; 2]) {
    // Jev would admit the older note: its question is asked about first.
    jev.script("helps.1", Jev::Noul(0.97));
    jev.script("helps.2", Jev::Noul(0.10));
    let r = rig(Some(jev), tweak);
    let c: &Arc<Core> = &r.core;
    let old = session(c, None, &[OLD]);
    let new = session(c, None, &[NEW]);
    let here = session(c, None, &[]);
    let node = |sid: &str| c.store.session_nodes(sid).unwrap().remove(0).1;
    let (older, newer) = (node(&old), node(&new));
    edge(
        &c.store,
        EdgeKind::SameEntity,
        &newer,
        &older,
        "memory.pass",
    );
    c.runner.memory.set_ask(index_of(c, vec![old, new]));
    (r, here, [older.id, newer.id])
}

/// Live: the recall's request admits the newer note alone, though Jev put
/// the older first (`Memory::refill` takes the links).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_rerank_repacks_with_the_links_and_admits_the_newer() {
    let jev = FakeJev::start().unwrap();
    let (r, here, [older, newer]) = rig_with_a_correction(&jev, |c| {
        c.memory.mode = MemoryMode::Live;
        c.memory.recall_max_items = 1;
        // A loaded machine's local Jev may take more than 200 ms.
        c.memory.rerank_wait_ms = 600;
    });
    let c = &r.core;
    turn(c, &here, "Where does the grey heron nest?").await;
    let req = sent(&r.model).pop().unwrap();
    assert!(req.contains(NEW), "the newer note: {req}");
    assert!(!req.contains(OLD), "not the older: {req}");
    let m = &recalls(c, &here)[0];
    let rr = m.rerank.as_ref().expect("the manifest's rerank");
    assert!(rr.applied, "Jev's order was applied: {rr:?}");
    let admitted: Vec<&str> = m.admitted.iter().map(|a| a.node_id.as_str()).collect();
    assert_eq!(admitted, [newer.as_str()], "the older ({older}) is dropped");
    let rows = until_reranked(&c.store, 1).await;
    assert_eq!(
        keys_of(&rows[0].data["context"]["rerank"]["reranked_admitted"]),
        [format!("{newer}#0")]
    );
}

/// Shadow: the `rerank.v1` row's `reranked_admitted` holds the newer note
/// alone (`Recalled.links`), and the baseline's own admission is the same.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shadow_rerank_repacks_with_the_links_and_admits_the_newer() {
    let jev = FakeJev::start().unwrap();
    let (r, here, [_, newer]) = rig_with_a_correction(&jev, |c| c.memory.recall_max_items = 1);
    let c = &r.core;
    turn(c, &here, "Where does the grey heron nest?").await;
    let rows = until_reranked(&c.store, 1).await;
    let rr = &rows[0].data["context"]["rerank"];
    assert_eq!(rr["asked"], 2, "{rr}");
    assert_eq!(
        keys_of(&rr["reranked_admitted"]),
        [format!("{newer}#0")],
        "{rr}"
    );
    assert_eq!(keys_of(&rr["fused_admitted"]), [format!("{newer}#0")]);
}
