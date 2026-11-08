//! A muted call (theseus-k2sw, `Command::Mute`): the engine stops hearing it
//! and goes on speaking.
//!
//! - **The door shuts** (`at_the_door`): no frame of the call, a listed
//!   speaker's or not, reaches the VAD, the echo canceller, the barge-in's
//!   count or the streaming listener. So nothing is sent to be transcribed
//!   and nothing talks over Theseus: a muted speaker is never a barge-in.
//!   What Theseus is saying plays to its end, and replies and reports
//!   queue and are spoken as they were. `/stop` is what silences it.
//! - **What was open is dropped, unheard.** A sentence the mute landed in the
//!   middle of is not a turn. On the pause rules' path its VAD closes on the
//!   silence that follows and its audio is never sent; a transcription already
//!   asked for settles as [`HeardAs::Muted`], its words discarded. On the
//!   streaming listener's path the listener, which has the audio already, goes
//!   on being fed silence and ends the turn: its `End`, whether it comes while
//!   muted or after the unmute, is not an utterance (`Lane::muted_turn`,
//!   `flux_end`), and the audio it streamed is booked.
//! - **A rejoin keeps it.** Mute is the engine's state, so the call's
//!   connection coming back (the deaf watch's rejoin) is still muted.

use tokio::time::Instant;

use super::Engine;

impl Engine {
    /// `Command::Mute(on)`. Muting an already muted call, or unmuting one that
    /// is not, changes nothing.
    pub(super) fn mute(&mut self, on: bool, _now: Instant) {
        if self.muted == on {
            return;
        }
        self.muted = on;
        if !on {
            return;
        }
        // Every utterance begun before this tick is one the mute landed in.
        self.muted_at = Some(self.ticks);
        self.talk_frames.clear();
        self.opening.clear();
        self.flux_muted();
    }

    /// Whether an utterance whose audio began at `first_tick` began before the
    /// last mute: the mute landed in it, so its words are not heard.
    pub(super) fn before_mute(&self, first_tick: u64) -> bool {
        self.muted_at.is_some_and(|at| first_tick < at)
    }
}
