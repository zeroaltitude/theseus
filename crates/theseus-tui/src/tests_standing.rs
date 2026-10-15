//! The standing records in the TUI (theseus-slvl.4): a pill in the header
//! counting the live records that hold in the open session on this surface,
//! asked for once when it opens (and after a turn, and at a reconnect), and
//! `p`, which lists them; an older daemon's refusal costs the operator no word.

use crossterm::event::KeyCode;
use serde_json::{json, Value};

use crate::tests::{harbour_world, open_dm, script, Rig};

fn record(id: &str, kind: &str, scope: Value, text: &str) -> Value {
    json!({"id": id, "kind": kind, "text": text, "scope": scope, "since_ms": 1_790_000_000_000u64,
           "source": "the CLI", "state": "live", "compiles": 0, "missing": 0})
}

fn two_records() -> Value {
    json!({"records": [
        record("std_a1b2c3d4", "order", json!({"kind": "everywhere"}), "never push to main"),
        record("std_e5f6a7b8", "preference", json!({"kind": "surface", "of": "terminal"}),
               "code first, no pleasantries"),
    ], "compiles": 0, "missing": 0})
}

#[tokio::test]
async fn the_header_counts_the_records_that_hold_in_the_open_session() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("standing.list".into(), Ok(two_records()));
    let mut rig = Rig::new(100, 24, script(world));
    open_dm(&mut rig).await;
    // The pin is two cells wide: the buffer's text has a blank after it.
    rig.until("the pill", |r| r.screen()[0].contains('📌'))
        .await;
    assert!(
        rig.screen()[0].replace("📌  ", "📌 ").contains("· 📌 2"),
        "{:?}",
        rig.screen()[0]
    );
    // It asked for this session's scope on this surface, and nothing else.
    assert_eq!(
        rig.daemon().asked("standing.list"),
        [json!({"session_id": "ses_dm0001", "surface": "terminal"})]
    );
}

#[tokio::test]
async fn no_records_no_pill_and_an_older_daemons_refusal_says_nothing() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "standing.list".into(),
        Ok(json!({"records": [], "compiles": 0, "missing": 0})),
    );
    let mut rig = Rig::new(100, 24, script(world));
    open_dm(&mut rig).await;
    rig.asked("standing.list", 1).await;
    rig.settle().await;
    assert!(!rig.screen()[0].contains('📌'), "{:?}", rig.screen()[0]);

    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "standing.list".into(),
        Err((-32601, "unknown method".into())),
    );
    let mut rig = Rig::new(100, 24, script(world));
    open_dm(&mut rig).await;
    rig.asked("standing.list", 1).await;
    rig.settle().await;
    let all = rig.screen().join("\n");
    assert!(
        !all.contains('📌') && !all.contains("unknown method"),
        "{all}"
    );
}

#[tokio::test]
async fn p_lists_the_records_by_kind_and_scope_and_any_key_closes_it() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("standing.list".into(), Ok(two_records()));
    let mut rig = Rig::new(100, 24, script(world));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('p')]).await;
    rig.shows("standing here (2)").await;
    let all = rig.screen().join("\n");
    assert!(
        all.contains("std_a1b2c3d4  order      everywhere  never push to main"),
        "{all}"
    );
    assert!(
        all.contains("std_e5f6a7b8  preference surface:terminal  code first, no pleasantries"),
        "{all}"
    );
    // Opening the list read the records again.
    rig.asked("standing.list", 2).await;
    rig.press(&[KeyCode::Char('x')]).await;
    rig.settle().await;
    assert!(!rig.screen().join("\n").contains("standing here"));
}

#[tokio::test]
async fn p_with_no_session_open_says_to_open_one() {
    let mut rig = Rig::new(100, 24, script(harbour_world()));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Char('p')]).await;
    rig.shows("open a session first").await;
    assert!(rig.daemon().asked("standing.list").is_empty());
}
