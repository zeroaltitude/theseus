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

/// One call's request. The state is JSON text, kept exactly as it is hashed
/// and sent, so "byte-identical states" means what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub state: String,
    pub model: String,
    pub questions: Vec<(String, Question)>,
}

impl Request {
    /// A request over a JSON state value (compact).
    pub fn new(state: &Value, model: &str, questions: Vec<(String, Question)>) -> Self {
        Self {
            state: serde_json::to_string(state).unwrap_or_else(|_| "null".into()),
            model: model.to_string(),
            questions,
        }
    }

    /// The body: `{"state", "model", "questions"}`, with the state's bytes
    /// as given and the questions in order.
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
        format!(
            "{{\"state\":{},\"model\":{},\"questions\":{{{}}}}}",
            self.state,
            serde_json::to_string(&self.model).unwrap_or_default(),
            questions.join(",")
        )
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
}

impl Default for ClientConfig {
    /// Connect 2 s, total 5 s, eight in flight (design §2.2 and §2.15).
    fn default() -> Self {
        Self {
            api_base: DEFAULT_API_BASE.into(),
            connect: Duration::from_secs(2),
            total: Duration::from_secs(5),
            max_in_flight: 8,
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
}

impl Urgency {
    pub fn live(deadline: Duration) -> Self {
        Urgency::Live {
            deadline_ms: deadline.as_millis() as u64,
        }
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
    permits: Arc<Semaphore>,
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
        Ok(Self {
            http,
            url: format!("{}{PATH}", config.api_base.trim_end_matches('/')),
            permits: Arc::new(Semaphore::new(config.max_in_flight)),
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
        if self.warm() || self.reach.warming.swap(true, Ordering::AcqRel) {
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

    /// Calls in flight now (health).
    pub fn in_flight(&self) -> usize {
        self.config
            .max_in_flight
            .saturating_sub(self.permits.available_permits())
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
        let budget = match urgency {
            Urgency::Shadow => self.config.total,
            Urgency::Live { deadline_ms } => self
                .config
                .total
                .min(Duration::from_millis(deadline_ms).saturating_sub(queued)),
        };
        let sent = Instant::now();
        let outcome = tokio::time::timeout(budget, self.post(req, &key)).await;
        called.timing.http_ms = ms(sent.elapsed());
        drop(permit);
        let result = match outcome {
            Err(_) => Err(JevError::Timeout {
                phase: TimeoutPhase::Total,
                elapsed_ms: ms(started.elapsed()),
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
        };
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
        let sent = self
            .http
            .post(&self.url)
            .bearer_auth(key)
            .header("content-type", "application/json")
            .body(req.body())
            .send()
            .await;
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
