use serde_json::{json, Value};
use theseus_core::config_change::{card, diff::Setting, edit, words};
use theseus_protocol::ConfirmRequest;

use super::*;
use crate::render::{self, Route};

fn setting(path: &str, before: Value, after: Value) -> Setting {
    Setting {
        path: edit::parse_path(path).unwrap(),
        before: Some(before),
        after: Some(after),
    }
}

/// The request the daemon would put for a card with these settings.
fn request(question: &str, settled: &str) -> ConfirmRequest {
    let args = json!({"question": question, "settled": settled});
    ConfirmRequest {
        correlation_id: "act_0000feed".into(),
        session_id: "ses_00000000harbour".into(),
        tool: "config.ack".into(),
        reason: question.into(),
        input: json!({"question": question, "asked_by": {"execution_id": "exe_x"}}),
        plain: words::plain_of(&args),
        requested_at_ms: 1_699_999_700_000,
        expires_at_ms: 1_700_000_000_000,
        needs_proof: true,
        ..Default::default()
    }
}

/// The longest card the daemon allows: twelve settings, each value as long as
/// a card shows whole, before and after, a sensitive one, outside text read,
/// and the longest reason.
fn longest() -> (ConfirmRequest, Vec<String>) {
    let values: Vec<String> = (0..12).map(|i| format!("{i:x}").repeat(380)).collect();
    let settings: Vec<Setting> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let path = match i {
                0 => "self.budget.usd".to_string(),
                i => format!("memory.some_long_setting_name_{i}"),
            };
            setting(&path, json!(v), json!(format!("{v}!")))
        })
        .collect();
    let q = card::question(&card::Facts {
        settings: &settings,
        outside_text: true,
        steered: true,
        reason: &"because ".repeat(40),
        wait_secs: 120,
    });
    (request(&q, &words::change(&settings, None)), values)
}

fn drawn(req: &ConfirmRequest) -> Vec<String> {
    let c = render::card(req, &Route::Here, "the cockpit");
    let mut all = c.more.clone();
    all.push(c.content);
    all
}

/// The card the daemon built, drawn in Discord: every character of its words
/// is in the messages, in order, none cut, each under the limit, and nothing
/// of the tool's name or its raw arguments.
#[test]
fn the_longest_card_is_drawn_whole_in_messages_each_under_the_limit() {
    let (req, values) = longest();
    let parts = drawn(&req);
    assert!(
        parts.len() > 1,
        "twelve long settings take several messages"
    );
    for p in &parts {
        assert!(p.chars().count() < 2000, "{}", p.chars().count());
    }
    let all = parts.concat();
    let whole = &req.plain.as_ref().unwrap().detail;
    assert!(
        all.starts_with(whole.as_str()),
        "the daemon's words come first and whole"
    );
    for v in &values {
        assert!(all.contains(v.as_str()), "a value is whole: {}", &v[..8]);
        assert!(all.contains(&format!("{v}!")));
    }
    assert!(
        all.contains("second proof") && all.contains("Take care"),
        "{all}"
    );
    for raw in ["config.ack", "execution_id", "asked_by", "{\""] {
        assert!(!all.contains(raw), "{raw}");
    }
}

/// A card that fits one message is one message, in the daemon's words, with no
/// header of a tool's name, and settles into the change in words.
#[test]
fn a_plain_change_is_one_message_that_settles_into_words() {
    let s = [setting("discord.task_channel_cap", json!(25), json!(26))];
    let q = card::question(&card::Facts {
        settings: &s,
        outside_text: false,
        steered: false,
        reason: "you said so",
        wait_secs: 120,
    });
    let mut req = request(&q, &words::change(&s, None));
    req.needs_proof = false;
    let c = render::card(&req, &Route::Here, "");
    assert!(c.more.is_empty());
    assert!(c
        .content
        .starts_with("Change 1 setting?\n- discord.task_channel_cap: 25 → 26"));
    assert!(!c.content.contains("config.ack"));
    assert_eq!(
        c.line,
        "change `discord.task_channel_cap` from 25 to 26; Theseus then restarts"
    );
    let closed = theseus_core::outbox::Closed::new("approved", None);
    assert_eq!(
        render::settled(&closed, &c.line, c.budget),
        "✅ **Approved** by the operator · change `discord.task_channel_cap` from 25 to 26; Theseus then restarts"
    );
}

/// An ordinary approval keeps its own card.
#[test]
fn an_ordinary_approval_is_not_a_settings_card() {
    let req = ConfirmRequest {
        tool: "fs.write".into(),
        ..Default::default()
    };
    assert!(!is_settings(&req));
}

/// `/answer` and Details show the words whole when they fit one message, and
/// otherwise say where to read them and offer no answer.
#[test]
fn a_view_that_cannot_hold_the_words_does_not_take_a_yes_on_part_of_them() {
    let (long, _) = longest();
    assert!(words_here(&long).is_none());
    let d = details(&long);
    assert!(
        d.contains("longer than one message") && d.chars().count() < 2000,
        "{d}"
    );
    let s = [setting("narrative", json!(false), json!(true))];
    let q = card::question(&card::Facts {
        settings: &s,
        outside_text: false,
        steered: false,
        reason: "",
        wait_secs: 120,
    });
    let short = request(&q, &words::change(&s, None));
    assert!(words_here(&short).unwrap().starts_with("Change 1 setting?"));
    assert!(details(&short).contains("- narrative: off → on"));
}
