//! The provider chokepoint (theseus-oejh3, theseus-xs5ah, theseus-a1wvq.3,
//! theseus-9ymz0): every provider is wrapped once, when the core is built
//! ([`guard`]), so one place covers every caller (the turn, compaction,
//! keep-warm, replay, the background calls). It refuses, before a byte is sent:
//!
//! - a request to a model of restricted origin (`cn` or `unknown`) that
//!   `[content_based_security] cn_models` does not allow: `never` refuses it;
//!   `follow_flags` (the default) refuses it when the session it is made for
//!   carries a label that disqualifies such a model
//!   (`ContentFlags::disqualifies_cn`); `always` sends it. **That is one read
//!   of the session's record and nothing more**: the transcript is never read,
//!   no item is proved judged, and no mark is looked at (theseus-gyjyo.6).
//!   A request made for no session is refused whatever the mode, and so is
//!   one whose session has no record that reads: those are about the caller,
//!   not the content, and fail closed. Each refusal is a `data_gate.blocked`
//!   row;
//!
//! **Nothing else is read here.** A name in `[privacy] sensitive_names`
//! (theseus-gyjyo.4) and a Google result (theseus-gyjyo.5) each raise a
//! removable label where the item enters (`content::deterministic`), and the
//! label is read as any is; no request is walked for a word, and no marker of
//! Google's is read. So the operator who removes a Google label can send that
//! session to a model of restricted origin: that is deliberate (P3).
//!
//! An unrestricted model's request costs one lookup of its model's origin,
//! memoised per model, and its text is never read. The session's record is
//! read only for a restricted model under `follow_flags`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use crate::catalog::{origin_of, Catalog, ModelOrigin};
use crate::config::content_based_security::CnModels;
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
/// at the call.
pub fn for_session<R>(session: &str, f: impl FnOnce() -> R) -> R {
    CALLER.sync_scope(session.to_string(), f)
}

/// `provider`, whose every call is made for `session`: for the steps that hold
/// the provider and call it later, or on another task, where a
/// [`for_session`] around the call's creation would not reach. The session is
/// set as each call is made, inside this wrapper.
pub fn bound(provider: Arc<dyn Provider>, session: &str) -> Arc<dyn Provider> {
    Arc::new(Bound {
        inner: provider,
        session: session.to_string(),
    })
}

struct Bound {
    inner: Arc<dyn Provider>,
    session: String,
}

impl Provider for Bound {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        for_session(&self.session, || self.inner.stream_message(req, on_delta))
    }
    fn warm(&self) -> WarmFuture<'_> {
        self.inner.warm()
    }
    fn count_tokens<'a>(&'a self, req: &'a ProviderRequest) -> CountFuture<'a> {
        for_session(&self.session, || self.inner.count_tokens(req))
    }
}

/// Where a refusal is counted on `theseus.route.data_gate`: the core's
/// telemetry, set once it exists (`Core::build`, then `install_telemetry`).
pub type Counter = Arc<std::sync::OnceLock<crate::telemetry::Telemetry>>;

/// A provider and the origin its models are served from.
struct Gated {
    inner: Arc<dyn Provider>,
    /// Its name in the config, which a refusal's row names.
    name: String,
    catalog: Arc<Catalog>,
    api_base: String,
    vouched: Option<ModelOrigin>,
    /// Each model's origin, worked out at its first request.
    origins: RwLock<HashMap<String, ModelOrigin>>,
    /// Where a refusal's row is written (none in a unit test).
    store: Option<Store>,
    counter: Counter,
    /// `[content_based_security] cn_models`: what a request to a model of
    /// restricted origin is sent under.
    cn_models: CnModels,
}

