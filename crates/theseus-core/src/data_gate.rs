//! The floor under the sensitive names (theseus-oejh3, theseus-xs5ah): a
//! request that holds one of `[privacy] sensitive_names` goes only to a model
//! of US origin (`ModelOrigin::cleared_for_sensitive`, an allowlist), so one
//! to a model of any other origin, `cn`, `eu` or `unknown`, is refused before
//! a byte is sent.
//!
//! One place covers every caller (the turn, compaction, keep-warm, replay, the
//! background calls): each provider is wrapped once, when the core is built
//! ([`guard`]). With the list empty nothing is wrapped, so a plain turn does
//! no work here; a request to a cleared model is passed on without a look at
//! its text. The matcher is `theseus_judge::names`; the walk is over each JSON
//! string value of the system blocks, the messages and the tool definitions,
//! never the serialized body, whose escapes (`\n`) would glue words together.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use theseus_judge::names::Names;

use crate::catalog::{origin_of, Catalog, ModelOrigin};
use crate::config::Config;
use crate::provider::{
    CountFuture, DeltaSink, Provider, ProviderError, ProviderFuture, ProviderRequest, WarmFuture,
};

/// The class of a request the floor refused.
pub const DATA_GATE_CLASS: &str = "data_gate";

/// A provider and the origin its models are served from.
struct Gated {
    inner: Arc<dyn Provider>,
    names: Names,
    catalog: Arc<Catalog>,
    api_base: String,
    vouched: Option<ModelOrigin>,
}

impl Gated {
    fn cleared(&self, model: &str) -> bool {
        origin_of(&self.api_base, self.vouched, model, self.catalog.get(model))
            .cleared_for_sensitive()
    }

    /// The refusal, when the request may not go to this model.
    fn refuse(&self, req: &ProviderRequest) -> Option<anyhow::Error> {
        if self.cleared(&req.model) || !names_in(req, &self.names) {
            return None;
        }
        let message = "this request names something in `[privacy] sensitive_names`, and its \
                       model is not of US origin; nothing was sent";
        Some(
            ProviderError::DataGate {
                message: message.into(),
            }
            .into(),
        )
    }
}

impl Provider for Gated {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        match self.refuse(req) {
            None => self.inner.stream_message(req, on_delta),
            Some(e) => Box::pin(async move { Err(e) }),
        }
    }
    fn warm(&self) -> WarmFuture<'_> {
        self.inner.warm()
    }
    fn count_tokens<'a>(&'a self, req: &'a ProviderRequest) -> CountFuture<'a> {
        match self.refuse(req) {
            None => self.inner.count_tokens(req),
            Some(e) => Box::pin(async move { Err(e) }),
        }
    }
}

/// Every string value of `v` that holds a name.
fn value_names(v: &Value, names: &Names) -> bool {
    match v {
        Value::String(s) => !names.hits(s).is_empty(),
        Value::Array(a) => a.iter().any(|x| value_names(x, names)),
        Value::Object(o) => o.values().any(|x| value_names(x, names)),
        _ => false,
    }
}

/// The request's system blocks, messages and tool definitions hold a name.
pub fn names_in(req: &ProviderRequest, names: &Names) -> bool {
    req.system
        .iter()
        .chain(&req.messages)
        .chain(&req.tools)
        .any(|v| value_names(v, names))
}

