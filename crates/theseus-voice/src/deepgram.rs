//! Deepgram (45a, rows 77 and 78): the real [`Speech`], over Deepgram's REST
//! API, both ways on one key.
//!
//! - **Speech to text**: one utterance per request to the pre-recorded
//!   endpoint, `POST /v1/listen`, as a 16-bit 48 kHz mono WAV, with smart
//!   formatting. The engine's VAD already cuts utterances, so v1 needs no
//!   streaming socket: an utterance is whole when it is sent.
//! - **Synthesis**: one sentence per request, `POST /v1/speak`, as raw
//!   `linear16` at 48 kHz with no container, so the bytes are the engine's
//!   [`Audio`] with no decoding.
//! - **The key** rides in the `Authorization` header alone, marked sensitive.
//!   No error, log line, or `Debug` of this type contains it, and an answer's
//!   body is cut short and scrubbed of it before an error quotes it.
//! - **Usage**, for spend (45b): speech to text by the audio Deepgram says it
//!   heard (`metadata.duration`), else the utterance's length; synthesis by
//!   the characters Deepgram counted (`dg-char-count`), else the sentence's.
//!
//! The client is the workspace's reqwest (rustls on ring), with a bound on the
//! connect and one on the whole call.

use std::fmt;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;

use crate::audio::{duration_of, encode_wav, Audio, SAMPLE_RATE};
use crate::io::Speaker;
use crate::speech::{Speech, SpeechError, SpeechFuture, Synthesis, Transcript, Usage};

/// Deepgram's API.
pub const DEEPGRAM_API: &str = "https://api.deepgram.com";
/// The provider's name in usage and ledger rows.
pub const PROVIDER: &str = "deepgram";

/// How Deepgram is asked. [`Default`] is v1's: nova-3 in US English, and the
/// Aura-2 voice Andromeda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeepgramSettings {
    /// Where the API is: Deepgram's, or a stand-in's in tests.
    pub api_base: String,
    /// The speech-to-text model.
    pub stt_model: String,
    /// The language spoken, as Deepgram names it.
    pub language: String,
    /// The synthesis voice: an Aura model id.
    pub tts_voice: String,
    /// The bound on reaching Deepgram.
    pub connect_timeout: Duration,
    /// The bound on a whole call, the answer's body included.
    pub timeout: Duration,
}

impl Default for DeepgramSettings {
    fn default() -> Self {
        Self {
            api_base: DEEPGRAM_API.into(),
            stt_model: "nova-3".into(),
            language: "en-US".into(),
            tts_voice: "aura-2-andromeda-en".into(),
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(20),
        }
    }
}

/// The real speech provider. Cheap to share: the client pools connections.
pub struct DeepgramSpeech {
    http: reqwest::Client,
    /// `Token <key>`, marked sensitive.
    auth: HeaderValue,
    /// The key alone, to scrub an answer's body before an error quotes it.
    key: String,
    settings: DeepgramSettings,
}

impl fmt::Debug for DeepgramSpeech {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeepgramSpeech")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// What a call was, for its error's words.
#[derive(Clone, Copy)]
enum Call {
    Transcribe,
    Synthesize,
}

impl Call {
    fn what(self) -> &'static str {
        match self {
            Call::Transcribe => "speech to text",
            Call::Synthesize => "synthesis",
        }
    }
}

/// The most of an answer's body an error quotes.
const BODY_QUOTED: usize = 200;

