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
        sources: &[],
        used: None,
    });
    (request(&q, &words::change(&settings, None)), values)
}

/// The text of messages with their "(i/n)" markers taken off, joined.
fn unmarked(parts: &[String]) -> String {
    let n = parts.len();
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        match n {
            1 => out.push_str(p),
            _ => out.push_str(
                p.strip_prefix(&format!("-# ({}/{n})\n", i + 1))
                    .unwrap_or_else(|| panic!("part {} has no marker: {p}", i + 1)),
            ),
        }
    }
    out
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
    let all = unmarked(&parts);
    let whole = &req.plain.as_ref().unwrap().detail;
    assert!(
        all.starts_with(&md(whole)),
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
        sources: &[],
        used: None,
    });
    let mut req = request(&q, &words::change(&s, None));
    req.needs_proof = false;
    let c = render::card(&req, &Route::Here, "");
    assert!(c.more.is_empty());
    assert!(c.content.starts_with(&md(
        "Change 1 setting?\n- discord.task_channel_cap: 25 → 26"
    )));
    assert!(!c.content.contains("config.ack"));
    let said = "change `discord.task_channel_cap` from 25 to 26; Theseus then restarts";
    assert_eq!(c.line, md(said));
    let closed = theseus_core::outbox::Closed::new("approved", None);
    assert_eq!(
        render::settled(&closed, &c.line, c.budget),
        format!("✅ **Approved** by the operator · {}", md(said))
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
        sources: &[],
        used: None,
    });
    let short = request(&q, &words::change(&s, None));
    assert!(words_here(&short).unwrap().starts_with("Change 1 setting?"));
    assert!(details(&short).contains("- narrative: off → on"));
}

/// What Discord reads as markdown in a value does not change what is drawn: a
/// wildcard's `*` is not italics that drop it, a pair of `||` or `~~` hides and
/// strikes nothing, a masked link is not one, a code span is not one.
#[test]
fn markdown_in_a_value_is_drawn_as_the_characters_it_is() {
    let s = [setting(
        "sandbox.egress",
        json!(["*.crates.io:443", "*.amazonaws.com:443"]),
        json!([
            "*.a",
            "*.b",
            "||hide||",
            "~~strike~~",
            "[docs](http://elsewhere.example)",
            "`code`",
            "<t:1:R>"
        ]),
    )];
    let q = card::question(&card::Facts {
        settings: &s,
        outside_text: false,
        steered: false,
        reason: "turn *this* on",
        wait_secs: 120,
        sources: &[],
        used: None,
    });
    let req = request(&q, &words::change(&s, None));
    let all = unmarked(&drawn(&req));
    for (raw, escaped) in [
        ("\"*.crates.io:443\"", "\"\\*.crates.io:443\""),
        ("\"*.a\", \"*.b\"", "\"\\*.a\", \"\\*.b\""),
        ("||hide||", "\\|\\|hide\\|\\|"),
        ("~~strike~~", "\\~\\~strike\\~\\~"),
        (
            "[docs](http://elsewhere.example)",
            "\\[docs\\]\\(http://elsewhere.example\\)",
        ),
        ("`code`", "\\`code\\`"),
        ("<t:1:R>", "\\<t:1:R\\>"),
        ("turn *this* on", "turn \\*this\\* on"),
    ] {
        assert!(all.contains(escaped), "{raw} is drawn as {escaped}: {all}");
    }
    // Nothing of the daemon's words is left for Discord to read as markdown.
    let words = md(&req.plain.as_ref().unwrap().detail);
    let mut chars = words.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '*' | '_' | '~' | '|' | '`' | '[' | ']' | '<' | '>' => panic!("unescaped {c}: {words}"),
            _ => {}
        }
    }
    // The settled line is escaped too.
    let c = render::card(&req, &Route::Here, "");
    assert!(c.line.contains("\\`sandbox.egress\\`"), "{}", c.line);
}

