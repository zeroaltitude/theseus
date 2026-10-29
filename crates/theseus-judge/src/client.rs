//! The Jev client (design §2.2): typed wire types, strict parsing, classified
//! errors, two timeouts, and the in-flight semaphore that sheds shadow work.
//!
//! One call is one `POST <api_base>/v1/systemone` with a bearer key, a state,
//! a pinned model, and a map of typed questions (`refs/jev.md`, "API shape
//! (verified)"). Nothing retries on its own: a failure is a classified error,
//! with `transient` and `usage_unknown` as the provider's errors have them,
//! and the caller decides.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

/// TypeSafe's API, compiled in; `api_base` overrides it (the fake, tests).
pub const DEFAULT_API_BASE: &str = "https://api.typesafe.ai";
pub const PATH: &str = "/v1/systemone";
/// The longest state Jev takes, in tokens (observed 2026-09-21).
pub const STATE_LIMIT_TOKENS: u64 = 32_000;
/// Jev's limits on a question's options and levels.
pub const MAX_CHOICE_OPTIONS: usize = 255;
pub const MIN_SCORE_LEVELS: usize = 2;
pub const MAX_SCORE_LEVELS: usize = 10;
/// How far a distribution's sum may sit from 1 before it is `malformed`.
pub const SUM_TOLERANCE: f64 = 0.02;
/// A response body longer than this is `malformed` (answers are small).
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;
/// How long the client keeps an idle connection open (theseus-otny). Jev's
/// edge closed one idle for 400 s and kept one idle for 200 s (measured
/// 2026-10-05), and reqwest's own default, 90 s, dropped every connection
/// in a conversation's pauses, so the next message paid DNS, TCP and TLS.
pub const POOL_IDLE: Duration = Duration::from_secs(180);
/// How long the connections may sit idle before the core's keeper uses
/// them again (theseus-ddbi): half a minute inside [`POOL_IDLE`], so a
/// refresh's answer lands before the pool closes them.
pub const KEEP_WARM: Duration = Duration::from_secs(150);

/// Rough tokens for a text of `bytes` bytes: a quarter, rounded up, as the
/// rest of Theseus estimates (`ProviderRequest::estimate_tokens`).
pub fn estimate_tokens(bytes: usize) -> u64 {
    bytes.div_ceil(4) as u64
}

// ------------------------------------------------------------------ request

/// One option of a Choice. `means` is what Jev reads; an option without it
/// goes out as `null`, as the verified shape's `"other": null` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub id: String,
    pub means: Option<String>,
}

/// A typed question as it goes on the wire. Ids are the caller's (never
/// shown to Jev); the instructions carry the whole meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// One of a defined set, the no-match option included.
    Choice {
        instructions: String,
        options: Vec<ChoiceOption>,
    },
    /// A degree along an ordered rubric of 2 to 10 levels, lowest first.
    Score {
        instructions: String,
        levels: Vec<String>,
    },
    /// A yes/no condition: the probability the statement is true.
    Noul {
        instructions: String,
        when_true: Option<String>,
        when_false: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Choice,
    Score,
    Noul,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Choice => "choice",
            Kind::Score => "score",
            Kind::Noul => "noul",
        }
    }
}

impl Question {
    pub fn kind(&self) -> Kind {
        match self {
            Question::Choice { .. } => Kind::Choice,
            Question::Score { .. } => Kind::Score,
            Question::Noul { .. } => Kind::Noul,
        }
    }

    pub fn instructions(&self) -> &str {
        match self {
            Question::Choice { instructions, .. }
            | Question::Score { instructions, .. }
            | Question::Noul { instructions, .. } => instructions,
        }
    }

    /// The wire form, `{"type", "instructions", "criteria"}`, with a Choice's
    /// options in their given order.
    pub fn wire(&self) -> String {
        let s = |t: &str| serde_json::to_string(t).unwrap_or_default();
        let head = |t: &str, i: &str| format!("{{\"type\":{},\"instructions\":{}", s(t), s(i));
        match self {
            Question::Choice {
                instructions,
                options,
            } => {
                let criteria: Vec<String> = options
                    .iter()
                    .map(|o| {
                        let m = o.means.as_deref().map_or_else(|| "null".to_string(), s);
                        format!("{}:{m}", s(&o.id))
                    })
                    .collect();
                format!(
                    "{},\"criteria\":{{{}}}}}",
                    head("choice", instructions),
                    criteria.join(",")
                )
            }
            Question::Score {
                instructions,
                levels,
            } => format!(
                "{},\"criteria\":{}}}",
                head("score", instructions),
                serde_json::to_string(levels).unwrap_or_default()
            ),
            Question::Noul {
                instructions,
                when_true,
                when_false,
            } => {
                let mut c = Vec::new();
                if let Some(t) = when_true {
                    c.push(format!("\"true\":{}", s(t)));
                }
                if let Some(f) = when_false {
                    c.push(format!("\"false\":{}", s(f)));
                }
                if c.is_empty() {
                    format!("{}}}", head("noul", instructions))
                } else {
                    format!(
                        "{},\"criteria\":{{{}}}}}",
                        head("noul", instructions),
                        c.join(",")
                    )
                }
            }
        }
    }
}

/// An image sent beside the state (theseus-und4g): the state names it by
/// `id`, once, and Jev receives the image in that place. Images are
/// experimental at TypeSafe: a request that carries one sends
/// [`IMAGES_HEADER`], and every limit is checked in code first
/// ([`crate::image`]), since one rejected image fails the whole request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// The reference the state holds: a UUID-shaped id of the bytes' hash, so
    /// packs sharing a state share its assets.
    pub id: String,
    /// `png` or `jpeg`.
    pub format: String,
    /// Standard base64 of the bytes, no `data:` prefix.
    pub data_base64: String,
}

