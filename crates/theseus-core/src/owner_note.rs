//! The head of every message Theseus sends the owner unasked (theseus-x8b9k):
//! a kind tag and an operational title on the first line, a hint at what
//! triggered it on the second, then the body, with the action, if any, last.
//! The Discord DM task flow (the task reply, its started and thread lines, the
//! board, the forum, the approval and question cards, the budget question and
//! the task reports) is locked and does not use it.

/// What the owner is meant to do with a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Nothing: just telling you.
    Fyi,
    /// You need to do something.
    Action,
    /// Choose.
    Decision,
    /// Something is wrong.
    Alert,
    /// A reminder the owner set: his own words are the title.
    Reminder,
}

impl Kind {
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Fyi => "ℹ️ FYI",
            Kind::Action => "✋ Action",
            Kind::Decision => "❓ Decision",
            Kind::Alert => "⚠️ Alert",
            Kind::Reminder => "⏰ Reminder",
        }
    }
}

/// The first two lines: `<tag> · <title>`, then the trigger, with no trailing
/// newline. The tag is the message's one icon: a glyph the title starts with
/// is dropped ("⚠️ Alert · Tide check failed", never "⚠️ Alert · ✗ Tide check
/// failed": the owner, 2026-10-22), except in a reminder, whose title is the
/// owner's own words.
pub fn head(kind: Kind, title: &str, trigger: &str) -> String {
    let title = match kind {
        Kind::Reminder => title,
        _ => title.trim_start_matches(is_glyph).trim_start(),
    };
    format!("{} · {title}\n{trigger}", kind.tag())
}

/// An icon's character: not ASCII (so a backtick, a quote or `$` is kept),
/// not a letter or a digit.
fn is_glyph(c: char) -> bool {
    !c.is_ascii() && !c.is_alphanumeric()
}

/// `head`, then the body on the next line (nothing more when it is empty).
pub fn note(kind: Kind, title: &str, trigger: &str, body: &str) -> String {
    let h = head(kind, title, trigger);
    let body = body.trim_end_matches('\n');
    if body.is_empty() {
        h
    } else {
        format!("{h}\n{body}")
    }
}

/// A day on this machine's clock, for a body's "since when": `2026-10-21`.
pub fn day(unix_ms: u64) -> String {
    let l = crate::wake::local(unix_ms);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}

/// Whether a text starts as `head` writes it: a kind tag, ` · `, a title that
/// starts with no icon of its own (a reminder's may), and a non-empty second
/// line. The tests of every producer ask it.
pub fn is_headed(text: &str) -> bool {
    let mut lines = text.lines();
    let (Some(first), Some(second)) = (lines.next(), lines.next()) else {
        return false;
    };
    let kinds = [
        Kind::Fyi,
        Kind::Action,
        Kind::Decision,
        Kind::Alert,
        Kind::Reminder,
    ];
    let titled = kinds.iter().any(|&k| {
        first
            .strip_prefix(k.tag())
            .and_then(|r| r.strip_prefix(" · "))
            .and_then(|t| t.chars().next())
            .is_some_and(|c| !c.is_whitespace() && (k == Kind::Reminder || !is_glyph(c)))
    });
    titled && !second.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_is_a_tagged_title_and_a_trigger() {
        let t = note(
            Kind::Alert,
            "Disk is low",
            "free space passed 5 GB",
            "body\n",
        );
        assert_eq!(t, "⚠️ Alert · Disk is low\nfree space passed 5 GB\nbody");
        assert!(is_headed(&t));
        assert!(!is_headed("Disk is low\nfree space"));
        assert!(!is_headed("ℹ️ FYI · Title"));
        assert!(!is_headed("ℹ️ FYI ·  Title\nwhy"));
        assert!(!is_headed("ℹ️ FYI · Title\n "));
        assert_eq!(note(Kind::Fyi, "A", "b", ""), "ℹ️ FYI · A\nb");
        let r = note(Kind::Reminder, "stretch", "a reminder you set", "");
        assert_eq!(r, "⏰ Reminder · stretch\na reminder you set");
        assert!(is_headed(&r));
    }

    /// The kind's tag is the one icon: a title's own glyph is dropped, and a
    /// text that keeps one is not headed; a reminder's title is the owner's
    /// words, whatever they start with (the owner, 2026-10-22).
    #[test]
    fn a_title_carries_no_icon_after_the_tag() {
        let t = head(Kind::Alert, "✗ Tide check failed: a timeout", "its run");
        assert_eq!(t, "⚠️ Alert · Tide check failed: a timeout\nits run");
        assert_eq!(
            head(Kind::Action, "🔁 Tide check: skipped", "spent"),
            "✋ Action · Tide check: skipped\nspent"
        );
        assert_eq!(
            head(Kind::Fyi, "`theseus` is up", "w"),
            "ℹ️ FYI · `theseus` is up\nw"
        );
        assert!(!is_headed("⚠️ Alert · ✗ Tide check failed\nits run"));
        let r = head(Kind::Reminder, "🌱 water the plants", "a reminder you set");
        assert_eq!(r, "⏰ Reminder · 🌱 water the plants\na reminder you set");
        assert!(is_headed(&r));
    }
}
