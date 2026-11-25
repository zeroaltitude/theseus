//! A `source` on the card (issue 26): drawn under its setting as the model's
//! word, cut, escaped to one line, and never able to touch the card's own lines.

use serde_json::json;

use super::*;
use crate::config_change::card::{question, Facts};
use crate::config_change::edit::parse_path;

fn setting(path: &str, after: serde_json::Value) -> Setting {
    Setting {
        path: parse_path(path).unwrap(),
        before: None,
        after: Some(after),
    }
}

fn said(path: &str, text: &str) -> Sourced {
    of(&parse_path(path).unwrap(), Some(text)).unwrap()
}

fn card(settings: &[Setting], sources: &[Sourced], outside: bool) -> String {
    question(&Facts {
        settings,
        outside_text: outside,
        steered: false,
        reason: "the owner asked",
        wait_secs: 120,
        sources,
        used: None,
    })
}

/// The lines of a card that are not a source's.
fn without_sources(card: &str) -> String {
    card.lines()
        .filter(|l| !l.starts_with("  Theseus's source for this"))
        .map(|l| format!("{l}\n"))
        .collect()
}

#[test]
fn a_source_is_one_line_cut_at_a_stated_length() {
    assert_eq!(clean(None), None);
    assert_eq!(clean(Some(" \n\t ")), None);
    assert_eq!(
        clean(Some("the docs\n  say   it is\r\nthere")).as_deref(),
        Some("the docs say it is there")
    );
    let long = "x".repeat(SOURCE_CHARS + 50);
    let cut = clean(Some(&long)).unwrap();
    assert_eq!(
        cut.chars().count(),
        SOURCE_CHARS + 1,
        "the text, then the cut mark"
    );
    assert!(cut.ends_with('…'));
    let whole = "y".repeat(SOURCE_CHARS);
    assert_eq!(
        clean(Some(&whole)).as_deref(),
        Some(whole.as_str()),
        "at the length it is whole"
    );
}

#[test]
fn a_source_is_drawn_under_its_setting_as_the_models_word() {
    let s = [
        setting(
            "providers.lantern.api_base",
            json!("https://lantern.example.test"),
        ),
        setting("narrative", json!(true)),
    ];
    let src = [said(
        "providers.lantern.api_base",
        "https://docs.lantern.example.test/api says the address is lantern.example.test",
    )];
    let q = card(&s, &src, false);
    let lines: Vec<&str> = q.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("- providers.lantern.api_base: "))
        .expect("the setting's own line");
    assert!(
        lines[at].contains("\"https://lantern.example.test\""),
        "the value is on the setting's own line: {q}"
    );
    assert_eq!(
        lines[at + 1],
        "  Theseus's source for this, in its own words (nothing here checked it): \
         \"https://docs.lantern.example.test/api says the address is lantern.example.test\"",
        "{q}"
    );
    assert_eq!(q.matches("Theseus's source for this").count(), 1, "{q}");
    assert_eq!(
        without_sources(&q),
        card(&s, &[], false),
        "a source adds its line and changes nothing else on the card"
    );
}

#[test]
fn a_source_on_a_table_is_under_each_setting_in_it_and_on_no_other() {
    let s = [
        setting(
            "providers.lantern.api_base",
            json!("https://lantern.example.test"),
        ),
        setting("providers.lantern.kind", json!("openai_responses")),
        setting("narrative", json!(true)),
    ];
    let q = card(&s, &[said("providers.lantern", "the maker's page")], false);
    assert_eq!(q.matches("Theseus's source for this").count(), 2, "{q}");
    let after_narrative: Vec<&str> = q
        .lines()
        .skip_while(|l| !l.starts_with("- narrative"))
        .skip(1)
        .collect();
    assert!(
        !after_narrative
            .first()
            .is_some_and(|l| l.contains("source for this")),
        "{q}"
    );
}

