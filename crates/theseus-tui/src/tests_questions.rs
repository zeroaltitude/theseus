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

/// The gap review G's G1 names, pinned so the next reader finds it rather
/// than rediscovering it. A call's question raised inside a **routine's run**
/// carries a real deadline — `ask_ttl_ms` gives it `[routines]
/// run_deadline_secs` — and at it `expire_questions` drops the call and
/// returns the run to its schedule, through `queue_if_routine`, before the
/// held test is ever reached. The card cannot tell: `ConfirmRequest` has no
/// field marking a routine run, so the kind test holds the question, throws
/// the deadline away, and goes on counting up after the work is gone.
/// Closing this needs F3's `expires` stamp on the Action, a store-format
/// change and so post-1.0.4; until then the proxy is wrong here and this
/// test says so out loud. Make `deadline_of` honour `expires_at_ms` and this
/// is the test that fails.
#[test]
fn a_routine_runs_question_says_waiting_while_the_run_deadline_drops_it() {
    // `[routines] run_deadline_secs = 600`: the run plans `proc.run`, parks.
    let mut q = question("ses_task0001", "cor_run0001", "run the gate", 0);
    let planned = q.requested_at_ms;
    q.expires_at_ms = planned + 600_000;

    // The deadline is real, and the daemon will act on it.
    assert!(
        q.expires_at_ms > planned,
        "a routine run's question carries its run deadline"
    );
    // The card cannot see it: the kind test alone decides, and it says held.
    assert!(crate::held::is_held(&q), "a proc.run reads as held");
    assert_eq!(
        crate::held::deadline_of(&q),
        0,
        "the run deadline is discarded, so no card counts down to it"
    );

    // Nine minutes in, the owner steps away on the card's word.
    let text = card_text(Question::Whole(&q), planned + 9 * 60_000);
    assert!(text.contains("waiting 9 min"), "{text}");
    assert!(!text.contains("expire"), "{text}");

    // Eleven minutes in the call has been dropped and the run requeued, and
    // the card still only counts up. That is the harm, pinned.
    let text = card_text(Question::Whole(&q), planned + 11 * 60_000);
    assert!(text.contains("waiting 11 min"), "{text}");
    assert!(!text.contains("expire"), "{text}");
}
