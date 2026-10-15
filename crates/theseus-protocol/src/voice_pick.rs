//! The speaking voice (theseus-slvl.7): `voice.show`, the voice in play and
//! where it comes from, and `voice.audition`, the same words in several
//! voices as WAV clips, so the owner picks by ear. The key is the daemon's
//! alone: neither answer carries it, nor its value.

use serde::{Deserialize, Serialize};

/// `voice.show`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VoiceShowParams {
    /// The session whose persona speaks; absent: the default persona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// `voice.show`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VoiceShowResult {
    /// `[voice] enabled`.
    pub enabled: bool,
    /// The `[personas]` key in play; absent for the built-in default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    /// The persona's name, as the greeting says it.
    pub name: String,
    /// The Aura voice a call speaks in now.
    pub voice: String,
    /// Where it comes from: `persona` or `[voice] tts_voice`.
    pub from: String,
    /// `[voice] tts_voice`, the fallback.
    pub global: String,
    /// What `/join` says first, with the name in; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub greeting: Option<String>,
    /// The `[secrets]` entry holding the Deepgram key, and whether it has
    /// resolved (never its value).
    pub key_secret: String,
    pub key_ready: bool,
}

/// `voice.audition`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VoiceAuditionParams {
    /// Aura voice ids; absent or empty: the daemon's default list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub voices: Vec<String>,
    /// What each says; absent: the persona's greeting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// The session whose persona speaks the greeting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// One voice's clip.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AuditionClip {
    pub voice: String,
    /// A WAV file, base64.
    pub wav: String,
    /// What it cost, in micro-dollars, when the voice has a price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cost_micros: Option<u64>,
}

/// A voice that could not be made, and why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AuditionFailure {
    pub voice: String,
    pub error: String,
}

/// `voice.audition`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VoiceAuditionResult {
    /// The words each voice said.
    pub text: String,
    pub clips: Vec<AuditionClip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<AuditionFailure>,
    /// The whole run's estimate, micro-dollars.
    pub estimate_micros: u64,
}
