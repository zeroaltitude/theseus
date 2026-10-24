//! `[voice]` on Deepgram Flux (theseus-v774k): which models are Flux, the
//! knobs it takes and their ranges, and the model the pre-recorded endpoint
//! keeps (voice notes and file transcripts are never streamed).
//!
//! Flux is a streaming listener whose own model decides when a speaker's turn
//! ends, so `pause_ms` and `merge_ms` do not apply to a call that uses it.
//! `stt_model` defaults to Flux English (the owner, 2026-10-24); `nova-3` is
//! the fallback by config (`stt_model = "nova-3"`: the pause rules).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::VoiceConfig;

/// The English Flux model (Deepgram's docs, "Flux quickstart").
pub const FLUX_ENGLISH: &str = "flux-general-en";

/// The model the pre-recorded endpoint is asked for when `stt_model` is a Flux
/// one: Flux is streaming only.
pub const PRERECORDED_MODEL: &str = "nova-3";

/// Whether `model` is a Deepgram Flux model: the one test every caller uses.
pub fn is_flux(model: &str) -> bool {
    model.starts_with("flux-")
}

/// A Flux threshold, 0 to 1. Compared by its bits, so the config stays `Eq`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Threshold(pub f64);

impl PartialEq for Threshold {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Threshold {}

/// `eot_threshold`: how sure Flux must be that a turn ended (Deepgram's docs,
/// "End-of-Turn Detection Parameters": 0.5 to 0.9, default 0.7).
pub const EOT_RANGE: (f64, f64) = (0.5, 0.9);
/// `eager_eot_threshold`: turns on early ends of turn (0.3 to 0.9, unset).
pub const EAGER_RANGE: (f64, f64) = (0.3, 0.9);
/// `eot_timeout_ms`: the forced end of a turn (500 to 10000, default 5000).
pub const TIMEOUT_RANGE: (u32, u32) = (500, 10_000);

impl VoiceConfig {
    /// Whether calls listen through Flux.
    pub fn uses_flux(&self) -> bool {
        is_flux(&self.stt_model)
    }

    /// The model voice notes and file transcripts ask the pre-recorded
    /// endpoint for: `stt_model`, unless that is a streaming one.
    pub fn prerecorded_model(&self) -> &str {
        match self.uses_flux() {
            true => PRERECORDED_MODEL,
            false => &self.stt_model,
        }
    }

