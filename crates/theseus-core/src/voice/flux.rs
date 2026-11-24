//! `[voice]` on Deepgram Flux (theseus-v774k): which models are Flux, and the
//! model the pre-recorded endpoint keeps (voice notes and file transcripts are
//! never streamed).
//!
//! Flux is a streaming listener whose own model decides when a speaker's turn
//! ends, by its own defaults (issue 46), so the pause and the merge of the
//! pause rules do not apply to a call that uses it.
//! `stt_model` defaults to Flux English (the owner, 2026-10-24); `nova-3` is
//! the fallback by config (`stt_model = "nova-3"`: the pause rules).

use anyhow::{bail, Result};

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

    /// Flux English only with English.
    pub(super) fn validate_flux(&self) -> Result<()> {
        if self.stt_model == FLUX_ENGLISH && !self.language.to_ascii_lowercase().starts_with("en") {
            bail!(
                "voice.stt_model = {FLUX_ENGLISH:?} hears English only, and voice.language = {:?} \
                 is not English (set the language to \"en-US\", or stt_model to \"nova-3\")",
                self.language
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

    /// The default is Flux English, with Flux's own end of turn; `nova-3` is the
    /// fallback by config, with the pause rules; a Flux id parses. Flux's knobs
    /// are no settings (issue 46): a file that still sets them loads with each
    /// named retired, and none reaches a reader.
    #[test]
    fn the_default_is_flux_and_nova_3_is_the_fallback_by_config() {
        let none = Config::example().voice;
        assert_eq!(none.stt_model, "flux-general-en");
        assert!(none.uses_flux());
        let (knobs, _) = Config::parse(&format!(
            "{}\n[voice]\nstt_model = \"flux-general-en\"\neot_threshold = 0.8\n\
             eager_eot_threshold = 0.5\neot_timeout_ms = 6000\n",
            Config::EXAMPLE_TOML
        ))
        .unwrap();
        let named = |k: &str| {
            knobs
                .ignored_keys
                .iter()
                .any(|i| i.name == format!("voice.{k}"))
        };
        assert!(named("eot_threshold"));
        assert!(named("eager_eot_threshold"));
        assert!(named("eot_timeout_ms"));
        let nova = with("[voice]\nstt_model = \"nova-3\"").unwrap();
        assert!(!nova.uses_flux());
        assert_eq!(
            crate::voice::PAUSE_MS,
            1_200,
            "the fallback by config keeps the pause rules"
        );
        let v = with("[voice]\nstt_model = \"flux-general-en\"").unwrap();
        assert!(v.uses_flux());
        assert_eq!(crate::voice::MERGE_MS, 1_500, "and their merge");
        assert_eq!(knobs.voice, v, "the knobs reach no reader");
        assert_eq!(knobs.ignored_keys.len(), 3);
        assert_eq!(knobs.voice.stt_model, super::FLUX_ENGLISH);
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
