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
