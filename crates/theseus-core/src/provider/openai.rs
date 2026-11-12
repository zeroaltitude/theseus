//! OpenAI's Responses API as a second wire protocol (theseus-bq73s, row B1).
//!
//! `ProviderKind` anticipated a second protocol from the first day and carried
//! one value; this is the second. It is an **in-process arm**, not a proxy:
//! the card weighed both and the decision taken was the arm, because a proxy
//! would be a new trust boundary and a second process to supervise for no
//! capability the arm lacks.
//!
//! **It lands switched off.** A provider no profile names is never called, and
//! the template ships the `[providers.openai]` block commented out with no
//! profile on it, so a build that carries this code reaches OpenAI only after
//! the operator un-comments the block, puts a key in the vault and names a
//! model on it.
//!
//! What is here is the client: the key, the connection, the one POST and the
//! event loop, which is the Anthropic arm's loop with the same timeouts, the
//! same phases and the same errors. Everything shape-shaped — the request
//! body, the events, the usage, the rate limits — is in [`wire`], which is
//! pure and tested on its own.
//!
//! Not here, and named in theseus-bq73s's report rather than guessed at: a
//! token counter (`/v1/responses` has no count endpoint, so
//! [`Provider::count_tokens`] keeps the trait's refusal and `theseus census
//! --exact` says so), and catalog rows for OpenAI's models, which an operator
//! writes under `[catalog.<model>]` as the template already shows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::StreamExt;

use super::{
    CallTiming, DeltaSink, ModelResponse, Provider, ProviderError, ProviderFuture, ProviderRequest,
    SseLines, TimeoutPhase, Timeouts, WarmFuture, Warmed, POOL_IDLE,
};
use crate::secrets::{Secret, SecretBoard};

mod wire;

#[cfg(test)]
mod tests;

/// The path the Responses API answers on, under the provider's `api_base`.
const PATH: &str = "/responses";

#[derive(Clone)]
pub struct OpenAi {
    http: reqwest::Client,
    /// When the API last answered on this client (a call or a warm-up).
    heard: Arc<std::sync::Mutex<Option<Instant>>>,
    pool_idle: Duration,
    api_base: String,
    /// The key, read from the board at each call: the daemon serves before it
    /// resolves (theseus-qa0), and a turn waits for it before calling.
    secrets: Arc<SecretBoard>,
    key_secret: String,
    timeouts: Timeouts,
}

impl OpenAi {
    pub fn new(
        api_base: &str,
        secrets: Arc<SecretBoard>,
        key_secret: &str,
        timeouts: Timeouts,
    ) -> Result<Self> {
        Self::with_pool_idle(api_base, secrets, key_secret, timeouts, POOL_IDLE)
    }

