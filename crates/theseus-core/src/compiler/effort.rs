//! A turn's effort without breaking the cache (theseus-o719).
//!
//! route.v3 sets a turn's effort (theseus-qe3v). Carried as the request's
//! top-level `output_config.effort`, a change of it between requests restarts
//! the provider's cache of the messages: only the tools and the system stay
//! cached. In a conversation of 625k tokens on Fable 5.1, one turn that Jev
//! put at `low` read 21k of its prefix and wrote 612k again ($7.68), two
//! minutes after the turn before it read all of it.
//!
//! On a model that takes per-message effort (beta
//! `mid-conversation-output-config-2026-07-01`; the catalog's `per_message_effort`), the top-level
//! effort stays the profile's own, and a turn that runs at another level says
//! so in an effort-only system message (`{"role": "system", "content": [],
//! "output_config": {"effort": …}}`) just before its user message. The level
//! holds from that message on, until another changes it. The message is part
//! of the history from then on, so it renders again wherever its turn
//! renders, and the bytes before it never change:
//!
//! - **Its record** is the turn's first answer's `effort` (format 27): the
//!   level its request placed ([`placed`]). An answer's record renders as the
//!   message before the user message its answer follows ([`place`]), whatever
//!   the profile's effort is now, so a later request repeats the earlier
//!   request's bytes. Nothing else reads it.
//! - **The turn's own** ([`retarget`]): the level the turn runs at (Jev's, or
//!   the profile's own) against the level in force at the end of its history
//!   (the last such message's, or the top-level one). Only a change places a
//!   message, and only before a user message that opens with the turn's text:
//!   one that opens with tool results stays next to its calls, and the turn
//!   runs at the level in force.
//! - **Any other model** gets no such message: the top-level effort, as
//!   before, and the history's messages are left out; Jev's level there is
//!   sticky instead (`routing::effort::sticky`). The catalog names the models
//!   that take messages ([`default_of`]); a 400 that names `output_config`
//!   marks any other model refusing for the daemon's life (theseus-posww).

use serde_json::{json, Value};

use super::RequestSpec;
use crate::catalog::{CatalogEntry, ThinkingMode};
use crate::config::Effort;
use crate::provider::ProviderRequest;

/// Per-message effort's beta.
pub const BETA_TURN_EFFORT: &str = "mid-conversation-output-config-2026-07-01";

/// The level a model that takes per-message effort runs at without one:
/// `high`, every such model's default. Which models take it is the catalog's
/// (`CatalogEntry::per_message_effort`, so a row's edit enables one with no
/// code): Fable 5.1 alone is seen to accept the message live. On 2026-10-11
/// Claude Sonnet 5.5 answered it with a 400 (`messages.190.output_config:
/// Extra inputs are not permitted`) and every request of a conversation that
/// held one failed; a row that turns it on needs a live check first
/// (theseus-posww). One that still refuses it is found out at run time
/// ([`refused_by`]).
pub fn default_of(entry: Option<&CatalogEntry>) -> Option<Effort> {
    entry.filter(|e| e.per_message_effort).map(|_| Effort::High)
}

/// The request's model takes per-message effort: Anthropic's own API (the beta
/// is not confirmed elsewhere), a catalog row that says so, and one this
/// daemon has not found to refuse it (`spec.effort_refused`).
pub fn takes(spec: &RequestSpec, entry: Option<&CatalogEntry>, model: &str) -> bool {
    spec.first_party
        && default_of(entry).is_some()
        && !spec.effort_refused.iter().any(|m| m == model)
}

/// Whether a failed call is the provider refusing the effort-only messages:
/// a 400 that names `output_config`, on a request that carries one. Then the
/// turn asks again without them (theseus-posww).
pub fn refused_by(req: &ProviderRequest, class: &str, error: &str) -> bool {
    class == "invalid_request"
        && error.contains("output_config")
        && req.messages.iter().any(|m| level_of(m).is_some())
}

/// The request without its effort-only messages and their beta: what a model
/// that refuses them is sent, its level the top-level one.
pub fn without_messages(req: &mut ProviderRequest) {
    req.messages.retain(|m| level_of(m).is_none());
    sync_beta(req);
}