impl DeepgramSpeech {
    /// The provider for `key`, a Deepgram API key. Builds a client and sends
    /// nothing: nothing reaches Deepgram before the first call.
    pub fn new(key: &str, settings: DeepgramSettings) -> Result<Self, SpeechError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(SpeechError("Deepgram: the API key is empty".into()));
        }
        let mut auth = HeaderValue::from_str(&format!("Token {key}"))
            .map_err(|_| SpeechError("Deepgram: the API key is not a valid header value".into()))?;
        auth.set_sensitive(true);
        let http = reqwest::Client::builder()
            .connect_timeout(settings.connect_timeout)
            .timeout(settings.timeout)
            .build()
            .map_err(|e| SpeechError(format!("Deepgram: building the HTTP client: {e}")))?;
        Ok(Self {
            http,
            auth,
            key: key.to_string(),
            settings,
        })
    }

    pub fn settings(&self) -> &DeepgramSettings {
        &self.settings
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.settings.api_base.trim_end_matches('/'))
    }

    fn headers(&self, content_type: &'static str, accept: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, self.auth.clone());
        h.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
        h.insert(ACCEPT, HeaderValue::from_static(accept));
        h
    }

    /// `text` without the key, cut to `BODY_QUOTED` characters.
    fn scrub(&self, text: &str) -> String {
        let clean = text.replace(&self.key, "[key]");
        let mut out: String = clean.chars().take(BODY_QUOTED).collect();
        if clean.chars().count() > BODY_QUOTED {
            out.push('…');
        }
        out
    }

    /// A transport error in words: a timeout says which bound, and nothing
    /// says the key.
    fn sent_error(&self, call: Call, e: &reqwest::Error) -> SpeechError {
        let why = if e.is_timeout() {
            format!(
                "no answer from Deepgram within {} s",
                self.settings.timeout.as_secs_f64()
            )
        } else if e.is_connect() {
            format!(
                "could not reach Deepgram (bounded at {} s): {}",
                self.settings.connect_timeout.as_secs_f64(),
                self.scrub(&chain(e))
            )
        } else {
            self.scrub(&chain(e))
        };
        SpeechError(format!("Deepgram {}: {why}", call.what()))
    }

    /// The answer's body when it is a success; its status and its scrubbed
    /// body as the error otherwise.
    async fn body(
        &self,
        call: Call,
        sent: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<(HeaderMap, Vec<u8>), SpeechError> {
        let r = sent.map_err(|e| self.sent_error(call, &e))?;
        let status = r.status();
        let headers = r.headers().clone();
        let body = r
            .bytes()
            .await
            .map_err(|e| self.sent_error(call, &e))?
            .to_vec();
        if !status.is_success() {
            let text = String::from_utf8_lossy(&body);
            return Err(SpeechError(format!(
                "Deepgram {}: Deepgram answered {status}: {}",
                call.what(),
                self.scrub(text.trim())
            )));
        }
        Ok((headers, body))
    }

    async fn listen(&self, audio: &Audio) -> Result<Transcript, SpeechError> {
        let s = &self.settings;
        let sent = self
            .http
            .post(self.url("/v1/listen"))
            .query(&[
                ("model", s.stt_model.as_str()),
                ("language", s.language.as_str()),
                ("smart_format", "true"),
            ])
            .headers(self.headers("audio/wav", "application/json"))
            .body(encode_wav(audio))
            .send()
            .await;
        let (_, body) = self.body(Call::Transcribe, sent).await?;
        let heard: Listened = serde_json::from_slice(&body).map_err(|e| {
            SpeechError(format!(
                "Deepgram speech to text: its answer could not be read ({e}): {}",
                self.scrub(String::from_utf8_lossy(&body).trim())
            ))
        })?;
        let text = heard
            .results
            .channels
            .first()
            .and_then(|c| c.alternatives.first())
            .map(|a| a.transcript.trim().to_string())
            .ok_or_else(|| {
                SpeechError(
                    "Deepgram speech to text: its answer has no channel or alternative".into(),
                )
            })?;
        // Deepgram bills the audio it heard; its own figure when it gives one.
        let heard_for = heard
            .metadata
            .and_then(|m| m.duration)
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map_or_else(|| audio.duration(), Duration::from_secs_f64);
        Ok(Transcript {
            usage: Usage {
                provider: PROVIDER.into(),
                model: s.stt_model.clone(),
                audio: heard_for,
                chars: text.chars().count(),
            },
            text,
        })
    }

    async fn speak(&self, text: &str) -> Result<Synthesis, SpeechError> {
        let s = &self.settings;
        let rate = SAMPLE_RATE.to_string();
        let sent = self
            .http
            .post(self.url("/v1/speak"))
            .query(&[
                ("model", s.tts_voice.as_str()),
                ("encoding", "linear16"),
                ("sample_rate", rate.as_str()),
                ("container", "none"),
            ])
            .headers(self.headers("application/json", "audio/l16"))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await;
        let (headers, body) = self.body(Call::Synthesize, sent).await?;
        let audio = pcm(&body).map_err(|why| {
            SpeechError(format!(
                "Deepgram synthesis: its answer is not 16-bit audio: {why}"
            ))
        })?;
        let counted = headers
            .get("dg-char-count")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<usize>().ok());
        Ok(Synthesis {
            usage: Usage {
                provider: PROVIDER.into(),
                model: s.tts_voice.clone(),
                audio: audio.duration(),
                chars: counted.unwrap_or_else(|| text.chars().count()),
            },
            audio,
        })
    }
}

