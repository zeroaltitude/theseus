//! Tiering through the whole core (M6 step 33, theseus-6fn.13; design
//! §2.10 and 33's rows in §3.1 and §3.2): the heat cache shared across turns
//! and readers, so a session's next turn decodes only its new nodes.

use std::sync::Arc;

use crate::config::MemoryMode;
use crate::tests_recall::{rig_with, session, turn};
use crate::Core;

/// The store's count of node decodes.
fn decodes(core: &Core) -> u64 {
    core.store.node_cache().decodes()
}

/// A session's nodes, counted without a decode (the records' keys).
fn count(core: &Arc<Core>, sid: &str) -> u64 {
    use theseus_store::Store as _;
    core.store
        .inner()
        .scan_scope(sid, 0, usize::MAX)
        .unwrap()
        .iter()
        .filter(|r| r.kind == theseus_store::kinds::NODE)
        .count() as u64
}

/// A long session's second turn decodes only its new nodes: every node a
/// reader decoded before is served from the cache by position, the turn's
/// first read of its transcript included. With the cache off, each turn
/// decodes the session whole again.
#[tokio::test]
async fn decodes_fall_on_a_long_session() {
    let mut per_turn = Vec::new();
    for mb in [crate::node_cache::DEFAULT_MB, 0] {
        let r = rig_with(MemoryMode::Off, |c| c.memory.node_cache_mb = mb);
        let c = &r.core;
        let said: Vec<String> = (0..60)
            .map(|i| format!("Note {i}: the lamp at the north pier was lit at dusk."))
            .collect();
        let said: Vec<&str> = said.iter().map(String::as_str).collect();
        let sid = session(c, None, &said);
        turn(c, &sid, "the first turn reads the session").await;
        let (n0, d0) = (count(c, &sid), decodes(c));
        turn(c, &sid, "the second turn").await;
        let (n1, d1) = (count(c, &sid), decodes(c));
        let new = n1 - n0;
        assert!(new >= 2, "the input and the answer: {new}");
        per_turn.push((d1 - d0, new, n1));
    }
    let (cached, new, _) = per_turn[0];
    assert_eq!(cached, new, "with the cache, only the turn's new nodes");
    let (off, _, all) = per_turn[1];
    assert!(off >= all, "without it, the session whole: {off} of {all}");
}
