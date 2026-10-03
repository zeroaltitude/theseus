//! Voice (rows 77 and 78): `[voice]`, speech in a Discord voice channel
//! through Deepgram.
//!
//! - **`[voice]`** is off by default, and every key is optional. Deepgram
//!   does both directions on one key, which only Theseus reads: health and
//!   `theseusd check` name it among the harness-only keys, and a job is never
//!   handed it unless `[broker]` names it.
//! - **Speech is spend** (45b). Its prices are the code's
//!   ([`crate::catalog::speech_price`]): speech to text by the minute heard,
//!   synthesis by the thousand characters. A speech call is no turn's, so it
//!   has no turn to reserve under (the kernel's `book_spend` says why): before
//!   a call, the binding asks whether its estimate fits the session's limit
//!   ([`Core::speech_fits`]); after it, the call is booked to the session's
//!   execution with its row, in one frame ([`Core::book_speech`]).

use std::time::Duration;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, Micros};
use theseus_protocol::LedgerKind;

use crate::catalog::speech_price;
use crate::rpc::Core;
use crate::session::SessionRecord;

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

/// Which way a speech call went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechKind {
    /// Speech to text: an utterance.
    Transcribed,
    /// Text to speech: a sentence.
    Synthesized,
}

impl SpeechKind {
    fn ledger(self) -> LedgerKind {
        match self {
            SpeechKind::Transcribed => LedgerKind::SpeechTranscribed,
            SpeechKind::Synthesized => LedgerKind::SpeechSynthesized,
        }
    }
}

/// A speech call (45b), as the binding books it.
#[derive(Debug, Clone)]
pub struct SpeechCall<'a> {
    pub kind: SpeechKind,
    pub provider: &'a str,
    pub model: &'a str,
    /// The audio heard, or made.
    pub audio: Duration,
    /// The characters heard, or made into speech.
    pub chars: usize,
    pub latency: Duration,
    /// The row's own words: the speaker, what was said.
    pub detail: Value,
}

impl SpeechCall<'_> {
    /// What it costs, when its model has a price.
    pub fn cost(&self) -> Option<Micros> {
        speech_price(self.provider, self.model).map(|p| p.cost_micros(self.audio, self.chars))
    }
}

/// What a booking did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Booked {
    /// What the call cost, when its model has a price.
    pub cost: Option<Micros>,
    /// The execution it was booked to, when the session has one.
    pub execution: Option<String>,
}

impl Core {
    fn execution_of(&self, session_id: &str) -> Result<Option<String>> {
        Ok(self
            .store
            .get_session::<SessionRecord>(session_id)?
            .and_then(|r| r.execution_id))
    }

    /// Whether a speech call that would cost about `estimate` fits its
    /// session's spend limit now: what its execution may still reserve. A
    /// call that does not fit is not made, and the reason says how to go on.
    pub fn speech_fits(&self, session_id: &str, estimate: Micros) -> Result<(), String> {
        let execution = self
            .execution_of(session_id)
            .map_err(|e| format!("reading the session: {e:#}"))?;
        let Some(e) = execution.and_then(|id| self.kernel.execution(&id).ok().flatten()) else {
            return Ok(());
        };
        let b = &e.budget;
        if b.available() >= estimate {
            return Ok(());
        }
        Err(format!(
            "this session is at its spend limit ({} of {} spent): reset its spend to go on",
            crate::narrative::dollars(b.spent_micros),
            crate::narrative::dollars(b.limit_micros)
        ))
    }

