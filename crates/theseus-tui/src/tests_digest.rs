//! "Since you last looked" in the terminal UI (theseus-qkoi3): at the start,
//! and after the terminal regains focus from an absence, one notice line from
//! `work.digest` since the position the operator last looked at the board.
//! Nothing is told on a first start, and a daemon that does not know the method
//! costs the operator no word.

use crossterm::event::Event as TermEvent;
use serde_json::json;

use crate::app::AWAY_MS;
use crate::tests::{harbour_world, script, Rig, NOW, T0};

fn digest() -> serde_json::Value {
    json!({
        "from": 500, "to": 900, "from_ledger": false,
        "now": {"needs_you": 0, "working": 0, "done": 0, "failed": 0, "total": 0},
        "questions": [],
        "lines": [
            {"at_ms": T0 + 10_000, "kind": "reported", "count": 1, "words": "📋 Beta summary reported",
             "work": {"id": "exe_b", "kind": "task", "title": "Beta summary"}},
            {"at_ms": T0 + 20_000, "kind": "reported", "count": 1, "words": "📋 Tide table reported",
             "work": {"id": "exe_t", "kind": "task", "title": "Tide table"}},
            {"at_ms": T0 + 30_000, "kind": "answered", "count": 1, "words": "✓ you answered: fs.write notes.txt",
             "work": {"id": "act_1", "kind": "question", "title": "fs.write notes.txt"}}
        ]
    })
}

fn footer(rig: &Rig) -> String {
    rig.screen().join("\n")
}

#[tokio::test]
async fn the_start_tells_what_happened_since_the_last_look_in_one_line() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.digest".into(), Ok(digest()));
    let mut rig = Rig::new(140, 24, script(world));
    rig.runner.app.seen.look(500, T0 - 3_600_000);
    rig.asked("work.digest", 1).await;
    assert_eq!(rig.daemon().asked("work.digest"), [json!({"since": 500})]);
    rig.shows("◆ 2 reported, 1 answered by you since").await;
    assert!(footer(&rig).contains("theseus digest"), "{}", footer(&rig));
    // The look moved to the digest's end, and the file will say so.
    assert_eq!(rig.runner.app.seen.looked().position, 900);
}

#[tokio::test]
async fn a_first_start_is_told_nothing_and_asks_for_nothing() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.digest".into(), Ok(digest()));
    let mut rig = Rig::new(140, 24, script(world));
    rig.asked("executions.watch", 1).await;
    rig.settle().await;
    assert!(rig.daemon().asked("work.digest").is_empty());
    assert!(!footer(&rig).contains("◆ 2 reported"));
    // And the first board it read is the look the next start tells from.
    assert!(rig.runner.app.seen.looked().position > 0);
}

#[tokio::test]
async fn regaining_focus_after_an_absence_asks_again_and_a_short_one_does_not() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.digest".into(), Ok(digest()));
    let mut rig = Rig::new(140, 24, script(world));
    rig.runner.app.seen.look(100, T0 - 1000);
    rig.asked("work.digest", 1).await;
    rig.settle().await;
    let looked = rig.runner.app.seen.looked().position;
    assert_eq!(looked, 900);
    // Away for a moment: nothing to tell.
    send(&mut rig, TermEvent::FocusLost).await;
    NOW.with(|n| n.set(T0 + 5_000));
    send(&mut rig, TermEvent::FocusGained).await;
    rig.settle().await;
    assert_eq!(rig.daemon().asked("work.digest").len(), 1);
    // Away for a while: asked once, since the look the blur left.
    send(&mut rig, TermEvent::FocusLost).await;
    NOW.with(|n| n.set(T0 + 5_000 + AWAY_MS));
    send(&mut rig, TermEvent::FocusGained).await;
    rig.asked("work.digest", 2).await;
    let asked = rig.daemon().asked("work.digest");
    assert_eq!(asked.len(), 2);
    assert!(asked[1]["since"].as_u64().unwrap() >= looked, "{asked:?}");
    NOW.with(|n| n.set(T0));
}

#[tokio::test]
async fn a_daemon_that_does_not_know_the_digest_costs_the_operator_no_word() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.digest".into(), Err((-32601, "unknown method".into())));
    let mut rig = Rig::new(140, 24, script(world));
    rig.runner.app.seen.look(500, T0 - 1000);
    rig.asked("work.digest", 1).await;
    rig.settle().await;
    assert!(!footer(&rig).contains("unknown method"), "{}", footer(&rig));
}

async fn send(rig: &mut Rig, e: TermEvent) {
    let n = rig.runner.terminal_events + 1;
    rig.keys.send(e).unwrap();
    rig.until("an event to be handled", move |r| {
        r.runner.terminal_events >= n
    })
    .await;
}
