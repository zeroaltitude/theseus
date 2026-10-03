//! Voice (rows 77 and 78): `[voice]`, speech in a Discord voice channel
//! through Deepgram.
//!
//! The section is off by default, and every key is optional. Deepgram does
//! both directions on one key, which only Theseus reads: health and
//! `theseusd check` name it among the harness-only keys, and a job is never
//! handed it unless `[broker]` names it.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Deepgram's API.
pub const DEEPGRAM_API: &str = "https://api.deepgram.com";

/// `[voice]`: speech in a Discord voice channel, through Deepgram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceConfig {
    /// `/join` answers only when this is on.
    #[serde(default)]
    pub enabled: bool,
    /// The `[secrets]` entry holding the Deepgram key.
    #[serde(default = "default_key_secret")]
    pub key_secret: String,
    /// The speech-to-text model.
    #[serde(default = "default_stt_model")]
    pub stt_model: String,
    /// The language spoken, as Deepgram names it.
    #[serde(default = "default_language")]
    pub language: String,
    /// The synthesis voice: an Aura model id.
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    /// Deepgram's API, or a local stand-in's (tests and scratch daemons).
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_key_secret() -> String {
    "deepgram_api_key".into()
}
fn default_stt_model() -> String {
    "nova-3".into()
}
fn default_language() -> String {
    "en-US".into()
}
fn default_tts_voice() -> String {
    "aura-2-andromeda-en".into()
}
fn default_api_base() -> String {
    DEEPGRAM_API.into()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            key_secret: default_key_secret(),
            stt_model: default_stt_model(),
            language: default_language(),
            tts_voice: default_tts_voice(),
            api_base: default_api_base(),
        }
    }
}

impl VoiceConfig {
    /// Nothing set: `theseusd config` leaves the section out.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl crate::config::Config {
    /// `[voice]`: each name non-empty, and the API an http(s) address.
    pub(crate) fn validate_voice(&self) -> Result<()> {
        let v = &self.voice;
        for (key, value) in [
            ("key_secret", &v.key_secret),
            ("stt_model", &v.stt_model),
            ("language", &v.language),
            ("tts_voice", &v.tts_voice),
        ] {
            if value.trim().is_empty() {
                bail!("voice.{key} is empty");
            }
        }
        if !(v.api_base.starts_with("https://") || v.api_base.starts_with("http://")) {
            bail!(
                "voice.api_base = {:?} must be an http(s) address",
                v.api_base
            );
        }
        Ok(())
    }

    /// The voice key's `[secrets]` name, when Theseus may read it: voice is
    /// on, or `[secrets]` holds an entry by that name. Harness-only.
    pub fn voice_key_secret(&self) -> Option<&str> {
        let v = &self.voice;
        (v.enabled || self.secrets.contains_key(&v.key_secret)).then_some(v.key_secret.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    /// A config with no `[voice]`, as Eddie's note has none until he pastes
    /// one: voice is off, its key is no secret Theseus reads, and the
    /// section stays out of `theseusd config`.
    #[test]
    fn a_config_without_voice_has_it_off() {
        let cfg = Config::example();
        assert!(!cfg.voice.enabled);
        assert!(cfg.voice.is_default());
        assert_eq!(cfg.voice_key_secret(), None);
        let printed = toml::to_string(&cfg).unwrap();
        assert!(!printed.contains("[voice]"), "{printed}");
    }

    #[test]
    fn a_voice_section_reads_and_each_key_is_optional() {
        let with = |section: &str| {
            Config::parse(&format!("{}\n{section}\n", Config::EXAMPLE_TOML)).map(|(c, _)| c)
        };
        let cfg = with("[voice]\nenabled = true").unwrap();
        assert!(cfg.voice.enabled);
        assert_eq!(cfg.voice.stt_model, "nova-3");
        assert_eq!(cfg.voice.language, "en-US");
        assert_eq!(cfg.voice.tts_voice, "aura-2-andromeda-en");
        assert_eq!(cfg.voice.api_base, super::DEEPGRAM_API);
        // On, its key is the harness's, whether or not [secrets] has it yet.
        assert_eq!(cfg.voice_key_secret(), Some("deepgram_api_key"));
        let cfg = with("[voice]\ntts_voice = \"aura-2-thalia-en\"\nlanguage = \"en-GB\"").unwrap();
        assert!(!cfg.voice.enabled);
        assert_eq!(
            (cfg.voice.tts_voice.as_str(), cfg.voice.language.as_str()),
            ("aura-2-thalia-en", "en-GB")
        );
        let e = with("[voice]\nstt_model = \" \"").unwrap_err();
        assert!(
            format!("{e:#}").contains("voice.stt_model is empty"),
            "{e:#}"
        );
        let e = with("[voice]\napi_base = \"ftp://example.invalid\"").unwrap_err();
        assert!(format!("{e:#}").contains("http(s)"), "{e:#}");
        assert!(
            with("[voice]\nvolume = 11").is_err(),
            "an unknown key fails"
        );
    }
}
