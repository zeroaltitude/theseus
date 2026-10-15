//! A notice wake's fact (theseus-8ux6x): a reminder fired. Its ledger row is
//! the `wake.fired` row the kernel writes in the firing frame, so this fact
//! has the rest: the `wake.noticed` notification to every connection, and the
//! sentence in the narrative.

use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event};
use theseus_protocol::{reminder_answers, WakeNoticed};

use super::{Fact, Say};
use theseus_kernel::FiredWake;

/// A reminder that fired, and where its note was posted.
pub struct WakeFired<'a> {
    pub fired: &'a FiredWake,
    pub session_id: &'a str,
    pub execution_id: &'a str,
    /// Where the post went; none when no place hears it.
    pub posted_to: Option<&'a str>,
    /// It lives in the owner's Home conversation.
    pub home: bool,
}

impl Fact for WakeFired<'_> {
    const METHOD: Option<&'static str> = Some(notify::WAKE_NOTICED);

    fn event(&self) -> Option<Event> {
        let (f, w) = (self.fired, &self.fired.wake);
        Some(Event::WakeNoticed(WakeNoticed {
            wake_id: w.id.clone(),
            short: crate::task::short(&w.id),
            session_id: self.session_id.into(),
            execution_id: self.execution_id.into(),
            purpose: w.purpose.as_str().into(),
            note: w.note.clone(),
            due_at_ms: w.due_at_ms,
            late_ms: f.late_ms,
            while_down: f.while_down,
            every: w.repeat.as_ref().map(|r| r.every.to_string()),
            occurrence: w.repeat.as_ref().map(|_| w.occurrence),
            posted_to: self.posted_to.map(str::to_string),
            home: self.home,
            answers: reminder_answers(),
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let (f, w) = (self.fired, &self.fired.wake);
        let late = if f.late_ms > crate::wake::LATE_AFTER_MS {
            let down = if f.while_down {
                " (the daemon was down)"
            } else {
                ""
            };
            format!(", {} late{down}", crate::wake::span(f.late_ms))
        } else {
            String::new()
        };
        let to = match self.posted_to {
            Some(t) => format!("posted to {t}"),
            None => "no place hears it; the surfaces were told".into(),
        };
        say.line(
            Session,
            format!(
                "Reminder {} fired{late}: \"{}\"; {to}. No turn, no model call.",
                crate::task::short(&w.id),
                crate::session::title_from(&w.note)
            ),
        );
    }
}

/// A routine's run came to something worth a notice (theseus-52tut): it
/// failed, was blocked, reported, or was skipped. The same `wake.noticed`
/// every surface reads for a reminder, with the routine entry's purpose
/// (`theseus_protocol::notices::ROUTINE_PURPOSES`), its line as the note, and
/// its answers (Run now, Pause, Details, and Add to grant when blocked).
pub struct RoutineNoticed<'a> {
    pub spec: &'a theseus_protocol::routines::RoutineSpec,
    /// The routine's series wake: the notice's id for `work.answer`.
    pub wake_id: &'a str,
    /// One of `ROUTINE_PURPOSES`.
    pub purpose: &'static str,
    pub line: &'a str,
    pub at_ms: u64,
    pub posted_to: Option<&'a str>,
    pub answers: Vec<theseus_protocol::WakeAnswer>,
}

impl Fact for RoutineNoticed<'_> {
    const METHOD: Option<&'static str> = Some(notify::WAKE_NOTICED);

    fn event(&self) -> Option<Event> {
        Some(Event::WakeNoticed(WakeNoticed {
            wake_id: self.wake_id.into(),
            short: crate::task::short(self.wake_id),
            session_id: self.spec.session_id.clone(),
            execution_id: self.spec.execution_id.clone(),
            purpose: self.purpose.into(),
            note: self.line.into(),
            due_at_ms: self.at_ms,
            late_ms: 0,
            while_down: false,
            every: None,
            occurrence: None,
            posted_to: self.posted_to.map(str::to_string),
            home: false,
            answers: self.answers.clone(),
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, self.line.to_string());
    }
}
