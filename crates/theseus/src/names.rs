//! What a session is called, in every client: the CLI's lists, the terminal
//! UI's tree, notices and prompts, and herdr's pane (theseus-0n1v). One rule:
//! a task's name is its title (the store labels every task `task`); a
//! conversation's is its label, else its title; with neither, its kind and
//! the end of its id.

use theseus_protocol::{SessionInfo, SessionKind};

/// The words that name a session, if it has any: a task's title, a
/// conversation's label or else its title. Blank words name nothing.
pub fn words<'a>(
    kind: SessionKind,
    label: Option<&'a str>,
    title: Option<&'a str>,
) -> Option<&'a str> {
    let some = |s: Option<&'a str>| s.filter(|s| !s.trim().is_empty());
    match kind {
        SessionKind::Task => some(title),
        SessionKind::Conversation => some(label).or(some(title)),
    }
}

/// A session's name: its words' first line with no control character (a
/// tab would break `sessions`' columns, an escape reach the terminal), else
/// its kind and the end of its id (`task a1b2c3`, `ses a1b2c3`).
pub fn name(kind: SessionKind, label: Option<&str>, title: Option<&str>, id: &str) -> String {
    let line = words(kind, label, title).map(|w| {
        let first = w.lines().next().unwrap_or_default();
        let kept: String = first.chars().filter(|c| !c.is_control()).collect();
        kept.trim().to_string()
    });
    match line {
        Some(l) if !l.is_empty() => l,
        _ => {
            let n = id.chars().count();
            let tail: String = id.chars().skip(n.saturating_sub(6)).collect();
            match kind {
                SessionKind::Task => format!("task {tail}"),
                SessionKind::Conversation => format!("ses {tail}"),
            }
        }
    }
}

/// A listed session's name.
pub fn of(s: &SessionInfo) -> String {
    name(
        s.kind,
        s.label.as_deref(),
        s.title.as_deref(),
        &s.session_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_is_named_by_its_title_and_a_conversation_by_its_label() {
        let task = SessionKind::Task;
        let conv = SessionKind::Conversation;
        let quay = Some("Survey the north quay");
        assert_eq!(
            name(task, Some("task"), quay, "ses_0a1b2c"),
            "Survey the north quay"
        );
        assert_eq!(name(task, Some("task"), None, "ses_0a1b2c"), "task 0a1b2c");
        assert_eq!(
            name(conv, Some("tide notes"), quay, "ses_0a1b2c"),
            "tide notes"
        );
        assert_eq!(
            name(conv, Some("  "), quay, "ses_0a1b2c"),
            "Survey the north quay"
        );
        assert_eq!(name(conv, None, Some("one\ntwo"), "ses_0a1b2c"), "one");
        assert_eq!(name(conv, None, None, "ses_0a1b2c"), "ses 0a1b2c");
        assert_eq!(words(task, Some("task"), None), None);
    }

    /// A title's control characters never reach a list, a notice or herdr:
    /// a tab, an escape's byte and a bell are dropped, the rest kept; words
    /// of nothing else name the session by its id.
    #[test]
    fn a_name_drops_control_characters() {
        let conv = SessionKind::Conversation;
        let task = SessionKind::Task;
        assert_eq!(
            name(conv, Some("tide\tnotes \u{1b}[2J"), None, "ses_0a1b2c"),
            "tidenotes [2J"
        );
        let quay = Some("\u{7} north quay\r\nmore");
        assert_eq!(name(task, None, quay, "ses_0a1b2c"), "north quay");
        assert_eq!(name(conv, Some("\u{7}"), None, "ses_0a1b2c"), "ses 0a1b2c");
    }
}
