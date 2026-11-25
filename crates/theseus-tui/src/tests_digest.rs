//! "Since you last looked" in the terminal UI (theseus-qkoi3): at each connect,
//! and after the terminal regains focus from an absence, one notice line from
//! `work.digest` since the store's look (issue 46), and a `work.look` at the
//! digest's end, as the board was shown. A first start is told nothing, and a
//! daemon that does not know the method costs the operator no word.

use crossterm::event::Event as TermEvent;
use serde_json::json;

use crate::app::{Effect, AWAY_MS};
use crate::tests::{harbour_rig_seen, harbour_world, opened, script, Rig, NOW, T0};

fn digest() -> serde_json::Value {
    json!({
        "from": 500, "to": 900, "from_ledger": false,
        "now": {"needs_you": 0, "working": 0, "done": 0, "failed": 0, "total": 0},
        "questions": [],
        "look": {"position": 500, "at_ms": T0 - 3_600_000},
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

/// How each digest was asked: its `since_look`, and whether it named a
/// position (never: a position this terminal holds may be another history's).
fn asked_how(asked: &[serde_json::Value]) -> Vec<(serde_json::Value, bool)> {
    asked
        .iter()
        .map(|a| (a["since_look"].clone(), a.get("since").is_some()))
        .collect()
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
    rig.asked("work.digest", 1).await;
    let asked = rig.daemon().asked("work.digest");
    assert_eq!(asked_how(&asked), [(json!(true), false)], "{asked:?}");
    rig.shows("◆ 2 reported, 1 answered by you since").await;
    assert!(footer(&rig).contains("theseus digest"), "{}", footer(&rig));
    // The store's look moves to the digest's end.
    rig.asked("work.look", 1).await;
    assert_eq!(rig.daemon().asked("work.look"), [json!({"position": 900})]);
}

#[tokio::test]
async fn a_first_start_is_told_nothing_and_its_board_is_the_first_look() {
    let world = harbour_world();
    let mut nothing = digest();
    nothing["lines"] = json!([]);
    nothing.as_object_mut().unwrap().remove("look");
    world
        .lock()
        .unwrap()
        .answers
        .insert("work.digest".into(), Ok(nothing));
    let mut rig = Rig::new(140, 24, script(world));
    rig.asked("executions.watch", 1).await;
    rig.asked("work.digest", 1).await;
    rig.settle().await;
    assert_eq!(
        asked_how(&rig.daemon().asked("work.digest")),
        [(json!(true), false)]
    );
    assert!(!footer(&rig).contains('◆'), "{}", footer(&rig));
    rig.asked("work.look", 1).await;
    assert_eq!(rig.daemon().asked("work.look"), [json!({"position": 900})]);
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
    rig.asked("work.digest", 1).await;
    rig.settle().await;
    // The store's look moved to the digest's end.
    assert_eq!(rig.daemon().asked("work.look"), [json!({"position": 900})]);
    // Away for a moment: nothing to tell.
    send(&mut rig, TermEvent::FocusLost).await;
    NOW.with(|n| n.set(T0 + 5_000));
    send(&mut rig, TermEvent::FocusGained).await;
    rig.settle().await;
    assert_eq!(rig.daemon().asked("work.digest").len(), 1);
    // Away for a while: asked once, since the store's look.
    send(&mut rig, TermEvent::FocusLost).await;
    NOW.with(|n| n.set(T0 + 5_000 + AWAY_MS));
    send(&mut rig, TermEvent::FocusGained).await;
    rig.asked("work.digest", 2).await;
    let asked = rig.daemon().asked("work.digest");
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked_how(&asked),
        [(json!(true), false), (json!(true), false)],
        "{asked:?}"
    );
    NOW.with(|n| n.set(T0));
}

/// A daemon restarted on a restored store holds lower positions than the board
/// this terminal last held: the digest asks since the store's own look, never
/// since a position from memory, which points into the newer history.
#[tokio::test]
async fn a_reconnect_after_a_restore_asks_since_the_stores_look() {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        w.board["position"] = json!(9_000);
        w.answers.insert("work.digest".into(), Ok(digest()));
    }
    let mut rig = Rig::new(140, 24, script(world.clone()));
    rig.shows("spec review").await;
    rig.asked("work.digest", 1).await;
    rig.settle().await;
    world.lock().unwrap().board = json!({
        "position": 6_050,
        "executions": [opened(6_050, "ses_new001")],
        "confirms": [],
        "total": 1,
    });
    rig.daemon().close();
    rig.shows("fresh start").await;
    rig.asked("work.digest", 1).await;
    let asked = rig.daemon().asked("work.digest");
    assert_eq!(asked_how(&asked), [(json!(true), false)], "{asked:?}");
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
    rig.asked("work.digest", 1).await;
    rig.settle().await;
    assert!(!footer(&rig).contains("unknown method"), "{}", footer(&rig));
}

/// The time an older build's seen file kept is carried into the store once (issue 46): a save of the file before
/// the store has taken a look keeps it, and so does a look the daemon refuses, so the next digest asks from it
/// again; the look the daemon takes hands it over.
#[tokio::test]
async fn the_old_time_waits_for_a_look_the_daemon_takes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theseus").join("seen.json");
    let old = T0 - 2 * 3_600_000;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = json!({"version": 1, "board_read": true, "executions": {},
        "looked": {"position": 999_999, "at_ms": old}});
    std::fs::write(&path, file.to_string()).unwrap();
    let mut none = digest();
    none.as_object_mut().unwrap().remove("look");
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        w.answers.insert("work.digest".into(), Ok(none.clone()));
        let busy = (-32000, "the store is busy".to_string());
        w.answers.insert("work.look".into(), Err(busy));
    }
    let mut rig = harbour_rig_seen(140, 24, world.clone(), &path);
    rig.asked("work.digest", 1).await;
    rig.asked("work.look", 1).await;
    rig.settle().await;
    assert_eq!(rig.daemon().asked("work.digest")[0]["since_ms"], old);
    // The TUI saves, and another client records what it showed: neither is the look.
    save(&mut rig);
    theseus_client::seen::record(&path, &[]).unwrap();
    assert_eq!(theseus_client::seen::old_look_time(&path), Some(old));
    // Back after an absence: asked from the old time again, and this look is taken.
    none["to"] = json!(950);
    {
        let mut w = world.lock().unwrap();
        w.answers.insert("work.digest".into(), Ok(none));
        let took = json!({"look": {"position": 950, "at_ms": T0 + 1}});
        w.answers.insert("work.look".into(), Ok(took));
    }
    send(&mut rig, TermEvent::FocusLost).await;
    NOW.with(|n| n.set(T0 + AWAY_MS));
    send(&mut rig, TermEvent::FocusGained).await;
    rig.asked("work.digest", 2).await;
    rig.asked("work.look", 2).await;
    rig.settle().await;
    NOW.with(|n| n.set(T0));
    let asked = rig.daemon().asked("work.digest");
    assert_eq!(asked[1]["since_ms"], old, "{asked:?}");
    save(&mut rig);
    assert_eq!(
        theseus_client::seen::old_look_time(&path),
        None,
        "handed over"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let on_disk: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        on_disk["store_look"],
        json!({"position": 950, "at_ms": T0 + 1})
    );
}

/// The TUI's save, as its loop writes it.
fn save(rig: &mut Rig) {
    rig.runner.app.seen.dirty = true;
    if let Some(Effect::Save(path, text)) = rig.runner.app.save() {
        theseus_client::seen::write_merged(&path, &text).unwrap();
    }
}

async fn send(rig: &mut Rig, e: TermEvent) {
    let n = rig.runner.terminal_events + 1;
    rig.keys.send(e).unwrap();
    rig.until("an event to be handled", move |r| {
        r.runner.terminal_events >= n
    })
    .await;
}
