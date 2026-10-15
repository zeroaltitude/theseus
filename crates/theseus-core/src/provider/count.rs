//! The provider's token counter (theseus-7m7w.1): `POST /v1/messages/count_tokens` takes the request's model,
//! system, messages, tools and thinking and answers its input tokens. It runs no model and bills nothing, so
//! `theseus census --exact` can count a request it never sends. It accepts the tools (measured 2026-10-06).

use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::{api_error_message, Anthropic, ProviderRequest, API_VERSION};

/// How long a count may take.
const BOUND: Duration = Duration::from_secs(20);

impl Anthropic {
    /// The body `count_tokens` takes from `req`: the fields of a request that make its input.
    pub(super) fn count_body(req: &ProviderRequest) -> Value {
        let mut b = json!({"model": req.model, "messages": req.messages});
        if !req.system.is_empty() {
            b["system"] = Value::Array(req.system.clone());
        }
        if !req.tools.is_empty() {
            b["tools"] = Value::Array(req.tools.clone());
        }
        if let Some(t) = &req.thinking {
            b["thinking"] = t.clone();
        }
        b
    }

    pub(super) async fn count_impl(&self, req: &ProviderRequest) -> Result<u64> {
        let key = self.key()?;
        let mut post = self
            .http
            .post(format!("{}/v1/messages/count_tokens", self.api_base))
            .header("x-api-key", key.expose())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .timeout(BOUND);
        if !req.betas.is_empty() {
            post = post.header("anthropic-beta", req.betas.join(","));
        }
        let resp = post
            .json(&Self::count_body(req))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.without_url()))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!(
                "count_tokens answered {status}: {}",
                api_error_message(&body)
            );
        }
        serde_json::from_str::<Value>(&body)?["input_tokens"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("count_tokens answered without input_tokens"))
    }
}
