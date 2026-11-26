//! The card for a changed prompt file (issue 23), built here from the two texts
//! and nothing the model wrote but its one-line reason.
//!
//! One card names the file, gives each text's size and fingerprint, counts the
//! lines added and removed, and shows the changed lines themselves, whole, with
//! two lines of context, when they fit [`CARD_CHARS`] with the rest of the
//! card. When they do not, the card is a summary and says so in plain words,
//! and names where the whole text is. It never shows cut text as whole.

use crate::config_change::shown::{self, Diff, Facts as Text};

/// How long a whole card may be, in characters: what the smallest place the
/// owner answers from carries. A Discord message is 2,000 characters and holds,
/// besides this text, a heading, the second proof's clause and a footer.
pub const CARD_CHARS: usize = 1_450;

/// What the card says.
pub struct Facts<'a> {
    /// The file's canonical path.
    pub path: &'a str,
    /// Which level names it, in words.
    pub who: &'a str,
    /// A shared place carries it too.
    pub public: bool,
    pub before: &'a Text,
    pub after: &'a Text,
    pub diff: &'a Diff,
    /// The turn has read outside text since the owner's last message.
    pub outside_text: bool,
    /// And Theseus's own check flagged what it read as steering it.
    pub steered: bool,
    pub reason: &'a str,
    /// Lines of the heading's own: the card of an undo says what it puts back.
    pub preface: Option<&'a str>,
    /// An undo's number; none for a change.
    pub undoing: Option<u32>,
}

/// The card, and whether the changed lines are on it whole.
pub fn question(f: &Facts<'_>) -> (String, bool) {
    let (path, _) = shown::escape_line(f.path);
    let mut head = match f.undoing {
        Some(n) => format!("Undo change #{n}: put back the prompt file {path}?\n"),
        None => format!("Change the prompt file {path}?\n"),
    };
    if let Some(p) = f.preface {
        head.push_str(p);
    }
    head.push_str(&format!("It is Theseus's {}.", f.who));
    if f.public {
        head.push_str(" A shared place carries it too (it is marked public).");
    }
    head.push_str(&format!(
        "\nBefore: {}\nAfter: {}\n{}, {}.\n",
        f.before.words(),
        f.after.words(),
        crate::narrative::count(f.diff.added as u64, "line added", "lines added"),
        crate::narrative::count(f.diff.removed as u64, "line removed", "lines removed"),
    ));
    let tail = tail(f);
    let room = CARD_CHARS.saturating_sub(head.chars().count() + tail.chars().count() + 100);
    match f.diff.block(room) {
        Some(block) => (
            format!(
                "{head}The changed lines, whole (unseen characters are written \\u{{..}}; a \
                 backslash is doubled):\n{block}{tail}"
            ),
            true,
        ),
        None => (
            format!(
                "{head}These changes are too long to show on a card whole, so this is a summary \
                 and not the text. They are in {}.\n{tail}",
                f.diff.places()
            ),
            false,
        ),
    }
}

/// What follows the changed lines: when it takes effect, where the whole text
/// is, the second proof, the outside-text warning and the reason.
fn tail(f: &Facts<'_>) -> String {
    let mut out = String::new();
    let hidden = f.before.unseen + f.after.unseen;
    if hidden > 0 || f.diff.unseen > 0 {
        out.push_str(&format!(
            "Characters you cannot see: {} on the changed lines shown; {} in the whole text \
             before and {} after.\n",
            f.diff.unseen, f.before.unseen, f.after.unseen
        ));
    }
    out.push_str(
        "Takes effect with the next message; no restart. The whole text, before and after: \
         `theseus config show`, or this question in the cockpit.\n",
    );
    if f.after.bytes > crate::context_files::MAX_BYTES {
        out.push_str(&format!(
            "Only the first {} bytes of a context file reach a prompt, and this one is longer.\n",
            crate::narrative::thousands(crate::context_files::MAX_BYTES as u64)
        ));
    }
    out.push_str("It goes into every later prompt: read it before you say yes.\n");
    out.push_str(&crate::config_change::card::care_lines(
        f.outside_text,
        f.steered,
    ));
    let why = crate::config_change::card::reason_line(f.reason);
    if !why.is_empty() {
        out.push_str(&format!("Theseus's reason, in its own words: \"{why}\"\n"));
    }
    out
}

#[cfg(test)]
mod tests;