/// The header a request with an image needs (verified 2026-10-20).
pub const IMAGES_HEADER: (&str, &str) = ("TypeSafe-Danger-Zone-Images", "true");
/// The tokens an image may add to a request's input, for a reserve: the probe
/// measured 1,566 for one, and an image is at most 1,024 px on a side.
pub const IMAGE_TOKENS: u64 = 2_000;

/// One call's request. The state is JSON text, kept exactly as it is hashed
/// and sent, so "byte-identical states" means what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub state: String,
    pub model: String,
    pub questions: Vec<(String, Question)>,
    /// Images the state refers to; none for a text-only request, whose body
    /// has no `assets` key at all.
    pub assets: Vec<Asset>,
}

impl Request {
    /// A request over a JSON state value (compact).
    pub fn new(state: &Value, model: &str, questions: Vec<(String, Question)>) -> Self {
        Self {
            state: serde_json::to_string(state).unwrap_or_else(|_| "null".into()),
            model: model.to_string(),
            questions,
            assets: Vec::new(),
        }
    }

    /// The same request carrying `assets`.
    pub fn with_assets(mut self, assets: Vec<Asset>) -> Self {
        self.assets = assets;
        self
    }

    /// The body: `{"state", ["assets",] "model", "questions"}`, with the
    /// state's bytes as given and the questions in order.
    pub fn body(&self) -> String {
        let questions: Vec<String> = self
            .questions
            .iter()
            .map(|(id, q)| {
                format!(
                    "{}:{}",
                    serde_json::to_string(id).unwrap_or_default(),
                    q.wire()
                )
            })
            .collect();
        let assets = match self.assets.is_empty() {
            true => String::new(),
            false => {
                let items: Vec<String> = self
                    .assets
                    .iter()
                    .map(|a| {
                        format!(
                            "{}:{{\"type\":\"image\",\"format\":\"{}\",\"data_base64\":\"{}\"}}",
                            serde_json::to_string(&a.id).unwrap_or_default(),
                            a.format,
                            a.data_base64
                        )
                    })
                    .collect();
                format!(",\"assets\":{{{}}}", items.join(","))
            }
        };
        format!(
            "{{\"state\":{}{},\"model\":{},\"questions\":{{{}}}}}",
            self.state,
            assets,
            serde_json::to_string(&self.model).unwrap_or_default(),
            questions.join(",")
        )
    }

    /// The body's size without the images' bytes: what the text costs.
    pub fn text_len(&self) -> usize {
        let images: usize = self.assets.iter().map(|a| a.data_base64.len()).sum();
        self.body().len() - images
    }

    /// The request again with no image: each asset's id in the state is
    /// replaced by a note that it was not sent, and the `assets` map is gone,
    /// so the text questions can still be answered when Jev refused an image.
    pub fn without_assets(&self) -> Self {
        let mut state = self.state.clone();
        for a in &self.assets {
            state = state.replace(&a.id, "(image not sent)");
        }
        Self {
            state,
            model: self.model.clone(),
            questions: self.questions.clone(),
            assets: Vec::new(),
        }
    }

    /// The state's size in tokens, estimated.
    pub fn state_tokens(&self) -> u64 {
        estimate_tokens(self.state.len())
    }

    pub fn question(&self, id: &str) -> Option<&Question> {
        self.questions.iter().find(|(q, _)| q == id).map(|(_, q)| q)
    }
}

// ----------------------------------------------------------------- response

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// One answer, checked against its question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Choice {
        choice: String,
        /// Per option, in the question's option order.
        probabilities: Vec<(String, f64)>,
        confidence: f64,
    },
    Score {
        /// The probability-weighted position: 0 for the first level, n − 1
        /// for the last (Jev's scale, verified 2026-09-30).
        score: f64,
        /// Per level, lowest first.
        probabilities: Vec<f64>,
        confidence: f64,
    },
    Noul {
        /// The probability that the statement is true.
        noul: f64,
    },
}

impl Answer {
    pub fn kind(&self) -> Kind {
        match self {
            Answer::Choice { .. } => Kind::Choice,
            Answer::Score { .. } => Kind::Score,
            Answer::Noul { .. } => Kind::Noul,
        }
    }

    /// The probability Jev gives a Choice's option (0 when absent).
    pub fn probability_of(&self, option: &str) -> Option<f64> {
        match self {
            Answer::Choice { probabilities, .. } => Some(
                probabilities
                    .iter()
                    .find(|(o, _)| o == option)
                    .map_or(0.0, |(_, p)| *p),
            ),
            _ => None,
        }
    }

    /// The most probable level of a Score, 0-based (the first on a tie).
    pub fn score_level(&self) -> Option<usize> {
        match self {
            Answer::Score { probabilities, .. } => Some(
                probabilities
                    .iter()
                    .enumerate()
                    .fold(
                        (0, f64::MIN),
                        |best, (i, &p)| if p > best.1 { (i, p) } else { best },
                    )
                    .0,
            ),
            _ => None,
        }
    }
}

/// A parsed, checked response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The concrete model that answered.
    pub model: String,
    pub usage: Usage,
    /// By question id, in the request's order.
    pub answers: Vec<(String, Answer)>,
}

impl Response {
    pub fn answer(&self, id: &str) -> Option<&Answer> {
        self.answers.iter().find(|(q, _)| q == id).map(|(_, a)| a)
    }
}

// ------------------------------------------------------------------- errors

/// Where in a call a timeout fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutPhase {
    /// Waiting for an in-flight permit, past a live call's deadline. Nothing
    /// was sent.
    Queue,
    /// TCP and TLS establishment.
    Connect,
    /// The whole call (no stream, so first byte is the total), or a live
    /// call's own shorter deadline.
    Total,
}

