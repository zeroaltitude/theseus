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

/// A session's name: its words' first line, else its kind and the end of
/// its id (`task a1b2c3`, `ses a1b2c3`).
pub fn name(kind: SessionKind, label: Option<&str>, title: Option<&str>, id: &str) -> String {
    match words(kind, label, title) {
        Some(w) => w.lines().next().unwrap_or_default().trim().to_string(),
        None => {
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
}
