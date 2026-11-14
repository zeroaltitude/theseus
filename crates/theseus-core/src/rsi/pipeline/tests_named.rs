//! A hold on the slots names the trees that hold them (issue 6, the install's reap).

use super::tests::rig_with;

/// KEY: a tree nothing owns (a join's, named for a merge) holds the one slot, and the hold's
/// reason says which tree it is.
#[tokio::test]
async fn a_hold_on_the_slots_names_the_open_tree() {
    let r = rig_with(1, 30.0);
    let trees = r.f.place.trees();
    std::fs::create_dir_all(trees.join("install-72af5c3fd1ff")).unwrap();
    let t = r.tick().await;
    assert!(t.started.is_empty(), "{t:?}");
    assert_eq!(t.hold.as_deref(), Some("slots"));
    let held = r.rows("self.pipeline.held");
    assert_eq!(held.len(), 1, "{held:?}");
    let why = held[0]["why"].as_str().unwrap();
    assert!(why.contains("open now: install-72af5c3fd1ff"), "{why}");
    assert_eq!(
        r.view()
            .room
            .why
            .as_deref()
            .map(|w| w.contains("install-72af5c3fd1ff")),
        Some(true)
    );
}
