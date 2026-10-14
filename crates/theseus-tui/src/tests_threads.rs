//! Threads in the TUI (theseus-j0yh): the status line names the current thread,
//! Ctrl-T opens the picker, `/t` switches from the input line, and a thread's
//! session wears its tag. A scripted daemon answers `thread.list`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};

use crate::tests::{harbour_world, open_dm, script, Rig, T0};

fn row(slug: &str, title: &str, session: Option<&str>, state: &str, ago_s: u64) -> Value {
    let mut thread = json!({
        "slug": slug, "title": title, "state": state,
        "last_active_ms": T0 - ago_s * 1000, "created_at_ms": T0 - 100_000_000,
    });
    if let Some(s) = session {
        thread["session_id"] = json!(s);
    }
    let mut r = json!({"thread": thread, "today_usd": 0.0});
    if session.is_some() {
        r["context_tokens"] = json!(41_200);
        r["cache_ttl_secs"] = json!(3600);
    }
    r
}

/// Three threads: one on the DM's session, one with a persona and a session
/// long cold, one with no session yet; and a paused one.
fn threads() -> Value {
    let mut tides = row(
        "tides-log",
        "Tide log rewrite",
        Some("ses_other"),
        "active",
        7200,
    );
    tides["thread"]["persona"] = json!("surveyor");
    json!({"threads": [
        row("pr", "bh-ai pr", Some("ses_dm0001"), "active", 60),
        tides,
        row("moons", "Moon table", None, "active", 9000),
        row("old-notes", "Old notes", None, "paused", 90000),
    ], "total": 4})
}

fn rig_with_threads() -> Rig {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("thread.list".into(), Ok(threads()));
    Rig::new(80, 24, script(world))
}

fn ctrl_t() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)
}

#[tokio::test]
async fn the_header_names_the_current_thread_and_a_tui_without_threads_says_nothing() {
    let mut plain = Rig::new(80, 24, script(harbour_world()));
    plain.shows("spec review").await;
    assert!(
        !plain.screen()[0].contains("home"),
        "{:?}",
        plain.screen()[0]
    );
    assert!(plain.daemon().asked("thread.list").is_empty());

    let mut rig = rig_with_threads();
    rig.runner.app.thread = Some("pr".into());
    rig.asked("thread.list", 1).await;
    rig.shows("bh-ai pr · active · 41K ctx · warm").await;
    // Narrower: the state goes first.
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("thread.list".into(), Ok(threads()));
    let mut narrow = Rig::new(50, 24, script(world));
    narrow.runner.app.thread = Some("pr".into());
    narrow.asked("thread.list", 1).await;
    narrow.shows("bh-ai pr · 41K ctx · warm").await;
    assert!(
        !narrow.screen()[0].contains("active"),
        "{}",
        narrow.screen()[0]
    );
}

#[tokio::test]
async fn ctrl_t_opens_the_picker_and_typing_moving_and_closing_ask_nothing() {
    let mut rig = rig_with_threads();
    rig.shows("spec review").await;
    rig.key(ctrl_t()).await;
    rig.asked("thread.list", 1).await;
    rig.shows("Tide log rewrite").await;
    let screen = rig.screen().join("\n");
    assert!(
        screen.contains(" 1   bh-ai pr · active · 14:12 · 41K ctx · warm"),
        "{screen}"
    );
    assert!(
        screen.contains("Tide log rewrite · surveyor · active") && screen.contains("cold"),
        "{screen}"
    );
    assert!(screen.contains("Moon table"), "{screen}");
    assert!(
        !screen.contains("Old notes"),
        "paused ones wait behind `a`: {screen}"
    );
    // Down, up, a filter's letters, backspace and esc: no request.
    rig.press(&[KeyCode::Down, KeyCode::Up, KeyCode::Down])
        .await;
    rig.type_text("/moon").await;
    rig.shows("1 shown").await;
    let filtered = rig.screen().join("\n");
    assert!(
        filtered.contains("Moon table") && !filtered.contains("bh-ai pr ·"),
        "{filtered}"
    );
    rig.press(&[KeyCode::Backspace, KeyCode::Esc]).await;
    rig.settle().await;
    assert_eq!(
        rig.daemon().asked("thread.list").len(),
        1,
        "no request for a key"
    );
    assert!(
        !rig.screen().join("\n").contains("threads"),
        "esc closes it"
    );
}

