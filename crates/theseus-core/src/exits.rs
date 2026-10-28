//! What leaves the machine carries no vault value (P2, theseus-gyjyo R7b).
//!
//! The record is raw: a tool's output, the owner's own message, a file read
//! are stored as they were. Redaction happens on the copies that leave, and
//! only for a value Theseus itself resolved from the vault: an exact value
//! is *known*, so substituting it has no false positive, and sending
//! Theseus's own key to a model is pointless. A string that merely looks
//! like a key (`sk-...`, a PEM header, `password=`) is a guess, and a guess
//! is never acted on here: it goes through untouched.
//!
//! The exits it covers:
//! - the provider request, in `Substituting`, which wraps every provider
//!   (the turn's calls, compaction, keep-warm, the token counter) and
//!   substitutes the copy it sends, never the caller's request, so the
//!   compiler's digest and the prompt cache see the same request as before;
//! - the Discord courier's messages (`theseus-discord`'s `Lane::write`),
//!   through `Exits::text`;
//! - the Jev state and the push line already pass the scrubber.
//!
//! `[privacy] redact_outbound = false` sends the vault's values as written:
//! the deliberate operator choice. The substitution is then not made, but a
//! `request.redacted` row still names the secrets the request carried, with
//! `substituted: false`, so "what key did we send?" has an answer either way.
//!
//! The substitution is deterministic: the same request and the same values
//! give the same bytes, whatever the order the values resolved in (values are
//! taken longest first, ties by name), so the provider's prompt cache is
//! unaffected.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use zeroize::Zeroizing;

use crate::provider::{
    CountFuture, DeltaSink, Provider, ProviderFuture, ProviderRequest, WarmFuture,
};
use crate::secrets::{SecretBoard, SecretState};
use crate::store::Store;

/// The shortest value substituted: a shorter one would match by chance.
const MIN_VALUE: usize = 8;

/// One value the vault resolved, and its name.
type Known = (Zeroizing<String>, String);

#[cfg(test)]
mod tests;

/// The vault's own values, as the exits know them.
pub struct Exits {
    board: Arc<SecretBoard>,
    /// `[privacy] redact_outbound`.
    on: bool,
}

/// The vault secrets a copy carried: each name, and how many times.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    names: BTreeMap<String, u32>,
}

impl Found {
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
    /// The secrets' names, sorted.
    pub fn names(&self) -> Vec<String> {
        self.names.keys().cloned().collect()
    }
    /// How many values were met, over every name.
    pub fn count(&self) -> u32 {
        self.names.values().sum()
    }
}

impl Exits {
    pub fn new(board: Arc<SecretBoard>, on: bool) -> Arc<Self> {
        Arc::new(Self { board, on })
    }

    /// Whether the vault's values are substituted (`redact_outbound`).
    pub fn on(&self) -> bool {
        self.on
    }

    /// The values the vault has resolved, longest first so a value that holds
    /// another is replaced whole, ties by name so the order never depends on
    /// which resolved first; one name for a value two secrets share.
    fn known(&self) -> Vec<Known> {
        let mut out: Vec<Known> = Vec::new();
        for (name, state) in self.board.states().iter() {
            let SecretState::Ready(v) = state else {
                continue;
            };
            let v = v.expose().trim();
            if v.len() >= MIN_VALUE && !out.iter().any(|(x, _)| x.as_str() == v) {
                out.push((Zeroizing::new(v.to_string()), name.clone()));
            }
        }
        out.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.1.cmp(&b.1)));
        out
    }

    /// `req` with the vault's values substituted, when it carries any and the
    /// switch is on, and the secrets it carried either way. Only the request's
    /// text is read: its system blocks, messages, tool definitions and extra
    /// fields, every string in them, as the model reads them.
    pub fn request(&self, req: &ProviderRequest) -> (Option<ProviderRequest>, Found) {
        let known = self.known();
        let mut found = Found::default();
        if known.is_empty() {
            return (None, found);
        }
        for v in parts(req) {
            scan(v, &mut |s| {
                substitute(&known, s, &mut found);
            });
        }
        if found.is_empty() || !self.on {
            return (None, found);
        }
        let mut out = req.clone();
        let mut sink = Found::default();
        for v in parts_mut(&mut out) {
            replace(v, &mut |s| substitute(&known, s, &mut sink));
        }
        (Some(out), found)
    }

    /// `s` with the vault's values substituted, and the secrets it carried.
    pub fn text<'a>(&self, s: &'a str) -> (Cow<'a, str>, Found) {
        let known = self.known();
        let mut found = Found::default();
        match substitute(&known, s, &mut found) {
            Some(out) if self.on => (Cow::Owned(out), found),
            _ => (Cow::Borrowed(s), found),
        }
    }
}

/// `s` with each known value replaced by `[redacted:<name>]`, noting each in
/// `found`; `None` when it holds none. An exact match, never a shape.
fn substitute(known: &[Known], s: &str, found: &mut Found) -> Option<String> {
    if !known.iter().any(|(v, _)| s.contains(v.as_str())) {
        return None;
    }
    let mut out = s.to_string();
    for (v, name) in known {
        let n = out.matches(v.as_str()).count();
        if n > 0 {
            *found.names.entry(name.clone()).or_default() += n as u32;
            out = out.replace(v.as_str(), &format!("[redacted:{name}]"));
        }
    }
    Some(out)
}

