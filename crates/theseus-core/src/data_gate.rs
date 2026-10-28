//! The provider chokepoint (theseus-oejh3, theseus-xs5ah, theseus-a1wvq.3,
//! theseus-9ymz0): every provider is wrapped once, when the core is built
//! ([`guard`]), so one place covers every caller (the turn, compaction,
//! keep-warm, replay, the background calls). It refuses, before a byte is sent:
//!
//! - a request to a model of restricted origin (`cn` or `unknown`) unless the
//!   session it is made for has no label that disqualifies such a model
//!   (`ContentFlags::disqualifies_cn`) and every item in it has been judged
//!   (`content_wait::unjudged`). A request made for no session is refused: fail
//!   closed, since nothing says what it carries. Nothing is proved per request;
//!   the session either carries a label or it does not. Each refusal is a
//!   `data_gate.blocked` row;
//! - a request that holds one of `[privacy] sensitive_names`, or is made for a
//!   session that read private content, to a model not of US origin
//!   (`ModelOrigin::cleared_for_sensitive`, an allowlist). No label lifts
//!   this, and no label is needed for it: it is the deterministic backstop
//!   that does not depend on a classifier being right.
//!
//! An unrestricted model's request costs one lookup of its model's origin,
//! memoised per model; a US model's is passed on without a look at its text,
//! and with no names configured no text is read at all. The session's record
//! is read only for a restricted model. The matcher is `theseus_judge::names`;
//! the walk is over each JSON string value of the system blocks, the messages
//! and the tool definitions, never the serialized body, whose escapes (`\n`)
//! would glue words together.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use serde_json::Value;
use theseus_judge::names::Names;

use crate::catalog::{cleared_for_sensitive, origin_of, Catalog, ModelOrigin};
use crate::config::Config;
use crate::content::ContentFlags;
use crate::provider::{
    CountFuture, DeltaSink, Provider, ProviderError, ProviderFuture, ProviderRequest, WarmFuture,
};
use crate::session::SessionRecord;
use crate::store::Store;

/// The class of a request the floor refused.
pub const DATA_GATE_CLASS: &str = "data_gate";

tokio::task_local! {
    /// The session a provider call is made for, which a refusal's row names.
    static CALLER: String;
}

/// Run `f`, the making of a provider call, for `session`. The floor reads it
/// at the call, as `google::private::scoped`'s mark is read.
pub fn for_session<R>(session: &str, f: impl FnOnce() -> R) -> R {
    CALLER.sync_scope(session.to_string(), f)
}

/// Where a refusal is counted on `theseus.route.data_gate`: the core's
/// telemetry, set once it exists (`Core::build`, then `install_telemetry`).
pub type Counter = Arc<std::sync::OnceLock<crate::telemetry::Telemetry>>;

/// A provider and the origin its models are served from.
struct Gated {
    inner: Arc<dyn Provider>,
    /// Its name in the config, which a refusal's row names.
    name: String,
    names: Names,
    catalog: Arc<Catalog>,
    api_base: String,
    vouched: Option<ModelOrigin>,
    /// Each model's origin, worked out at its first request.
    origins: RwLock<HashMap<String, ModelOrigin>>,
    /// Where a refusal's row is written (none in a unit test).
    store: Option<Store>,
    counter: Counter,
}

impl Gated {
    fn new(
        name: &str,
        inner: Arc<dyn Provider>,
        names: Names,
        catalog: Arc<Catalog>,
        (api_base, vouched): (String, Option<ModelOrigin>),
        store: Option<Store>,
        counter: Counter,
    ) -> Self {
        Self {
            inner,
            name: name.into(),
            names,
            catalog,
            api_base,
            vouched,
            origins: RwLock::default(),
            store,
            counter,
        }
    }

    fn origin(&self, model: &str) -> ModelOrigin {
        let known = self.origins.read().ok().and_then(|o| o.get(model).copied());
        known.unwrap_or_else(|| {
            let o = origin_of(&self.api_base, self.vouched, model, self.catalog.get(model));
            if let Ok(mut m) = self.origins.write() {
                m.insert(model.into(), o);
            }
            o
        })
    }

