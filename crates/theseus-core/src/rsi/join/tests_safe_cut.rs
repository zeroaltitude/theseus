//! The self loop's cuts stop at a known value's edge: a prompt's diff at its
//! cap, an owner's note quoting a reason, a digest cut to one post, and an
//! issue's first line in its observation each hold a value the board knows
//! whole or not at all. All values invented.

use theseus_protocol::rsi::SelfMode;

use crate::rsi::backlog::{attempts, issues};
use crate::rsi::join::fix::{turns, ApproveAsk, FixAsk, Point};
use crate::rsi::join::review::{self, Ask, DIFF_CHARS};
use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::{whole_or_absent, V};

/// A live board that knows `V`, for as long as it is held.
fn knowing() -> std::sync::Arc<SecretBoard> {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    board
}

/// `n` filler characters, then `V`, then more: a cut at `n + 10` splits it.
fn straddling(n: usize, fill: char) -> String {
    format!("{}{V} and the rest", fill.to_string().repeat(n))
}

#[test]
fn a_judging_prompt_cut_at_its_diff_cap_holds_a_value_whole_or_not_at_all() {
    let _board = knowing();
    let r = crate::rsi::join::tests::rig(SelfMode::Act);
    let live = r.core.live_profile().0;
    let target = r
        .core
        .runner
        .resolve_target(&live, Some("fable"), None, None)
        .unwrap();
    // The fixer's cap is 40,000 characters and the judge's `DIFF_CHARS`: one
    // copy of the value across each.
    let first = straddling(40_000 - 10, 'x');
    let between = DIFF_CHARS - 10 - first.chars().count();
    let diff = format!("{first}{}", straddling(between, 'y'));
    let ask = Ask {
        branch: "self/20261014-trim".into(),
        head: "abc".into(),
        item: "trim".into(),
        diff: diff.clone(),
        report: String::new(),
        evidence: String::new(),
        target: target.clone(),
        built_ms: 0,
        scratch: "/tmp/s".into(),
        repo: "/tmp/r".into(),
        offer_fix: None,
        fixed: None,
        proof: String::new(),
    };
    let said = review::prompt(&ask);
    assert!(said.contains("[the diff is cut here"), "the cut is said");
    whole_or_absent("the judging prompt", &said, V);
    let fix = FixAsk {
        item: "trim".into(),
        branch: "self/20261014-trim".into(),
        head: "abc".into(),
        point: Point::Gate,
        tree: "/tmp/t".into(),
        slug: "trim".into(),
        diff: diff.clone(),
        failure: "a red gate".into(),
        evidence: String::new(),
        target: target.clone(),
        lines: 60,
        files: 6,
    };
    let said = turns::fix_prompt(&fix);
    assert!(said.contains("[the diff is cut here"), "the cut is said");
    whole_or_absent("the fixer's prompt", &said, V);
    let approve = ApproveAsk {
        item: "trim".into(),
        branch: "self/20261014-trim".into(),
        head: "def".into(),
        point: Point::Gate,
        diff,
        reason: "it repairs the fault".into(),
        failure: "a red gate".into(),
        fixer: "fable".into(),
        target,
    };
    whole_or_absent("the approver's prompt", &turns::approve_prompt(&approve), V);
}

#[test]
fn an_owner_note_of_the_self_loop_stops_before_a_known_value() {
    let _board = knowing();
    let card = super::unreviewable::card(
        "self/20261014-trim",
        "trim",
        "auth",
        3,
        &straddling(400 - 10, 'w'),
    );
    whole_or_absent("the unreviewable card", &card, V);
    let e = crate::rsi::pipeline::queues::Entry {
        item: "trim".into(),
        title: "Trim the beacon".into(),
        findings: vec![straddling(300 - 10, 'f'), straddling(300 - 10, 'g')],
        ..Default::default()
    };
    whole_or_absent(
        "the note for two rejections",
        &crate::rsi::pipeline::rejected_twice_text(&e),
        V,
    );
    let tries = [attempts::Try {
        at_ms: 1,
        why: straddling(300 - 10, 't'),
    }];
    whole_or_absent(
        "the notice for a capped item",
        &attempts::notice("trim", "Trim the beacon", &tries, 3),
        V,
    );
}

#[test]
fn a_digest_cut_to_one_post_holds_a_value_whole_or_not_at_all() {
    let _board = knowing();
    // One line, so the cut has no line end to fall back to.
    let post = crate::rsi::digest::capped(&straddling(1_800 - 10, 'd'));
    assert!(post.contains("the rest: `theseus self digest`"), "{post}");
    whole_or_absent("the capped digest", &post, V);
}

#[test]
fn an_issues_first_line_in_its_observation_holds_a_value_whole_or_not_at_all() {
    let _board = knowing();
    // The observation's note keeps 240 characters of the body's first line.
    let listing = serde_json::json!([{
        "number": 7,
        "title": "Tide tables lose a footnote",
        "state": "open",
        "body": straddling(240 - 10, 'b'),
    }]);
    let found = issues::parse(listing.to_string().as_bytes()).unwrap();
    let obs = issues::obs(&found, "harbourworks/lanterns", "", 5, 0);
    assert!(obs[0].note.ends_with('…'), "{}", obs[0].note);
    whole_or_absent("the issue's note", &obs[0].note, V);
}
