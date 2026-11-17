//! A prompt file's card as Discord draws it (issue 23): the daemon's own text,
//! whole, in a message that fits, and never through the ordinary approval card
//! that clips a reason in the middle.

use serde_json::{json, Value};
use theseus_core::config_change::prompt_file::card::{question, Facts, CARD_CHARS};
use theseus_core::config_change::shown;
use theseus_protocol::ConfirmRequest;

use crate::render::{card, Route};

/// The card the daemon builds for a change of `before` to `after`, and whether
/// its changed lines are on it.
fn daemon_card(before: &str, after: &str) -> (String, bool) {
    let (b, a) = (shown::facts(before), shown::facts(after));
    let d = shown::diff(before, after);
    question(&Facts {
        path: "/state/personas/warm/PERSONA.md",
        who: "file of persona warm",
        public: false,
        before: &b,
        after: &a,
        diff: &d,
        outside_text: true,
        steered: false,
        reason: "you asked for it to be warmer",
        preface: None,
        undoing: None,
    })
}

fn request(reason: &str, input: Value) -> ConfirmRequest {
    serde_json::from_value(json!({
        "correlation_id": "act_1", "session_id": "ses_a", "execution_id": "exe_a",
        "tool": "config.ack", "input": input, "reason": reason, "by": "operator",
        "requested_at_ms": 1, "expires_at_ms": 1_790_000_000_000_u64,
        "needs_proof": true,
    }))
    .unwrap()
}

fn lines(n: usize) -> String {
    (1..=n)
        .map(|i| format!("Rule {i}: be careful.\n"))
        .collect()
}

/// Every character of the daemon's card, the changed lines included, is in
/// the message: nothing is clipped and nothing is run together.
#[test]
fn the_card_is_drawn_whole() {
    let before = lines(80);
    let after = before.replace(
        "Rule 40: be careful.",
        "Rule 40: be kind & careful. *Always*.",
    );
    let (text, whole) = daemon_card(&before, &after);
    assert!(whole);
    let c = card(
        &request(&text, json!({"whole": true, "kind": "prompt_file"})),
        &Route::Here,
        "",
    );
    assert!(c.content.contains(&text), "{}", c.content);
    assert!(c
        .content
        .contains("\n-Rule 40: be careful.\n+Rule 40: be kind & careful. *Always*.\n"));
    assert!(!c.content.contains('…'), "{}", c.content);
    assert!(
        c.content.contains("**Second proof:**"),
        "the buttons' form is explained"
    );
    assert!(
        c.content.chars().count() <= crate::render::PART_LIMIT,
        "{}",
        c.content.chars().count()
    );
    assert!(
        c.line.contains("a change to Theseus's own text"),
        "{}",
        c.line
    );
}

/// The biggest card the daemon builds still fits a message, with its heading,
/// the second proof's clause and the footer.
#[test]
fn the_biggest_card_still_fits_a_message() {
    let before = lines(300);
    let mut biggest = 0;
    for n in 1..40 {
        let mut after = before.clone();
        for i in 0..n {
            let k = 5 + i * 7;
            after = after.replace(
                &format!("Rule {k}: "),
                &format!("Rule {k} (changed, and a good deal longer): "),
            );
        }
        let (text, whole) = daemon_card(&before, &after);
        assert!(
            text.chars().count() <= CARD_CHARS + 120,
            "{n}: {}",
            text.chars().count()
        );
        let c = card(&request(&text, json!({"whole": true})), &Route::Here, "");
        assert!(
            c.content.chars().count() <= crate::render::PART_LIMIT,
            "{n}: {}",
            c.content.chars().count()
        );
        assert!(c.content.contains(&text), "{n}");
        if whole {
            biggest = n;
        }
    }
    assert!(biggest >= 3, "a few changed lines fit: {biggest}");
}

/// A card that is not the size the daemon builds is not drawn cut.
#[test]
fn a_card_too_long_to_show_says_where_to_read_it() {
    let long = "x".repeat(CARD_CHARS + 1);
    let c = card(&request(&long, json!({"whole": true})), &Route::Here, "");
    assert!(!c.content.contains("xxxx"), "{}", c.content);
    assert!(c.content.contains("theseus config show") && c.content.contains("not shown cut"));
}

/// Another settings question keeps the card it had.
#[test]
fn an_ordinary_settings_card_is_not_drawn_in_this_form() {
    let c = card(
        &request(
            "Change 1 setting?\n- narrative: off → on (at once)\n",
            json!({}),
        ),
        &Route::Here,
        "",
    );
    assert!(
        !c.content.contains("a change to Theseus's own text"),
        "{}",
        c.content
    );
}
