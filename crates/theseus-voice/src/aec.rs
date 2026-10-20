//! Echo cancellation (theseus-yaxcp), pure DSP: one [`Aec`] per listener
//! takes what Theseus sent, as played, and what the listener's stream carried,
//! and leaves what is not Theseus's own voice.
//!
//! On speakers the stream carries Theseus's voice again: later by 100 to
//! 300 ms (varying per call), quieter, and filtered. The delay is found by
//! correlating the 10 ms energy envelopes of the two (lags of 40 to 500 ms,
//! over the last second), and an NLMS filter of 48 ms around it learns the
//! rest of the path and subtracts it. All of it runs at 8 kHz, so a frame
//! costs a few hundred microseconds at most. While Theseus has been silent for
//! a second ([`QUIET`]) there is nothing to cancel: [`Aec::step`] returns
//! `None`, and nothing is computed. The delay and the filter's taps are kept
//! for the next time Theseus speaks, since the room has not changed.

use std::collections::VecDeque;

use crate::audio::FRAME_SAMPLES;

/// 48 kHz down to 8 kHz.
const DECIMATION: usize = 6;
/// Samples per 20 ms frame at 8 kHz.
pub(crate) const FRAME8: usize = FRAME_SAMPLES / DECIMATION;
/// Samples in an envelope frame: 10 ms.
const ENVELOPE: usize = FRAME8 / 2;
/// The delays searched, in envelope frames: 40 to 500 ms.
const MIN_LAG: usize = 4;
const MAX_LAG: usize = 50;
/// The envelope frames a delay is judged on (1 s), and the fewest that may.
const WINDOW: usize = 100;
const MIN_FRAMES: usize = 50;
/// Taps of the adaptive filter (48 ms), the part of them before the estimated
/// delay (20 ms: the estimate is as coarse as an envelope frame, and the
/// playout starts anywhere within a tick), and the history they need.
const TAPS: usize = 384;
const LEAD: usize = FRAME8;
const HISTORY: usize = MAX_LAG * ENVELOPE + TAPS + FRAME8;
/// NLMS's step, and the pre-emphasis its adaptation sees, which whitens
/// speech so the taps converge at the same pace in every band.
const STEP: f32 = 0.5;
const PRE_EMPHASIS: f32 = 0.9;
/// Added to the energy NLMS divides by, so a quiet stretch of Theseus's voice
/// (an echo's tail, a pause) can't take the taps a long step: an RMS of 50.
const REGULARIZER: f32 = TAPS as f32 * 2500.0;
/// A delay is found at this correlation, and a better one replaces it when
/// it beats it by `SWITCH_MARGIN` twice running.
const LOCK_CORRELATION: f32 = 0.5;
const SWITCH_MARGIN: f32 = 0.15;
/// A frame whose RMS (16-bit units) reaches this carries Theseus's voice.
const FAR_FLOOR: f32 = 10.0;
/// A frame louder than this many times the echo expected (plus `NEAR_FLOOR`)
/// holds the near end's voice too: the filter keeps what it has.
const NEAR_RATIO: f32 = 2.5;
const NEAR_FLOOR: f32 = 30.0;
/// Ticks of silence from Theseus after which there is nothing to cancel: 1 s,
/// the longest delay and a room's tail.
pub(crate) const QUIET: u32 = 50;
/// Ticks, from Theseus's first sound, within which a listener's frames can't
/// yet be told from echo if no delay was found: 800 ms.
const WARM_UP: u32 = 40;
/// Ticks after a delay is first found that the filter is still converging,
/// and what is left of an echo can't be told from a voice: 500 ms.
const SETTLE: u32 = 25;

/// Theseus's outgoing frame of one tick, at 8 kHz.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Far([f32; FRAME8]);

impl Far {
    /// A tick of Theseus's silence.
    pub const SILENT: Far = Far([0.0; FRAME8]);

    /// A 20 ms frame of 48 kHz samples; short ones are padded with silence.
    pub fn new(frame: &[i16]) -> Self {
        Far(decimate(frame))
    }
}

/// A frame of 48 kHz samples as 8 kHz, each the mean of six.
fn decimate(frame: &[i16]) -> [f32; FRAME8] {
    let mut out = [0.0; FRAME8];
    for (slot, chunk) in out.iter_mut().zip(frame.chunks(DECIMATION)) {
        *slot = chunk.iter().map(|&s| f32::from(s)).sum::<f32>() / DECIMATION as f32;
    }
    out
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s * s).sum()
}

