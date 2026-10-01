//! Audio as the engine handles it: 48 kHz mono 16-bit PCM, cut into 20 ms
//! frames as Discord sends it. The stand-in clips (a tone per sentence, and
//! the chime for the canned lines) and a small WAV reader and writer for the
//! tests' fixtures live here too.

use std::f64::consts::TAU;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Samples per second, in and out.
pub const SAMPLE_RATE: u32 = 48_000;
/// One frame: Discord's packet length, and the engine's tick.
pub const FRAME: Duration = Duration::from_millis(20);
/// The samples in one frame.
pub const FRAME_SAMPLES: usize = 960;

/// A run of mono samples at 48 kHz: an utterance heard, or a clip to play.
/// Cheap to clone.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Audio(Arc<[i16]>);

impl Audio {
    pub fn new(samples: Vec<i16>) -> Self {
        Self(samples.into())
    }

    pub fn samples(&self) -> &[i16] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn duration(&self) -> Duration {
        duration_of(self.0.len())
    }

    pub fn silence(length: Duration) -> Self {
        Self::new(vec![0; samples_in(length)])
    }

    /// A sine tone at `level` of full scale, faded in and out over 5 ms so it
    /// starts and stops without a click.
    pub fn tone(hz: f64, length: Duration, level: f64) -> Self {
        let n = samples_in(length);
        let fade = samples_in(Duration::from_millis(5)).min(n / 2).max(1);
        let peak = level.clamp(0.0, 1.0) * f64::from(i16::MAX);
        let samples = (0..n)
            .map(|i| {
                let envelope = (i.min(n - 1 - i) as f64 / fade as f64).min(1.0);
                let phase = TAU * hz * i as f64 / f64::from(SAMPLE_RATE);
                (peak * envelope * phase.sin()).round() as i16
            })
            .collect();
        Self::new(samples)
    }

    /// Clips one after another.
    pub fn concat(parts: &[Audio]) -> Self {
        Self::new(
            parts
                .iter()
                .flat_map(|p| p.samples().iter().copied())
                .collect(),
        )
    }

    /// The stand-in clip for the canned lines (design §2.8): a rising
    /// two-note chime, 420 ms. A canned line costs no synthesis.
    pub fn chime() -> Self {
        Self::concat(&[
            Self::tone(660.0, Duration::from_millis(160), 0.3),
            Self::silence(Duration::from_millis(40)),
            Self::tone(880.0, Duration::from_millis(220), 0.3),
        ])
    }
}

impl fmt::Debug for Audio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Audio({:.3} s)", self.duration().as_secs_f64())
    }
}

/// How many samples `length` holds, rounded down.
pub fn samples_in(length: Duration) -> usize {
    (length.as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize
}

/// How long `samples` samples last.
pub fn duration_of(samples: usize) -> Duration {
    Duration::from_nanos((samples as u128 * 1_000_000_000 / u128::from(SAMPLE_RATE)) as u64)
}

/// The root mean square of `samples`: the stand-in VAD's measure of speech.
pub fn rms(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt()
}

/// Why a WAV file was refused.
#[derive(Debug, thiserror::Error)]
pub enum WavError {
    #[error("reading the WAV file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a WAV file the stand-in reads: {0}")]
    Format(&'static str),
}

/// Read a WAV file: 16-bit PCM at 48 kHz, mono or stereo (stereo is mixed
/// down). The stand-in's fixtures are written by [`write_wav`].
pub fn read_wav(path: &Path) -> Result<Audio, WavError> {
    parse_wav(&std::fs::read(path)?)
}

/// Write `audio` as a 16-bit mono 48 kHz WAV file.
pub fn write_wav(path: &Path, audio: &Audio) -> std::io::Result<()> {
    std::fs::write(path, encode_wav(audio))
}

/// `audio` as the bytes of a 16-bit mono 48 kHz WAV file.
pub fn encode_wav(audio: &Audio) -> Vec<u8> {
    let data_len = (audio.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + audio.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in audio.samples() {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Parse a WAV file's bytes (see [`read_wav`]). Chunks other than `fmt ` and
/// `data` are skipped.
pub fn parse_wav(bytes: &[u8]) -> Result<Audio, WavError> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(WavError::Format("no RIFF/WAVE header"));
    }
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let mut channels = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32_at(at + 4) as usize;
        let body = at + 8;
        let end = body
            .checked_add(len)
            .filter(|&end| end <= bytes.len())
            .ok_or(WavError::Format("a chunk runs past the end of the file"))?;
        match id {
            b"fmt " => {
                if len < 16 {
                    return Err(WavError::Format("a short fmt chunk"));
                }
                if u16_at(body) != 1 {
                    return Err(WavError::Format("not PCM"));
                }
                if u32_at(body + 4) != SAMPLE_RATE {
                    return Err(WavError::Format("not 48 kHz"));
                }
                if u16_at(body + 14) != 16 {
                    return Err(WavError::Format("not 16-bit"));
                }
                channels = match u16_at(body + 2) {
                    1 => Some(1),
                    2 => Some(2),
                    _ => return Err(WavError::Format("neither mono nor stereo")),
                };
            }
            b"data" => {
                let channels = channels.ok_or(WavError::Format("data before fmt"))?;
                let samples = bytes[body..end]
                    .chunks_exact(2 * channels)
                    .map(|frame| {
                        let sum: i32 = frame
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|&s| i32::from(i16::from_le_bytes(s)))
                            .sum();
                        (sum / channels as i32) as i16
                    })
                    .collect();
                return Ok(Audio::new(samples));
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        at = end + (len & 1);
    }
    Err(WavError::Format("no data chunk"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_file_round_trips() {
        let audio = Audio::concat(&[
            Audio::tone(220.0, Duration::from_millis(300), 0.5),
            Audio::silence(Duration::from_millis(100)),
        ]);
        let back = parse_wav(&encode_wav(&audio)).unwrap();
        assert_eq!(back, audio);
        assert_eq!(back.duration(), Duration::from_millis(400));
    }

    #[test]
    fn stereo_is_mixed_down_and_odd_chunks_are_skipped() {
        let mut bytes = encode_wav(&Audio::new(vec![0; 2]));
        // Rewrite as stereo: two channels, four bytes a frame, samples (100, 300) and (-50, -150).
        bytes[22] = 2;
        bytes[28..32].copy_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
        bytes[32] = 4;
        bytes.truncate(36);
        bytes.extend_from_slice(b"LIST");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3, 0]); // three bytes, and the pad
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for s in [100i16, 300, -50, -150] {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        assert_eq!(parse_wav(&bytes).unwrap().samples(), &[200, -100]);
    }

    #[test]
    fn other_formats_are_refused() {
        let mut bytes = encode_wav(&Audio::silence(FRAME));
        bytes[24..28].copy_from_slice(&16_000u32.to_le_bytes());
        assert!(matches!(
            parse_wav(&bytes),
            Err(WavError::Format("not 48 kHz"))
        ));
        assert!(parse_wav(b"RIFF").is_err());
        let mut short = encode_wav(&Audio::silence(FRAME));
        short.truncate(50);
        assert!(parse_wav(&short).is_err());
    }

    #[test]
    fn a_tone_is_speech_and_silence_is_not() {
        let tone = Audio::tone(220.0, FRAME, 0.3);
        assert_eq!(tone.len(), FRAME_SAMPLES);
        assert!(rms(tone.samples()) > 4000.0);
        assert_eq!(rms(Audio::silence(FRAME).samples()), 0.0);
        assert_eq!(Audio::chime().duration(), Duration::from_millis(420));
    }
}