/// A cut never falls between a backslash and the character it escapes, and
/// every message says which of how many it is.
#[test]
fn a_cut_never_splits_an_escape_and_each_message_says_its_place() {
    let stars = "*".repeat(380);
    let settings: Vec<Setting> = (0..12)
        .map(|i| {
            setting(
                &format!("memory.k{i}"),
                json!(stars),
                json!(format!("{stars}!")),
            )
        })
        .collect();
    let q = card::question(&card::Facts {
        settings: &settings,
        outside_text: false,
        steered: false,
        reason: "",
        wait_secs: 120,
        sources: &[],
        used: None,
    });
    let mut req = request(&q, &words::change(&settings, None));
    req.needs_proof = false;
    let parts = drawn(&req);
    assert!(parts.len() > 3, "{}", parts.len());
    let n = parts.len();
    for (i, p) in parts.iter().enumerate() {
        assert!(p.starts_with(&format!("-# ({}/{n})\n", i + 1)), "{p}");
        assert!(p.chars().count() < 2000);
        let body = p.split_once('\n').unwrap().1;
        let trailing = body.chars().rev().take_while(|c| *c == '\\').count();
        assert_eq!(trailing % 2, 0, "part {} ends inside an escape", i + 1);
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            if c == '*' {
                panic!("a bare * in part {}", i + 1);
            }
            if c == '\\' {
                chars.next();
            }
        }
    }
    let all = unmarked(&parts);
    assert!(all.contains(&md(&stars)), "every star, escaped");
}

/// The needs-you post in the owner's DM (the route of the install card, which
/// stages no card of its own): the same words, whole, as several messages when
/// longer than one, the proof's clause and how else to answer; and no Approve
/// is ever under less than all of it.
#[test]
fn the_needs_you_post_draws_the_longest_card_whole() {
    let (req, values) = longest();
    let detail = req.plain.as_ref().unwrap().detail.clone();
    let body = json!({
        "kind": "needs_you", "question": "act_0000feed", "held": false,
        "expires_at_ms": 1_700_000_000_000u64,
        "card": {"who": "Theseus", "what": "change 12 settings", "risk": "x", "yes": "y", "no": "n"},
        "settings": {"detail": detail, "needs_proof": true, "needs_fresh_proof": false, "short": "0000feed"},
    });
    let (before, last) = render::settings::needs_you(&body).expect("a settings post");
    let mut all = before;
    all.push(last.clone());
    assert!(all.len() > 1);
    for p in &all {
        assert!(p.chars().count() < 2000, "{}", p.chars().count());
    }
    let text = unmarked(&all);
    assert!(text.starts_with(&md(&detail)), "the words first and whole");
    for v in &values {
        assert!(text.contains(v.as_str()) && text.contains(&format!("{v}!")));
    }
    assert!(text.contains("Second proof") && text.contains("theseus answer 0000feed approve"));
    assert!(last.contains("expires <t:"), "{last}");
    // A post with no settings words is not this form.
    assert!(render::settings::needs_you(&json!({"card": {"what": "x"}})).is_none());
    assert!(render::settings::needs_you(&json!({"settings": null})).is_none());
}

/// A source the model gave is drawn in the card's own words as one line under
/// its setting: the address is on its own line, markdown in the source is
/// drawn as the characters it is (never emphasis, a spoiler or a link), and the
/// care line, the reason and the buttons' footer are as they were.
#[test]
fn a_source_is_drawn_literally_under_its_setting() {
    use theseus_core::config_change::source;
    let settings = [setting(
        "providers.lantern.api_base",
        Value::Null,
        json!("https://lantern.example.test"),
    )];
    let said = source::of(
        &settings[0].path,
        Some("**the docs** say ||this|| is [the address](https://evil.example.test) `x`\n> - quoted\n# heading"),
    )
    .unwrap();
    let q = card::question(&card::Facts {
        settings: &settings,
        outside_text: true,
        steered: false,
        reason: "the owner asked",
        wait_secs: 120,
        sources: std::slice::from_ref(&said),
        used: None,
    });
    let req = request(&q, &words::change(&settings, None));
    let all = unmarked(&drawn(&req));
    let lines: Vec<&str> = all.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("\\- providers.lantern.api\\_base"))
        .unwrap_or_else(|| panic!("the setting's line: {all}"));
    assert!(lines[at].contains("https://lantern.example.test"), "{all}");
    let said_line = lines[at + 1];
    assert!(
        said_line
            .contains("Theseus's source for this, in its own words \\(nothing here checked it\\)"),
        "{said_line}"
    );
    for literal in [
        "\\*\\*the docs\\*\\*",
        "\\|\\|this\\|\\|",
        "\\[the address\\]\\(https://evil.example.test\\)",
        "\\`x\\`",
        "\\> \\- quoted \\# heading",
    ] {
        assert!(said_line.contains(literal), "{literal} in {said_line}");
    }
    assert!(all.contains("Take care: since your last message"), "{all}");
    assert!(all.contains("Theseus's reason, in its own words"), "{all}");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("source for this"))
            .count(),
        1,
        "{all}"
    );
}
