//! Where a sign-in's values enter (issue 55, rounds 5 and 6), and the one
//! check at each: a value that equals anything Theseus already shows of its
//! servers is not kept. The doors: a registration's client secret, the
//! authorization code (from the listener or a paste) and a sign-in's
//! redemption (`sign_in.rs`); every renewal and the kept file read at start
//! (`oauth.rs`); and the verifier the daemon makes, made again on a match.
//! What is shown is read from the board's own producers (`McpBoard::shown`),
//! with the sign-in's own issuer, endpoints and client id. The daemon's scrubber
//! reads them with the new values alone as its values, so its encodings
//! count. A list accepted is filtered against the values known when it came
//! (`offered.rs`); a value that would make an accepted string a secret is
//! refused here. So no list, label or name ever holds a kept value.

use std::sync::Arc;

use super::offered::{self, tool_texts};
use super::McpBoard;
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
    /// Everything the board shows of its servers, read from the producers
    /// that make it (issue 55, round 6), so a field added later is read with
    /// them: `mcp.list` as the RPC returns it now (every server's status with
    /// all its fields, every tool as the catalog renders it, every prompt
    /// listed), `mcp.prompt.list`'s; each server's stored tool, prompt and
    /// used-prompt records; every list an attach asked for; every pending
    /// attach card's proposal; and each server's name and address. Each is
    /// read whole, serialized, and as its every key and scalar.
    pub(super) fn shown(&self) -> Vec<String> {
        let core = self.core.get().and_then(std::sync::Weak::upgrade);
        let mut out = Vec::new();
        let mut add = |v: serde_json::Value| {
            out.push(v.to_string());
            offered::strings_of(&v, &mut out);
        };
        match &core {
            Some(c) => add(json(&self.list(&c.tools))),
            // A board outside a daemon (unit tests): its parts, as `list` reads them.
            None => {
                add(json(&self.status()));
                add(json(&self.prompt_infos(None)));
                for t in self.catalog.all().iter() {
                    add(serde_json::json!([
                        t.canonical,
                        t.wire,
                        t.description,
                        t.listed
                    ]));
                }
            }
        }
        add(json(&theseus_protocol::mcp::McpPromptListResult {
            prompts: self.prompt_infos(None),
        }));
        let trials: Vec<Arc<super::Server>> = self
            .trials
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for s in self
            .servers
            .values()
            .cloned()
            .chain(self.loaded_servers())
            .chain(trials)
        {
            add(serde_json::json!(s.name));
            if let Some(u) = &s.cfg.url {
                address(u)
                    .into_iter()
                    .for_each(|a| add(serde_json::json!(a)));
            }
            for t in &s.live().tools {
                tool_texts(&s.name, t)
                    .into_iter()
                    .for_each(|x| add(serde_json::json!(x)));
            }
            if let Some(c) = &core {
                if let Some(stored) = super::read_stored(&c.store, &s.name) {
                    for t in &stored.tools {
                        tool_texts(&s.name, t)
                            .into_iter()
                            .for_each(|x| add(serde_json::json!(x)));
                    }
                }
                add(json(&super::prompts::read_stored(&c.store, &s.name)));
                add(json(&super::prompts::read_used(&c.store, &s.name)));
            }
        }
        let listed = self
            .listed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (name, list) in listed.iter() {
            add(serde_json::json!(name));
            for t in &list.tools {
                tool_texts(name, t)
                    .into_iter()
                    .for_each(|x| add(serde_json::json!(x)));
            }
        }
        drop(listed);
        if let Some(c) = &core {
            for a in c.kernel.pending_confirms().unwrap_or_default() {
                if crate::config_change::mcp_attach::is_card(&a) {
                    add(json(&a.proposal));
                }
            }
        }
        out
    }

    /// `shown`, for a sign-in to read at its doors: the board, while it is.
    pub(super) fn shown_by(&self) -> Shown {
        let me = self.me.clone();
        Arc::new(move || me.upgrade().map(|b| b.shown()).unwrap_or_default())
    }
}

/// A producer's answer, as JSON.
fn json<T: serde::Serialize>(t: &T) -> serde_json::Value {
    serde_json::to_value(t).unwrap_or(serde_json::Value::Null)
}