    fn cleared(&self, model: &str) -> bool {
        cleared_for_sensitive(&self.api_base, self.vouched, model, self.catalog.get(model))
    }

    /// The refusal, when the request may not go to this model.
    fn refuse(&self, req: &ProviderRequest) -> Option<anyhow::Error> {
        let origin = self.origin(&req.model);
        let message = match self.cn_refusal(origin) {
            Some((why, said)) => {
                self.blocked(&req.model, origin, why);
                self.counted(why);
                format!(
                    "{} is of restricted origin ({}) and {said}; nothing was sent",
                    req.model,
                    word(origin)
                )
            }
            None if self.cleared(&req.model)
                || !(crate::google::private::in_scope()
                    || (!self.names.is_empty() && names_in(req, &self.names))) =>
            {
                return None;
            }
            None => {
                self.counted("sensitive");
                let level = crate::google::private::level_in_scope();
                let held = if level == crate::google::private::Level::Sensitive {
                    "its session read a sensitive calendar"
                } else {
                    "its session read private content"
                };
                format!(
                    "this request names something in `[privacy] sensitive_names`, or {held}, \
                     and its model is not of US origin; nothing was sent"
                )
            }
        };
        Some(ProviderError::DataGate { message }.into())
    }

    /// Why a request to a model of `origin` is refused for what its session
    /// holds: the row's word, and the words for the refusal. None for a model
    /// that is not restricted, and for a session with no label that keeps it
    /// off one, every item of which was judged. Fails closed: a call made for
    /// no session, a session with no record, or a record or transcript that
    /// does not read is refused.
    fn cn_refusal(&self, origin: ModelOrigin) -> Option<(&'static str, String)> {
        if !origin.restricted() {
            return None;
        }
        let Some(session) = CALLER.try_with(Clone::clone).ok() else {
            return Some((
                "no_session",
                "this request is not made for a session, so nothing says what it carries".into(),
            ));
        };
        let read = self.store.as_ref().map(|store| {
            theseus_store::blocking(|| {
                let rec = store.get_session::<SessionRecord>(&session)?;
                let nodes = store.transcript(&session)?;
                Ok::<_, anyhow::Error>((rec, nodes))
            })
        });
        let (rec, nodes) = match read {
            Some(Ok((Some(rec), nodes))) => (rec, nodes),
            _ => {
                return Some((
                    "no_record",
                    format!("its session {session} has no record that reads"),
                ))
            }
        };
        let flags = rec.data.as_deref();
        if let Some(flag) = flags.and_then(ContentFlags::disqualifies_cn) {
            return Some((
                flag.as_str(),
                format!(
                    "its session carries a `{}` label: {}",
                    flag.as_str(),
                    flag.means()
                ),
            ));
        }
        crate::content_wait::unjudged(&nodes, flags).then(|| {
            (
                "unjudged",
                "its session holds an item that has not been judged yet".into(),
            )
        })
    }

    /// A refusal, counted by its class word (theseus-4h85l).
    fn counted(&self, why: &str) {
        if let Some(t) = self.counter.get() {
            t.record_data_gate("chokepoint", why);
        }
    }

    /// The refusal's `data_gate.blocked` row, with the session the call is
    /// made for, when it is a session's.
    fn blocked(&self, model: &str, origin: ModelOrigin, why: &str) {
        let Some(store) = &self.store else { return };
        let session = CALLER.try_with(Clone::clone).ok();
        let fact = crate::fact::data::DataGateBlocked {
            provider: &self.name,
            model,
            origin: word(origin),
            why,
        };
        let written = crate::fact::row(&fact, session.as_deref(), None)
            .and_then(|r| theseus_store::blocking(|| store.defer(r)));
        if let Err(e) = written {
            tracing::warn!(error = %e, "data_gate.blocked: the row was not written");
        }
    }
}

