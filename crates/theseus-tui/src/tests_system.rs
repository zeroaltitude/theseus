//! The daemon's own work in the header (theseus-jn17m): the `Theseus` root
//! collapsed, from the TUI's one `work.watch` (the whole board, which a
//! private scope shows the `system` root on) at each connect and
//! `work.changed` after; a daemon that does not know it costs nothing.

use serde_json::{json, Value};

use crate::tests::{harbour_world, script, Rig};

fn job(position: u64, label: &str, done: u64, state: &str) -> Value {
    json!({
        "id": "sys_import_1", "kind": "system", "position": position, "at_ms": position,
        "parent": "system", "root": "system", "title": "Importing Claude Code history",
        "state": state,
        "attention": {"level": if state == "running" { "working" } else { "idle" },
                      "label": label, "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1,
        "progress": {"steps_done": done, "steps_total": 120, "current": "batch", "at_ms": position}
    })
}

fn root(position: u64) -> Value {
    json!({
        "id": "system", "kind": "system", "position": position, "at_ms": position,
        "root": "system", "title": "Theseus", "state": "ready",
        "attention": {"level": "ready", "label": "ready", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1,
        "below": {"needs_you": 0, "working": 1, "done": 0, "failed": 0, "total": 1}
    })
}

/// The header says the running job and its count, follows `work.changed` to
/// the job's end, and then the root's count closed; the request names
/// `system` alone.
#[tokio::test]
async fn the_header_says_the_daemons_jobs_and_follows_them_to_their_end() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "work.watch".into(),
        Ok(
            json!({"position": 5, "views": [root(5), job(5, "batch 41 of 120", 41, "running")],
                  "total": 1}),
        ),
    );
    let mut rig = Rig::new(140, 24, script(world));
    rig.shows("⚙ importing 41/120").await;
    assert_eq!(
        rig.daemon().asked("work.watch"),
        [json!({"interactive": true})]
    );
    rig.daemon().notify(
        "work.changed",
        json!({"position": 6, "at_ms": 6, "views": [
            job(6, "batch 42 of 120", 42, "running")]}),
    );
    rig.shows("⚙ importing 42/120").await;
    // An older view does not undo a newer one (the position rule).
    rig.daemon().notify(
        "work.changed",
        json!({"position": 5, "at_ms": 5, "views": [job(5, "batch 41 of 120", 41, "running")]}),
    );
    rig.settle().await;
    assert!(
        rig.screen()[0].contains("⚙ importing 42/120"),
        "{:?}",
        rig.screen()[0]
    );
    // It ends: the root stands closed over what it holds.
    let mut done_root = root(7);
    done_root["below"] = json!({"needs_you": 0, "working": 0, "done": 1, "failed": 0, "total": 1});
    rig.daemon().notify(
        "work.changed",
        json!({"position": 7, "at_ms": 7, "views": [done_root, job(7, "done · 120 sessions", 120, "done")]}),
    );
    rig.shows("⚙ Theseus · 1 done").await;
    assert!(
        !rig.screen()[0].contains("importing"),
        "{:?}",
        rig.screen()[0]
    );
}

/// A daemon without the board answers an error, and the header says nothing.
#[tokio::test]
async fn a_daemon_without_the_board_costs_the_operator_no_word() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.watch".into(), Err((-32601, "unknown method".into())));
    let mut rig = Rig::new(120, 24, script(world));
    rig.asked("work.watch", 1).await;
    rig.settle().await;
    let all = rig.screen().join("\n");
    assert!(
        !all.contains('⚙') && !all.contains("unknown method"),
        "{all}"
    );
}
