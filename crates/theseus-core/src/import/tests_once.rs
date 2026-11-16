//! What an import pass reads once is not kept (issue 21): the people step, an
//! append's check and an erase each read every node of the sessions they are
//! given, one time, and the heat cache (`NodeCache`) keeps whatever it decodes.
//! After the nightly import of 565 sessions it held 44 MiB of nodes no turn
//! would read, and the daemon rested 46 MiB higher until it restarted.

use serde_json::json;
use theseus_protocol::method;

use super::session_id_of;
use super::tests::{call, import, lines, live};
use super::tests_append::{sync, TAG};

const SESSIONS: u64 = 4;

/// The heat cache's entries, and its decodes so far.
fn cache(c: &std::sync::Arc<crate::Core>) -> (u64, u64) {
    let h = c.store.node_cache().health();
    (h.entries, h.decodes)
}

/// An import's writes, its people step, an append and an erase leave the heat
/// cache as they found it, and the people step still reads the nodes.
#[tokio::test]
async fn the_nodes_an_import_reads_once_are_not_kept() {
    let r = live();
    let c = &r.core;
    let eps: Vec<_> = (0..SESSIONS).map(|n| sync(n, TAG, &[0, 1, 2, 3])).collect();
    import(c, lines(&eps)).await;
    assert_eq!(cache(c).0, 0, "an import's writes keep no node");

    let people = call(
        c,
        method::IMPORT_PEOPLE,
        json!({"tag": TAG, "dry_run": false}),
    )
    .await
    .unwrap();
    let (entries, decodes) = cache(c);
    assert!(
        decodes >= SESSIONS * 4,
        "the people step read the nodes ({decodes} decodes): {people}"
    );
    assert_eq!(entries, 0, "and kept none of them");

    let grown: Vec<_> = (0..SESSIONS)
        .map(|n| sync(n, TAG, &[0, 1, 2, 3, 5]))
        .collect();
    let got = import(c, lines(&grown)).await;
    assert_eq!(got.appended, SESSIONS, "{got:?}");
    assert_eq!(
        cache(c).0,
        0,
        "an append's check of what is stored keeps none"
    );

    call(
        c,
        method::IMPORT_ERASE,
        json!({"tag": TAG, "why": "a test"}),
    )
    .await
    .unwrap();
    assert_eq!(cache(c).0, 0, "an erase keeps none");
    let sid = session_id_of(eps[0]["episode_id"].as_str().unwrap());
    assert!(c.store.session_nodes_once(&sid).is_ok());
}

/// A node a turn already holds is served from the cache by the once-read, left
/// as hot or as cold as it was; one it lacks is decoded and not kept.
#[test]
fn a_once_read_serves_what_is_kept_and_keeps_nothing_it_decodes() {
    use crate::node::Node;
    let cache = crate::node_cache::NodeCache::default();
    let n = Node::user("ses_tern", None, "web", "the heron left at dawn");
    let bytes = serde_json::to_vec(&n).unwrap();
    let a = cache.node_once(7, &bytes).unwrap();
    assert_eq!(a.id, n.id);
    let h = cache.health();
    assert_eq!((h.entries, h.bytes, h.decodes), (0, 0, 1));
    let kept = cache.node(8, &bytes).unwrap();
    let again = cache.node_once(8, &bytes).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&kept, &again),
        "the kept node, not a decode"
    );
    let h = cache.health();
    assert_eq!((h.entries, h.decodes, h.hits), (1, 2, 0), "and no touch");
}