fn rms(samples: &[f32]) -> f32 {
    (energy(samples) / samples.len().max(1) as f32).sqrt()
}

/// What is left of one listener's frame once Theseus's voice is taken out.
#[derive(Clone, Debug)]
pub(crate) struct Residual {
    /// 8 kHz samples, 16-bit units.
    pub samples: Vec<f32>,
    /// How far the filter brought the frame down, in dB (the echo-return-loss
    /// enhancement): 0 when it took nothing out.
    pub erle_db: f32,
    /// The delay found, in ms.
    pub delay_ms: Option<u32>,
    /// Theseus began speaking just now and no delay is found yet, or the
    /// filter is still converging on the one found: what is left of an echo
    /// can't be told from a voice.
    pub warming: bool,
}

impl Residual {
    /// The frame holds a voice that isn't Theseus's: its RMS reaches
    /// `threshold`, and it isn't a frame that can't be told from echo yet.
    pub fn speaks(&self, threshold: f64) -> bool {
        !self.warming && f64::from(rms(&self.samples)) >= threshold
    }
}

/// One listener's canceller.
pub(crate) struct Aec {
    /// Theseus's last `HISTORY` samples at 8 kHz, and the same pre-emphasised.
    far: Vec<f32>,
    far_whitened: Vec<f32>,
    /// The envelope frames of both, aligned, the last `WINDOW + MAX_LAG`.
    far_envelope: VecDeque<f32>,
    mic_envelope: VecDeque<f32>,
    /// The filter, its taps ordered from the longest lag to the shortest.
    taps: Vec<f32>,
    /// The delay in samples, and the echo's gain, once found.
    delay: Option<usize>,
    gain: f32,
    /// A better delay seen once (in envelope frames).
    candidate: Option<usize>,
    last_far: f32,
    last_error: f32,
    /// Ticks since Theseus last sounded, since it began, and since the delay
    /// was first found.
    quiet: u32,
    active: u32,
    locked: u32,
}

impl Aec {
    pub fn new() -> Self {
        Self {
            far: vec![0.0; HISTORY],
            far_whitened: vec![0.0; HISTORY],
            far_envelope: VecDeque::new(),
            mic_envelope: VecDeque::new(),
            taps: vec![0.0; TAPS],
            delay: None,
            gain: 0.0,
            candidate: None,
            last_far: 0.0,
            last_error: 0.0,
            quiet: QUIET,
            active: 0,
            locked: 0,
        }
    }

    /// One 20 ms tick: what Theseus sent in it (`None`: nothing) and what the
    /// listener's stream carried (`None`: no packet, a silent mic). The
    /// listener's frame without Theseus's voice, or `None` while there is
    /// none to take out.
    pub fn step(&mut self, far: Option<&Far>, mic: Option<&[i16]>) -> Option<Residual> {
        let far = far.unwrap_or(&Far::SILENT);
        self.quiet = if rms(&far.0) >= FAR_FLOOR {
            0
        } else {
            self.quiet.saturating_add(1)
        };
        if self.quiet >= QUIET {
            self.active = 0;
            return None;
        }
        if self.active == 0 {
            self.begin();
        }
        self.active += 1;
        if self.delay.is_some() {
            self.locked = self.locked.saturating_add(1);
        }
        self.push_far(&far.0);
        let mic = mic.map_or([0.0; FRAME8], decimate);
        let samples = self.cancel(&mic);
        let (mic_energy, left) = (energy(&mic), energy(&samples));
        // A subtraction that adds energy (a stale delay, a double talk the
        // filter hasn't caught) is no help: the frame is left as it came.
        let (samples, left) = if left > mic_energy {
            (mic.to_vec(), mic_energy)
        } else {
            (samples, left)
        };
        self.push_envelopes(&far.0, &mic);
        if self.active.is_multiple_of(5) {
            self.estimate();
        }
        Some(Residual {
            samples,
            erle_db: 10.0 * ((mic_energy + 1.0) / (left + 1.0)).log10(),
            delay_ms: self.delay.map(|d| (d * 1000 / 8000) as u32),
            warming: match self.delay {
                None => self.active <= WARM_UP,
                Some(_) => self.locked <= SETTLE,
            },
        })
    }

