//! What the seam sends around a clip (theseus-9d9aq): the lead-in before it,
//! and the warm silence after it.
//!
//! songbird sends audio only while a track plays: once every track has ended
//! it sends five silent frames and stops. With nothing sent, Discord's
//! speaking state can drop and the listener's jitter buffer drains, so the
//! head of the next sound is eaten. A reply is spoken a sentence at a time,
//! and the next sentence often starts a few hundred ms after the last one
//! ended (longer under load), so the stream is kept warm inside a spell: each
//! clip is played with a silent track beside it, paused, that its end starts
//! ([`AfterClip`]). The warm track plays [`WARM`] of zeros, past
//! [`SPELL_GAP`], and is never the engine's: its end tells the engine nothing.
//!
//! The lead-in ([`Spell::begin`]): a clip that starts a spell gets the full
//! lead-in; one that starts inside a spell while the warm silence plays gets
//! none; one that starts inside a spell with nothing sent before it (the warm
//! track failed or was stopped) gets the short one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::{Audio, FRAME, FRAME_SAMPLES};
use crate::io::ClipId;

/// A clip that begins this long after the last one ended starts a new spell.
pub(crate) const SPELL_GAP: Duration = Duration::from_millis(500);

/// The silence played after each clip's end. Longer than [`SPELL_GAP`]: the
/// spell reads the end when the engine hears it, after songbird played it, so
/// a clip begun just inside the gap still starts under the warm silence.
pub(crate) const WARM: Duration = Duration::from_millis(700);

/// The silence put before a clip, as the binding configures it: `spell`
/// before the first clip after quiet (`[voice] lead_in_ms`), `gap` before a
/// later one that nothing kept warm (`[voice] gap_lead_in_ms`). Zero is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeadIn {
    pub spell: Duration,
    pub gap: Duration,
}

/// `d` in whole frames, rounded up.
fn frames(d: Duration) -> u32 {
    d.as_nanos().div_ceil(FRAME.as_nanos()) as u32
}

/// `clip` after `frames` silent frames.
pub(crate) fn led_in(clip: &Audio, frames: u32) -> Audio {
    let mut samples = vec![0; frames as usize * FRAME_SAMPLES];
    samples.extend_from_slice(clip.samples());
    Audio::new(samples)
}

/// The warm track's audio: [`WARM`] of zeros.
pub(crate) fn warm_silence() -> Audio {
    Audio::new(vec![0; frames(WARM) as usize * FRAME_SAMPLES])
}

/// Whether the bot is speaking, and how much silence the next clip needs.
pub(crate) struct Spell {
    /// Silent frames before the first clip of a spell, so the listener's
    /// jitter buffer and Discord's speaking-state flip don't eat its head
    /// ("Theseus here" heard as "seus here", theseus-yf9ct).
    pub(crate) lead_in: u32,
    /// Silent frames before a later clip that nothing kept warm.
    pub(crate) gap_lead_in: u32,
    pub(crate) clip: Option<ClipId>,
    quiet_since: Option<Instant>,
}

impl Spell {
    pub(crate) fn new(lead: LeadIn) -> Self {
        Self {
            lead_in: frames(lead.spell),
            gap_lead_in: frames(lead.gap),
            clip: None,
            quiet_since: None,
        }
    }

    /// Clip `id` begins at `now`, with the warm silence playing or not: the
    /// silent frames to put before it.
    pub(crate) fn begin(&mut self, id: ClipId, now: Instant, warm: bool) -> u32 {
        let frames = match self.quiet_since {
            _ if self.clip.is_some() => 0,
            Some(since) if now.saturating_duration_since(since) < SPELL_GAP => {
                if warm {
                    0
                } else {
                    self.gap_lead_in
                }
            }
            _ => self.lead_in,
        };
        self.clip = Some(id);
        frames
    }

    /// Clip `id` ended or was stopped (an earlier clip's end, after a later
    /// began, changes nothing).
    pub(crate) fn end(&mut self, id: ClipId, now: Instant) {
        if self.clip == Some(id) {
            self.clip = None;
            self.quiet_since = Some(now);
        }
    }
}

