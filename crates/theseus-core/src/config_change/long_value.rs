//! A setting whose value is too long for the ordinary card (issue 23, line 2).
//!
//! `config.propose` used to refuse such a change: the card gave each value
//! whole in a line, and a line of more than [`diff::SHOWN`] characters is not
//! whole on a card. Neither limit moved. A value past it, up to the proposal's
//! own limit ([`super::MAX_VALUE_BYTES`]), now goes on the card in the form of a
//! prompt file's (`shown.rs`): each such setting names its size and fingerprint
//! before and after, and shows its changed lines, whole, when they fit, else
//! says it is a summary; and its whole value, before and after, is readable
//! before the yes (`show.rs`). A short value keeps exactly the line it had.

use serde_json::Value;

use super::diff::{self, Setting};
use super::{edit, shown};

/// The most long values one change may carry, so each keeps room on the card.
pub const MAX_LONG: usize = 3;

/// A value as the text it is: a string as itself, anything else as its JSON,
/// and a setting that is not set as nothing.
pub fn text_of(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// The line a long setting has in the list of settings.
pub fn stub(s: &Setting) -> String {
    format!("- {}: a long text, shown below", edit::dotted(&s.path))
}

/// What each long setting says of itself on the card, in order, using at most
/// `room` characters for the changed lines of all of them together. Empty when
/// no setting is long.
pub fn blocks(settings: &[Setting], room: usize) -> String {
    let mut out = String::new();
    let mut left = room;
    for s in settings.iter().filter(|s| diff::too_long(s)) {
        let (before, after) = (text_of(s.before.as_ref()), text_of(s.after.as_ref()));
        let (b, a) = (shown::facts(&before), shown::facts(&after));
        let d = shown::diff(&before, &after);
        let name = edit::dotted(&s.path);
        let was = match s.before {
            None => "not set (its default)".to_string(),
            Some(_) => b.words(),
        };
        out.push_str(&format!(
            "{name}:\n  Before: {was}\n  After: {}\n  {}, {}.\n",
            a.words(),
            crate::narrative::count(d.added as u64, "line added", "lines added"),
            crate::narrative::count(d.removed as u64, "line removed", "lines removed"),
        ));
        if b.unseen + a.unseen + d.unseen > 0 {
            out.push_str(&format!(
                "  Characters you cannot see: {} on the changed lines shown; {} in the whole value \
                 before and {} after.\n",
                d.unseen, b.unseen, a.unseen
            ));
        }
        match d.block(left) {
            Some(block) => {
                left = left.saturating_sub(block.chars().count());
                out.push_str(&format!(
                    "The changed lines, whole (unseen characters are written \\u{{..}}; a \
                     backslash is doubled):\n{block}"
                ));
            }
            None => out.push_str(&format!(
                "These changes are too long to show on a card whole, so this is a summary and not \
                 the text. They are in {}.\n",
                d.places()
            )),
        }
    }
    if !out.is_empty() {
        out.push_str(
            "The whole value of each, before and after: `theseus config show`, or this question \
             in the cockpit.\n",
        );
    }
    out
}

/// The whole value of each long setting, before and after, for `config.show`.
pub fn whole(settings: &[Setting]) -> String {
    let mut out = String::from("The settings with a value too long for the card, each whole:\n");
    for s in settings.iter().filter(|s| diff::too_long(s)) {
        let name = edit::dotted(&s.path);
        out.push('\n');
        out.push_str(&shown::whole(
            &format!("{name}, before"),
            &text_of(s.before.as_ref()),
        ));
        out.push_str(&shown::whole(
            &format!("{name}, after"),
            &text_of(s.after.as_ref()),
        ));
    }
    out
}

