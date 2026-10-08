//! A session's title (theseus-emqx): the first line of its first input, as
//! it always was, and once, among its first `[sessions]
//! retitle_within_turns` inputs, a better one: the first input with
//! substance, when the first had none (a probe, a greeting, a bare
//! command). The old title is kept in `title_was`, which the cockpit shows
//! ("was: …") and its palette matches.
//!
//! Deterministic, and in memory: the turn's copy of the record carries it,
//! and the turn's own session write takes it (`take_turns_fields`), so it
//! adds no frame, no store read and no model call to the turn. A model's
//! title was the other way; it would cost a call per session and a frame
//! off the turn's path, for a title the owner's own words already give.
//!
//! Never a task's title (its brief names it), never the owner's `label`,
//! and never twice: a session with a `title_was` is done.

use super::{title_from, SessionRecord, TurnRunner};

impl TurnRunner {
    /// The title an input gives `session`: its first, or the re-title.
    /// `first_file` names the input's first file, for an input of only files.
    pub(super) fn title_input(
        &self,
        session: &mut SessionRecord,
        text: &str,
        first_file: Option<&str>,
    ) {
        let title = match (title_from(text), first_file) {
            (t, Some(name)) if t.is_empty() => title_from(name),
            (t, _) => t,
        };
        retitle(session, title, self.cfg.sessions.retitle_within_turns);
    }
}

/// `title` is the title of `session`'s next input: its first title, or,
/// within the first `within` inputs, the one that replaces a title with no
/// substance.
pub(crate) fn retitle(session: &mut SessionRecord, title: String, within: u64) {
    let Some(was) = session.title.clone() else {
        session.title = Some(title);
        return;
    };
    // This input is the session's `turns + 1`th.
    let open = session.task.is_none()
        && session.title_was.is_empty()
        && session.turns < within
        && !has_substance(&was)
        && has_substance(&title);
    if open {
        session.title_was.push(was);
        session.title = Some(title);
    }
}

/// Whether a title says what a session is about: three words or more, a
/// dozen characters or more, and not a command (`/new`, `!ls`).
pub(crate) fn has_substance(title: &str) -> bool {
    let t = title.trim();
    let words = t.split_whitespace().count();
    words >= 3 && t.chars().count() >= 12 && !t.starts_with(['/', '!'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::SessionKind;

    fn after(inputs: &[&str], within: u64) -> SessionRecord {
        let mut s = SessionRecord::new(SessionKind::Conversation, Some("the owner's label".into()));
        for i in inputs {
            retitle(&mut s, title_from(i), within);
            s.turns += 1;
        }
        s
    }

    #[test]
    fn a_probe_is_retitled_by_the_first_input_with_substance_once() {
        let s = after(
            &[
                "hi",
                "Read /etc/hostname",
                "Chart the soundings of the outer harbour",
                "Now the inner harbour's soundings, please",
            ],
            3,
        );
        assert_eq!(
            s.title.as_deref(),
            Some("Chart the soundings of the outer harbour")
        );
        assert_eq!(s.title_was, ["hi"]);
        assert_eq!(
            s.label.as_deref(),
            Some("the owner's label"),
            "the label is the owner's"
        );
    }

    #[test]
    fn past_the_window_or_with_a_good_first_title_nothing_changes() {
        let late = after(
            &[
                "hi",
                "ok",
                "yes",
                "Chart the soundings of the outer harbour",
            ],
            3,
        );
        assert_eq!(late.title.as_deref(), Some("hi"));
        assert!(late.title_was.is_empty());
        let good = after(
            &[
                "Chart the reef's north edge",
                "Now the inner harbour's soundings",
            ],
            3,
        );
        assert_eq!(good.title.as_deref(), Some("Chart the reef's north edge"));
        assert!(
            after(&["hi", "Chart the soundings of the outer harbour"], 0)
                .title_was
                .is_empty()
        );
    }

    #[test]
    fn a_task_is_never_retitled() {
        let mut s = SessionRecord::new(SessionKind::Task, None);
        s.task = Some(crate::session::TaskOf {
            parent_session: "ses_parent".into(),
            parent_execution: "exe_parent".into(),
            by: "act_x".into(),
            target: None,
            arrangement: None,
            check: None,
        });
        s.title = Some("tide".into());
        retitle(&mut s, "Chart the soundings of the outer harbour".into(), 3);
        assert_eq!(s.title.as_deref(), Some("tide"));
        assert!(s.title_was.is_empty());
    }

    #[test]
    fn the_stored_record_takes_the_retitle_once() {
        let mut stored = after(&["hi"], 3);
        let mut turn = stored.clone();
        retitle(
            &mut turn,
            "Chart the soundings of the outer harbour".into(),
            3,
        );
        stored.take_turns_fields(&turn);
        assert_eq!(
            stored.title.as_deref(),
            Some("Chart the soundings of the outer harbour")
        );
        assert_eq!(stored.title_was, ["hi"]);
        // A later copy whose title differs never re-titles it again.
        let mut later = stored.clone();
        later.title = Some("something else entirely here".into());
        stored.take_turns_fields(&later);
        assert_eq!(
            stored.title.as_deref(),
            Some("Chart the soundings of the outer harbour")
        );
    }
}