/// A classified Jev failure. The class is what the ledger records and the
/// judge reasons about; the message is for people, and it never holds the
/// key or a request header.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum JevError {
    #[error("jev timeout ({phase:?}) after {elapsed_ms} ms")]
    Timeout {
        phase: TimeoutPhase,
        elapsed_ms: u64,
    },
    #[error("jev network error: {message}")]
    Network { message: String },
    #[error("jev rate limited (429): {message}")]
    RateLimited {
        message: String,
        retry_after_secs: Option<u64>,
    },
    #[error("jev server error ({status}): {message}")]
    Server { status: u16, message: String },
    #[error("jev refused the key ({status}): {message}")]
    Auth { status: u16, message: String },
    #[error("jev refused the request ({status}): {message}")]
    InvalidRequest { status: u16, message: String },
    #[error("jev's answer is malformed: {reason}")]
    Malformed {
        reason: String,
        /// What the call cost, when the body still said.
        usage: Option<Usage>,
        model: Option<String>,
    },
    #[error("the state is over the limit: about {tokens} tokens, limit {limit}")]
    OverState { tokens: u64, limit: u64 },
}

impl JevError {
    pub fn class(&self) -> &'static str {
        match self {
            JevError::Timeout { .. } => "timeout",
            JevError::Network { .. } => "network",
            JevError::RateLimited { .. } => "rate_limited",
            JevError::Server { .. } => "server",
            JevError::Auth { .. } => "auth",
            JevError::InvalidRequest { .. } => "invalid_request",
            JevError::Malformed { .. } => "malformed",
            JevError::OverState { .. } => "over_state",
        }
    }

    /// Whether a later identical call could plausibly succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            JevError::Timeout { .. }
                | JevError::Network { .. }
                | JevError::RateLimited { .. }
                | JevError::Server { .. }
        )
    }

    /// Whether Jev may have charged for tokens the caller cannot see: a total
    /// timeout after the send, or a malformed body that did not say. Such a
    /// call holds its reservation until it is reconciled (§3.13).
    pub fn usage_unknown(&self) -> bool {
        match self {
            JevError::Timeout { phase, .. } => *phase == TimeoutPhase::Total,
            JevError::Malformed { usage, .. } => usage.is_none(),
            _ => false,
        }
    }

    /// Whether the breaker counts it: a transient failure of Jev's, not a
    /// local queue's.
    pub fn counts_for_breaker(&self) -> bool {
        self.is_transient()
            && !matches!(
                self,
                JevError::Timeout {
                    phase: TimeoutPhase::Queue,
                    ..
                }
            )
    }

    /// A non-success status, classified as the provider's are.
    pub fn from_status(
        status: u16,
        body: &str,
        retry_after: Option<u64>,
        state_tokens: u64,
    ) -> Self {
        let message = error_message(body);
        match status {
            429 => JevError::RateLimited {
                message,
                retry_after_secs: retry_after,
            },
            401..=403 => JevError::Auth { status, message },
            413 => JevError::OverState {
                tokens: state_tokens,
                limit: STATE_LIMIT_TOKENS,
            },
            500..=599 => JevError::Server { status, message },
            _ => JevError::InvalidRequest { status, message },
        }
    }

    fn malformed(reason: impl Into<String>) -> Self {
        JevError::Malformed {
            reason: reason.into(),
            usage: None,
            model: None,
        }
    }
}

/// An error body's message: `error.message`, `detail`, or `message` when the
/// body is JSON, else its first 300 characters.
fn error_message(body: &str) -> String {
    let v: Option<Value> = serde_json::from_str(body).ok();
    let pick = |v: &Value| -> Option<String> {
        let e = &v["error"];
        if let Some(m) = e["message"].as_str() {
            return Some(m.to_string());
        }
        if let Some(m) = e.as_str() {
            return Some(m.to_string());
        }
        if let Some(m) = v["detail"].as_str() {
            return Some(m.to_string());
        }
        if !v["detail"].is_null() {
            return Some(v["detail"].to_string());
        }
        v["message"].as_str().map(str::to_string)
    };
    let m = v
        .as_ref()
        .and_then(pick)
        .unwrap_or_else(|| body.chars().take(300).collect());
    m.chars().take(300).collect()
}

/// Replaces every appearance of the key with `[redacted]`, so no error
/// carries it even when a server echoes it.
fn redact(message: String, key: &str) -> String {
    if key.len() >= 8 && message.contains(key) {
        message.replace(key, "[redacted]")
    } else {
        message
    }
}

fn redact_error(e: JevError, key: &str) -> JevError {
    match e {
        JevError::Network { message } => JevError::Network {
            message: redact(message, key),
        },
        JevError::RateLimited {
            message,
            retry_after_secs,
        } => JevError::RateLimited {
            message: redact(message, key),
            retry_after_secs,
        },
        JevError::Server { status, message } => JevError::Server {
            status,
            message: redact(message, key),
        },
        JevError::Auth { status, message } => JevError::Auth {
            status,
            message: redact(message, key),
        },
        JevError::InvalidRequest { status, message } => JevError::InvalidRequest {
            status,
            message: redact(message, key),
        },
        JevError::Malformed {
            reason,
            usage,
            model,
        } => JevError::Malformed {
            reason: redact(reason, key),
            usage,
            model,
        },
        other => other,
    }
}

// ------------------------------------------------------------------ parsing

fn unit(x: &Value, what: &str) -> Result<f64, String> {
    let v = x
        .as_f64()
        .ok_or_else(|| format!("{what} is not a number"))?;
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("{what} {v} is outside 0..1"));
    }
    Ok(v)
}

fn sums_to_one(ps: impl Iterator<Item = f64>, what: &str) -> Result<(), String> {
    let sum: f64 = ps.sum();
    if (sum - 1.0).abs() > SUM_TOLERANCE {
        return Err(format!("{what}'s probabilities sum to {sum:.4}, not 1"));
    }
    Ok(())
}