    /// [`OpenAi::new`] with the pool's idle time named (a test's seam).
    pub fn with_pool_idle(
        api_base: &str,
        secrets: Arc<SecretBoard>,
        key_secret: &str,
        timeouts: Timeouts,
        pool_idle: Duration,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("theseus/{}", crate::VERSION))
            .connect_timeout(Duration::from_secs(timeouts.connect_secs))
            .pool_idle_timeout(pool_idle)
            .build()?;
        Ok(Self {
            http,
            heard: Arc::default(),
            pool_idle,
            api_base: api_base.trim_end_matches('/').to_string(),
            secrets,
            key_secret: key_secret.to_string(),
            timeouts,
        })
    }

    /// Whether the API answered within the pool's idle time, less a margin for
    /// a pause that ends as the connection closes: one is likely open.
    fn is_warm(&self) -> bool {
        let margin = Duration::from_secs(10).min(self.pool_idle / 2);
        self.heard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|t| t.elapsed() < self.pool_idle - margin)
    }

    fn heard(&self) {
        *self.heard.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }

    /// The key, or why there is none. Fail closed: no call goes out without it.
    fn key(&self) -> Result<Secret> {
        self.secrets.get(&self.key_secret).ok_or_else(|| {
            anyhow::anyhow!(
                "secret {} has not resolved; a turn waits for it before calling",
                self.key_secret
            )
        })
    }

    fn url(&self) -> String {
        format!("{}{PATH}", self.api_base)
    }

    /// Open a connection unless one is warm: a `HEAD` of the responses path
    /// with no key and no body, so it is never a model call and costs nothing.
    /// The Anthropic arm's warm-up, with this protocol's path.
    async fn warm_impl(&self) -> Result<Warmed, String> {
        if self.is_warm() {
            return Ok(Warmed {
                took: Duration::ZERO,
                already: true,
            });
        }
        let t0 = Instant::now();
        let bound = Duration::from_secs(self.timeouts.connect_secs + 1);
        let head = self.http.head(self.url()).send();
        match tokio::time::timeout(bound, head).await {
            Ok(Ok(_)) => {
                self.heard();
                Ok(Warmed {
                    took: t0.elapsed(),
                    already: false,
                })
            }
            Ok(Err(e)) => Err(e.without_url().to_string()),
            Err(_) => Err(format!("no answer within {} ms", bound.as_millis())),
        }
    }

    /// The response's headers, within the first-byte budget. The error phases
    /// are the Anthropic arm's, so every reader that classifies a provider
    /// failure reads one vocabulary whatever the protocol.
    async fn send(
        &self,
        req: &ProviderRequest,
        key: &Secret,
        started: Instant,
        budget: Duration,
    ) -> Result<reqwest::Response> {
        let body = serde_json::to_vec(&wire::body(req, true))?;
        let post = self
            .http
            .post(self.url())
            .header("authorization", format!("Bearer {}", key.expose()))
            .header("content-type", "application/json")
            .body(body)
            .send();
        let elapsed = started.elapsed().as_millis() as u64;
        match tokio::time::timeout(budget, post).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) if e.is_connect() && e.is_timeout() => Err(ProviderError::Timeout {
                phase: TimeoutPhase::Connect,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }
            .into()),
            Ok(Err(e)) => Err(ProviderError::Network {
                message: e.to_string(),
            }
            .into()),
            Err(_) => Err(ProviderError::Timeout {
                phase: TimeoutPhase::FirstByte,
                elapsed_ms: elapsed.max(budget.as_millis() as u64),
            }
            .into()),
        }
    }

    async fn stream_message_impl(
        &self,
        req: &ProviderRequest,
        on_delta: DeltaSink<'_>,
    ) -> Result<ModelResponse> {
        let key = self.key()?;
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.timeouts.total_secs);
        let elapsed = |s: Instant| s.elapsed().as_millis() as u64;

        let first_byte_budget =
            Duration::from_secs(self.timeouts.first_byte_secs).min(deadline - started);
        let resp = self.send(req, &key, started, first_byte_budget).await?;
        self.heard();
        let first_byte_ms = elapsed(started);
        // The stream began: it may go on while events arrive, to the hard bound.
        let deadline = started + self.timeouts.whole_call();
        let headers = resp.headers().clone();
        let status = resp.status();
        let request_id = headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::from_status(status.as_u16(), &text, &headers).into());
        }

        let mut out = ModelResponse {
            model: req.model.clone(),
            request_id,
            rate_limit: wire::rate_limit(&headers),
            timing: CallTiming {
                first_byte_ms: Some(first_byte_ms),
                ..Default::default()
            },
            ..Default::default()
        };
        let done = self
            .events(resp, &mut out, started, deadline, on_delta)
            .await?;
        out.text = super::text_of(&out.content);
        out.timing.total_ms = elapsed(started);
        match done {
            Some(response) => {
                wire::settle(&mut out, &response);
                Ok(out)
            }
            // The stream ended with no completion event: the same state the
            // Anthropic arm calls truncated, and the same error, so the
            // turn's retry reads it the way it always has.
            None => Err(ProviderError::Truncated {
                elapsed_ms: out.timing.total_ms,
            }
            .into()),
        }
    }

    /// The event loop: chunks to lines to events, the blocks appended in the
    /// order the far end finished them. `Ok(Some(response))` is a complete
    /// response; `Ok(None)` a stream that stopped without one.
    async fn events(
        &self,
        resp: reqwest::Response,
        out: &mut ModelResponse,
        started: Instant,
        deadline: Instant,
        on_delta: DeltaSink<'_>,
    ) -> Result<Option<serde_json::Value>> {
        let elapsed = |s: Instant| s.elapsed().as_millis() as u64;
        let mut lines = SseLines::default();
        let mut stream = resp.bytes_stream();
        let idle = Duration::from_secs(self.timeouts.stream_idle_secs);
        let mut invalid = std::collections::BTreeMap::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(ProviderError::Timeout {
                    phase: TimeoutPhase::Total,
                    elapsed_ms: elapsed(started),
                }
                .into());
            }
            let budget = idle.min(deadline - now);
            let chunk = match tokio::time::timeout(budget, stream.next()).await {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    return Err(ProviderError::Network {
                        message: format!("reading stream: {e}"),
                    }
                    .into())
                }
                Ok(None) => break,
                Err(_) => {
                    let phase = if Instant::now() >= deadline {
                        TimeoutPhase::Total
                    } else {
                        TimeoutPhase::StreamIdle
                    };
                    return Err(ProviderError::Timeout {
                        phase,
                        elapsed_ms: elapsed(started),
                    }
                    .into());
                }
            };
            lines.push(&chunk);
            while let Some(line) = lines.next_line() {
                let Some(ev) = data_of(&line) else { continue };
                match wire::step(&ev, &mut invalid, on_delta) {
                    wire::Step::Nothing => {}
                    wire::Step::Item(block) => {
                        if out.timing.first_token_ms.is_none() {
                            out.timing.first_token_ms = Some(elapsed(started));
                        }
                        if let Some(b) = block {
                            out.content.push(b);
                        }
                    }
                    wire::Step::Done(response) => {
                        out.invalid_tool_inputs = invalid;
                        return Ok(Some(response));
                    }
                    // A failure reported inside a 200 stream is a stream
                    // error, which is what the Anthropic arm calls one too,
                    // so `class()` and `is_transient` read it unchanged.
                    wire::Step::Failed(message) => {
                        return Err(ProviderError::Stream {
                            kind: "response_failed".into(),
                            message,
                        }
                        .into())
                    }
                }
            }
        }
        out.invalid_tool_inputs = invalid;
        Ok(None)
    }
}

/// The JSON of one `data:` line, or `None` for any other line (the `event:`
/// name, a comment, a blank). The Responses stream names its event in the
/// payload's own `type`, so the `event:` line is redundant and is not read:
/// one source of truth for what an event is.
fn data_of(line: &str) -> Option<serde_json::Value> {
    let rest = line.strip_prefix("data:")?.trim();
    if rest.is_empty() || rest == "[DONE]" {
        return None;
    }
    serde_json::from_str(rest).ok()
}

impl Provider for OpenAi {
    fn name(&self) -> &str {
        "openai"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(self.stream_message_impl(req, on_delta))
    }
    fn warm<'a>(&'a self, _model: &'a str) -> WarmFuture<'a> {
        Box::pin(self.warm_impl())
    }
}
