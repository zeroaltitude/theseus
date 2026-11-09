//! The sidebar's questions are the whole daemon's (theseus-7zs0f): `confirm.list` answers for every
//! session, and the board keeps a question whichever turn asked it. The chat's `ours_in` filter
//! belongs to one conversation's own transcript; applied here it would hide a question the owner
//! must answer.

use serde_json::json;
use theseus_protocol::utc_hm;

use crate::app::{App, Purpose};
use crate::board::Question;
use crate::tests::{harbour, question};

#[test]
fn a_question_of_another_turn_in_a_placed_session_still_shows() {
    theseus_client::render::time::pin_for_tests();
    let mut app = App::new(utc_hm);
    app.answered(Purpose::Snapshot, Ok(harbour().0));
    let mut q = question("ses_dm0001", "cor_other01", "run the gate", 500);
    q.turn_id = Some("turn_from_discord".into());
    app.answered(Purpose::Questions, Ok(json!({"confirms": [q]})));
    let shown = app.board.questions("ses_dm0001");
    assert!(
        shown
            .iter()
            .any(|s| matches!(s, Question::Whole(c) if c.correlation_id == "cor_other01")),
        "the sidebar lists every turn's question"
    );
}

/// What the card says of a call's question that has waited past the life it
/// would have had (theseus-lrc3m, review C's F2): it is held, not expired,
/// and `y` still runs the call. `expires_at_ms` on the live request is the
/// first notice's time, not a deadline.
fn card_text(q: Question<'_>, now_ms: u64) -> String {
    crate::card::lines(q, now_ms, None, 0)
        .into_iter()
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_held_calls_card_says_how_long_it_waited_not_that_it_expired() {
    let mut q = question("ses_dm0001", "cor_held01", "run the gate", 0);
    q.expires_at_ms = q.requested_at_ms + 15 * 60_000;
    let text = card_text(Question::Whole(&q), q.requested_at_ms + 20 * 60_000);
    assert!(
        !text.contains("expire"),
        "a held question has no deadline: {text}"
    );
    assert!(text.contains("waiting 20 min"), "{text}");
    q.floor = true;
    let text = card_text(Question::Whole(&q), q.requested_at_ms + 20 * 60_000);
    assert!(text.contains("waiting 20 min · FLOOR"), "{text}");
}

/// The whole question and the brief a view carries say the same of a held
/// call: neither names a deadline, whichever the TUI has received.
#[test]
fn the_whole_question_and_its_brief_agree_that_a_held_call_has_no_deadline() {
    let mut q = question("ses_dm0001", "cor_held01", "run the gate", 0);
    q.expires_at_ms = q.requested_at_ms + 15 * 60_000;
    let brief: theseus_protocol::PendingConfirm = serde_json::from_value(json!({
        "correlation_id": "cor_held01", "tool": "proc.run", "reason": "run the gate",
        "floor": false, "budget": false, "expires_at_ms": 0,
    }))
    .unwrap();
    let now = q.requested_at_ms + 20 * 60_000;
    for text in [
        card_text(Question::Whole(&q), now),
        card_text(Question::Brief(&brief), now),
    ] {
        assert!(!text.contains("expire"), "{text}");
    }
}

/// A question that does expire still counts down, and says so when it has.
#[test]
fn a_question_that_expires_still_counts_down_and_says_expired() {
    let mut q = question("ses_dm0001", "cor_ack01", "grant the extension", 0);
    q.tool = "extend.ack".into();
    q.expires_at_ms = q.requested_at_ms + 15 * 60_000;
    let text = card_text(Question::Whole(&q), q.requested_at_ms + 60_000);
    assert!(text.contains("expires in 14:00"), "{text}");
    let text = card_text(Question::Whole(&q), q.requested_at_ms + 20 * 60_000);
    assert!(text.contains("expired"), "{text}");
}

/// A held card repaints when its label turns over: each second for its first
/// minute, then each minute; none for a question with a deadline of its own.
#[test]
fn a_held_cards_label_has_a_next_change_and_an_expiring_ones_has_none() {
    let mut q = question("ses_dm0001", "cor_held01", "run the gate", 0);
    let t = q.requested_at_ms;
    assert_eq!(crate::held::next_change(&q, t + 400), Some(t + 1_000));
    assert_eq!(crate::held::next_change(&q, t + 61_000), Some(t + 120_000));
    assert_eq!(
        crate::held::next_change(&q, t + 3_700_000),
        Some(t + 7_200_000)
    );
    q.tool = "pack.promote".into();
    assert_eq!(crate::held::next_change(&q, t + 400), None);
}