    /// Theseus sounds again after quiet: the history starts afresh, and the
    /// delay and taps are kept.
    fn begin(&mut self) {
        self.far.fill(0.0);
        self.far_whitened.fill(0.0);
        self.far_envelope.clear();
        self.mic_envelope.clear();
        self.candidate = None;
        self.last_far = 0.0;
        self.last_error = 0.0;
    }

    fn push_far(&mut self, far: &[f32; FRAME8]) {
        self.far.copy_within(FRAME8.., 0);
        self.far_whitened.copy_within(FRAME8.., 0);
        let at = HISTORY - FRAME8;
        for (i, &x) in far.iter().enumerate() {
            self.far[at + i] = x;
            self.far_whitened[at + i] = x - PRE_EMPHASIS * self.last_far;
            self.last_far = x;
        }
    }

    /// The frame with the filter's echo taken out, the filter learning from
    /// what is left unless the near end is talking too.
    fn cancel(&mut self, mic: &[f32; FRAME8]) -> Vec<f32> {
        let Some(delay) = self.delay else {
            return mic.to_vec();
        };
        let first = HISTORY - FRAME8;
        // Tap 0 sees the sample `delay - LEAD + TAPS - 1` before the one
        // being cleaned, and the last tap `delay - LEAD`.
        let reach = delay - LEAD + TAPS - 1;
        let expected = self.gain * rms(&self.far[first - delay..HISTORY - delay]);
        let near = rms(mic) > NEAR_RATIO * expected + NEAR_FLOOR;
        let mut window_energy = energy(&self.far_whitened[first - reach..first - reach + TAPS]);
        let mut out = Vec::with_capacity(FRAME8);
        for (n, &m) in mic.iter().enumerate() {
            let base = first + n - reach;
            let heard = &self.far[base..base + TAPS];
            let echo: f32 = self.taps.iter().zip(heard).map(|(w, x)| w * x).sum();
            let error = m - echo;
            let whitened = error - PRE_EMPHASIS * self.last_error;
            self.last_error = error;
            out.push(error);
            let seen = &self.far_whitened[base..base + TAPS];
            if !near {
                let k = STEP * whitened / (window_energy + REGULARIZER);
                for (w, x) in self.taps.iter_mut().zip(seen) {
                    *w += k * x;
                }
            }
            // The window slides by one sample.
            let (old, new) = (self.far_whitened[base], self.far_whitened[base + TAPS]);
            window_energy = (window_energy + new * new - old * old).max(0.0);
        }
        out
    }

    fn push_envelopes(&mut self, far: &[f32; FRAME8], mic: &[f32; FRAME8]) {
        for (f, m) in far.chunks(ENVELOPE).zip(mic.chunks(ENVELOPE)) {
            self.far_envelope.push_back(rms(f));
            self.mic_envelope.push_back(rms(m));
        }
        while self.far_envelope.len() > WINDOW + MAX_LAG {
            self.far_envelope.pop_front();
            self.mic_envelope.pop_front();
        }
    }

    /// Re-estimate the delay: the lag at which the listener's envelope best
    /// follows Theseus's over the last second. A first one is taken at
    /// `LOCK_CORRELATION`; a later one only when it beats the one in use by a
    /// margin on two estimates running.
    fn estimate(&mut self) {
        let Some((lag, correlation, gain)) = self.best_lag() else {
            return;
        };
        let Some(current) = self.delay else {
            if correlation >= LOCK_CORRELATION {
                self.delay = Some(lag * ENVELOPE);
                self.gain = gain;
            }
            return;
        };
        let held = current / ENVELOPE;
        if lag == held || correlation < LOCK_CORRELATION {
            self.candidate = None;
            if lag == held {
                self.gain = gain;
            }
            return;
        }
        if correlation < self.correlation_at(held) + SWITCH_MARGIN {
            return;
        }
        if self.candidate != Some(lag) {
            self.candidate = Some(lag);
            return;
        }
        self.candidate = None;
        self.delay = Some(lag * ENVELOPE);
        self.gain = gain;
        self.locked = 0;
        // The taps keep their lags: a shift of the window moves them within it.
        let (from, to) = (current as isize, (lag * ENVELOPE) as isize);
        let shift = to - from;
        if shift.unsigned_abs() >= TAPS {
            self.taps.fill(0.0);
        } else if shift > 0 {
            let s = shift as usize;
            self.taps.copy_within(..TAPS - s, s);
            self.taps[..s].fill(0.0);
        } else {
            let s = (-shift) as usize;
            self.taps.copy_within(s.., 0);
            self.taps[TAPS - s..].fill(0.0);
        }
    }