#[tokio::test]
async fn a_command_letter_filters_after_a_slash_and_a_toggle_shows_the_paused() {
    let mut rig = rig_with_threads();
    rig.shows("spec review").await;
    rig.key(ctrl_t()).await;
    rig.shows("Tide log rewrite").await;
    // `a` shows the paused one, and asks for the finished ones once.
    rig.press(&[KeyCode::Char('a')]).await;
    rig.shows("Old notes").await;
    rig.asked("thread.list", 2).await;
    assert_eq!(
        rig.daemon().asked("thread.list")[1],
        json!({"state": "done"})
    );
    rig.press(&[KeyCode::Char('a')]).await;
    // `/pr` filters by the letters `p` and `r`, which are commands without it.
    rig.type_text("/pr").await;
    rig.shows("1 shown").await;
    assert!(rig.daemon().asked("thread.pause").is_empty());
}

#[tokio::test]
async fn enter_switches_remembers_and_focuses_the_threads_session() {
    let mut rig = rig_with_threads();
    rig.runner.app.thread_file = Some(("/nonexistent/thread.json".into(), "tty7".into()));
    rig.shows("spec review").await;
    rig.key(ctrl_t()).await;
    rig.shows("Tide log rewrite").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.shows("Low water is at 14:10.").await;
    assert_eq!(rig.runner.app.thread.as_deref(), Some("pr"));
    // Nothing was sent to a model: a switch is the client's.
    assert!(rig.daemon().asked("turn.submit").is_empty());
    // The session's header carries the thread's tag, and the status line its state.
    let screen = rig.screen();
    assert!(
        screen.iter().any(|l| l.contains("→ bh-ai pr")),
        "{screen:#?}"
    );
    assert!(
        screen[0].contains("bh-ai pr · active · 41K ctx · warm"),
        "{}",
        screen[0]
    );
}

#[tokio::test]
async fn p_r_and_d_act_and_d_asks_twice() {
    let mut rig = rig_with_threads();
    rig.shows("spec review").await;
    rig.key(ctrl_t()).await;
    rig.shows("Tide log rewrite").await;
    rig.press(&[KeyCode::Char('p')]).await;
    rig.asked("thread.pause", 1).await;
    assert_eq!(
        rig.daemon().asked("thread.pause")[0],
        json!({"slug": "pr", "author": "the TUI"})
    );
    // The list is read again after an act.
    rig.asked("thread.list", 2).await;
    rig.press(&[KeyCode::Char('r')]).await;
    rig.asked("thread.resume", 1).await;
    // `d` once asks; any other key withdraws; `d` twice finishes.
    rig.press(&[KeyCode::Char('d')]).await;
    rig.shows("finish pr? d again").await;
    rig.settle().await;
    assert!(rig.daemon().asked("thread.done").is_empty());
    rig.press(&[KeyCode::Down]).await;
    rig.press(&[KeyCode::Char('d')]).await;
    rig.press(&[KeyCode::Char('d')]).await;
    rig.asked("thread.done", 1).await;
    assert_eq!(
        rig.daemon().asked("thread.done")[0],
        json!({"slug": "tides-log", "author": "the TUI"})
    );
}

#[tokio::test]
async fn slash_t_in_the_input_line_lists_picks_by_number_and_goes_home() {
    let mut rig = rig_with_threads();
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("/t").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.shows("Tide log rewrite").await;
    rig.press(&[KeyCode::Esc]).await;
    // `/t 1` picks the first; nothing is sent to the session as a message.
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("/t 1").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.settle().await;
    assert_eq!(rig.runner.app.thread.as_deref(), Some("pr"));
    assert!(rig.daemon().asked("turn.submit").is_empty());
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("/t tides-l").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.settle().await;
    assert_eq!(rig.runner.app.thread.as_deref(), Some("tides-log"));
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("/t home").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.settle().await;
    assert_eq!(rig.runner.app.thread, None);
    assert!(rig.screen()[0].contains("home"), "{}", rig.screen()[0]);
    // A name that is no thread is said, and nothing changes.
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("/t zzzz").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.shows("no thread is named").await;
}

#[tokio::test]
async fn a_thread_with_no_session_opens_one_with_the_first_message() {
    let mut rig = rig_with_threads();
    rig.shows("spec review").await;
    rig.key(ctrl_t()).await;
    rig.shows("Moon table").await;
    rig.type_text("/moon").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.shows("no session yet").await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("when is the next new moon").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    assert_eq!(
        rig.daemon().asked("turn.submit")[0],
        json!({"thread": "moons", "input": "when is the next new moon", "author": "the TUI"})
    );
}
