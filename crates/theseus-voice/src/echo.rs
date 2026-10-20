//! The engine's side of echo cancellation (theseus-yaxcp): Theseus's own
//! outgoing audio, tick by tick as it plays, and a canceller per listener
//! that takes it out of what they send (`aec.rs`). The engine judges a
//! barge-in on what is left.

use std::collections::BTreeMap;
use std::time::Duration;

use tokio::time::Instant;

use crate::aec::{Aec, Far, Residual};
use crate::audio::{samples_in, Audio, FRAME, FRAME_SAMPLES};
use crate::io::Speaker;

/// A clip on its way out: its samples, and how far it has gone. A lead-in of
/// silence the seam sent before it is a negative start.
struct Sent {
    clip: Audio,
    at: isize,
}

pub(crate) struct Echo {
    on: bool,
    /// The clips on their way out: a clip begun in the 20 ms its predecessor
    /// ended in shares its tick.
    sent: Vec<Sent>,
    /// When the last tick came: a clip begins within the next one's 20 ms.
    ticked: Option<Instant>,
    aecs: BTreeMap<Speaker, Aec>,
    /// Each listener's spell of cancelling, for the journal.
    spells: BTreeMap<Speaker, Spell>,
}

/// One listener's spell of cancelling: from Theseus's sound to its quiet.
#[derive(Default)]
struct Spell {
    ticks: u32,
    erle_db: f32,
    delay_ms: Option<u32>,
}

impl Echo {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            sent: Vec::new(),
            ticked: None,
            aecs: BTreeMap::new(),
            spells: BTreeMap::new(),
        }
    }

    /// `clip` begins at `now`, after `lead_in` samples of silence: part of
    /// the way into the next tick's frame.
    pub fn play(&mut self, clip: &Audio, lead_in: usize, now: Instant) {
        if self.on {
            let into = self.ticked.map_or(Duration::ZERO, |t| (now - t).min(FRAME));
            self.sent.push(Sent {
                clip: clip.clone(),
                at: -((lead_in + samples_in(into)) as isize),
            });
        }
    }

    /// The clip playing was stopped.
    pub fn stop(&mut self) {
        self.sent.clear();
    }

    /// One tick: what Theseus sent in it, taken out of each listener's frame.
    /// A listener with a residual is one Theseus's voice may be in; the rest
    /// pass through as they came.
    pub fn tick(
        &mut self,
        heard: &BTreeMap<Speaker, &[i16]>,
        now: Instant,
    ) -> BTreeMap<Speaker, Residual> {
        if !self.on {
            return BTreeMap::new();
        }
        self.ticked = Some(now);
        let far = self.next_far();
        for speaker in heard.keys() {
            self.aecs.entry(*speaker).or_insert_with(Aec::new);
        }
        let mut residuals = BTreeMap::new();
        for (speaker, aec) in &mut self.aecs {
            let mic = heard.get(speaker).copied();
            match aec.step(far.as_ref(), mic) {
                Some(residual) => {
                    let spell = self.spells.entry(*speaker).or_default();
                    spell.ticks += 1;
                    spell.erle_db += residual.erle_db;
                    if residual.delay_ms != spell.delay_ms {
                        tracing::info!(
                            speaker = speaker.0,
                            delay_ms = ?residual.delay_ms,
                            "voice: the echo's delay"
                        );
                        spell.delay_ms = residual.delay_ms;
                    }
                    residuals.insert(*speaker, residual);
                }
                None => {
                    if let Some(spell) = self.spells.remove(speaker) {
                        tracing::info!(
                            speaker = speaker.0,
                            ticks = spell.ticks,
                            mean_erle_db = spell.erle_db / spell.ticks.max(1) as f32,
                            "voice: an echo spell ended"
                        );
                    }
                }
            }
        }
        residuals
    }

    /// The next 20 ms of what is playing, if anything is.
    fn next_far(&mut self) -> Option<Far> {
        if self.sent.is_empty() {
            return None;
        }
        let mut frame = [0i16; FRAME_SAMPLES];
        for sent in &mut self.sent {
            let samples = sent.clip.samples();
            for (i, slot) in frame.iter_mut().enumerate() {
                let at = usize::try_from(sent.at + i as isize).ok();
                *slot =
                    slot.saturating_add(at.and_then(|at| samples.get(at)).copied().unwrap_or(0));
            }
            sent.at += FRAME_SAMPLES as isize;
        }
        self.sent.retain(|s| s.at < s.clip.len() as isize);
        Some(Far::new(&frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip that begins in the tick its predecessor ends in shares it:
    /// the predecessor's last samples aren't dropped for the new clip.
    #[tokio::test(start_paused = true)]
    async fn a_clip_begun_as_the_last_one_ends_shares_its_tick() {
        let mut echo = Echo::new(true);
        let first = Audio::new(vec![1000; FRAME_SAMPLES * 3 / 2]);
        let mut now = Instant::now();
        echo.tick(&BTreeMap::new(), now);
        echo.play(&first, 0, now);
        now += FRAME;
        echo.tick(&BTreeMap::new(), now);
        assert_eq!(echo.sent.len(), 1, "the first has half a frame left");
        echo.play(&first, 0, now + Duration::from_millis(10));
        assert_eq!(echo.sent.len(), 2, "both are on their way");
        now += FRAME;
        echo.tick(&BTreeMap::new(), now);
        assert_eq!(echo.sent.len(), 1, "the first ended, the second goes on");
        assert_eq!(
            echo.sent[0].at,
            FRAME_SAMPLES as isize - (FRAME_SAMPLES / 2) as isize
        );
        echo.stop();
        assert!(echo.next_far().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn off_it_takes_nothing_in() {
        let mut echo = Echo::new(false);
        echo.play(&Audio::new(vec![1000; FRAME_SAMPLES]), 0, Instant::now());
        assert!(echo.sent.is_empty());
        assert!(echo.tick(&BTreeMap::new(), Instant::now()).is_empty());
    }
}
