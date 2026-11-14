//! Pausing the loop's builds (issue 6, theseus-7glm6): a pause stops new self builds from
//! starting, clearing it lets them start, and a restart clears it.

use super::tests::rig_with;

/// KEY: set, a tick starts nothing and says why once; the backlog's own start (the night's, and
/// the one the pipeline calls) starts nothing; cleared, they start.
#[tokio::test]
async fn a_pause_stops_new_builds_and_clearing_it_lets_them_start() {
    let r = rig_with(2, 30.0);
    assert!(r
        .core
        .self_pause_set(Some("a benchmark is running"), "the owner"));
    for _ in 0..2 {
        let t = r.tick().await;
        assert!(t.started.is_empty(), "{t:?}");
        assert_eq!(t.hold.as_deref(), Some("paused"));
    }
    assert!(r.fake.starts.lock().unwrap().is_empty());
    let held = r.rows("self.pipeline.held");
    assert_eq!(held.len(), 1, "said once: {held:?}");
    let why = held[0]["why"].as_str().unwrap();
    assert!(why.contains("paused by the owner"), "{why}");
    assert!(why.contains("a benchmark is running"), "{why}");
    assert!(
        why.contains("until it is unpaused or the daemon restarts"),
        "{why}"
    );
    assert_eq!(r.view().room.hold.as_deref(), Some("paused"));
    // Every start path goes through the backlog's start, the night's own included: while paused
    // it starts nothing (the rig's night is told to start none, so this is the path to reach it).
    let item = r.view().ready[0].item.clone();
    assert!(r.core.backlog_start_item(&item).unwrap().is_none());
    assert_eq!(r.state(&item), theseus_protocol::tasks::TaskState::Proposed);
    // Cleared: they start.
    assert!(r.core.self_pause_clear());
    assert!(!r.core.self_pause_clear(), "nothing was paused");
    assert!(
        r.core.backlog_start_item(&item).unwrap().is_some(),
        "and cleared, it starts"
    );
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "the other slot: {t:?}");
    assert_ne!(t.hold.as_deref(), Some("paused"));
}

/// A pause is the process's memory and nowhere in the store, so a restart clears it: setting it
/// writes no frame, and a core built from the store as it stands is not paused.
#[tokio::test]
async fn a_restart_clears_a_pause() {
    let r = rig_with(1, 30.0);
    let before = r.core.store.last_position();
    r.core.self_pause_set(None, "the owner");
    assert_eq!(r.core.store.last_position(), before, "nothing was written");
    assert!(r.core.self_paused().is_some());
    // A restart: a core over a copy of the store, as the daemon opens it again.
    let copy = crate::scratch::tempdir();
    let from = r.f.place.state.join("store");
    let to = copy.path().join("store");
    let status = std::process::Command::new("cp")
        .arg("-r")
        .arg(&from)
        .arg(&to)
        .status()
        .unwrap();
    assert!(status.success());
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = copy.path().to_string_lossy().into_owned();
    let store = crate::store::Store::open(&to).unwrap();
    let again = crate::Core::build(crate::rpc::Parts::for_tests(
        cfg,
        std::sync::Arc::new(crate::provider::FakeProvider::default()),
        store,
    ))
    .unwrap();
    assert_eq!(
        again.self_paused(),
        None,
        "a restarted daemon is not paused"
    );
}

/// Setting it twice replaces the words, and the state says who and why.
#[test]
fn a_pause_says_who_set_it_and_why() {
    let p = super::super::pause::Pause {
        why: Some("benchmarks".into()),
        by: "the owner".into(),
        at_ms: 0,
    };
    let said = p.words();
    assert!(said.starts_with("paused by the owner at "), "{said}");
    assert!(said.contains(": benchmarks."), "{said}");
}
