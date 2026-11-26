//! Where a sign-in's values enter (issue 55, round 5), and the one check at
//! each: a value that equals anything Theseus already shows of its servers
//! is not kept. The doors: a sign-in's redemption (`sign_in.rs`), every
//! renewal and the kept file read at start (`oauth.rs`), and a
//! registration's client secret (`sign_in.rs`). What is shown: every
//! configured server's name, URL and host; every tool of every list the
//! board holds or the store keeps, read as `offered.rs` reads one; and the
//! sign-in's own issuer, endpoints and client id. The daemon's scrubber
//! reads them with the new values alone as its values, so its encodings
//! count. A list accepted is filtered against the values known when it came
//! (`offered.rs`); a value that would make an accepted string a secret is
//! refused here. So no list, label or name ever holds a kept value.

use std::sync::Arc;

use super::{offered, McpBoard};
use crate::scrub::Scrubber;

/// What a sign-in, or a registration, says when it returned a value
/// Theseus already shows.
pub const SHOWN_WORDS: &str =
    "the sign-in returned a value Theseus already shows, so it is not kept: nothing changed";

/// Why a renewal is refused, and the sign-in lost, for the same reason.
pub const RENEWAL_SHOWN: &str =
    "its renewal returned a value Theseus already shows, so it was not kept";

/// What Theseus shows of its servers, read at the moment a value enters.
pub type Shown = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// A `Shown` that shows nothing (a sign-in outside a board: tests).
pub fn nothing_shown() -> Shown {
    Arc::new(Vec::new)
}

/// Whether any of `values` is in any of `shown`: the scrubber, with `values`
/// alone as its values, would change something. A value too short to hide
/// is never one the scrubber keeps, and is refused where it is parsed.
pub fn already_shown(values: &[&str], shown: &[String]) -> bool {
    let scrubber = Scrubber::with_values(
        values
            .iter()
            .map(|v| (v.to_string(), "candidate".to_string()))
            .collect(),
    );
    shown.iter().any(|t| scrubber.scrub(t).1 > 0)
}

/// An address and its host, as shown.
pub fn address(url: &str) -> Vec<String> {
    let mut v = vec![url.to_string()];
    if let Some(h) = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
    {
        v.push(h);
    }
    v
}

impl McpBoard {
    /// Everything the board shows of its servers: each one's name, URL and
    /// host, and every tool of its live list, of the store's list, and of a
    /// list asked for, as `offered.rs` reads a tool.
    pub(super) fn shown(&self) -> Vec<String> {
        let store = self.core.get().and_then(std::sync::Weak::upgrade);
        let mut out = Vec::new();
        for s in self.servers.values().cloned().chain(self.loaded_servers()) {
            out.push(s.name.clone());
            if let Some(u) = &s.cfg.url {
                out.extend(address(u));
            }
            for t in &s.live().tools {
                out.extend(offered::tool_texts(&s.name, t));
            }
            if let Some(stored) = store
                .as_ref()
                .and_then(|c| super::read_stored(&c.store, &s.name))
            {
                for t in &stored.tools {
                    out.extend(offered::tool_texts(&s.name, t));
                }
            }
        }
        let listed = self
            .listed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (name, list) in listed.iter() {
            out.push(name.clone());
            for t in &list.tools {
                out.extend(offered::tool_texts(name, t));
            }
        }
        for t in self.catalog.all().iter() {
            out.push(t.canonical.clone());
            out.push(t.wire.clone());
        }
        out
    }

    /// `shown`, for a sign-in to read at its doors: the board, while it is.
    pub(super) fn shown_by(&self) -> Shown {
        let me = self.me.clone();
        Arc::new(move || me.upgrade().map(|b| b.shown()).unwrap_or_default())
    }
}
