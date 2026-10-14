//! A watch's end as a fact (theseus-f2r2b): it was met, it gave up, or it
//! keeps failing. The ledger row is the one the kernel wrote in the same
//! frame (`watch.met`, `watch.expired`, `watch.failing`), so this fact has the
//! rest: the `wake.noticed` notification every surface reads (the ping rule
//! makes a met watch an Interrupt with sound and the others an Inform:
//! `theseus_protocol::notices::wake_urgency`), and the sentence in the
//! narrative.

use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, WakeNoticed};

use super::{Fact, Say};

/// How a watch ended its checking, or told of a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Met,
    Expired,
    Failing,
}

impl Ending {
    /// The notice's purpose, which the ping rule reads.
    pub fn purpose(self) -> &'static str {
        match self {
            Ending::Met => "watch.met",
            Ending::Expired => "watch.expired",
            Ending::Failing => "watch.failing",
        }
    }
}

pub struct WatchEnded<'a> {
    pub ending: Ending,
    pub wake_id: &'a str,
    pub session_id: &'a str,
    pub execution_id: &'a str,
    /// What the owner reads: the message, whole.
    pub text: &'a str,
    pub at_ms: u64,
    /// Where the post went; none when no place hears it or a turn carries it.
    pub posted_to: Option<&'a str>,
    /// It lives in the owner's Home conversation.
    pub home: bool,
}

impl Fact for WatchEnded<'_> {
    const METHOD: Option<&'static str> = Some(notify::WAKE_NOTICED);

    fn event(&self) -> Option<Event> {
        Some(Event::WakeNoticed(WakeNoticed {
            wake_id: self.wake_id.into(),
            short: crate::task::short(self.wake_id),
            session_id: self.session_id.into(),
            execution_id: self.execution_id.into(),
            purpose: self.ending.purpose().into(),
            note: self.text.into(),
            due_at_ms: self.at_ms,
            late_ms: 0,
            while_down: false,
            every: None,
            occurrence: None,
            posted_to: self.posted_to.map(str::to_string),
            home: self.home,
            answers: vec![],
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let to = match self.posted_to {
            Some(t) => format!("posted to {t}"),
            None => "no post; the surfaces were told".into(),
        };
        say.line(
            Session,
            format!(
                "Watch {} {}: {} ({to}). No model call.",
                crate::task::short(self.wake_id),
                match self.ending {
                    Ending::Met => "met",
                    Ending::Expired => "gave up",
                    Ending::Failing => "is failing",
                },
                crate::session::title_from(self.text),
            ),
        );
    }
}