    fn best_lag(&self) -> Option<(usize, f32, f32)> {
        let n = self.mic_envelope.len().min(WINDOW);
        if n < MIN_FRAMES {
            return None;
        }
        (MIN_LAG..=MAX_LAG)
            .filter_map(|lag| self.at_lag(lag, n).map(|(c, g)| (lag, c, g)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn correlation_at(&self, lag: usize) -> f32 {
        let n = self.mic_envelope.len().min(WINDOW);
        self.at_lag(lag, n).map_or(0.0, |(c, _)| c)
    }

    /// The correlation of the listener's last `n` envelope frames with
    /// Theseus's `lag` frames before them, and the gain that best fits one to
    /// the other. Before the history began Theseus was silent.
    fn at_lag(&self, lag: usize, n: usize) -> Option<(f32, f32)> {
        let len = self.mic_envelope.len();
        let (mut sm, mut sf, mut smm, mut sff, mut smf) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for j in len - n..len {
            let m = self.mic_envelope[j];
            let f = j.checked_sub(lag).map_or(0.0, |k| self.far_envelope[k]);
            sm += m;
            sf += f;
            smm += m * m;
            sff += f * f;
            smf += m * f;
        }
        let n = n as f32;
        let (var_m, var_f) = (smm - sm * sm / n, sff - sf * sf / n);
        // Under 1 RMS unit of movement in either, there is nothing to follow.
        if var_m < n || var_f < n || sff <= 0.0 {
            return None;
        }
        let correlation = (smf - sm * sf / n) / (var_m * var_f).sqrt();
        Some((correlation, smf / sff))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::audio::{samples_in, Audio, SAMPLE_RATE};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// What a listener's stream carries of `far`: `delay_at(t)` later at
    /// sample `t`, `gain` times as loud, low-passed by `smoothing`.
    fn echo_of(
        far: &[i16],
        delay_at: impl Fn(usize) -> Duration,
        gain: f64,
        smoothing: f64,
    ) -> Vec<i16> {
        let mut low = 0.0;
        (0..far.len())
            .map(|t| {
                let source = t
                    .checked_sub(samples_in(delay_at(t)))
                    .map_or(0.0, |s| f64::from(far[s]));
                low = smoothing * low + (1.0 - smoothing) * source * gain;
                low.round() as i16
            })
            .collect()
    }

    fn mixed(a: &[i16], b: &[i16]) -> Vec<i16> {
        a.iter().zip(b).map(|(x, y)| x.saturating_add(*y)).collect()
    }

    /// Run `far` and `mic` through a canceller, a frame a tick: the residual
    /// of each tick's frame, and the canceller.
    fn run(aec: &mut Aec, far: &[i16], mic: &[i16]) -> Vec<Option<Residual>> {
        far.chunks(FRAME_SAMPLES)
            .zip(mic.chunks(FRAME_SAMPLES))
            .map(|(f, m)| aec.step(Some(&Far::new(f)), Some(m)))
            .collect()
    }

    /// The energy of the frames' mic, and of what was left, over `ticks`.
    fn loss_db(mic: &[i16], residuals: &[Option<Residual>], ticks: std::ops::Range<usize>) -> f32 {
        let mut before = 0.0;
        let mut after = 0.0;
        for t in ticks {
            before += energy(&decimate(&mic[t * FRAME_SAMPLES..(t + 1) * FRAME_SAMPLES]));
            after += energy(&residuals[t].as_ref().expect("a residual").samples);
        }
        10.0 * (before / after).log10()
    }

    fn voice(seed: u64, secs: u64) -> Vec<i16> {
        Audio::babble(seed, Duration::from_secs(secs), 0.1)
            .samples()
            .to_vec()
    }

    fn delay_ms(aec: &Aec) -> f32 {
        aec.delay.expect("a delay") as f32 * 1000.0 / 8000.0
    }

    #[test]
    fn the_delay_is_found_within_10_ms() {
        for delay in [100, 180, 300] {
            let far = voice(1, 6);
            let mic = echo_of(&far, |_| ms(delay), 0.25, 0.6);
            let mut aec = Aec::new();
            run(&mut aec, &far, &mic);
            let found = delay_ms(&aec);
            assert!(
                (found - delay as f32).abs() <= 10.0,
                "{delay} ms found as {found}"
            );
        }
    }

    #[test]
    fn a_delay_that_changes_is_followed_within_2_s() {
        let far = voice(2, 10);
        let change = SAMPLE_RATE as usize * 5;
        let mic = echo_of(
            &far,
            |t| if t < change { ms(120) } else { ms(260) },
            0.25,
            0.6,
        );
        let mut aec = Aec::new();
        let ticks = far.len() / FRAME_SAMPLES;
        let mut found = Vec::new();
        for t in 0..ticks {
            let at = t * FRAME_SAMPLES..(t + 1) * FRAME_SAMPLES;
            aec.step(Some(&Far::new(&far[at.clone()])), Some(&mic[at]));
            found.push(aec.delay.map(|d| d as f32 / 8.0));
        }
        let before = found[249].expect("found before the change");
        assert!((before - 120.0).abs() <= 10.0, "{before}");
        let after = found[ticks - 1 - 150].expect("found after");
        assert!(
            (after - 260.0).abs() <= 10.0,
            "260 ms followed as {after} 2 s on"
        );
    }

    #[test]
    fn the_echo_alone_comes_out_15_db_down_after_convergence() {
        let far = voice(3, 8);
        let mic = echo_of(&far, |_| ms(180), 0.25, 0.6);
        let mut aec = Aec::new();
        let residuals = run(&mut aec, &far, &mic);
        let ticks = residuals.len();
        let loss = loss_db(&mic, &residuals, ticks - 100..ticks);
        assert!(loss >= 15.0, "the last 2 s came out {loss:.1} dB down");
    }

    #[test]
    fn a_voice_over_the_echo_survives_in_what_is_left() {
        let far = voice(4, 10);
        let near = voice(5, 10);
        let echo = echo_of(&far, |_| ms(180), 0.25, 0.6);
        let begin = SAMPLE_RATE as usize * 6;
        let near: Vec<i16> = near
            .iter()
            .enumerate()
            .map(|(t, &s)| if t < begin { 0 } else { s })
            .collect();
        let mic = mixed(&echo, &near);
        let mut aec = Aec::new();
        let residuals = run(&mut aec, &far, &mic);
        let ticks = residuals.len();
        let (mut kept, mut said) = (0.0, 0.0);
        for t in ticks - 150..ticks {
            kept += energy(&residuals[t].as_ref().expect("a residual").samples);
            said += energy(&decimate(&near[t * FRAME_SAMPLES..(t + 1) * FRAME_SAMPLES]));
        }
        let kept_db = 10.0 * (kept / said).log10();
        assert!(
            kept_db.abs() <= 3.0,
            "the voice came out {kept_db:+.1} dB of what was said"
        );
    }

    #[test]
    fn nothing_comes_back_while_theseus_is_silent_and_a_second_after() {
        let mut aec = Aec::new();
        let mic = vec![900i16; FRAME_SAMPLES];
        assert!(aec.step(None, Some(&mic)).is_none());
        let far = Far::new(&voice(6, 1)[..FRAME_SAMPLES]);
        assert!(aec.step(Some(&far), Some(&mic)).is_some());
        for _ in 0..QUIET - 1 {
            assert!(aec.step(None, Some(&mic)).is_some());
        }
        assert!(aec.step(None, Some(&mic)).is_none());
    }

    /// The cost of a listener's frame, on this machine, for 60 s of audio.
    /// Reported, not asserted tightly: `cargo test --release -p theseus-voice
    /// aec::tests::the_cost -- --nocapture`.
    #[test]
    fn the_cost_of_a_frame_is_reported() {
        let far = voice(7, 60);
        let mic = echo_of(&far, |_| ms(180), 0.25, 0.6);
        let mut aec = Aec::new();
        let started = Instant::now();
        let residuals = run(&mut aec, &far, &mic);
        let per_frame = started.elapsed() / residuals.len() as u32;
        println!(
            "aec: {per_frame:?} per 20 ms frame per listener over {} frames",
            residuals.len()
        );
        assert!(
            per_frame < Duration::from_millis(20),
            "{per_frame:?} is over a frame"
        );
    }
}