/// One answer against its question.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn parse_answer(id: &str, q: &Question, a: &Value) -> Result<Answer, String> {
    let obj = a
        .as_object()
        .ok_or_else(|| format!("answer {id} is not an object"))?;
    if let Some(t) = obj.get("type") {
        if t.as_str() != Some(q.kind().as_str()) {
            return Err(format!(
                "answer {id} is typed {t}, but the question is a {}",
                q.kind().as_str()
            ));
        }
    }
    match q {
        Question::Choice { options, .. } => {
            let choice = obj
                .get("choice")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("answer {id} has no choice (wrong type)"))?;
            if !options.iter().any(|o| o.id == choice) {
                return Err(format!(
                    "answer {id} chose {choice:?}, not one of its options"
                ));
            }
            let probs = obj
                .get("probabilities")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("answer {id} has no probabilities object"))?;
            for k in probs.keys() {
                if !options.iter().any(|o| &o.id == k) {
                    return Err(format!(
                        "answer {id} gives a probability to {k:?}, not an option"
                    ));
                }
            }
            // Every option gets a probability, zeros included (verified).
            let mut probabilities = Vec::with_capacity(options.len());
            for o in options {
                let v = probs
                    .get(&o.id)
                    .ok_or_else(|| format!("answer {id} gives option {:?} no probability", o.id))?;
                let p = unit(v, &format!("answer {id}'s probability of {}", o.id))?;
                probabilities.push((o.id.clone(), p));
            }
            sums_to_one(
                probabilities.iter().map(|(_, p)| *p),
                &format!("answer {id}"),
            )?;
            let confidence = unit(
                obj.get("confidence")
                    .ok_or_else(|| format!("answer {id} has no confidence"))?,
                &format!("answer {id}'s confidence"),
            )?;
            Ok(Answer::Choice {
                choice: choice.to_string(),
                probabilities,
                confidence,
            })
        }
        Question::Score { levels, .. } => {
            let score = obj
                .get("score")
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("answer {id} has no score (wrong type)"))?;
            let n = levels.len();
            // Levels are keyed by their 0-based position, in `probabilities`
            // and in the `legend` that names them (verified 2026-09-30).
            let probs = obj
                .get("probabilities")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("answer {id} has no probabilities object"))?;
            let index = |k: &str| k.parse::<usize>().ok().filter(|i| *i < n);
            for k in probs.keys() {
                if index(k).is_none() {
                    return Err(format!(
                        "answer {id} gives a probability to {k:?}, not a level"
                    ));
                }
            }
            let probabilities: Vec<f64> = (0..n)
                .map(|i| {
                    let v = probs
                        .get(&i.to_string())
                        .ok_or_else(|| format!("answer {id} gives level {i} no probability"))?;
                    unit(v, &format!("answer {id}'s level {i}"))
                })
                .collect::<Result<_, _>>()?;
            sums_to_one(probabilities.iter().copied(), &format!("answer {id}"))?;
            if let Some(legend) = obj.get("legend") {
                let legend = legend
                    .as_object()
                    .ok_or_else(|| format!("answer {id}'s legend is not an object"))?;
                let agrees = legend.len() == n
                    && levels.iter().enumerate().all(|(i, l)| {
                        legend.get(&i.to_string()).and_then(Value::as_str) == Some(l)
                    });
                if !agrees {
                    return Err(format!(
                        "answer {id}'s legend does not match the levels asked"
                    ));
                }
            }
            if !(0.0..=(n - 1) as f64).contains(&score) {
                return Err(format!(
                    "answer {id}'s score {score} is outside 0..{}",
                    n - 1
                ));
            }
            let confidence = unit(
                obj.get("confidence")
                    .ok_or_else(|| format!("answer {id} has no confidence"))?,
                &format!("answer {id}'s confidence"),
            )?;
            Ok(Answer::Score {
                score,
                probabilities,
                confidence,
            })
        }
        Question::Noul { .. } => {
            let noul = unit(
                obj.get("noul")
                    .ok_or_else(|| format!("answer {id} has no noul (wrong type)"))?,
                &format!("answer {id}'s noul"),
            )?;
            Ok(Answer::Noul { noul })
        }
    }
}

/// A response body, parsed strictly against its request: a missing or
/// extra id, a wrong type, a Choice outside its options, a value outside
/// 0..1, or probabilities that don't sum to about 1 is `malformed`. The usage
/// and model are kept on a malformed answer when the body still gave them.
pub fn parse_response(body: &[u8], req: &Request) -> Result<Response, JevError> {
    let v: Value = serde_json::from_slice(body)
        .map_err(|e| JevError::malformed(format!("the body is not JSON: {e}")))?;
    let usage = v.get("usage").and_then(|u| {
        Some(Usage {
            input_tokens: u.get("input_tokens")?.as_u64()?,
            output_tokens: u.get("output_tokens")?.as_u64()?,
        })
    });
    let model = v.get("model").and_then(Value::as_str).map(str::to_string);
    let fail = |reason: String| JevError::Malformed {
        reason,
        usage,
        model: model.clone(),
    };
    let answers = v
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| fail("no answers object".into()))?;
    for id in answers.keys() {
        if req.question(id).is_none() {
            return Err(fail(format!("an answer for {id:?}, which was not asked")));
        }
    }
    let mut out = Vec::with_capacity(req.questions.len());
    for (id, q) in &req.questions {
        let a = answers
            .get(id)
            .ok_or_else(|| fail(format!("no answer for {id:?}")))?;
        out.push((id.clone(), parse_answer(id, q, a).map_err(fail)?));
    }
    let usage = usage.ok_or_else(|| fail("no usage".into()))?;
    let model = model.clone().ok_or_else(|| fail("no model".into()))?;
    Ok(Response {
        model,
        usage,
        answers: out,
    })
}

