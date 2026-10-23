//! Every message Theseus sends the owner unasked starts with a kind and a
//! title, then a hint at what triggered it (theseus-x8b9k): one fixture per
//! family, through its producer's text function. The Discord binding's own
//! families are its `tests_owner_note.rs`.

use serde_json::json;

use crate::owner_note::is_headed;

/// Self-improvement's messages.
fn self_families() -> Vec<(&'static str, String)> {
    use crate::rsi::backlog::{start, Night, Started};
    let night = Night {
        filed: vec!["tsk_ab12".into(), "tsk_cd34".into()],
        waiting: vec!["tsk_cd34".into()],
        started: vec![Started {
            item: "tsk_ef56".into(),
            title: "Turns fail with timeout".into(),
            session_id: "ses_1".into(),
            objective: String::new(),
            mint: None,
        }],
        trimmed: vec!["tsk_gh78".into()],
        ..Night::default()
    };
    let backlog = start::night_text(&night, |id| start::Item {
        title: format!("Calls from harness fail ({id})"),
        count: 3,
        since_ms: 1_790_000_000_000,
    });
    let week = crate::rsi::digest::week_post(&theseus_protocol::rsi::SelfDigestResult {
        since_ms: 1_790_000_000_000,
        until_ms: 1_790_600_000_000,
        text: "What Theseus changed about itself this week (x to y)\nNothing changed.\n".into(),
        rows: 0,
    });
    let self_notice = crate::rsi::digest::notice::notice_text(
        &[],
        &crate::rsi::digest::notice::Night {
            until_ms: 1_790_000_000_000,
            spent_usd: 1.0,
            day_usd: 30.0,
            waits: vec![
                "• graduate ext-fetch asks for network (keel): theseus self backlog show 7".into(),
            ],
            mint_items: Default::default(),
        },
    )
    .expect("a wait is something to say");
    let joined = json!({"branch": "self/2026-10-20-lsp", "item": "tsk_ab12", "what": "w", "why": "y",
        "gate": "green", "numbers": {"gate_secs": 30, "tries": 1}, "cost_usd": 0.5});
    let installed = crate::rsi::join::install::notice(&joined, None, "4a1c9e0b77aa55");
    use crate::rsi::join::joining;
    let keel = joining::keel_card("self/b", "tsk_1", "sealed", "- sealed: x\n", "1 file");
    let merge_keel = joining::merge_keel_card("self/b", "tsk_1", "/state/self/gates/b.log");
    let unsigned = joining::unsigned_card("self/b", "tsk_1");
    let pr_keel = crate::rsi::pr::keel_text("self/b", "tsk_1", "/state/self/reports/b.md");
    let j = crate::rsi::revert::attribute::Joined {
        merge: "4a1c9e0b77aa55".into(),
        branch: "self/b".into(),
        item: "tsk_1".into(),
        head: "bb".into(),
        at_ms: 0,
    };
    use crate::rsi::revert::doing;
    let revert_held = doing::held_text(&j, "gate", "main's gate is red");
    let reverted = doing::reverted_text(&j, "cc11", "gate", 24, "a red gate", 3, true);
    let vetoed = doing::vetoed_text(&j, "cc11");
    let narrow = crate::aws::narrow::proposal::tests::sample().card();
    let narrow_red = crate::rsi::narrow::red_card("theseus-hand", "nar_1", "s3:GetObject", 2);
    vec![
        ("self backlog night", backlog),
        ("weekly self digest", week),
        ("self notice", self_notice),
        ("self-join installed", installed),
        ("self-join keel card", keel),
        ("self-join merge keel card", merge_keel),
        ("self-join unsigned card", unsigned),
        ("keel stop", pr_keel),
        ("revert held", revert_held),
        ("reverted", reverted),
        ("veto done", vetoed),
        ("narrowed policy card", narrow),
        ("narrowed policy red", narrow_red),
    ]
}

fn families() -> Vec<(&'static str, String)> {
    self_families()
}

/// The key test: each in-scope family's first line is a kind tag and a
/// title, and its second a trigger hint; a family that lost its head is named.
#[test]
fn every_unasked_message_starts_with_a_kind_and_a_title() {
    let bad: Vec<String> = families()
        .into_iter()
        .filter(|(_, text)| !is_headed(text))
        .map(|(name, text)| format!("{name}: {text:?}"))
        .collect();
    assert!(
        bad.is_empty(),
        "families without a head:\n{}",
        bad.join("\n")
    );
}

/// The owner's example: each night's item says what it is, how often and
/// since when, and the commands come last.
#[test]
fn the_backlog_night_says_each_item_in_plain_words() {
    let text = families().remove(0).1;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "✋ Action · Self-improvement backlog: 1 waiting for your word, 2 new, 1 started, 1 dropped"
    );
    assert_eq!(
        lines[1],
        "from last night's read of the day's failures and slow turns"
    );
    let day = crate::owner_note::day(1_790_000_000_000);
    assert_eq!(
        lines[2],
        format!("- started work on: Calls from harness fail (tsk_ef56): seen 3 times since {day} (item tsk_ef56)")
    );
    assert!(
        lines[4].starts_with("- new, waits for your word (keel): "),
        "{text}"
    );
    assert!(lines
        .last()
        .unwrap()
        .starts_with("Reorder or drop with `theseus self move"));
}
