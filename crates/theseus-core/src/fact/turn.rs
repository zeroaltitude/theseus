//! The turn's facts: a turn, its loops, and its model calls (`turn.rs`).

use serde_json::{json, Value};
use theseus_protocol::{notify, Event, NarrativePart::Turn};

use super::{Fact, Say};
use crate::narrative;
use crate::trace::Trace;
use crate::turn::Target;

/// A wait for admission and the turn lock the narrative mentions.
pub const LOCK_WAIT_NOTICEABLE_US: u64 = 50_000;

/// A turn began, admitted under its execution's lock (`turn.started`), and
/// what it waited for first: the vault's word on the config, its secrets,
/// and admission.
pub struct TurnStarted<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub execution_id: &'a str,
    /// The kernel's count of the execution's turns.
    pub kernel_turn: u64,
    pub target: &'a Target,
    /// No input: the driver's continuation.
    pub continuation: bool,
    /// The input's length in characters; `None` for a continuation.
    pub input_chars: Option<usize>,
    pub attachments: usize,
    /// Who asked: a client (`web#3`), or `harness`.
    pub author: &'a str,
    /// What it waited for, in microseconds from its arrival: the vault's
    /// confirmation of the config (theseus-2fo), its secrets (theseus-qa0),
    /// arrival to admission (the others included), and admission and the
    /// turn lock alone.
    pub config_us: u64,
    pub secrets_us: u64,
    pub lock_us: u64,
    pub admit_us: u64,
}

impl Fact for TurnStarted<'_> {
    const KIND: Option<&'static str> = Some("turn.started");
    const METHOD: Option<&'static str> = Some(notify::TURN_STARTED);

    fn row(&self) -> Value {
        let t = self.target;
        let mut row = json!({"input_chars": self.input_chars, "profile": t.profile, "provider": t.provider, "model": t.model, "execution_id": self.execution_id, "kernel_turn": self.kernel_turn, "continuation": self.continuation, "author": self.author});
        if self.attachments > 0 {
            row["attachments"] = json!(self.attachments);
        }
        row
    }

    fn event(&self) -> Option<Event> {
        Some(Event::TurnStarted(theseus_protocol::TurnStarted {
            session_id: self.session_id.into(),
            turn_id: self.turn_id.into(),
            execution_id: Some(self.execution_id.into()),
            continuation: self.continuation,
        }))
    }

    fn span(&self, trace: &mut Trace) {
        if self.config_us > 0 {
            trace.record(
                "config.wait",
                "lock",
                0,
                self.config_us,
                json!({"note": "the vault's confirmation of the config this start served from (theseus-2fo)"}),
            );
        }
        let secrets_end = self.config_us + self.secrets_us;
        if self.secrets_us > 0 {
            trace.record(
                "secrets.wait",
                "lock",
                self.config_us,
                secrets_end,
                json!({"provider": self.target.provider, "note": "the provider's key and the first round of secrets (theseus-qa0)"}),
            );
        }
        trace.record(
            "admission.wait",
            "lock",
            secrets_end,
            self.lock_us,
            json!({"execution_id": self.execution_id, "turn": self.kernel_turn, "note": "kernel admission + per-execution turn lock"}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let t = self.target;
        if self.config_us > 0 {
            say.line(
                Turn,
                format!(
                    "Waited {} for the vault to confirm the config this daemon started from:                  nothing acts on its copy's word.",
                    narrative::duration(self.config_us / 1000)
                ),
            );
        }
        if self.secrets_us > 0 {
            say.line(
                Turn,
                format!(
                    "Waited {} for the vault: {} needs its key, and every secret must be known \
                     before a tool result is scrubbed.",
                    narrative::duration(self.secrets_us / 1000),
                    t.provider
                ),
            );
        }
        let with_files = match self.attachments {
            0 => String::new(),
            n => format!(
                " and {}",
                narrative::count(n as u64, "attachment", "attachments")
            ),
        };
        let loops = narrative::count(t.max_loops as u64, "loop", "loops");
        say.line(
            Turn,
            match self.input_chars {
                Some(chars) => format!(
                    "Turn {} started by {} on {} ({}): {} of input{with_files}; up to {loops}.",
                    narrative::short(self.turn_id),
                    self.author,
                    t.profile,
                    t.model,
                    narrative::count(chars as u64, "character", "characters"),
                ),
                None => format!(
                    "Continuation turn {} started by the {} on {} ({}): no new input; up to \
                     {loops}.",
                    narrative::short(self.turn_id),
                    self.author,
                    t.profile,
                    t.model,
                ),
            },
        );
        if self.admit_us >= LOCK_WAIT_NOTICEABLE_US {
            say.line(
                Turn,
                format!(
                    "It waited {} for admission and the turn lock.",
                    narrative::duration(self.admit_us / 1000)
                ),
            );
        }
    }
}