/// The request's text-bearing values.
fn parts(req: &ProviderRequest) -> impl Iterator<Item = &Value> {
    req.system
        .iter()
        .chain(&req.messages)
        .chain(&req.tools)
        .chain(req.extra.values())
}

fn parts_mut(req: &mut ProviderRequest) -> impl Iterator<Item = &mut Value> {
    req.system
        .iter_mut()
        .chain(req.messages.iter_mut())
        .chain(req.tools.iter_mut())
        .chain(req.extra.values_mut())
}

/// Every string of `v`.
fn scan(v: &Value, f: &mut impl FnMut(&str)) {
    match v {
        Value::String(s) => f(s),
        Value::Array(a) => a.iter().for_each(|x| scan(x, f)),
        Value::Object(o) => o.values().for_each(|x| scan(x, f)),
        _ => {}
    }
}

/// Every string of `v`, replaced by what `f` gives for it.
fn replace(v: &mut Value, f: &mut impl FnMut(&str) -> Option<String>) {
    match v {
        Value::String(s) => {
            if let Some(n) = f(s) {
                *s = n;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| replace(x, f)),
        Value::Object(o) => o.values_mut().for_each(|x| replace(x, f)),
        _ => {}
    }
}

tokio::task_local! {
    /// The call being made: the turn's, for the row a substitution writes.
    static CALL: Call;
}

/// Whose request it is, for its `request.redacted` row.
#[derive(Debug, Clone, Default)]
pub struct Call {
    pub session: Option<String>,
    pub turn: Option<String>,
    pub loop_index: Option<u32>,
    pub correlation_id: Option<String>,
}

/// Run `f`, the making of a provider call, for `call`. Read at the call, as
/// `data_gate::for_session`'s session is.
pub fn for_call<R>(call: Call, f: impl FnOnce() -> R) -> R {
    CALL.sync_scope(call, f)
}

/// A provider that sends its inner's requests with the vault's values
/// substituted.
pub struct Substituting {
    inner: Arc<dyn Provider>,
    out: Arc<Exits>,
    store: Option<Store>,
    name: String,
}

impl Substituting {
    pub fn new(
        name: &str,
        inner: Arc<dyn Provider>,
        out: Arc<Exits>,
        store: Option<Store>,
    ) -> Self {
        Self {
            inner,
            out,
            store,
            name: name.to_string(),
        }
    }
}

/// Every provider, behind the substitution.
pub fn wrap(
    providers: BTreeMap<String, Arc<dyn Provider>>,
    out: &Arc<Exits>,
    store: &Store,
) -> BTreeMap<String, Arc<dyn Provider>> {
    providers
        .into_iter()
        .map(|(name, inner)| {
            let p: Arc<dyn Provider> = Arc::new(Substituting::new(
                &name,
                inner,
                out.clone(),
                Some(store.clone()),
            ));
            (name, p)
        })
        .collect()
}

impl Substituting {
    /// The row that says which vault secrets a request carried and whether
    /// they were substituted. Written at the call, where the turn's `Call` is
    /// in scope, and never carrying a value.
    fn note(&self, req: &ProviderRequest, found: &Found, substituted: bool, op: &str) {
        let Some(store) = self.store.as_ref().filter(|_| !found.is_empty()) else {
            return;
        };
        let call = CALL.try_with(Clone::clone).unwrap_or_default();
        let fact = crate::fact::data::RequestRedacted {
            provider: &self.name,
            model: &req.model,
            names: found.names(),
            count: found.count(),
            substituted,
            op,
            loop_index: call.loop_index,
            correlation_id: call.correlation_id.as_deref(),
        };
        let written = crate::fact::row(&fact, call.session.as_deref(), call.turn.as_deref())
            .and_then(|r| theseus_store::blocking(|| store.defer(r)));
        if let Err(e) = written {
            tracing::warn!(error = %e, "request.redacted: the row was not written");
        }
    }
}

impl Provider for Substituting {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let (sent, found) = self.out.request(req);
        self.note(req, &found, sent.is_some(), "stream");
        match sent {
            None => self.inner.stream_message(req, on_delta),
            Some(owned) => {
                let inner = self.inner.clone();
                Box::pin(async move { inner.stream_message(&owned, on_delta).await })
            }
        }
    }
    fn warm(&self) -> WarmFuture<'_> {
        self.inner.warm()
    }
    fn count_tokens<'a>(&'a self, req: &'a ProviderRequest) -> CountFuture<'a> {
        let (sent, found) = self.out.request(req);
        self.note(req, &found, sent.is_some(), "count_tokens");
        match sent {
            None => self.inner.count_tokens(req),
            Some(owned) => {
                let inner = self.inner.clone();
                Box::pin(async move { inner.count_tokens(&owned).await })
            }
        }
    }
}