// ------------------------------------------------------------------- client

/// Where the key comes from, read at each call: the daemon settles secrets
/// after it serves, so a key may not be there yet.
pub trait KeySource: Send + Sync {
    fn key(&self) -> Option<Zeroizing<String>>;
}

/// A key held in memory, zeroed on drop. Its `Debug` never shows it.
pub struct StaticKey(Zeroizing<String>);

impl StaticKey {
    pub fn new(key: String) -> Self {
        Self(Zeroizing::new(key.trim().to_string()))
    }
}

impl std::fmt::Debug for StaticKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StaticKey([redacted])")
    }
}

impl KeySource for StaticKey {
    fn key(&self) -> Option<Zeroizing<String>> {
        (!self.0.is_empty()).then(|| self.0.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfig {
    pub api_base: String,
    pub connect: Duration,
    pub total: Duration,
    pub max_in_flight: usize,
    /// The permits kept back for [`Urgency::Awaited`] calls, out of
    /// `max_in_flight`. `None` is the default share ([`reserved_of`]);
    /// `Some(0)` keeps none, so a deployment that makes no awaited call
    /// (`[content_based_security] session_flags = false`) leaves every permit
    /// to the ordinary pool (theseus-oc9n4). Never all of them: a count of
    /// `max_in_flight` or more keeps all but one.
    #[serde(default)]
    pub reserved: Option<usize>,
}

impl ClientConfig {
    /// The permits kept back for awaited calls: the configured count, or the
    /// default share ([`reserved_of`]), and never all of `max_in_flight`.
    fn reserved_permits(&self) -> usize {
        self.reserved
            .unwrap_or_else(|| reserved_of(self.max_in_flight))
            .min(self.max_in_flight.saturating_sub(1))
    }
}

impl Default for ClientConfig {
    /// Connect 2 s, total 5 s, eight in flight (design §2.2 and §2.15).
    fn default() -> Self {
        Self {
            api_base: DEFAULT_API_BASE.into(),
            connect: Duration::from_secs(2),
            total: Duration::from_secs(5),
            max_in_flight: 8,
            reserved: None,
        }
    }
}

/// How a call waits for a permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "urgency", rename_all = "snake_case")]
pub enum Urgency {
    /// Only tries: with no permit free, the call is shed.
    Shadow,
    /// Waits for a permit up to the deadline, which also bounds the call.
    Live { deadline_ms: u64 },
    /// [`Urgency::Live`], for a call **a turn may be blocked on**: it may take
    /// one of the permits kept back for such calls ([`RESERVED_SHARE`]) as
    /// well as an ordinary one, whichever frees first (theseus-oc9n4).
    ///
    /// Every content call goes out so (`content.v3`'s `ask_once`), and the
    /// cn floor is the reader it is for: it waits at most
    /// `[content_based_security] wait_ms` for the content asks out for its
    /// session and then goes on the labels it has, so a content call that
    /// has not been *made* by the end of that window is a window spent for
    /// nothing and a turn moved off its model. Without a share kept back,
    /// that call queues behind every ordinary judge call.
    ///
    /// The variant does not know whether a floor is waiting: the per-session
    /// drain judges every item that enters every session, and each of those
    /// calls takes the reserve alike, so one nobody waits on can hold a
    /// reserved permit another session's floor needs. A call that is told
    /// apart by a waiter is a later change.
    Awaited { deadline_ms: u64 },
}

impl Urgency {
    pub fn live(deadline: Duration) -> Self {
        Urgency::Live {
            deadline_ms: deadline.as_millis() as u64,
        }
    }

    /// [`Urgency::Awaited`]: live, and a turn may be blocked on the answer.
    pub fn awaited(deadline: Duration) -> Self {
        Urgency::Awaited {
            deadline_ms: deadline.as_millis() as u64,
        }
    }

    /// How long this call may wait for a permit, and none for a shadow one.
    fn deadline(self) -> Option<Duration> {
        match self {
            Urgency::Shadow => None,
            Urgency::Live { deadline_ms } | Urgency::Awaited { deadline_ms } => {
                Some(Duration::from_millis(deadline_ms))
            }
        }
    }
}

/// One in `RESERVED_SHARE` of the in-flight permits is kept for content calls
/// ([`Urgency::Awaited`]), which a turn may be blocked on, rounded down but
/// never to none, and never all of them: a client of one permit keeps none
/// back and behaves as it always did. A client built with
/// [`ClientConfig::reserved`] of `Some(0)` keeps none either: the ordinary
/// pool is `max_in_flight` less the share, so a deployment that makes no
/// awaited call and kept it would run that many fewer concurrent calls.
const RESERVED_SHARE: usize = 4;

/// The permits kept back for awaited calls, out of `max_in_flight`.
fn reserved_of(max_in_flight: usize) -> usize {
    match max_in_flight {
        0 | 1 => 0,
        n => (n / RESERVED_SHARE).max(1).min(n - 1),
    }
}

/// Where a call's time went, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    /// Waiting for an in-flight permit.
    pub queued_ms: u64,
    /// From the send to the last byte of the body.
    pub http_ms: u64,
    /// Everything, parsing included.
    pub total_ms: u64,
}

/// Why a call never reached Jev, or how it failed there.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallError {
    /// No permit was free for a shadow call; it is counted, not sent.
    #[error("shed: every in-flight permit is taken")]
    Shed,
    /// The key has not settled (or is empty); nothing was sent.
    #[error("no key: the Jev key has not settled")]
    NoKey,
    #[error(transparent)]
    Jev(JevError),
}

/// One call's outcome, with its timing either way.
#[derive(Debug, Clone)]
pub struct Called {
    pub timing: Timing,
    /// Rate-limit and retry headers from the response, by lowercase name.
    pub rate_limit: BTreeMap<String, String>,
    /// The body as received (empty when no body arrived); the probe prints
    /// it, and the core may keep it for a malformed answer.
    pub raw: Vec<u8>,
    /// Jev refused an image and the request went again without them
    /// (theseus-und4g); the answers are the text's.
    pub images_dropped: bool,
    pub result: Result<Response, CallError>,
}

/// Shed calls, with a report at most once a minute (`judge.shed`).
#[derive(Debug, Default)]
struct ShedMeter {
    total: AtomicU64,
    unreported: AtomicU64,
    last_report: Mutex<Option<Instant>>,
}

pub struct JevClient {
    http: reqwest::Client,
    url: String,
    config: ClientConfig,
    key: Arc<dyn KeySource>,
    /// The permits any call may take: `max_in_flight` less the reserved
    /// share.
    permits: Arc<Semaphore>,
    /// The permits only an [`Urgency::Awaited`] call may take
    /// ([`reserved_of`]). Empty on a client of one permit, and on one built with
    /// [`ClientConfig::reserved`] of `Some(0)`. The two together
    /// are `max_in_flight`, so the ceiling is what it always was: this
    /// divides the permits, it does not add any.
    reserved: Arc<Semaphore>,
    shed: ShedMeter,
    reach: Reach,
}

/// What the client knows of its connections (theseus-otny), in ms since it
/// was built, 0 for never: when Jev last answered (any status, a call's or
/// a warm-up's), when a try last failed to connect, and whether a warm-up
/// is in flight. reqwest's pool says nothing of what it holds.
#[derive(Debug)]
struct Reach {
    born: Instant,
    answered: AtomicU64,
    failed: AtomicU64,
    warming: std::sync::atomic::AtomicBool,
}

impl Reach {
    fn new() -> Self {
        Self {
            born: Instant::now(),
            answered: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            warming: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn now(&self) -> u64 {
        self.born.elapsed().as_millis() as u64 + 1
    }

    fn heard(&self) {
        self.answered.store(self.now(), Ordering::Relaxed);
    }

    fn missed(&self) {
        self.failed.store(self.now(), Ordering::Relaxed);
    }
}

impl std::fmt::Debug for JevClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JevClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl JevClient {
    pub fn new(config: ClientConfig, key: Arc<dyn KeySource>) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("theseus-judge/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(config.connect)
            .pool_idle_timeout(POOL_IDLE)
            .build()?;
        let kept = config.reserved_permits();
        Ok(Self {
            http,
            url: format!("{}{PATH}", config.api_base.trim_end_matches('/')),
            permits: Arc::new(Semaphore::new(config.max_in_flight - kept)),
            reserved: Arc::new(Semaphore::new(kept)),
            config,
            key,
            shed: ShedMeter::default(),
            reach: Reach::new(),
        })
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Whether a connection is likely open: Jev answered within the pool's
    /// idle time, less a margin for a pause that ends as it closes.
    pub fn warm(&self) -> bool {
        let a = self.reach.answered.load(Ordering::Relaxed);
        let open = (POOL_IDLE - Duration::from_secs(10)).as_millis() as u64;
        a > 0 && self.reach.now().saturating_sub(a) < open
    }

    /// Whether Jev is known unreachable: the last try failed to connect (or
    /// its connect timed out), and nothing has answered since.
    pub fn unreachable(&self) -> bool {
        let f = self.reach.failed.load(Ordering::Relaxed);
        f > 0 && f >= self.reach.answered.load(Ordering::Relaxed)
    }

    /// Open `n` connections now (theseus-otny), unless the client is warm
    /// or a warm-up is in flight: `n` HEADs of the judge's path at once,
    /// each under the connect timeout and a second more. No key and no body,
    /// so nothing is billed (the edge answers 405 and keeps the connection);
    /// each answer, whatever its status, leaves its connection in the pool
    /// for the judgments that follow. Returns how long it took, and whether
    /// the client is warm after it; `None`: nothing was sent.
    pub async fn warm_up(&self, n: usize) -> Option<(Duration, bool)> {
        if self.warm() {
            return None;
        }
        self.refresh(n).await
    }

    /// How long since Jev last answered (a call or a warm-up); `None`:
    /// never.
    pub fn heard_ago(&self) -> Option<Duration> {
        let a = self.reach.answered.load(Ordering::Relaxed);
        (a > 0).then(|| Duration::from_millis(self.reach.now().saturating_sub(a)))
    }

    /// [`JevClient::warm_up`] whether or not the client is warm, so the
    /// connections it holds are used again before the pool's idle time
    /// closes them (theseus-ddbi: the keeper, every [`KEEP_WARM`] of
    /// silence). `None`: a warm-up was in flight, nothing was sent.
    pub async fn refresh(&self, n: usize) -> Option<(Duration, bool)> {
        if self.reach.warming.swap(true, Ordering::AcqRel) {
            return None;
        }
        let bound = self.config.connect + Duration::from_secs(1);
        let one = || async {
            let head = self.http.head(&self.url).send();
            match tokio::time::timeout(bound, head).await {
                Ok(Ok(_)) => self.reach.heard(),
                Ok(Err(e)) if !e.is_connect() && !e.is_timeout() => {}
                _ => self.reach.missed(),
            }
        };
        let t0 = Instant::now();
        futures_util::future::join_all((0..n).map(|_| one())).await;
        self.reach.warming.store(false, Ordering::Release);
        Some((t0.elapsed(), self.warm()))
    }

    /// Calls in flight now (health): both pools', since the two together are
    /// `max_in_flight`.
    pub fn in_flight(&self) -> usize {
        self.config
            .max_in_flight
            .saturating_sub(self.permits.available_permits())
            .saturating_sub(self.reserved.available_permits())
    }

    /// Calls shed since the client was built.
    pub fn shed_total(&self) -> u64 {
        self.shed.total.load(Ordering::Relaxed)
    }

    /// The calls shed since the last report, when there are some and a
    /// minute has passed since that report: the core writes one `judge.shed`
    /// row per minute at most.
    pub fn take_shed_report(&self, now: Instant) -> Option<u64> {
        let mut last = self
            .shed
            .last_report
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| now.saturating_duration_since(t) < Duration::from_secs(60)) {
            return None;
        }
        let n = self.shed.unreported.swap(0, Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        *last = Some(now);
        Some(n)
    }

    async fn admit(&self, urgency: Urgency) -> Result<OwnedSemaphorePermit, CallError> {
        match urgency {
            Urgency::Shadow => self.permits.clone().try_acquire_owned().map_err(|_| {
                self.shed.total.fetch_add(1, Ordering::Relaxed);
                self.shed.unreported.fetch_add(1, Ordering::Relaxed);
                CallError::Shed
            }),
            Urgency::Live { deadline_ms } => {
                let d = Duration::from_millis(deadline_ms);
                match tokio::time::timeout(d, self.permits.clone().acquire_owned()).await {
                    Ok(Ok(p)) => Ok(p),
                    _ => Err(CallError::Jev(JevError::Timeout {
                        phase: TimeoutPhase::Queue,
                        elapsed_ms: deadline_ms,
                    })),
                }
            }
            // A turn may be blocked on this answer: it waits on the reserved
            // pool and the ordinary one together and takes whichever frees
            // first, so the longest it can wait for a permit is one
            // reserved call's length and not the whole ordinary queue's.
            // Dropping the loser is safe: tokio hands a permit granted to a
            // cancelled waiter on to the next.
            Urgency::Awaited { deadline_ms } => {
                let d = Duration::from_millis(deadline_ms);
                let (ordinary, reserved) = (self.permits.clone(), self.reserved.clone());
                let got = tokio::time::timeout(d, async move {
                    tokio::select! {
                        // Ordinary first, so the scarce reserved permits are
                        // spent only when the ordinary pool is full, which
                        // is the one case they exist for.
                        biased;
                        p = ordinary.acquire_owned() => p,
                        p = reserved.acquire_owned() => p,
                    }
                })
                .await;
                match got {
                    Ok(Ok(p)) => Ok(p),
                    _ => Err(CallError::Jev(JevError::Timeout {
                        phase: TimeoutPhase::Queue,
                        elapsed_ms: deadline_ms,
                    })),
                }
            }
        }
    }

    /// One call: a permit (or shed), the key, then the POST under the total
    /// timeout (or the live deadline, whichever is shorter), then strict
    /// parsing. Never retries.
    pub async fn call(&self, req: &Request, urgency: Urgency) -> Called {
        let started = Instant::now();
        let ms = |d: Duration| d.as_millis() as u64;
        let mut called = Called {
            timing: Timing::default(),
            rate_limit: BTreeMap::new(),
            raw: Vec::new(),
            images_dropped: false,
            result: Err(CallError::NoKey),
        };
        let tokens = req.state_tokens();
        if tokens > STATE_LIMIT_TOKENS {
            called.result = Err(CallError::Jev(JevError::OverState {
                tokens,
                limit: STATE_LIMIT_TOKENS,
            }));
            return called;
        }
        let permit = match self.admit(urgency).await {
            Ok(p) => p,
            Err(e) => {
                called.timing.queued_ms = ms(started.elapsed());
                called.timing.total_ms = called.timing.queued_ms;
                called.result = Err(e);
                return called;
            }
        };
        let queued = started.elapsed();
        called.timing.queued_ms = ms(queued);
        let Some(key) = self.key.key() else {
            called.timing.total_ms = ms(started.elapsed());
            return called;
        };
        let budget = match urgency.deadline() {
            None => self.config.total,
            Some(deadline) => self.config.total.min(deadline.saturating_sub(queued)),
        };
        let sent = Instant::now();
        let outcome = tokio::time::timeout(budget, self.post(req, &key)).await;
        let mut result = settle(&mut called, req, tokens, outcome, started);
        // One refused image fails the whole request, so the text's answers
        // would be lost with it: when Jev names an image, ask again, once,
        // without them (theseus-und4g).
        if !req.assets.is_empty() && refused_image(&result) {
            let plain = req.without_assets();
            let left = budget.saturating_sub(sent.elapsed());
            let outcome = tokio::time::timeout(left, self.post(&plain, &key)).await;
            result = settle(&mut called, &plain, tokens, outcome, started);
            called.images_dropped = true;
        }
        called.timing.http_ms = ms(sent.elapsed());
        drop(permit);
        called.result = result.map_err(|e| CallError::Jev(redact_error(e, &key)));
        called.timing.total_ms = ms(started.elapsed());
        called
    }

    /// The POST and the whole body, bounded in size. Errors are classified;
    /// none carries the URL or a header.
    async fn post(
        &self,
        req: &Request,
        key: &str,
    ) -> Result<(u16, BTreeMap<String, String>, Vec<u8>), JevError> {
        let mut post = self
            .http
            .post(&self.url)
            .bearer_auth(key)
            .header("content-type", "application/json");
        if !req.assets.is_empty() {
            post = post.header(IMAGES_HEADER.0, IMAGES_HEADER.1);
        }
        let sent = post.body(req.body()).send().await;
        let mut resp = match sent {
            Ok(r) => {
                self.reach.heard();
                r
            }
            Err(e) => {
                if e.is_connect() {
                    self.reach.missed();
                }
                return Err(classify(e));
            }
        };
        let status = resp.status().as_u16();
        let headers: BTreeMap<String, String> = resp
            .headers()
            .iter()
            .filter(|(k, _)| {
                let k = k.as_str();
                k == "retry-after" || k.contains("ratelimit") || k.contains("rate-limit")
            })
            .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(classify)? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(JevError::malformed(format!(
                    "the body is over {MAX_RESPONSE_BYTES} bytes"
                )));
            }
        }
        Ok((status, headers, body))
    }
}

/// One POST's status, headers and body.
type Posted = (u16, BTreeMap<String, String>, Vec<u8>);

/// What one POST came to, as a [`Response`] or its classified error; the
/// call's rate-limit headers and raw body are the last attempt's.
fn settle(
    called: &mut Called,
    req: &Request,
    tokens: u64,
    outcome: Result<Result<Posted, JevError>, tokio::time::error::Elapsed>,
    started: Instant,
) -> Result<Response, JevError> {
    match outcome {
        Err(_) => Err(JevError::Timeout {
            phase: TimeoutPhase::Total,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }),
        Ok(Err(e)) => Err(e),
        Ok(Ok((status, headers, body))) => {
            called.rate_limit = headers;
            called.raw = body;
            if (200..300).contains(&status) {
                parse_response(&called.raw, req)
            } else {
                let text = String::from_utf8_lossy(&called.raw).into_owned();
                let retry = called
                    .rate_limit
                    .get("retry-after")
                    .and_then(|v| v.trim().parse().ok());
                Err(JevError::from_status(status, &text, retry, tokens))
            }
        }
    }
}

/// Jev refused the request over an image: a 400 whose message names an
/// image or an asset ("Invalid image dimensions.", "Invalid image request.",
/// "format must be png, jpg or jpeg.").
fn refused_image(r: &Result<Response, JevError>) -> bool {
    match r {
        Err(JevError::InvalidRequest {
            status: 400,
            message,
        }) => {
            let m = message.to_lowercase();
            m.contains("image") || m.contains("asset") || m.contains("png")
        }
        _ => false,
    }
}

/// A transport error, classified; the URL is left out of the message.
fn classify(e: reqwest::Error) -> JevError {
    if e.is_connect() && e.is_timeout() {
        return JevError::Timeout {
            phase: TimeoutPhase::Connect,
            elapsed_ms: 0,
        };
    }
    if e.is_timeout() {
        return JevError::Timeout {
            phase: TimeoutPhase::Total,
            elapsed_ms: 0,
        };
    }
    let e = e.without_url();
    // reqwest's own line is vague ("error sending request"); the causes
    // below it say what happened, and none of them names the URL.
    let mut parts = vec![e.to_string()];
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        parts.push(c.to_string());
        cause = c.source();
    }
    JevError::Network {
        message: parts.join(": ").chars().take(300).collect(),
    }
}

#[cfg(test)]
mod tests_permits {
    use super::*;

