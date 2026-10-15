//! Rewind in the chat (theseus-23rk, R20): Esc Esc on an empty line while no
//! turn runs opens the picker; Enter chooses a message; `f`, `c` and `o`
//! restore the files and fork, fork alone, or restore alone; Esc never minds.
//! A single Esc during a turn is chat-ask's (the stop), so it opens nothing.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;

use super::tests::{Rig, SID, T0};
use crate::tests::{Answer, Script};

const FORK: &str = "ses_fedcba9876543210";
const SECOND: &str = "msg_00000000000000000000000000000052";
const FIRST: &str = "msg_00000000000000000000000000000031";

fn script() -> Script {
    let base = super::tests::script();
    Arc::new(move |m, p| -> Answer {
        Ok(match m {
            "files.changes" => json!({
                "session_id": SID,
                "note": "only files Theseus's own edit tools changed are restored",
                "points": [
                    {"node_id": SECOND, "position": 52, "at_ms": T0 + 5_000,
                     "text": "add a comment line at the top of tides.py",
                     "message": "add a comment line at the top of tides.py",
                     "files": [{"path": "/work/tides.py", "added": 1, "removed": 1}],
                     "commands": 0, "fork_at": "msg_00000000000000000000000000000041"},
                    {"node_id": FIRST, "position": 31, "at_ms": T0 + 1_000,
                     "text": "what does this project do?", "message": "what does this project do?",
                     "files": [], "commands": 0}
                ]
            }),
            "files.restore" => json!({
                "session_id": SID, "since_node": SECOND, "since_position": 52,
                "dry_run": false, "commands": 0, "undo_since": 80,
                "files": [{"path": "/work/tides.py", "outcome": "restored"}],
                "words": "Restored 1 file"
            }),
            "session.fork" => json!({
                "session_id": FORK, "title": "tides (fork)", "source_session": SID,
                "at_node": "msg_00000000000000000000000000000041", "at_position": 41,
                "compilation_id": "cmp_1", "bytes": 300, "ms": 3, "not_restored": "x"
            }),
            _ => return base(m, p),
        })
    })
}

async fn picker() -> Rig {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.press(KeyCode::Esc).await;
    r.shows("esc again to rewind").await;
    r.press(KeyCode::Esc).await;
    r.shows("Rewind to before which message?").await;
    r
}

#[tokio::test(start_paused = true)]
async fn esc_esc_on_an_empty_idle_line_lists_each_message_with_its_file_changes() {
    let r = picker().await;
    assert_eq!(r.daemon.asked("files.changes")[0]["session_id"], SID);
    let text = r.text();
    assert!(
        text.contains("add a comment line at the top of tides.py"),
        "{text}"
    );
    assert!(text.contains("tides.py +1 −1"), "{text}");
    assert!(text.contains("no file changes"), "{text}");
    assert!(text.contains("esc never mind"), "{text}");
}

#[tokio::test(start_paused = true)]
async fn a_single_esc_opens_nothing_and_a_key_between_resets_it() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Left).await;
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(r.daemon.asked("files.changes").is_empty());
    // Text in the line: Esc is not the rewind's.
    r.type_text("hel").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(r.daemon.asked("files.changes").is_empty());
}

#[tokio::test(start_paused = true)]
async fn esc_during_a_turn_is_the_stops_and_is_not_bound() {
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.type_text("go").await;
    r.press(KeyCode::Enter).await;
    r.starts_turn("turn_1");
    r.settle().await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(r.daemon.asked("files.changes").is_empty());
    assert!(!r.text().contains("Rewind to before"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn f_restores_the_files_then_forks_and_puts_the_message_back() {
    let mut r = picker().await;
    r.press(KeyCode::Enter).await;
    r.shows("f files and conversation").await;
    r.type_text("f").await;
    r.asked("files.restore", 1).await;
    assert_eq!(r.daemon.asked("files.restore")[0]["since_node"], SECOND);
    r.asked("session.fork", 1).await;
    let fork = &r.daemon.asked("session.fork")[0];
    assert_eq!(fork["session"], SID);
    assert_eq!(fork["at"], "msg_00000000000000000000000000000041");
    // The chat goes on in the fork, with the message to edit.
    r.asked("session.watch", 2).await;
    assert_eq!(r.daemon.asked("session.watch")[1]["session_id"], FORK);
    r.asked("session.history", 1).await;
    assert_eq!(r.daemon.asked("session.history")[0]["session_id"], FORK);
    r.shows("add a comment line at the top of tides.py").await;
    assert!(!r.text().contains("Rewind to before"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn c_forks_without_restoring_and_o_restores_without_forking() {
    let mut r = picker().await;
    r.press(KeyCode::Enter).await;
    r.type_text("c").await;
    r.asked("session.fork", 1).await;
    r.settle().await;
    assert!(r.daemon.asked("files.restore").is_empty());

    let mut r = picker().await;
    r.press(KeyCode::Enter).await;
    r.type_text("o").await;
    r.asked("files.restore", 1).await;
    r.settle().await;
    assert!(r.daemon.asked("session.fork").is_empty());
    r.shows("Restored 1 file").await;
}

#[tokio::test(start_paused = true)]
async fn esc_never_minds_and_the_first_message_cannot_fork() {
    let mut r = picker().await;
    r.press(KeyCode::Esc).await;
    r.settle().await;
    assert!(!r.text().contains("Rewind to before"), "{}", r.text());
    assert!(r.daemon.asked("files.restore").is_empty());
    assert!(r.daemon.asked("session.fork").is_empty());

    // The first message has nothing before it: conversation modes say so.
    let mut r = picker().await;
    r.press(KeyCode::Down).await;
    r.press(KeyCode::Enter).await;
    r.type_text("f").await;
    r.shows("nothing before it").await;
    assert!(r.daemon.asked("files.restore").is_empty());
}

#[tokio::test(start_paused = true)]
async fn two_escapes_read_at_once_are_one_alt_esc_and_open_the_picker() {
    use crossterm::event::{KeyEvent, KeyModifiers};
    let mut r = Rig::with(80, 24, script()).await;
    r.ready().await;
    r.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT)).await;
    r.shows("Rewind to before which message?").await;
    // Both at once close it again, as one Esc would.
    r.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT)).await;
    r.settle().await;
    assert!(!r.text().contains("Rewind to before"), "{}", r.text());
}
