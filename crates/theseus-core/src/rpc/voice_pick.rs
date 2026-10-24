//! The speaking voice (theseus-slvl.7): which Aura voice a call speaks in,
//! what `/join` says first, `voice.show`, and `voice.audition`.
//!
//! - **The voice follows the persona** (`Core::voice_of`): the persona in play
//!   for the voice place's session (its own choice, its place's, the default,
//!   `Config::persona_for`'s order) gives `tts_voice`; `[voice] tts_voice` is
//!   the fallback. The binding asks at each sentence, from memory, so a change
//!   of persona mid-call is heard at the next sentence with no reconnect.
//! - **The greeting** (`Core::greeting_for`): `[voice] greeting` with `{name}`
//!   the persona's name; empty is none.
//! - **The audition** is the owner's run: one clip per voice, WAV, made with
//!   the daemon's Deepgram key (which no answer carries) and sent back for the
//!   CLI to write. It books to no session, so it is bounded instead: at most
//!   `AUDITION_MAX_VOICES` voices of at most `AUDITION_MAX_CHARS` characters.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use theseus_protocol::method;
use theseus_protocol::voice_pick::{
    AuditionClip, AuditionFailure, VoiceAuditionParams, VoiceAuditionResult, VoiceShowParams,
    VoiceShowResult,
};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::config::PersonaView;
use crate::session::SessionRecord;
use crate::voice::DEFAULT_GREETING;

pub(super) const OWN: [&str; 2] = [method::VOICE_SHOW, method::VOICE_AUDITION];

/// The audition's default voices: a spread of Aura-2's, the daemon's default
/// first.
pub const AUDITION_VOICES: [&str; 6] = [
    "aura-2-andromeda-en",
    "aura-2-thalia-en",
    "aura-2-luna-en",
    "aura-2-athena-en",
    "aura-2-orion-en",
    "aura-2-apollo-en",
];
/// The most voices, and the most characters each says, in one audition.
pub const AUDITION_MAX_VOICES: usize = 8;
pub const AUDITION_MAX_CHARS: usize = 500;

/// Who speaks: the persona's name and the voice a call uses for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenAs {
    pub persona: Option<String>,
    pub name: String,
    pub voice: String,
    /// The voice came from the persona rather than `[voice] tts_voice`.
    pub from_persona: bool,
}

impl Core {
    /// The persona in play for `session` and the voice it speaks in. Memory
    /// and one session read; nothing is written.
    pub fn voice_of(&self, session: Option<&str>) -> SpokenAs {
        let record =
            session.and_then(|id| self.store.get_session::<SessionRecord>(id).ok().flatten());
        let key = match &record {
            Some(r) => self.runner.persona_of(r),
            None => self
                .personas()
                .persona_key_for(None, None)
                .map(str::to_string),
        };
        let view = key
            .as_deref()
            .and_then(|k| self.personas().get(k).cloned())
            .unwrap_or_else(PersonaView::built_in);
        let from_persona = view.tts_voice.is_some();
        SpokenAs {
            persona: view.key,
            name: view.name,
            voice: view
                .tts_voice
                .unwrap_or_else(|| self.cfg.voice.tts_voice.clone()),
            from_persona,
        }
    }

    /// What `/join` says first for `session`, or none.
    pub fn greeting_for(&self, session: Option<&str>) -> Option<String> {
        greeting(&self.cfg.voice.greeting, &self.voice_of(session).name)
    }

    pub(super) fn voice_show(&self, p: VoiceShowParams) -> Result<VoiceShowResult, RpcFailure> {
        if let Some(id) = &p.session_id {
            if self.store.get_session::<SessionRecord>(id)?.is_none() {
                return Err(RpcFailure::invalid(anyhow!("no session {id}")));
            }
        }
        let who = self.voice_of(p.session_id.as_deref());
        let v = &self.cfg.voice;
        Ok(VoiceShowResult {
            enabled: v.enabled,
            greeting: greeting(&v.greeting, &who.name),
            persona: who.persona,
            name: who.name,
            from: match who.from_persona {
                true => "persona".into(),
                false => "[voice] tts_voice".into(),
            },
            voice: who.voice,
            global: v.tts_voice.clone(),
            key_secret: v.key_secret.clone(),
            key_ready: self.secrets.get(&v.key_secret).is_some(),
        })
    }