    /// **The reserved share divides the permits, it never adds any**
    /// (theseus-oc9n4): a quarter of them, rounded down but never to none,
    /// and never all of them, so an ordinary call always has one to take
    /// too. A client of one permit keeps none back and behaves as it did.
    #[test]
    fn the_reserved_share_divides_the_permits() {
        for n in [1usize, 2, 3, 4, 5, 8, 16, 64] {
            let kept = reserved_of(n);
            assert!(kept < n, "{n}: the reserve took every permit");
            assert!(
                n == 1 || kept >= 1,
                "{n}: nothing was kept for the calls a turn waits on"
            );
            assert!(kept <= n / RESERVED_SHARE.min(n).max(1) + 1, "{n}: {kept}");
        }
        assert_eq!(reserved_of(0), 0);
        assert_eq!(reserved_of(1), 0, "one permit: nothing to divide");
        assert_eq!(reserved_of(2), 1);
        assert_eq!(reserved_of(8), 2, "the default: two of eight");
        assert_eq!(reserved_of(64), 16);
    }

    /// A client's two pools add up to its `max_in_flight`, so the ceiling on
    /// what Theseus asks of Jev at once is exactly what it was.
    #[test]
    fn the_two_pools_add_up_to_max_in_flight() {
        struct NoKey;
        impl KeySource for NoKey {
            fn key(&self) -> Option<Zeroizing<String>> {
                None
            }
        }
        for n in [1usize, 2, 8, 64] {
            let c = JevClient::new(
                ClientConfig {
                    max_in_flight: n,
                    ..ClientConfig::default()
                },
                Arc::new(NoKey),
            )
            .unwrap();
            assert_eq!(
                c.permits.available_permits() + c.reserved.available_permits(),
                n,
                "max_in_flight {n}"
            );
            assert_eq!(c.in_flight(), 0, "max_in_flight {n}: none in flight yet");
        }
    }