/// A request's `output_config`: its effort, for a model whose catalog row
/// takes effort (route.v3 sets it after the first compile, theseus-qe3v). A
/// model that thinks implicitly (GLM, DeepSeek) documents three levels, low,
/// high and max, so the wire carries only those: medium and xhigh go as high,
/// as DeepSeek maps them itself.
pub fn output_config(entry: Option<&CatalogEntry>, effort: Option<Effort>) -> Option<Value> {
    let implicit = entry.is_some_and(|e| e.thinking == ThinkingMode::Implicit);
    match (entry.map(|e| e.effort), effort) {
        (Some(true), Some(e)) if implicit => Some(json!({"effort": three_levels(e)})),
        (Some(true), Some(e)) => Some(json!({"effort": e})),
        _ => None,
    }
}

/// The nearest of low, high and max a three-level model documents.
fn three_levels(e: Effort) -> Effort {
    match e {
        Effort::Low | Effort::Max => e,
        Effort::Medium | Effort::High | Effort::Xhigh => Effort::High,
    }
}

/// The effort-only system message of `e`.
fn message(e: Effort) -> Value {
    json!({"role": "system", "content": [], "output_config": {"effort": e}})
}

/// The level of an effort-only system message.
fn level_of(m: &Value) -> Option<Effort> {
    (m["role"] == "system")
        .then(|| serde_json::from_value(m["output_config"]["effort"].clone()).ok())
        .flatten()
}

/// A user message that opens with its turn's text, not with tool results.
fn opens_turn(m: &Value) -> bool {
    m["role"] == "user" && m["content"][0]["type"] != "tool_result"
}

/// The beta rides exactly the requests that carry a message.
fn sync_beta(req: &mut ProviderRequest) {
    let carries = req.messages.iter().any(|m| level_of(m).is_some());
    req.betas.retain(|b| b != BETA_TURN_EFFORT);
    if carries {
        req.betas.push(BETA_TURN_EFFORT.to_string());
    }
}

/// The rendered request's messages, with each recorded effort before the
/// user message its answer follows (`answers`: the index each answer's
/// message took, and its record), then the turn's own (`retarget`); the tail's
/// first message, moved past what went in before it. A model that takes no
/// per-message effort gets none.
pub fn place(
    req: &mut ProviderRequest,
    answers: &[(usize, Effort)],
    spec: &RequestSpec,
    entry: Option<&CatalogEntry>,
    tail_from: usize,
) -> usize {
    if !takes(spec, entry, &req.model) {
        return tail_from;
    }
    let mut tail_from = tail_from;
    // From the last, so each index still holds when it is reached.
    for &(at, e) in answers.iter().rev() {
        let follows = at > 0
            && req
                .messages
                .get(at)
                .is_some_and(|m| m["role"] == "assistant");
        if follows && opens_turn(&req.messages[at - 1]) {
            req.messages.insert(at - 1, message(e));
            tail_from += usize::from(at - 1 < tail_from);
        }
    }
    if let Some(at) = retarget(req, spec, entry) {
        tail_from += usize::from(at < tail_from);
    }
    tail_from
}

/// The turn's own level, before its user message when it changes the level
/// in force, replacing one placed before (route.v3 decides after the first
/// compile); none for any other model, or after the turn's first answer.
/// Where a message went in.
pub fn retarget(
    req: &mut ProviderRequest,
    spec: &RequestSpec,
    entry: Option<&CatalogEntry>,
) -> Option<usize> {
    let default = default_of(entry).filter(|_| takes(spec, entry, &req.model))?;
    let mut at = req.messages.len().checked_sub(1)?;
    if !opens_turn(&req.messages[at]) {
        return None;
    }
    if at > 0 && level_of(&req.messages[at - 1]).is_some() {
        req.messages.remove(at - 1);
        at -= 1;
    }
    let running = req.messages[..at]
        .iter()
        .rev()
        .find_map(level_of)
        .or(spec.effort);
    let want = spec.turn_effort.or(spec.effort);
    let placed = (want != running).then(|| {
        req.messages.insert(at, message(want.unwrap_or(default)));
        at
    });
    sync_beta(req);
    placed
}

/// The level a request placed before its turn's user message: what its
/// answer records.
pub fn placed(req: &ProviderRequest) -> Option<Effort> {
    let n = req.messages.len();
    (n >= 2 && opens_turn(&req.messages[n - 1]))
        .then(|| level_of(&req.messages[n - 2]))
        .flatten()
}

/// The level a request runs at: its last effort message's, else its
/// top-level one.
#[cfg(test)]
pub(crate) fn effective(req: &ProviderRequest) -> Option<Effort> {
    req.messages.iter().rev().find_map(level_of).or_else(|| {
        req.output_config
            .as_ref()
            .and_then(|o| serde_json::from_value(o["effort"].clone()).ok())
    })
}