/// A warm track, as the seam drives it: songbird's `TrackHandle`, or a test's.
pub(crate) trait Silence {
    /// Start it; false when it is gone.
    fn play(&self) -> bool;
    fn stop(&self);
}

/// Which clip's warm silence plays now, shared with songbird's event
/// handlers: 0 for none, else the clip's id plus one.
#[derive(Clone, Default)]
pub(crate) struct Warmth(Arc<AtomicU64>);

impl Warmth {
    pub(crate) fn on(&self) -> bool {
        self.0.load(Ordering::SeqCst) != 0
    }

    pub(crate) fn began(&self, id: ClipId) {
        self.0.store(id.0 + 1, Ordering::SeqCst);
    }

    /// Clip `id`'s warm track ended: a later clip's warmth stays.
    pub(crate) fn ended(&self, id: ClipId) {
        let _ = self
            .0
            .compare_exchange(id.0 + 1, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// What a clip's end does before the engine hears it: start its warm track.
pub(crate) struct AfterClip<S> {
    pub(crate) id: ClipId,
    pub(crate) warm: Option<S>,
    pub(crate) warmth: Warmth,
}

impl<S: Silence> AfterClip<S> {
    pub(crate) fn ended(&self) {
        if self.warm.as_ref().is_some_and(Silence::play) {
            self.warmth.began(self.id);
        }
    }

    /// The clip could not be watched for its end, so nothing starts its warm
    /// track: stop it, or it would stay paused in the mixer for the call.
    pub(crate) fn abandoned(&self) {
        if let Some(warm) = &self.warm {
            warm.stop();
        }
    }
}

/// The warm tracks the seam may still have to stop: the last few clips'.
pub(crate) struct Warm<S> {
    pub(crate) warmth: Warmth,
    tracks: Vec<S>,
}

/// Warm tracks kept for a stop; an older one has played out.
const KEPT: usize = 4;

impl<S: Silence> Warm<S> {
    pub(crate) fn new() -> Self {
        Self {
            warmth: Warmth::default(),
            tracks: Vec::new(),
        }
    }

    pub(crate) fn add(&mut self, track: S) {
        if self.tracks.len() == KEPT {
            self.tracks.remove(0).stop();
        }
        self.tracks.push(track);
    }

    /// A barge-in: no warm silence plays on, and none starts.
    pub(crate) fn stop(&mut self) {
        for track in self.tracks.drain(..) {
            track.stop();
        }
        self.warmth = Warmth::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A warm track: whether it plays, and whether it is gone.
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<(bool, bool)>>);

    impl Silence for Fake {
        fn play(&self) -> bool {
            let mut s = self.0.lock().unwrap();
            if s.1 {
                return false;
            }
            s.0 = true;
            true
        }
        fn stop(&self) {
            *self.0.lock().unwrap() = (false, true);
        }
    }

    impl Fake {
        fn playing(&self) -> bool {
            self.0.lock().unwrap().0
        }
    }

    /// A track that is kept out of the last `KEPT` is stopped, not just forgotten.
    #[test]
    fn an_evicted_warm_track_is_stopped() {
        let mut warm = Warm::new();
        let tracks: Vec<Fake> = (0..=KEPT).map(|_| Fake::default()).collect();
        for t in &tracks {
            t.play();
            warm.add(t.clone());
        }
        assert!(!tracks[0].playing(), "the oldest is stopped");
        assert!(tracks[1..].iter().all(Fake::playing), "the rest play on");
    }

    #[test]
    fn a_clip_whose_end_event_fails_stops_its_warm_track() {
        let track = Fake::default();
        let after = AfterClip {
            id: ClipId(0),
            warm: Some(track.clone()),
            warmth: Warmth::default(),
        };
        after.abandoned();
        assert!(!track.playing());
        after.ended();
        assert!(
            !track.playing() && !after.warmth.on(),
            "nothing starts it later"
        );
    }

    fn lead(spell: u64, gap: u64) -> LeadIn {
        LeadIn {
            spell: Duration::from_millis(spell),
            gap: Duration::from_millis(gap),
        }
    }

    /// The owner's case: a reply's second sentence starts 300 ms after the
    /// first ended. The warm silence covers the gap, so something is sent
    /// before its first sample; without it the clip carries the short lead-in.
    #[test]
    fn a_later_sentence_after_a_short_gap_is_kept_warm_and_led_in() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        let gap = ms(300);
        assert!(
            WARM > SPELL_GAP && WARM > gap,
            "the warm silence spans the gap"
        );
        assert_eq!(warm_silence().len(), 35 * FRAME_SAMPLES);
        let chime = Audio::chime();
        for warm_plays in [true, false] {
            let mut warm = Warm::new();
            let mut spell = Spell::new(lead(600, 200));
            assert_eq!(spell.begin(ClipId(0), t0, warm.warmth.on()), 30);
            let track = Fake::default();
            warm.add(track.clone());
            if !warm_plays {
                track.stop();
            }
            let after = AfterClip {
                id: ClipId(0),
                warm: Some(track.clone()),
                warmth: warm.warmth.clone(),
            };
            after.ended();
            spell.end(ClipId(0), t0 + ms(1200));
            assert_eq!(track.playing(), warm_plays);
            let frames = spell.begin(ClipId(1), t0 + ms(1200) + gap, warm.warmth.on());
            // What is sent after the gap begins: the warm zeros, or the clip
            // led in. Either way its first sample is not the clip's first.
            let sent = led_in(&chime, frames);
            let warm_sent = if track.playing() {
                warm_silence().len()
            } else {
                0
            };
            assert!(
                warm_sent + (sent.len() - chime.len()) > 0,
                "warm {warm_plays}: something before the clip's first sample"
            );
            if warm_plays {
                assert_eq!(frames, 0, "the warm silence is the lead-in");
            } else {
                assert_eq!(frames, 10, "200 ms of silence when nothing was warm");
                assert!(sent.samples()[..10 * FRAME_SAMPLES].iter().all(|&s| s == 0));
                assert_eq!(&sent.samples()[10 * FRAME_SAMPLES..], chime.samples());
            }
        }
    }

    #[test]
    fn a_spell_after_quiet_still_gets_the_full_lead_in() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        let mut spell = Spell::new(lead(600, 200));
        assert_eq!(spell.begin(ClipId(0), t0, false), 30);
        let mut ended = t0 + ms(1000);
        spell.end(ClipId(0), ended);
        for (n, (after, warm)) in [(ms(500), false), (ms(900), true), (ms(5000), false)]
            .into_iter()
            .enumerate()
        {
            let id = ClipId(n as u64 + 1);
            assert_eq!(
                spell.begin(id, ended + after, warm),
                30,
                "after {after:?} of quiet"
            );
            ended += after + ms(800);
            spell.end(id, ended);
        }
        // Inside the gap, never the full lead-in.
        assert_eq!(spell.begin(ClipId(9), ended + ms(499), false), 10);
    }

    /// A barge-in during the warm gap stops every warm track, the warmth goes
    /// out, and the warm track's end tells the engine nothing: its handler is
    /// `Warmth::ended`, which holds no channel.
    #[test]
    fn a_stop_ends_the_warm_silence() {
        let mut warm = Warm::new();
        let tracks: Vec<Fake> = (0..6).map(|_| Fake::default()).collect();
        for (n, track) in tracks.iter().enumerate() {
            warm.add(track.clone());
            AfterClip {
                id: ClipId(n as u64),
                warm: Some(track.clone()),
                warmth: warm.warmth.clone(),
            }
            .ended();
        }
        assert!(warm.warmth.on());
        let old = warm.warmth.clone();
        warm.stop();
        assert!(!warm.warmth.on(), "the next clip reads no warmth");
        assert!(tracks[2..].iter().all(|t| !t.playing()), "nothing plays on");
        // A stopped clip's late end starts no warm silence.
        let late = AfterClip {
            id: ClipId(5),
            warm: Some(tracks[5].clone()),
            warmth: warm.warmth.clone(),
        };
        late.ended();
        assert!(!tracks[5].playing() && !warm.warmth.on());
        // An earlier clip's warm end leaves a later clip's warmth alone.
        old.ended(ClipId(4));
        assert!(old.on());
        old.ended(ClipId(5));
        assert!(!old.on());
    }
}