    /// **A client told to keep none back leaves every permit to the ordinary
    /// pool** (review I-4): with `session_flags` off nothing takes a reserved
    /// permit, so the share must not be held idle against `Live` and `Shadow`.
    /// `None` is still the default share, and a count is never all of them.
    #[test]
    fn a_client_told_to_keep_none_back_has_one_pool() {
        struct NoKey;
        impl KeySource for NoKey {
            fn key(&self) -> Option<Zeroizing<String>> {
                None
            }
        }
        for (n, reserved, ordinary, kept) in [
            (8usize, Some(0usize), 8usize, 0usize),
            (2, Some(0), 2, 0),
            (8, None, 6, 2),
            (2, None, 1, 1),
            (8, Some(3), 5, 3),
            (4, Some(40), 1, 3),
            (1, Some(1), 1, 0),
        ] {
            let c = JevClient::new(
                ClientConfig {
                    max_in_flight: n,
                    reserved,
                    ..ClientConfig::default()
                },
                Arc::new(NoKey),
            )
            .unwrap();
            assert_eq!(
                (
                    c.permits.available_permits(),
                    c.reserved.available_permits()
                ),
                (ordinary, kept),
                "max_in_flight {n}, reserved {reserved:?}"
            );
            assert_eq!(c.in_flight(), 0, "max_in_flight {n}, reserved {reserved:?}");
        }
    }
}