/// An origin's word, as the config and the rows write it.
pub(crate) fn word(o: ModelOrigin) -> &'static str {
    match o {
        ModelOrigin::Us => "us",
        ModelOrigin::Eu => "eu",
        ModelOrigin::Cn => "cn",
        ModelOrigin::Unknown => "unknown",
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

/// The providers, each behind the floor, always: a restricted model is refused
/// whatever the config names. A refusal's row is written to `store`.
pub fn guard(
    providers: BTreeMap<String, Arc<dyn Provider>>,
    cfg: &Config,
    catalog: &Arc<Catalog>,
    store: &Store,
    counter: &Counter,
) -> BTreeMap<String, Arc<dyn Provider>> {
    let names = cfg.privacy.names();
    providers
        .into_iter()
        .map(|(name, inner)| {
            let conf = cfg.all_providers().get(&name);
            let base = conf.map(|p| p.api_base.clone()).unwrap_or_default();
            let gated: Arc<dyn Provider> = Arc::new(Gated::new(
                &name,
                inner,
                names.clone(),
                catalog.clone(),
                (base, conf.and_then(|p| p.origin)),
                Some(store.clone()),
                counter.clone(),
            ));
            (name, gated)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FakeProvider;
    use serde_json::json;
    use theseus_protocol::SessionKind;

    /// A gated provider over a real store holding one clean session, the
    /// fake behind it, the session's id, and the store's directory.
    fn gated(
        api_base: &str,
        names: &[&str],
    ) -> (Gated, Arc<FakeProvider>, String, tempfile::TempDir) {
        let dir = crate::scratch::tempdir();
        let store = Store::open(dir.path()).unwrap();
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        store.put_session(&rec.session_id, &rec).unwrap();
        let fake = Arc::new(FakeProvider::default());
        let names: Vec<String> = names.iter().map(|n| (*n).to_string()).collect();
        let g = Gated::new(
            "fake",
            fake.clone(),
            Names::new(&names),
            Arc::new(Catalog::builtin()),
            (api_base.into(), None),
            Some(store),
            Counter::default(),
        );
        (g, fake, rec.session_id, dir)
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

    /// Send `req` as a call made for `session`; the refusal's class if any.
    async fn send(g: &Gated, session: &str, req: &ProviderRequest) -> Result<(), String> {
        let mut sink = |_: crate::provider::Delta<'_>| {};
        let made = for_session(session, || g.stream_message(req, &mut sink));
        match made.await {
            Ok(_) => Ok(()),
            Err(e) => Err(match e.downcast_ref::<ProviderError>() {
                Some(p) => p.class().to_string(),
                None => format!("{e:#}"),
            }),
        }
    }

    /// A restricted model is sent a clean session's request, and never a
    /// sensitive name, with no label to say so: the tripwire is independent of
    /// the labels. A US model is sent a named request.
    #[tokio::test]
    async fn a_restricted_model_is_never_sent_a_sensitive_name() {
        let named = request("deepseek-v4", "Draft the Examplecorp's budget\nmemo.");
        let plain = request("deepseek-v4", "Draft a budget memo.");
        let (cn, sent, sid, _dir) = gated("https://api.deepseek.com/anthropic", &["Examplecorp"]);
        assert_eq!(send(&cn, &sid, &named).await, Err("data_gate".into()));
        assert!(sent.requests().is_empty(), "nothing was sent");
        assert_eq!(
            send(&cn, &sid, &plain).await,
            Ok(()),
            "a clean session goes"
        );
        assert_eq!(sent.requests().len(), 1);
        let (us, sent, sid, _dir) = gated("https://api.anthropic.com", &["Examplecorp"]);
        let named = request("claude-sonnet-5-5", "Draft the Examplecorp's budget memo.");
        assert_eq!(send(&us, &sid, &named).await, Ok(()));
        assert_eq!(sent.requests().len(), 1, "an unrestricted model is sent it");
    }

    /// Kimi is gated exactly as GLM is (theseus-rgbtz): by its host and by its
    /// name, a restricted model is sent a clean session's plain request and
    /// never a sensitive name, and a Claude id asked of the Moonshot host is
    /// gated the same way.
    #[tokio::test]
    async fn kimi_is_gated_exactly_as_glm_is() {
        let cases = [
            ("https://api.z.ai/api/anthropic", "glm-5.3"),
            ("https://api.moonshot.ai/anthropic", "kimi-k3.1-pro"),
            (
                "https://api.moonshot.ai/anthropic",
                "kimi-k3.1-pro-highspeed",
            ),
            ("https://api.moonshot.ai/anthropic", "claude-sonnet-5-5"),
            // The name alone, behind a host that vouches `us`.
            ("https://proxy.example.com", "kimi-k3.1-pro"),
            ("https://proxy.example.com", "acme/kimi-k4"),
        ];
        for (base, model) in cases {
            let named = request(model, "Draft the Examplecorp's budget\nmemo.");
            let plain = request(model, "Draft a budget memo.");
            let (mut g, sent, sid, _dir) = gated(base, &["Examplecorp"]);
            g.vouched = Some(ModelOrigin::Us);
            assert_eq!(
                send(&g, &sid, &named).await,
                Err("data_gate".into()),
                "{model}: a name"
            );
            assert!(sent.requests().is_empty(), "{model}: nothing was sent");
            assert_eq!(send(&g, &sid, &plain).await, Ok(()), "{model}: clean work");
            assert_eq!(sent.requests().len(), 1, "{model}");
        }
    }

    /// A model of EU origin is not on the allowlist (theseus-xs5ah): a
    /// provider the operator vouches `eu` for is refused a named request with
    /// nothing sent, though the China-origin block alone would pass it; a
    /// request that names nothing still goes.
    #[tokio::test]
    async fn a_model_of_eu_origin_is_never_sent_a_sensitive_name() {
        let (mut eu, sent, sid, _dir) = gated("https://api.example.eu/anthropic", &["Examplecorp"]);
        eu.vouched = Some(ModelOrigin::Eu);
        assert!(!ModelOrigin::Eu.restricted(), "not blocked as China origin");
        let named = request("claude-sonnet-5-5", "Draft the Examplecorp's budget memo.");
        assert_eq!(send(&eu, &sid, &named).await, Err("data_gate".into()));
        assert!(sent.requests().is_empty(), "nothing was sent");
        let plain = request("claude-sonnet-5-5", "Draft a budget memo.");
        assert_eq!(send(&eu, &sid, &plain).await, Ok(()));
        assert_eq!(sent.requests().len(), 1);
    }

    /// A host the operator vouches `us` for clears only a model the catalog
    /// lists as of US origin (theseus-xs5ah): a family it does not know, served
    /// by the same proxy, is refused a named request with nothing sent.
    #[tokio::test]
    async fn a_us_vouch_clears_no_model_the_catalog_does_not_list_as_us() {
        let (mut proxy, sent, sid, _dir) =
            gated("https://llm-proxy.example.com/v1", &["Examplecorp"]);
        proxy.vouched = Some(ModelOrigin::Us);
        let named = |m: &str| request(m, "Draft the Examplecorp's budget memo.");
        assert_eq!(
            send(&proxy, &sid, &named("qwen-4-max")).await,
            Err("data_gate".into())
        );
        assert!(sent.requests().is_empty(), "nothing was sent");
        assert_eq!(
            send(&proxy, &sid, &named("claude-sonnet-5-5")).await,
            Ok(())
        );
        assert_eq!(sent.requests().len(), 1);
    }

    /// A call for no session, to a restricted model, is refused with nothing
    /// sent, whatever the request holds; a Claude model is sent it (the
    /// session is read only for a restricted one).
    #[tokio::test]
    async fn a_call_for_no_session_is_refused_a_restricted_model_alone() {
        let (cn, sent, _sid, _dir) = gated("https://api.z.ai/api/anthropic", &[]);
        let plain = request("glm-5.3", "Draft a budget memo.");
        let mut sink = |_: crate::provider::Delta<'_>| {};
        let said = cn.stream_message(&plain, &mut sink).await.unwrap_err();
        assert!(
            format!("{said:#}").contains("not made for a session"),
            "{said:#}"
        );
        assert!(sent.requests().is_empty());
        let (us, sent, _sid, _dir) = gated("https://api.anthropic.com", &[]);
        let ok = request("claude-sonnet-5-5", "Draft a budget memo.");
        us.stream_message(&ok, &mut sink).await.unwrap();
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