    /// Book a speech call to its session after the call: its cost to the
    /// execution's spend, and its row (`speech.transcribed` or
    /// `speech.synthesized`), in one frame. A model with no price, or a
    /// session with no execution, gets its row alone, which says why.
    pub fn book_speech(&self, session_id: &str, call: &SpeechCall<'_>) -> Result<Booked> {
        let cost = call.cost();
        let mut data = json!({
            "provider": call.provider,
            "model": call.model,
            "seconds": (call.audio.as_secs_f64() * 1000.0).round() / 1000.0,
            "chars": call.chars,
            "latency_ms": call.latency.as_millis() as u64,
        });
        if let (Value::Object(m), Value::Object(d)) = (&mut data, &call.detail) {
            m.extend(d.clone());
        }
        let execution = self.execution_of(session_id)?;
        match (cost, &execution) {
            (Some(cost), Some(id)) => {
                self.kernel.book_spend(id, cost, call.kind.ledger(), data)?;
            }
            _ => {
                data["cost_usd"] = cost.map_or(Value::Null, |c| micros_to_usd(c).into());
                data["unbooked"] = match cost {
                    None => "no price for this model".into(),
                    Some(_) => "the session has no execution".into(),
                };
                self.binding_ledger(call.kind.ledger(), Some(session_id), data);
            }
        }
        Ok(Booked { cost, execution })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// The ledger rows of `kind` in the session, oldest first.
    fn rows(core: &Core, sid: &str, kind: &str) -> Vec<Value> {
        let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(1000).unwrap();
        rows.into_iter()
            .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid))
            .map(|(_, r)| r.data)
            .collect()
    }

    /// Speech is spend (45b): each call is booked to its session's
    /// execution after it, with its row and its cost from the code's prices;
    /// a call that would pass the limit does not fit, but one already made is
    /// booked in full; a model with no price gets its row alone.
    #[tokio::test]
    async fn speech_is_booked_to_its_session_and_a_call_past_the_limit_does_not_fit() {
        let d = tempfile::tempdir().unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = d.path().to_string_lossy().into_owned();
        cfg.kernel.spend_limit_usd = 0.001;
        let store = crate::store::Store::open(&d.path().join("store")).unwrap();
        let model = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![]));
        let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
        let sid = core
            .open_session(theseus_protocol::SessionOpenParams::default())
            .unwrap()
            .session_id;
        // 3 s heard on nova-3: 3,000 ms × 4,300 µ$ a minute / 60,000 = 215 µ$.
        let heard = SpeechCall {
            kind: SpeechKind::Transcribed,
            provider: "deepgram",
            model: "nova-3",
            audio: Duration::from_secs(3),
            chars: 41,
            latency: Duration::from_millis(250),
            detail: json!({"speaker": "7"}),
        };
        assert_eq!(heard.cost(), Some(215));
        assert_eq!(core.speech_fits(&sid, 215), Ok(()));
        let booked = core.book_speech(&sid, &heard).unwrap();
        assert_eq!(booked.cost, Some(215));
        let id = booked.execution.expect("the session's execution");
        let spent = |core: &Core| {
            core.kernel
                .execution(&id)
                .unwrap()
                .unwrap()
                .budget
                .spent_micros
        };
        assert_eq!(spent(&core), 215);
        let row = &rows(&core, &sid, "speech.transcribed")[0];
        assert_eq!(
            (
                row["model"].as_str(),
                row["seconds"].as_f64(),
                row["speaker"].as_str()
            ),
            (Some("nova-3"), Some(3.0), Some("7"))
        );
        assert_eq!(row["cost_usd"].as_f64(), Some(0.000215));
        assert_eq!(row["latency_ms"], 250);
        // 30 characters on Aura-2: 30 × 30 µ$ = 900 µ$, past the 785 left.
        let said = SpeechCall {
            kind: SpeechKind::Synthesized,
            provider: "deepgram",
            model: "aura-2-andromeda-en",
            audio: Duration::from_millis(1960),
            chars: 30,
            latency: Duration::from_millis(640),
            detail: json!({"what": "reply 0"}),
        };
        assert_eq!(said.cost(), Some(900));
        let refused = core.speech_fits(&sid, 900).unwrap_err();
        assert!(
            refused.contains("at its spend limit ($0.0002 of $0.001 spent)"),
            "{refused}"
        );
        // A call already made is booked in full: a real cost is never hidden.
        core.book_speech(&sid, &said).unwrap();
        assert_eq!(spent(&core), 1_115);
        assert_eq!(
            rows(&core, &sid, "speech.synthesized")[0]["spent_usd"].as_f64(),
            Some(0.001115)
        );
        // A model with no price: its row alone, saying so; nothing booked.
        let odd = SpeechCall {
            model: "whisper-9",
            ..heard
        };
        assert_eq!(core.book_speech(&sid, &odd).unwrap().cost, None);
        assert_eq!(spent(&core), 1_115);
        let row = &rows(&core, &sid, "speech.transcribed")[1];
        assert_eq!(row["unbooked"], "no price for this model");
        assert!(row["cost_usd"].is_null());
    }
}
