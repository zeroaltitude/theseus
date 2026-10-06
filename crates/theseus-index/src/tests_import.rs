//! An imported session's nodes in the index, and their erase
//! (theseus-0lrr.6): the import's nodes are indexed, outside text marked
//! external; their tombstones, written after them as `import.erase` writes
//! them, take every one out of the index as the follower meets them; and a
//! rebuild from the WAL, a fresh index over the same log, holds none of
//! them either.

use theseus_core::import::Integrity;

use crate::proto::QueryParams;
use crate::tests::{imported_node, settle, user, Rig};

#[test]
fn an_erased_import_leaves_the_index_and_a_rebuild_brings_none_back() {
    let rig = Rig::new();
    let (a, b) = ("ses_ep0a", "ses_ep0b");
    let imported = vec![
        imported_node(a, 0, Integrity::Operator),
        imported_node(a, 1, Integrity::Outside),
        imported_node(b, 0, Integrity::Agent),
    ];
    rig.put(&imported);
    rig.put(&[user("ses_live", "the tide log moved to the shed again")]);
    let mut t = rig.open();
    settle(&mut t);
    let shared = t.shared();
    for n in &imported {
        assert!(shared.engine.holds(&n.id).unwrap(), "{} is indexed", n.id);
    }
    let hits = shared
        .query(&QueryParams::new("tide log shed"))
        .unwrap()
        .hits;
    let outside = hits
        .iter()
        .find(|h| h.node_id == imported[1].id)
        .expect("the outside text is a hit");
    assert!(outside.external, "outside text is external: {outside:?}");
    assert_eq!(outside.origin, "import");

    // The erase: each node written again as its tombstone.
    let tombstones: Vec<_> = imported
        .iter()
        .map(|n| n.erased(1_790_000_000_000, "import.erase of reef-2026"))
        .collect();
    rig.put(&tombstones);
    settle(&mut t);
    for n in &imported {
        assert!(
            !shared.engine.holds(&n.id).unwrap(),
            "{} left the index",
            n.id
        );
    }
    let left: Vec<String> = shared
        .query(&QueryParams::new("tide log shed"))
        .unwrap()
        .hits
        .into_iter()
        .map(|h| h.node_id)
        .collect();
    assert!(
        left.iter().all(|id| !id.starts_with("imp_")),
        "no imported node answers: {left:?}"
    );
    assert_eq!(
        left.len(),
        1,
        "the live session's node still does: {left:?}"
    );
    drop(t);

    // A rebuild: a fresh index over the same WAL.
    let fresh = rig._tmp.path().join("index-rebuilt");
    let mut again = crate::tender::Tender::open(rig.cfg(&fresh)).unwrap();
    settle(&mut again);
    let s = again.shared();
    for n in &imported {
        assert!(!s.engine.holds(&n.id).unwrap(), "{} is not rebuilt", n.id);
    }
    assert_eq!(s.engine.counts().unwrap().1, 1, "only the live node");
}
