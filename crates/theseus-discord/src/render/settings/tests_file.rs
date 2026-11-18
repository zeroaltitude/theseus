//! A change to a prompt file, and a setting too long for the ordinary card
//! (issue 23), drawn by the settings form: the daemon's own card whole, its
//! block of changed lines as it is, everything else as text (issue 22's rules).

use serde_json::json;
use theseus_core::config_change::prompt_file::card::{question, Facts};
use theseus_core::config_change::{shown, words};
use theseus_protocol::ConfirmRequest;

use super::*;
use crate::render::{self, Route, PART_LIMIT};

fn lines(n: usize) -> String {
    (1..=n)
        .map(|i| format!("Rule {i}: be careful.\n"))
        .collect()
}

/// The card the daemon builds for a change of `before` to `after` of `path`,
/// and the request that carries it, worded as the daemon words it.
fn change(path: &str, reason: &str, before: &str, after: &str) -> (ConfirmRequest, bool) {
    let (b, a) = (shown::facts(before), shown::facts(after));
    let d = shown::diff(before, after);
    let (text, whole) = question(&Facts {
        path,
        who: "file of persona warm",
        public: false,
        before: &b,
        after: &a,
        diff: &d,
        outside_text: true,
        steered: false,
        reason,
        preface: None,
        undoing: None,
    });
    let args = json!({
        "question": text, "kind": "prompt_file", "whole": true,
        "settled": words::file(path, d.added, d.removed, None),
    });
    let req = ConfirmRequest {
        correlation_id: "act_0000feed".into(),
        session_id: "ses_00000000harbour".into(),
        tool: "config.ack".into(),
        reason: text,
        input: args.clone(),
        plain: words::plain_of(&args),
        requested_at_ms: 1_699_999_700_000,
        expires_at_ms: 1_700_000_000_000,
        needs_proof: true,
        ..Default::default()
    };
    (req, whole)
}

/// Every character of the daemon's card, the changed lines in their block as
/// they are, is in one message; the second proof's clause is there; the line it
/// settles into says what it changed.
#[test]
fn the_card_is_drawn_whole_and_its_changed_lines_as_they_are() {
    let before = lines(80);
    let after = before.replace(
        "Rule 40: be careful.",
        "Rule 40: be kind & careful. *Always* use_snake_case.",
    );
    let (req, whole) = change("/state/personas/warm/PERSONA.md", "warmer", &before, &after);
    assert!(whole);
    let c = render::card(&req, &Route::Here, "");
    assert!(c.more.is_empty(), "one message");
    assert!(
        c.content.contains(
            "\n-Rule 40: be careful.\n+Rule 40: be kind & careful. *Always* use_snake_case.\n"
        ),
        "{}",
        c.content
    );
    assert!(c.content.contains("```diff\n") && c.content.contains("\n```\n"));
    assert!(!c.content.contains('…'), "{}", c.content);
    assert!(c.content.contains("**Second proof:**"), "{}", c.content);
    assert!(
        c.content.chars().count() <= PART_LIMIT,
        "{}",
        c.content.chars().count()
    );
    assert!(c.line.contains("change the prompt file"), "{}", c.line);
    assert!(
        !c.content.contains("config.ack"),
        "no tool name: {}",
        c.content
    );
}

/// The path and the model's reason are text and not markdown: an underline
/// pair in the path and emphasis in the reason are drawn as the characters they
/// are, while the block of changed lines is not escaped.
#[test]
fn the_path_and_the_reason_are_drawn_as_text_and_the_block_is_not_escaped() {
    let before = "a\nb\nc\n";
    let after = "a\nb_c_d *x*\nc\n";
    let (req, _) = change(
        "/state/__notes__/PERSONA.md",
        "make it *warmer*, and `quote` it",
        before,
        after,
    );
    let text = render::card(&req, &Route::Here, "").content;
    assert!(
        text.contains("Change the prompt file /state/\\_\\_notes\\_\\_/PERSONA.md?"),
        "{text}"
    );
    assert!(
        text.contains("make it \\*warmer\\*, and \\`quote\\` it"),
        "{text}"
    );
    assert!(text.contains("\n+b_c_d *x*\n"), "{text}");
}

/// A block is only the daemon's when it closes: an opening line with no closing
/// one, whatever follows it, is escaped like any other text.
#[test]
fn an_opening_fence_with_no_closing_one_is_text() {
    let said = md("```diff\n+a *b*\n");
    assert!(
        said.contains("\\`\\`\\`diff") && said.contains("\\*b\\*"),
        "{said}"
    );
    let closed = md("```diff\n+a *b*\n```\nthen *c*\n");
    assert!(closed.contains("+a *b*\n```\nthen \\*c\\*"), "{closed}");
}

/// A card whose changed lines do not fit is the daemon's summary, drawn whole
/// the same way; `/answer`'s view and Details hold it whole when one message
/// does, and Details names the id.
#[test]
fn the_views_hold_the_whole_card() {
    let before = lines(80);
    let after = before.replace(
        "Rule 40: be careful.",
        &format!("Rule 40: {}", "w".repeat(900)),
    );
    let (req, whole) = change("/state/P.md", "warmer", &before, &after);
    assert!(!whole, "this one is a summary");
    let here = words_here(&req).expect("one message holds it");
    assert!(here.contains("summary") && !here.contains("wwww"), "{here}");
    let d = details(&req);
    assert!(d.contains("act_0000feed") && d.contains("summary"), "{d}");
    // A card that fits has its changed lines in both, whole.
    let (small, whole) = change("/state/P.md", "warmer", "a\nb\n", "a\nb2\n");
    assert!(whole);
    assert!(words_here(&small).unwrap().contains("\n-b\n+b2\n"));
    assert!(details(&small).contains("\n-b\n+b2\n"));
}