/// The providers, each behind the floor; the same map when the list is empty.
pub fn guard(
    providers: BTreeMap<String, Arc<dyn Provider>>,
    cfg: &Config,
    catalog: &Arc<Catalog>,
) -> BTreeMap<String, Arc<dyn Provider>> {
    let names = cfg.privacy.names();
    if names.is_empty() {
        return providers;
    }
    providers
        .into_iter()
        .map(|(name, inner)| {
            let conf = cfg.all_providers().get(&name);
            let gated: Arc<dyn Provider> = Arc::new(Gated {
                inner,
                names: names.clone(),
                catalog: catalog.clone(),
                api_base: conf.map(|p| p.api_base.clone()).unwrap_or_default(),
                vouched: conf.and_then(|p| p.origin),
            });
            (name, gated)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FakeProvider;
    use serde_json::json;

    fn gated(api_base: &str, names: &[&str]) -> (Gated, Arc<FakeProvider>) {
        let fake = Arc::new(FakeProvider::default());
        let names: Vec<String> = names.iter().map(|n| (*n).to_string()).collect();
        let g = Gated {
            inner: fake.clone(),
            names: Names::new(&names),
            catalog: Arc::new(Catalog::builtin()),
            api_base: api_base.into(),
            vouched: None,
        };
        (g, fake)
    }

    fn request(model: &str, text: &str) -> ProviderRequest {
        ProviderRequest {
            model: model.into(),
            max_tokens: 16,
            system: vec![json!({"type": "text", "text": "You are a helper."})],
            messages: vec![
                json!({"role": "user", "content": [{"type": "text", "text": "Hello."}]}),
                json!({"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"note": text}}]}),
            ],
            tools: vec![],
            thinking: None,
            output_config: None,
            cache_control: None,
            betas: vec![],
            extra: Default::default(),
            image_tokens: 0,
        }
    }

    async fn send(g: &Gated, req: &ProviderRequest) -> Result<(), String> {
        let mut sink = |_: crate::provider::Delta<'_>| {};
        match g.stream_message(req, &mut sink).await {
            Ok(_) => Ok(()),
            Err(e) => Err(match e.downcast_ref::<ProviderError>() {
                Some(p) => p.class().to_string(),
                None => format!("{e:#}"),
            }),
        }
    }

    /// A request that names a configured name is refused for a restricted
    /// model with nothing sent (the fake records every send), and goes to an
    /// unrestricted one; a request that names nothing goes to both.
    #[tokio::test]
    async fn a_restricted_model_is_never_sent_a_sensitive_name() {
        let named = request("deepseek-v4", "Draft the Examplecorp's budget\nmemo.");
        let plain = request("deepseek-v4", "Draft a budget memo.");
        let (cn, sent) = gated("https://api.deepseek.com/anthropic", &["Examplecorp"]);
        assert_eq!(send(&cn, &named).await, Err("data_gate".into()));
        assert!(sent.requests().is_empty(), "nothing was sent");
        assert_eq!(send(&cn, &plain).await, Ok(()));
        assert_eq!(sent.requests().len(), 1);
        let (us, sent) = gated("https://api.anthropic.com", &["Examplecorp"]);
        let named = request("claude-sonnet-5-5", "Draft the Examplecorp's budget memo.");
        assert_eq!(send(&us, &named).await, Ok(()));
        assert_eq!(sent.requests().len(), 1, "an unrestricted model is sent it");
    }

    /// A model of EU origin is not on the allowlist (theseus-xs5ah): a
    /// provider the operator vouches `eu` for is refused a named request with
    /// nothing sent, though the China-origin block alone would pass it; a
    /// request that names nothing still goes.
    #[tokio::test]
    async fn a_model_of_eu_origin_is_never_sent_a_sensitive_name() {
        let (mut eu, sent) = gated("https://api.example.eu/anthropic", &["Examplecorp"]);
        eu.vouched = Some(ModelOrigin::Eu);
        assert!(!ModelOrigin::Eu.restricted(), "not blocked as China origin");
        let named = request("claude-sonnet-5-5", "Draft the Examplecorp's budget memo.");
        assert_eq!(send(&eu, &named).await, Err("data_gate".into()));
        assert!(sent.requests().is_empty(), "nothing was sent");
        let plain = request("claude-sonnet-5-5", "Draft a budget memo.");
        assert_eq!(send(&eu, &plain).await, Ok(()));
        assert_eq!(sent.requests().len(), 1);
    }

    /// A name in a tool result, a tool definition or the system is found, and a
    /// name is found across an escaped line break, which the unescaped string
    /// holds as a break and the serialized body as `\n`.
    #[test]
    fn every_string_of_the_request_is_walked() {
        let names = Names::new(&["Big Co".to_string()]);
        let mut r = request("m", "nothing");
        assert!(!names_in(&r, &names));
        r.system = vec![json!({"type": "text", "text": "for big\nco only"})];
        assert!(names_in(&r, &names));
        r.system = vec![];
        r.tools = vec![json!({"name": "x", "description": "reads Big Co files"})];
        assert!(names_in(&r, &names));
        r.tools = vec![];
        r.messages = vec![json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t", "content": [{"type": "text", "text": "BIG  CO"}]}]})];
        assert!(names_in(&r, &names));
    }
}