impl Speech for DeepgramSpeech {
    fn transcribe<'a>(
        &'a self,
        _speaker: Speaker,
        audio: &'a Audio,
    ) -> SpeechFuture<'a, Transcript> {
        Box::pin(self.listen(audio))
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        Box::pin(self.speak(text))
    }
}

/// Raw little-endian 16-bit samples as audio. No audio at all, or an odd
/// byte, is refused: either is an answer that is not what was asked for.
fn pcm(bytes: &[u8]) -> Result<Audio, String> {
    if bytes.is_empty() {
        return Err("it is empty".into());
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(format!("{} bytes, an odd count", bytes.len()));
    }
    let samples: Vec<i16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect();
    let audio = Audio::new(samples);
    debug_assert_eq!(audio.duration(), duration_of(bytes.len() / 2));
    Ok(audio)
}

/// An error and its sources, as one line.
fn chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        out.push_str(": ");
        out.push_str(&s.to_string());
        source = s.source();
    }
    out
}

/// The pre-recorded endpoint's answer, as much of it as is read.
#[derive(Deserialize)]
struct Listened {
    #[serde(default)]
    metadata: Option<Metadata>,
    results: Results,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct Results {
    channels: Vec<Channel>,
}

#[derive(Deserialize)]
struct Channel {
    alternatives: Vec<Alternative>,
}

#[derive(Deserialize)]
struct Alternative {
    transcript: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech() -> DeepgramSpeech {
        DeepgramSpeech::new("dg-test-key-0123456789", DeepgramSettings::default()).unwrap()
    }

    #[test]
    fn the_key_is_in_no_debug_and_no_quoted_body() {
        let s = speech();
        assert!(!format!("{s:?}").contains("dg-test-key"), "{s:?}");
        let quoted = s.scrub("bad token dg-test-key-0123456789 here");
        assert_eq!(quoted, "bad token [key] here");
        assert!(s.auth.is_sensitive());
        let long = "x".repeat(500);
        assert_eq!(s.scrub(&long).chars().count(), BODY_QUOTED + 1);
    }

    #[test]
    fn an_empty_key_is_refused() {
        let e = DeepgramSpeech::new("  ", DeepgramSettings::default()).unwrap_err();
        assert!(e.0.contains("empty"), "{e}");
    }

    #[test]
    fn raw_linear16_is_the_engines_audio() {
        let audio = pcm(&[1, 0, 0xff, 0xff, 0, 0x80]).unwrap();
        assert_eq!(audio.samples(), &[1, -1, i16::MIN]);
        assert!(pcm(&[1, 2, 3]).unwrap_err().contains("odd"));
        assert!(pcm(&[]).unwrap_err().contains("empty"));
    }
}