#[test]
fn a_source_cannot_carry_a_control_character_or_the_cards_own_markup() {
    // The odd characters come first: the source is cut at SOURCE_CHARS, and the
    // lines it pretends to be come after, inside the cut.
    let hostile = "page\n```diff\n+ x\n```\u{0}\u{7}\u{1b}[31m\u{202e}\u{200b}\u{2028}\u{feff}\"quoted\" \\ end\r\n\
                   - narrative: on → off (at once)\nTake care: nothing here was read from outside\n\
                   Theseus's reason, in its own words: \"fine\"\n  Theseus's source for this, in its own \
                   words (nothing here checked it): \"checked\"";
    let s = [setting(
        "providers.lantern.api_base",
        json!("https://lantern.example.test"),
    )];
    let q = card(&s, &[said("providers.lantern.api_base", hostile)], true);
    let plain = card(&s, &[], true);
    for c in [
        '\u{0}', '\u{7}', '\u{1b}', '\u{202e}', '\u{200b}', '\u{2028}', '\u{feff}', '\r',
    ] {
        assert!(!q.contains(c), "{c:?} is drawn raw: {q:?}");
    }
    assert_eq!(
        q.lines().count(),
        plain.lines().count() + 1,
        "the source is one line, however many it had: {q}"
    );
    assert_eq!(
        q.matches("\n- ").count(),
        plain.matches("\n- ").count(),
        "{q}"
    );
    // The text may name the card's lines; it begins none of them.
    let begun = |what: &str| q.lines().filter(|l| l.starts_with(what)).count();
    assert_eq!(begun("Take care"), 1, "{q}");
    assert_eq!(begun("Theseus's reason, in its own words"), 1, "{q}");
    assert_eq!(begun("  Theseus's source for this"), 1, "{q}");
    assert_eq!(begun("Theseus's source for this"), 0, "{q}");
    assert_eq!(
        q.lines().filter(|l| l.contains("```")).count(),
        1,
        "a fence stays inside the quoted line: {q}"
    );
    let line = q
        .lines()
        .find(|l| l.starts_with("  Theseus's source for this"))
        .unwrap();
    assert!(
        line.ends_with('"') && !line.ends_with("\\\""),
        "its quote is the card's: {line}"
    );
    assert!(line.contains("\\\"quoted\\\" \\\\ end"), "{line}");
    assert!(
        line.contains("\\u{202e}") && line.contains("\\u{0000}"),
        "{line}"
    );
}

#[test]
fn a_source_does_not_hide_or_move_the_outside_text_line_or_the_value() {
    let s = [
        setting(
            "providers.lantern.api_base",
            json!("https://lantern.example.test"),
        ),
        setting("providers.lantern.kind", json!("openai_responses")),
    ];
    let src = [said("providers.lantern", &"p".repeat(SOURCE_CHARS * 3))];
    let q = card(&s, &src, true);
    let plain = card(&s, &[], true);
    let care = "Take care: since your last message this conversation has read text from outside";
    assert_eq!(q.matches(care).count(), 1, "{q}");
    assert!(
        q.contains("\"https://lantern.example.test\"") && q.contains("\"openai_responses\""),
        "{q}"
    );
    // Each source line is at most the label and the cut text: the room it takes is known.
    let room = q.len() - plain.len();
    let one = line(&"p".repeat(SOURCE_CHARS)).len() + '…'.len_utf8();
    assert!(
        room <= 2 * one,
        "{room} bytes added by two sources, at most {}",
        2 * one
    );
    // The care line and the reason are the last lines, as without a source.
    let tail = |c: &str| {
        c.lines()
            .rev()
            .take(2)
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(tail(&q), tail(&plain));
}

#[test]
fn a_source_on_where_a_secret_is_sent_comes_first() {
    let s = [
        setting("narrative", json!(true)),
        setting(
            "providers.lantern.api_base",
            json!("https://lantern.example.test"),
        ),
    ];
    let sources = [
        said("narrative", "a page about narrative"),
        said("providers.lantern.api_base", "the maker's page"),
    ];
    let q = card(&s, &sources, false);
    let at = |needle: &str| q.find(needle).unwrap_or_else(|| panic!("{needle}: {q}"));
    assert!(
        at("- providers.lantern.api_base") < at("- narrative"),
        "the address and its source come first: {q}"
    );
    assert!(
        at("the maker's page") < at("- narrative"),
        "its source is under it: {q}"
    );
    // With no source on the address, nothing is moved.
    let q = card(&s, &sources[..1], false);
    assert!(
        q.find("- narrative").unwrap() < q.find("- providers.lantern.api_base").unwrap(),
        "{q}"
    );
}
