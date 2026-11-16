//! What an import pass reads once is not kept (issue 21): the people step, an
//! append's check, an erase and the identity-file read each decode every node of
//! the sessions they are given, one time, and the heat cache (`NodeCache`) keeps
//! whatever it decodes. After the nightly import of 565 sessions it held 44 MiB of
//! nodes no turn would read, and the daemon rested 46 MiB higher until it restarted.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::method;

use super::episode;
use super::tests::{call, import, lines, live};
use super::tests_append::{sync, TAG};
use crate::tests_recall::Rig;

const SESSIONS: u64 = 4;

/// The heat cache's entries, and its decodes so far.
fn cache(c: &Arc<crate::Core>) -> (u64, u64) {
    let h = c.store.node_cache().health();
    (h.entries, h.decodes)
}

/// A rig with `SESSIONS` imported sessions of four messages each, and a cache
/// that has kept nothing.
async fn imported() -> Rig {
    let r = live();
    let eps: Vec<_> = (0..SESSIONS).map(|n| sync(n, TAG, &[0, 1, 2, 3])).collect();
    import(&r.core, lines(&eps)).await;
    assert_eq!(cache(&r.core).0, 0, "an import's writes keep no node");
    r
}

#[tokio::test]
async fn the_people_step_reads_every_node_and_keeps_none() {
    let r = imported().await;
    let c = &r.core;
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
}

#[tokio::test]
async fn an_appends_check_of_what_is_stored_keeps_none() {
    let r = imported().await;
    let c = &r.core;
    let grown: Vec<_> = (0..SESSIONS)
        .map(|n| sync(n, TAG, &[0, 1, 2, 3, 5]))
        .collect();
    let got = import(c, lines(&grown)).await;
    assert_eq!(got.appended, SESSIONS, "{got:?}");
    let (entries, decodes) = cache(c);
    assert!(
        decodes >= SESSIONS * 4,
        "it read what was stored: {decodes}"
    );
    assert_eq!(entries, 0, "and kept none of it");
}

#[tokio::test]
async fn an_erase_keeps_none() {
    let r = imported().await;
    let c = &r.core;
    call(
        c,
        method::IMPORT_ERASE,
        json!({"tag": TAG, "why": "a test"}),
    )
    .await
    .unwrap();
    let (entries, decodes) = cache(c);
    assert!(decodes >= SESSIONS * 4, "it read every node: {decodes}");
    assert_eq!(entries, 0, "and kept none");
}

/// An identity file's lines name an agent, and the people house reads them from
/// the store without keeping them.
#[tokio::test]
async fn the_identity_file_read_keeps_none() {
    let r = live();
    let c = &r.core;
    let mut e: Value = sync(9, TAG, &[0, 1]);
    e["place"] = json!({"kind": "file", "name": "IDENTITY.md", "id": null});
    e["messages"][0]["text"] = json!("- **Name:** Wren Ash");
    e["hash"] = json!(episode::hash_of(&e));
    import(c, lines(std::slice::from_ref(&e))).await;
    assert_eq!(cache(c).0, 0);
    let names = c.people_house();
    assert!(names.contains("wren ash"), "{names:?}");
    let (entries, decodes) = cache(c);
    assert!(decodes >= 2, "it read the file's nodes: {decodes}");
    assert_eq!(entries, 0, "and kept none");
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
    assert!(Arc::ptr_eq(&kept, &again), "the kept node, not a decode");
    let h = cache.health();
    assert_eq!((h.entries, h.decodes, h.hits), (1, 2, 0), "and no touch");
}
