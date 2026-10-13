//! The OpenClaw sync's warning in the header (theseus-pjmj7): `health` is read
//! at each connect, and a failed or a missed night shows beside the counts; a
//! daemon that does not know it (or has no sync) costs nothing.

use serde_json::json;

use crate::tests::{harbour_world, script, Rig};

fn health(warn: Option<&str>) -> serde_json::Value {
    let mut sync = json!({"at_ms": 1, "sessions_new": 0, "sessions_appended": 0, "messages": 0,
        "rejected": 0, "people_usd": 0.0, "next_ms": 2});
    if let Some(w) = warn {
        sync["warn"] = json!(w);
    }
    json!({"imported": {"sessions": 0, "erased": 0, "sync": sync}})
}

#[tokio::test]
async fn a_failed_night_shows_in_the_header_and_a_good_one_says_nothing() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "health".into(),
        Ok(health(Some("the last sync failed: no python3"))),
    );
    let mut rig = Rig::new(120, 24, script(world));
    rig.shows("⚠ the last sync failed: no python3").await;
    assert!(rig.screen()[0].contains("theseus"), "{:?}", rig.screen()[0]);

    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("health".into(), Ok(health(None)));
    let mut rig = Rig::new(120, 24, script(world));
    rig.asked("health", 1).await;
    rig.settle().await;
    assert!(!rig.screen()[0].contains('⚠'), "{:?}", rig.screen()[0]);
}

/// An older daemon that refuses `health`'s sync (or the method) says nothing.
#[tokio::test]
async fn a_daemon_with_no_sync_costs_the_operator_no_word() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("health".into(), Err((-32601, "unknown method".into())));
    let mut rig = Rig::new(120, 24, script(world));
    rig.asked("health", 1).await;
    rig.settle().await;
    assert!(!rig.screen()[0].contains('⚠') && !rig.screen().join("\n").contains("unknown method"));
}
