//! Esc stops, and Ctrl-Enter stops and sends (theseus-wy6v.1). While a turn
//! runs, Esc sends `execution.stop` (as `theseus stop` does) at once; the
//! transcript then says what ended, and what the daemon verified, and asks
//! what to do instead. Esc has other uses first: it leaves a note, goes back
//! to typing from a question, and closes the command list. With nothing
//! running it only clears the notice, and the double Esc is left free for a
//! later row (rewind).

use serde_json::json;
use theseus_protocol::{method, CancelVerdict, ExecutionStopResult};

use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::Sty;

impl Chat {
    /// Esc.
    pub(super) fn escape(&mut self) -> Vec<Effect> {
        if self.escape_question() {
            return Vec::new();
        }
        if !self.suggestions().is_empty() {
            self.menu = 0;
            return Vec::new();
        }
        if self.live.turn.is_some() {
            return self.stop_turn();
        }
        self.menu = 0;
        self.notice = None;
        Vec::new()
    }

    /// The stop, sent now: the turn that runs ends at its next step, the
    /// calls in flight are told to stop, and the daemon says how each ended.
    pub(super) fn stop_turn(&mut self) -> Vec<Effect> {
        let Some(execution) = self.execution.clone() else {
            self.flash("nothing to stop yet · the conversation has no execution");
            return Vec::new();
        };
        vec![call(
            method::EXECUTION_STOP,
            json!({ "execution_id": execution, "author": AUTHOR }),
            Purpose::Stop,
        )]
    }

    /// The stop's answer.
    pub(super) fn stopped(&mut self, r: &ExecutionStopResult) {
        if !r.stopped {
            self.flash("nothing was running");
            return;
        }
        let mut words = "stopped by you (esc)".to_string();
        if let Some(v) = verified(&r.verdicts) {
            words.push_str(" · ");
            words.push_str(&v);
        }
        self.gap();
        self.say_words(Sty::Yellow, &words);
        if self.queued.is_empty() {
            self.say_plain(
                Sty::Bold,
                "What should it do instead? (enter: nothing · ↑: edit your last message)",
            );
        } else {
            self.say_plain(Sty::Dim, "your queued message goes next");
        }
    }
}

/// What the stop verified, in words: `verified: 2 processes ended`, or what
/// it could not verify.
fn verified(verdicts: &[CancelVerdict]) -> Option<String> {
    if verdicts.is_empty() {
        return None;
    }
    if let Some(bad) = verdicts.iter().find(|v| v.state != "termination_verified") {
        return Some(format!("not verified: {}", bad.words()));
    }
    let n: u32 = verdicts.iter().filter_map(|v| v.killed).sum();
    Some(match n {
        0 => "verified: nothing left running".to_string(),
        1 => "verified: 1 process ended".to_string(),
        n => format!("verified: {n} processes ended"),
    })
}
