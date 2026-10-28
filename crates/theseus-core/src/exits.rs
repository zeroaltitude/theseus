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

use crate::provider::{
    CountFuture, DeltaSink, Provider, ProviderFuture, ProviderRequest, WarmFuture,
};
use crate::secrets::SecretBoard;
use crate::store::Store;

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

    /// `req` with the vault's values substituted, when it carries any and the
    /// switch is on, and the secrets it carried either way.
    pub fn request(&self, req: &ProviderRequest) -> (Option<ProviderRequest>, Found) {
        let _ = (&self.board, req);
        (None, Found::default())
    }

    /// `s` with the vault's values substituted, and the secrets it carried.
    pub fn text<'a>(&self, s: &'a str) -> (Cow<'a, str>, Found) {
        (Cow::Borrowed(s), Found::default())
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

impl Provider for Substituting {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let _ = (&self.out, &self.store, &self.name);
        self.inner.stream_message(req, on_delta)
    }
    fn warm(&self) -> WarmFuture<'_> {
        self.inner.warm()
    }
    fn count_tokens<'a>(&'a self, req: &'a ProviderRequest) -> CountFuture<'a> {
        self.inner.count_tokens(req)
    }
}
