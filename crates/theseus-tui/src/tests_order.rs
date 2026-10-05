//! Another surface's message in the detail pane keeps its place (theseus-v6yc):
//! its `node.written` marks where it goes, and the node read for it lands
//! there, above the reply that streamed while the read was out. Each test
//! drives the app alone, the order forced by hand.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use theseus_protocol::utc_hm;

use crate::app::{App, Effect, Purpose};
use crate::tests::{dm_history, T0};

const SID: &str = "ses_dm0001";

/// An app focused on the DM, its history read.
fn focused() -> App {
    let mut app = App::new(utc_hm);
    app.focus(SID);
    app.answered(Purpose::History(SID.into()), Ok(dm_history()));
    app
}

/// The pane's lines, as text.
fn lines(app: &App) -> Vec<String> {
    let d = app.detail.as_ref().expect("a pane");
    d.lines().iter().map(|(_, t)| t.clone()).collect()
}

/// Another surface's message `id` was written: the read the app asks for.
fn written(app: &mut App, id: &str) -> Purpose {
    let out = app.notified(
        "node.written",
        &json!({"session_id": SID, "node_id": id, "kind": "user_message"}),
    );
    out.into_iter()
        .find_map(|e| match e {
            Effect::Call(c) if c.method == "session.history" => Some(c.purpose),
            _ => None,
        })
        .expect("the node is read")
}

/// The history's last five, with the message `id` from Discord last.
fn read_with(id: &str, text: &str) -> Value {
    let mut h = dm_history();
    h["nodes"].as_array_mut().unwrap().push(json!({
        "node_id": id, "kind": "user_message", "session_id": SID, "position": 20,
        "at_unix_ms": T0 + 20_000, "author": "discord:wren", "text": text, "detail": {},
        "bytes": text.len(),
    }));
    h
}

/// The turn that answers, streaming `pieces`.
fn reply(app: &mut App, turn: &str, pieces: &[&str]) {
    app.notified(
        "turn.started",
        &json!({"session_id": SID, "turn_id": turn, "execution_id": "exe_ses_dm0001"}),
    );
    for piece in pieces {
        app.notified(
            "model.delta",
            &json!({"turn_id": turn, "loop_index": 0, "text": piece}),
        );
    }
}

const ASKED: &str = "[14:13:40.000Z] operator (discord:wren): and the next high water?";

/// The message's line shows above the reply that streamed while it was read,
/// and the reply's open line goes on below it.
#[test]
fn another_surfaces_message_shows_above_the_reply_that_streamed_meanwhile() {
    let mut app = focused();
    let before = lines(&app).len();
    let read = written(&mut app, "nod_c1");
    reply(
        &mut app,
        "turn_c2",
        &["High water", " is at 20:30.\nThen", " slack"],
    );
    app.answered(read, Ok(read_with("nod_c1", "and the next high water?")));
    app.notified(
        "model.delta",
        &json!({"turn_id": "turn_c2", "loop_index": 0, "text": " water."}),
    );
    let got = lines(&app);
    assert_eq!(
        got[before..],
        [
            ASKED,
            "── turn turn_c2",
            "High water is at 20:30.",
            "Then slack water.",
        ],
        "{got:#?}"
    );
}

/// Two messages read out of order keep the order their `node.written` came in.
#[test]
fn two_messages_keep_the_order_they_were_written_in() {
    let mut app = focused();
    let before = lines(&app).len();
    let first = written(&mut app, "nod_c1");
    let second = written(&mut app, "nod_c3");
    reply(&mut app, "turn_c2", &["Noted."]);
    app.answered(second, Ok(read_with("nod_c3", "and the moon?")));
    app.answered(first, Ok(read_with("nod_c1", "and the next high water?")));
    let got = lines(&app);
    assert_eq!(
        got[before..],
        [
            ASKED,
            "[14:13:40.000Z] operator (discord:wren): and the moon?",
            "── turn turn_c2",
            "Noted.",
        ],
        "{got:#?}"
    );
}

/// A read that does not find the node, or fails, leaves nothing behind: a
/// later message goes in its own place.
#[test]
fn a_read_that_finds_no_node_leaves_nothing_behind() {
    let mut app = focused();
    let shown = lines(&app);
    let lost = written(&mut app, "nod_gone");
    let failed = written(&mut app, "nod_fail");
    app.answered(lost, Ok(dm_history()));
    app.answered(
        failed,
        Err(theseus_protocol::RpcError {
            code: -32000,
            message: "the store is busy".into(),
            data: Value::Null,
        }),
    );
    assert_eq!(lines(&app), shown);
    let read = written(&mut app, "nod_c1");
    reply(&mut app, "turn_c2", &["Noted."]);
    app.answered(read, Ok(read_with("nod_c1", "and the next high water?")));
    let got = lines(&app);
    assert_eq!(
        got[shown.len()..],
        [ASKED, "── turn turn_c2", "Noted."],
        "{got:#?}"
    );
}

/// The oldest lines going past the pane's 5,000 move the place with them; a
/// place that went itself puts the message at the end.
#[test]
fn a_place_moves_with_the_dropped_lines_or_goes_to_the_end() {
    let mut app = focused();
    let before = lines(&app).len();
    let read = written(&mut app, "nod_c1");
    // The turn's line and 4,995 of the reply's: five over the pane's 5,000
    // with the history's lines, so five of those go.
    let long = vec!["x"; 4_995].join("\n");
    reply(&mut app, "turn_c2", &[&long]);
    assert_eq!(lines(&app).len(), 5_000);
    app.answered(read, Ok(read_with("nod_c1", "and the next high water?")));
    let got = lines(&app);
    // The reply dropped five of the history's lines, and the message's own
    // line one more.
    let dropped = before + 1 + 4_995 - 5_000 + 1;
    let at = before - dropped;
    assert_eq!(got[at - 1], lines(&focused())[before - 1]);
    assert_eq!(got[at], ASKED, "{:#?}", &got[..at + 2]);
    assert_eq!(got[at + 1], "── turn turn_c2");
    assert_eq!(got.len(), 5_000);

    let mut app = focused();
    let read = written(&mut app, "nod_c1");
    let long = vec!["y"; 5_100].join("\n");
    reply(&mut app, "turn_c2", &[&long]);
    app.answered(read, Ok(read_with("nod_c1", "and the next high water?")));
    let got = lines(&app);
    assert_eq!(got.last().map(String::as_str), Some(ASKED));
    assert_eq!(got[got.len() - 2], "y");
}

/// The TUI's own message shows at once, in order, and is not read again.
#[test]
fn the_tuis_own_message_shows_at_once_as_before() {
    let mut app = focused();
    let before = lines(&app).len();
    let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
    app.key(key(KeyCode::Char('i')));
    for c in "and tomorrow?".chars() {
        app.key(key(KeyCode::Char(c)));
    }
    let sent = app.key(key(KeyCode::Enter));
    assert!(
        sent.iter()
            .any(|e| matches!(e, Effect::Call(c) if c.method == "turn.submit")),
        "{sent:?}"
    );
    let out = app.notified(
        "node.written",
        &json!({"session_id": SID, "node_id": "nod_b1", "kind": "user_message"}),
    );
    assert!(out.is_empty(), "its own message is not read again: {out:?}");
    reply(&mut app, "turn_b2", &["Tomorrow at 15:02."]);
    let got = lines(&app);
    assert_eq!(
        got[before..],
        [
            "you: and tomorrow?",
            "── turn turn_b2",
            "Tomorrow at 15:02."
        ],
        "{got:#?}"
    );
}
