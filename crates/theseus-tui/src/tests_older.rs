//! The pane reads a session's newest 200 nodes, and the next older page when
//! the scroll reaches the start of what it holds (theseus-kym3): the protocol
//! pages both ways, and the pane used to page one.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use theseus_protocol::utc_hm;

use crate::app::{App, Effect, Purpose};
use crate::tests::{dm_history, T0};

const SID: &str = "ses_dm0001";

/// A page of `n` operator messages, positions `from..from + n`.
fn page(from: u64, n: u64, older: Option<u64>) -> Value {
    let mut h = dm_history();
    h["nodes"] = (from..from + n)
        .map(|p| {
            json!({
                "node_id": format!("nod_{p}"), "kind": "user_message", "session_id": SID,
                "position": p, "at_unix_ms": T0 + p * 1000, "author": "discord:wren",
                "text": format!("message {p}"), "detail": {}, "bytes": 10,
            })
        })
        .collect();
    if let Some(o) = older {
        h["older"] = json!(o);
    }
    h
}

fn focused(first: Value) -> App {
    theseus_client::render::time::pin_for_tests();
    let mut app = App::new(utc_hm);
    app.resized(120, 30);
    app.focus(SID);
    app.answered(Purpose::History(SID.into()), Ok(first));
    app
}

fn lines(app: &App) -> Vec<String> {
    let d = app.detail.as_ref().expect("a pane");
    d.lines().iter().map(|(_, t)| t.clone()).collect()
}

fn page_up(app: &mut App) -> Vec<Effect> {
    app.key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
}

/// The older-page read among `effects`, if one was asked.
fn asked(effects: Vec<Effect>) -> Option<(Value, Purpose)> {
    effects.into_iter().find_map(|e| match e {
        Effect::Call(c) if c.method == "session.history" => Some((c.params, c.purpose)),
        _ => None,
    })
}

#[test]
fn a_scroll_to_the_start_asks_for_the_next_older_page() {
    let mut app = focused(page(100, 40, Some(100)));
    let mut got = None;
    for _ in 0..12 {
        got = got.or(asked(page_up(&mut app)));
    }
    let (params, purpose) = got.expect("the start was reached, so the older page is asked for");
    assert_eq!(params["session_id"], SID);
    assert_eq!(
        params["before"], 100,
        "before the first node held, as the protocol says"
    );
    assert_eq!(params["n"], 200);
    assert_eq!(purpose, Purpose::Older(SID.into()));
}

#[test]
fn a_scroll_that_is_not_at_the_start_asks_for_nothing() {
    let mut app = focused(page(100, 200, Some(100)));
    assert!(
        asked(page_up(&mut app)).is_none(),
        "a screen up from the end is far from the start"
    );
}

#[test]
fn a_session_with_nothing_older_never_asks() {
    let mut app = focused(page(100, 5, None));
    for _ in 0..4 {
        assert!(asked(page_up(&mut app)).is_none());
    }
}

#[test]
fn the_older_page_goes_above_and_the_view_stays_where_it_was() {
    let mut app = focused(page(100, 40, Some(100)));
    let before = lines(&app);
    for _ in 0..12 {
        page_up(&mut app);
    }
    let scroll = app.detail.as_ref().unwrap().scroll;
    app.answered(Purpose::Older(SID.into()), Ok(page(60, 40, Some(60))));
    let after = lines(&app);
    assert_eq!(after.len(), before.len() + 40, "the page's nodes are held");
    assert_eq!(
        &after[40..],
        &before[..],
        "what was held is unmoved, below the page"
    );
    assert!(
        after[0].contains("message 60"),
        "the oldest node leads: {:?}",
        after[0]
    );
    assert_eq!(
        app.detail.as_ref().unwrap().scroll,
        scroll,
        "rows are counted from the end, so the view holds"
    );
    let (params, _) = asked(page_up(&mut app)).expect("the page after that is asked for next");
    assert_eq!(params["before"], 60);
}

#[test]
fn one_older_read_is_out_at_a_time() {
    let mut app = focused(page(100, 40, Some(100)));
    let mut asks = 0;
    for _ in 0..20 {
        asks += usize::from(asked(page_up(&mut app)).is_some());
    }
    assert_eq!(
        asks, 1,
        "the answer has not come, so the next key asks nothing"
    );
}

#[test]
fn a_failed_older_read_lets_the_next_scroll_ask_again() {
    let mut app = focused(page(100, 40, Some(100)));
    let mut asks = 0;
    for _ in 0..12 {
        asks += usize::from(asked(page_up(&mut app)).is_some());
    }
    assert_eq!(asks, 1);
    let err = theseus_protocol::RpcError {
        code: -32000,
        message: "no".into(),
        data: json!({}),
    };
    app.answered(Purpose::Older(SID.into()), Err(err));
    assert!(
        asked(page_up(&mut app)).is_some(),
        "a read that failed is not a read that is out"
    );
}

/// A page nobody is waiting for (the pane was read again after it was asked
/// for) would be spliced above a gap; it is dropped.
#[test]
fn an_older_page_nobody_asked_for_is_dropped() {
    let mut app = focused(page(100, 40, Some(100)));
    let before = lines(&app);
    app.answered(Purpose::Older(SID.into()), Ok(page(60, 40, Some(60))));
    assert_eq!(lines(&app), before, "no read was out");
    // Asked, then the pane is read again, and the old answer comes after.
    for _ in 0..12 {
        page_up(&mut app);
    }
    app.answered(Purpose::History(SID.into()), Ok(page(100, 40, Some(100))));
    app.answered(Purpose::Older(SID.into()), Ok(page(60, 40, Some(60))));
    assert_eq!(lines(&app), before, "the late page was not spliced in");
}