    pub(super) async fn rpc_voice_pick(
        self: Arc<Self>,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        if name == method::VOICE_SHOW {
            return super::server::route(params, |p| self.voice_show(p));
        }
        let p: VoiceAuditionParams = parse(params)?;
        let who = conn.answerer(None, None);
        let r = self
            .voice_audition(p, who)
            .await
            .map_err(RpcFailure::invalid)?;
        super::server::reply(r)
    }

    /// `voice.audition`: the owner's run, one clip a voice.
    pub async fn voice_audition(
        &self,
        p: VoiceAuditionParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> Result<VoiceAuditionResult> {
        let who = who.into();
        self.judge_act(
            &who,
            super::Act::JudgeRun {
                method: method::VOICE_AUDITION,
                what: "a voice audition",
            },
        )?;
        let v = &self.cfg.voice;
        if !self.cfg.secrets.contains_key(&v.key_secret) {
            bail!(
                "no Deepgram key is configured: add a [secrets] entry named {} (voice's key), then restart",
                v.key_secret
            );
        }
        let Some(key) = self.secrets.get(&v.key_secret) else {
            bail!("the Deepgram key ({}) has not resolved yet", v.key_secret);
        };
        let voices: Vec<String> = match p.voices.is_empty() {
            true => AUDITION_VOICES.iter().map(|s| s.to_string()).collect(),
            false => p.voices.iter().map(|s| s.trim().to_string()).collect(),
        };
        if voices.len() > AUDITION_MAX_VOICES {
            bail!("an audition tries at most {AUDITION_MAX_VOICES} voices");
        }
        if let Some(bad) = voices.iter().find(|s| !voice_id_ok(s)) {
            bail!("{bad:?} is not an Aura voice id (letters, digits and dashes)");
        }
        let text = match p.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => t.to_string(),
            None => self
                .greeting_for(p.session_id.as_deref())
                .unwrap_or_else(|| {
                    greeting(
                        DEFAULT_GREETING,
                        &self.voice_of(p.session_id.as_deref()).name,
                    )
                    .unwrap_or_default()
                }),
        };
        if text.chars().count() > AUDITION_MAX_CHARS {
            bail!("the audition text is at most {AUDITION_MAX_CHARS} characters");
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()?;
        let (mut clips, mut failed, mut estimate) = (Vec::new(), Vec::new(), 0u64);
        for voice in voices {
            let cost = crate::catalog::speech_price("deepgram", &voice)
                .map(|price| price.cost_micros(Duration::ZERO, text.chars().count()));
            match speak(&http, &v.api_base, key.expose(), &voice, &text).await {
                Ok(wav) => {
                    estimate += cost.unwrap_or(0);
                    clips.push(AuditionClip {
                        voice,
                        wav: base64::engine::general_purpose::STANDARD.encode(wav),
                        cost_micros: cost,
                    });
                }
                Err(error) => failed.push(AuditionFailure { voice, error }),
            }
        }
        Ok(VoiceAuditionResult {
            text,
            clips,
            failed,
            estimate_micros: estimate,
        })
    }
}

/// The greeting's words for `name`; none when the template is empty.
pub fn greeting(template: &str, name: &str) -> Option<String> {
    let said = template.replace("{name}", name);
    let said = said.trim();
    (!said.is_empty()).then(|| said.to_string())
}

fn voice_id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// One sentence in `voice`, as a WAV file. The key rides in the
/// Authorization header alone, and an error's quoted body has it cut out.
async fn speak(
    http: &reqwest::Client,
    api_base: &str,
    key: &str,
    voice: &str,
    text: &str,
) -> std::result::Result<Vec<u8>, String> {
    let url = format!("{}/v1/speak", api_base.trim_end_matches('/'));
    let resp = http
        .post(url)
        .query(&[
            ("model", voice),
            ("encoding", "linear16"),
            ("container", "wav"),
            ("sample_rate", "24000"),
        ])
        .header(reqwest::header::AUTHORIZATION, format!("Token {key}"))
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .map_err(|e| format!("Deepgram could not be reached ({})", e.without_url()))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("Deepgram's answer could not be read ({})", e.without_url()))?;
    if !status.is_success() {
        let said: String = String::from_utf8_lossy(&body)
            .replace(key, "<key>")
            .chars()
            .take(200)
            .collect();
        return Err(format!("Deepgram answered {status}: {}", said.trim()));
    }
    if !body.starts_with(b"RIFF") {
        return Err("Deepgram's answer is not a WAV file".into());
    }
    Ok(body.to_vec())
}