    /// The Flux keys: only with a Flux model, each in Deepgram's range, and
    /// Flux English only with English.
    pub(super) fn validate_flux(&self) -> Result<()> {
        let knobs = self.eot_threshold.is_some()
            || self.eager_eot_threshold.is_some()
            || self.eot_timeout_ms.is_some();
        if !self.uses_flux() {
            if knobs {
                bail!(
                    "voice.eot_threshold, eager_eot_threshold and eot_timeout_ms are Flux's: \
                     voice.stt_model = {:?} is not a Flux model (use {FLUX_ENGLISH:?}, or leave \
                     them out; pause_ms and merge_ms are this model's end of turn)",
                    self.stt_model
                );
            }
            return Ok(());
        }
        if self.stt_model == FLUX_ENGLISH && !self.language.to_ascii_lowercase().starts_with("en") {
            bail!(
                "voice.stt_model = {FLUX_ENGLISH:?} hears English only, and voice.language = {:?} \
                 is not English (set the language to \"en-US\", or stt_model to \"nova-3\")",
                self.language
            );
        }
        let in_range =
            |t: Threshold, (lo, hi): (f64, f64)| t.0.is_finite() && t.0 >= lo && t.0 <= hi;
        if let Some(t) = self.eot_threshold.filter(|t| !in_range(*t, EOT_RANGE)) {
            bail!(
                "voice.eot_threshold = {}: Flux ends a turn at a confidence of {} to {} (0.7 is \
                 its default); higher: fewer early ends, a later reply",
                t.0,
                EOT_RANGE.0,
                EOT_RANGE.1
            );
        }
        if let Some(t) = self
            .eager_eot_threshold
            .filter(|t| !in_range(*t, EAGER_RANGE))
        {
            bail!(
                "voice.eager_eot_threshold = {}: eager end of turn is {} to {}; leave it out to \
                 turn it off",
                t.0,
                EAGER_RANGE.0,
                EAGER_RANGE.1
            );
        }
        if let Some(ms) = self
            .eot_timeout_ms
            .filter(|ms| !(TIMEOUT_RANGE.0..=TIMEOUT_RANGE.1).contains(ms))
        {
            bail!(
                "voice.eot_timeout_ms = {ms}: Flux forces a turn's end after {} to {} ms of \
                 silence (5000 is its default)",
                TIMEOUT_RANGE.0,
                TIMEOUT_RANGE.1
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn with(section: &str) -> anyhow::Result<crate::voice::VoiceConfig> {
        Config::parse(&format!("{}\n{section}\n", Config::EXAMPLE_TOML)).map(|(c, _)| c.voice)
    }

    /// The default is Flux English, with Flux's knobs unset; `nova-3` is the
    /// fallback by config, with the pause rules; a Flux id parses.
    #[test]
    fn the_default_is_flux_and_nova_3_is_the_fallback_by_config() {
        let none = Config::example().voice;
        assert_eq!(none.stt_model, "flux-general-en");
        assert!(none.uses_flux());
        assert_eq!(none.eot_threshold, None);
        assert_eq!(none.eager_eot_threshold, None);
        assert_eq!(none.eot_timeout_ms, None);
        let nova = with("[voice]\nstt_model = \"nova-3\"").unwrap();
        assert!(!nova.uses_flux());
        assert_eq!(
            nova.pause_ms, 1_200,
            "the fallback by config keeps the pause rules"
        );
        let v = with("[voice]\nstt_model = \"flux-general-en\"").unwrap();
        assert!(v.uses_flux());
        assert_eq!(v.pause_ms, 1_200, "the pause rules keep their checks");
        let v = with(
            "[voice]\nstt_model = \"flux-general-en\"\neot_threshold = 0.8\n\
             eager_eot_threshold = 0.5\neot_timeout_ms = 6000",
        )
        .unwrap();
        assert_eq!(v.eot_threshold.map(|t| t.0), Some(0.8));
        assert_eq!(v.eager_eot_threshold.map(|t| t.0), Some(0.5));
        assert_eq!(v.eot_timeout_ms, Some(6_000));
    }

    #[test]
    fn is_flux_names_the_flux_models_alone() {
        assert!(super::is_flux("flux-general-en"));
        assert!(super::is_flux("flux-general-multi"));
        for other in ["nova-3", "nova-2", "whisper-large", "influx"] {
            assert!(!super::is_flux(other), "{other}");
        }
    }

    #[test]
    fn bad_thresholds_are_refused_in_words_with_the_key_named() {
        for (key, bad) in [
            ("eot_threshold", "0.4"),
            ("eot_threshold", "0.95"),
            ("eager_eot_threshold", "0.2"),
            ("eager_eot_threshold", "0.95"),
            ("eot_timeout_ms", "499"),
            ("eot_timeout_ms", "10001"),
        ] {
            let e = with(&format!(
                "[voice]\nstt_model = \"flux-general-en\"\n{key} = {bad}"
            ))
            .unwrap_err();
            let e = format!("{e:#}");
            assert!(e.contains(&format!("voice.{key} = {bad}")), "{e}");
        }
    }

    #[test]
    fn flux_english_and_another_language_are_refused_naming_both() {
        let e = with("[voice]\nstt_model = \"flux-general-en\"\nlanguage = \"de\"").unwrap_err();
        let e = format!("{e:#}");
        assert!(e.contains("flux-general-en") && e.contains("\"de\""), "{e}");
        with("[voice]\nstt_model = \"flux-general-en\"\nlanguage = \"en-GB\"").unwrap();
        // Nova-3 hears German as it always did, by config.
        with("[voice]\nstt_model = \"nova-3\"\nlanguage = \"de\"").unwrap();
        // With no stt_model the default is Flux English: German alone is
        // refused, and the message names the fallback.
        let e = with("[voice]\nlanguage = \"de\"").unwrap_err();
        let e = format!("{e:#}");
        assert!(e.contains("\"nova-3\"") && e.contains("\"de\""), "{e}");
    }

    #[test]
    fn flux_knobs_without_a_flux_model_are_refused() {
        let e = with("[voice]\nstt_model = \"nova-3\"\neot_threshold = 0.8").unwrap_err();
        assert!(format!("{e:#}").contains("not a Flux model"), "{e:#}");
    }

    /// Voice notes and file transcripts keep the pre-recorded model.
    #[test]
    fn the_prerecorded_model_is_nova_3_under_a_flux_stt_model() {
        let flux = with("[voice]\nstt_model = \"flux-general-en\"").unwrap();
        assert_eq!(flux.prerecorded_model(), "nova-3");
        let nova2 = with("[voice]\nstt_model = \"nova-2\"").unwrap();
        assert_eq!(nova2.prerecorded_model(), "nova-2");
        assert_eq!(Config::example().voice.prerecorded_model(), "nova-3");
    }
}