impl Gated {
    fn new(
        name: &str,
        inner: Arc<dyn Provider>,
        catalog: Arc<Catalog>,
        (api_base, vouched): (String, Option<ModelOrigin>),
        store: Option<Store>,
        counter: Counter,
        cn_models: CnModels,
    ) -> Self {
        Self {
            inner,
            name: name.into(),
            catalog,
            api_base,
            vouched,
            origins: RwLock::default(),
            store,
            counter,
            cn_models,
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

    /// The refusal, when the request may not go to this model.
    fn refuse(&self, req: &ProviderRequest) -> Option<anyhow::Error> {
        let origin = self.origin(&req.model);
        let (why, said) = self.cn_refusal(origin)?;
        self.blocked(&req.model, origin, why);
        self.counted(why);
        let message = format!(
            "{} is of restricted origin ({}) and {said}; nothing was sent",
            req.model,
            word(origin)
        );
        Some(ProviderError::DataGate { message }.into())
    }

    /// Why a request to a model of `origin` is refused: the row's word, and
    /// the words for the refusal. None for a model that is not restricted, for
    /// `always`, and under `follow_flags` for a session with no label that
    /// keeps it off one. One read of the session's record, and nothing else:
    /// no transcript, no proof that an item was judged. Fails closed on what
    /// is about the caller: a call made for no session, and a session with no
    /// record that reads.
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
        match self.cn_models {
            CnModels::Always => return None,
            CnModels::Never => {
                return Some((
                    "never",
                    "`[content_based_security] cn_models` is `never`".into(),
                ))
            }
            CnModels::FollowFlags => {}
        }
        let rec = self
            .store
            .as_ref()
            .map(|store| theseus_store::blocking(|| store.get_session::<SessionRecord>(&session)));
        let Some(Ok(Some(rec))) = rec else {
            return Some((
                "no_record",
                format!("its session {session} has no record that reads"),
            ));
        };
        let flag = rec
            .data
            .as_deref()
            .and_then(ContentFlags::disqualifies_cn)?;
        Some((
            flag.as_str(),
            format!(
                "its session carries a `{}` label: {}",
                flag.as_str(),
                flag.means()
            ),
        ))
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

/// The providers, each behind the floor, always: a restricted model is refused
/// whatever the config names. A refusal's row is written to `store`.
pub fn guard(
    providers: BTreeMap<String, Arc<dyn Provider>>,
    cfg: &Config,
    catalog: &Arc<Catalog>,
    store: &Store,
    counter: &Counter,
) -> BTreeMap<String, Arc<dyn Provider>> {
    providers
        .into_iter()
        .map(|(name, inner)| {
            let conf = cfg.all_providers().get(&name);
            let base = conf.map(|p| p.api_base.clone()).unwrap_or_default();
            let gated: Arc<dyn Provider> = Arc::new(Gated::new(
                &name,
                inner,
                catalog.clone(),
                (base, conf.and_then(|p| p.origin)),
                Some(store.clone()),
                counter.clone(),
                cfg.content_based_security.cn_models,
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
    fn gated(api_base: &str) -> (Gated, Arc<FakeProvider>, String, tempfile::TempDir) {
        let dir = crate::scratch::tempdir();
        let store = Store::open(dir.path()).unwrap();
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        store.put_session(&rec.session_id, &rec).unwrap();
        let fake = Arc::new(FakeProvider::default());
        let g = Gated::new(
            "fake",
            fake.clone(),
            Arc::new(Catalog::builtin()),
            (api_base.into(), None),
            Some(store),
            Counter::default(),
            CnModels::default(),
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

    /// The session carries a `confidential` label, as a names hit leaves it.
    fn label_it(g: &Gated, sid: &str) {
        use crate::content::{deterministic, ItemKind};
        let label = deterministic::label(
            crate::content::Flag::Confidential,
            "nod_1",
            ItemKind::ToolResult,
            "name: Examplecorp".into(),
            1,
        );
        g.store
            .as_ref()
            .unwrap()
            .update_session(sid, |rec| {
                let mut f = crate::content::ContentFlags::default();
                f.raise(label);
                rec.data = Some(Box::new(f));
                Ok(vec![])
            })
            .unwrap();
    }

    /// The chokepoint reads no request for a name (theseus-gyjyo.4): a
    /// restricted model is sent a request that names one when its session
    /// carries no label, and is refused any request when it does, so the
    /// label is what refuses. A US model is sent it either way.
    #[tokio::test]
    async fn a_restricted_model_is_refused_by_the_label_and_not_the_name() {
        let named = request("deepseek-v4", "Draft the Examplecorp's budget\nmemo.");
        let plain = request("deepseek-v4", "Draft a budget memo.");
        let (cn, sent, sid, _dir) = gated("https://api.deepseek.com/anthropic");
        assert_eq!(
            send(&cn, &sid, &named).await,
            Ok(()),
            "no label, no refusal"
        );
        assert_eq!(sent.requests().len(), 1);
        label_it(&cn, &sid);
        assert_eq!(send(&cn, &sid, &plain).await, Err("data_gate".into()));
        assert_eq!(sent.requests().len(), 1, "nothing more was sent");
        let (us, sent, sid, _dir) = gated("https://api.anthropic.com");
        label_it(&us, &sid);
        let named = request("claude-sonnet-5-5", "Draft the Examplecorp's budget memo.");
        assert_eq!(send(&us, &sid, &named).await, Ok(()));
        assert_eq!(sent.requests().len(), 1, "an unrestricted model is sent it");
    }

    /// Kimi is gated exactly as GLM is (theseus-rgbtz): by its host and by its
    /// name, a restricted model is sent a clean session's request and refused
    /// a labelled session's, and a Claude id asked of the Moonshot host is
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
            let plain = request(model, "Draft a budget memo.");
            let (mut g, sent, sid, _dir) = gated(base);
            g.vouched = Some(ModelOrigin::Us);
            assert_eq!(send(&g, &sid, &plain).await, Ok(()), "{model}: clean work");
            assert_eq!(sent.requests().len(), 1, "{model}");
            label_it(&g, &sid);
            assert_eq!(
                send(&g, &sid, &plain).await,
                Err("data_gate".into()),
                "{model}: a label"
            );
            assert_eq!(sent.requests().len(), 1, "{model}: nothing more was sent");
        }
    }

    /// A model that is not of restricted origin is sent a request that names
    /// one of the operator's names, whether its host is vouched `eu` or `us`
    /// for a family the catalog does not know: the US allowlist is not read for
    /// a name (theseus-gyjyo.4).
    #[tokio::test]
    async fn a_model_not_restricted_is_sent_a_named_request() {
        let named = |m: &str| request(m, "Draft the Examplecorp's budget memo.");
        let (mut eu, sent, sid, _dir) = gated("https://api.example.eu/anthropic");
        eu.vouched = Some(ModelOrigin::Eu);
        assert!(!ModelOrigin::Eu.restricted());
        assert_eq!(send(&eu, &sid, &named("claude-sonnet-5-5")).await, Ok(()));
        assert_eq!(sent.requests().len(), 1);
        let (mut proxy, sent, sid, _dir) = gated("https://llm-proxy.example.com/v1");
        proxy.vouched = Some(ModelOrigin::Us);
        assert_eq!(send(&proxy, &sid, &named("qwen-4-max")).await, Ok(()));
        assert_eq!(sent.requests().len(), 1);
    }

    /// A provider bound to a session is made for that session wherever it is
    /// called (theseus-gyjyo.5): the background steps that hold a provider and
    /// call it later, or on another task, read the session they were bound to
    /// and not the caller's. A bound call of a labelled session to a restricted
    /// model is refused, a clean one is sent, and the binding wins over an
    /// ambient session in both directions, so no call is held by the wrong
    /// session's labels (a leak) or lets the wrong one through.
    #[tokio::test]
    async fn a_bound_provider_is_made_for_its_session_wherever_it_is_called() {
        let (cn, sent, plain, _dir) = gated("https://api.z.ai/api/anthropic");
        let labelled = SessionRecord::new(SessionKind::Conversation, None);
        cn.store
            .as_ref()
            .unwrap()
            .put_session(&labelled.session_id, &labelled)
            .unwrap();
        let labelled = labelled.session_id;
        label_it(&cn, &labelled);
        let cn: Arc<dyn Provider> = Arc::new(cn);
        let req = request("glm-5.3", "Draft a budget memo.");
        let mut sink = |_: crate::provider::Delta<'_>| {};
        let for_plain = bound(cn.clone(), &plain);
        let for_labelled = bound(cn.clone(), &labelled);
        // No ambient session: each is held by its own.
        for_plain.stream_message(&req, &mut sink).await.unwrap();
        assert_eq!(sent.requests().len(), 1, "a clean session's call is sent");
        let said = for_labelled
            .stream_message(&req, &mut sink)
            .await
            .unwrap_err();
        assert!(
            format!("{said:#}").contains("`confidential` label"),
            "{said:#}"
        );
        assert_eq!(sent.requests().len(), 1, "a labelled session's is not");
        // An ambient session does not change whose call it is.
        for_session(&labelled, || for_plain.stream_message(&req, &mut sink))
            .await
            .unwrap();
        assert_eq!(sent.requests().len(), 2, "bound to the clean session");
        for_session(&plain, || for_labelled.stream_message(&req, &mut sink))
            .await
            .unwrap_err();
        assert_eq!(sent.requests().len(), 2, "bound to the labelled one");
        // Unbound, the same provider is still refused for want of a session.
        cn.stream_message(&req, &mut sink).await.unwrap_err();
        assert_eq!(sent.requests().len(), 2);
    }

    /// A call for no session, to a restricted model, is refused with nothing
    /// sent, whatever the request holds; a Claude model is sent it (the
    /// session is read only for a restricted one).
    #[tokio::test]
    async fn a_call_for_no_session_is_refused_a_restricted_model_alone() {
        let (cn, sent, _sid, _dir) = gated("https://api.z.ai/api/anthropic");
        let plain = request("glm-5.3", "Draft a budget memo.");
        let mut sink = |_: crate::provider::Delta<'_>| {};
        let said = cn.stream_message(&plain, &mut sink).await.unwrap_err();
        assert!(
            format!("{said:#}").contains("not made for a session"),
            "{said:#}"
        );
        assert!(sent.requests().is_empty());
        let (us, sent, _sid, _dir) = gated("https://api.anthropic.com");
        let ok = request("claude-sonnet-5-5", "Draft a budget memo.");
        us.stream_message(&ok, &mut sink).await.unwrap();
        assert_eq!(sent.requests().len(), 1);
    }
}
