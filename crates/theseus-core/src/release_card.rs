//! The release card's second look (issue 44, the design's step 3b; `docs/design/release-card.md`).
//!
//! When the trigger (`judge::leans_on`) says the owner's message in company leans on something not said in that
//! place, a separate look under HIS private conversation's own rules looks for the one piece the message leans on.
//!
//! - **Under his private conversation's rules, never the company turn's.** The look is a history-tool lens
//!   ([`page::Lens`]) anchored at his private conversation (the first direct conversation the bindings bind, as
//!   the ledger names places), the owner alone there: it reads that conversation's own exchanges of his, under
//!   the place rule and the seal, failing closed ([`page::Lens::admits`], `Core::ladder_hidden`). Nothing of the
//!   company turn's permit or presence is used, and the company place's own exchanges are never candidates.
//! - **By the means that exist.** The words index, scoped first to the sessions the lens admits on that
//!   conversation's timeline (`history_tool::found`), with the message as the query.
//! - **Off the company turn's path, and nothing back.** It runs in the trigger's own spawned task, after the
//!   trigger's judgment; the company turn has already gone on and is never told anything: not the piece, not
//!   whether one was found, not a count. Its record ([`record`]) is one `context.release_look` row for EVERY
//!   judgment of the trigger, whatever it answered and whatever the look found, written under no session and no
//!   turn, holding ids and counts and never a piece's words.
//! - **In shadow.** The card that would offer the piece, and the release, are not built (the design note says
//!   what they take). The row says what would have been offered: [`Outcome::One`] is the one case a card would
//!   be posted; several candidates are no card (the smaller safe thing; the design asks which of up to three,
//!   and a card that asks is a later step).

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;
use theseus_memory::recall::Place;
use theseus_protocol::LedgerKind;

use crate::exchange::{self, OWNER};
use crate::history_tool::{self, page};
use crate::ledger::LedgerRow;
use crate::view::timeline::TimelineRoot;
use crate::view::Present;
use crate::view_gate::RecallPlaces;
use crate::Core;

/// The trigger's answer the look follows.
pub const LEANS_ELSEWHERE: &str = "not_said_here";
/// The most candidates' ids a record names.
pub const NAMED: usize = 3;

/// What the look found, as the record says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The trigger did not say the message leans on something not said in its place: no look.
    NotAsked,
    /// No direct conversation with the owner is bound: there is no private conversation to look in, or to post to.
    NoPrivateConversation,
    /// The words index did not answer.
    NoIndex,
    /// Nothing of his in his private conversation matched.
    Nothing,
    /// One candidate: the one case a card would offer.
    One,
    /// More than one: no card.
    Several,
}

impl Outcome {
    /// The outcome of `n` candidates.
    pub fn of(n: usize) -> Self {
        match n {
            0 => Outcome::Nothing,
            1 => Outcome::One,
            _ => Outcome::Several,
        }
    }
}

/// A look's result: its outcome, how many candidates, and the newest few's exchange ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub outcome: Outcome,
    pub found: usize,
    pub ids: Vec<String>,
}

impl Look {
    fn bare(outcome: Outcome) -> Self {
        Look {
            outcome,
            found: 0,
            ids: Vec::new(),
        }
    }
}

/// His private conversation, as the ledger names places: the first direct conversation the bindings bind.
pub fn private_conversation(core: &Core) -> Option<String> {
    let dm = core
        .runner
        .place_rule
        .owner_dm_places()
        .into_iter()
        .next()?;
    Some(format!("discord:{dm}"))
}

/// The candidates among `found`'s exchanges: the ones the lens admits, of his own words, in its own place alone;
/// newest first, by start then id.
pub fn candidates(lens: &page::Lens<'_>, found: &HashSet<String>) -> Vec<String> {
    let mut kept: Vec<(u64, String)> = found
        .iter()
        .filter_map(|t| exchange::exchange(lens.store, t).ok().flatten())
        .filter(|ex| {
            ex.place == lens.here && ex.person.as_deref() == Some(OWNER) && lens.admits(ex)
        })
        .map(|ex| (ex.start_ms, ex.turn_id))
        .collect();
    kept.sort_by(|a, b| b.cmp(a));
    kept.into_iter().map(|(_, id)| id).collect()
}

/// The look for `message`, under his private conversation's own rules.
pub async fn look(core: &Arc<Core>, message: &str) -> Look {
    let Some(here) = private_conversation(core) else {
        return Look::bare(Outcome::NoPrivateConversation);
    };
    let places = RecallPlaces::default();
    let private = |s: &str| places.of_session(&core.runner, s) == Place::Private;
    let hidden = |s: &str| core.ladder_hidden(s);
    let lens = page::Lens {
        store: &core.store,
        here: here.clone(),
        present: Present::OwnerAlone,
        hidden: &hidden,
        private: &private,
        in_view: None,
        words: None,
    };
    let Ok(scope) = lens.scope(&TimelineRoot::Place(here)) else {
        return Look::bare(Outcome::NoIndex);
    };
    let Ok(found) = history_tool::found(core, message, scope).await else {
        return Look::bare(Outcome::NoIndex);
    };
    let all = candidates(&lens, &found);
    Look {
        outcome: Outcome::of(all.len()),
        found: all.len(),
        ids: all.into_iter().take(NAMED).collect(),
    }
}

/// The one row a judgment of the trigger leaves: under no session and no turn, ids and counts only.
pub fn record(turn: &str, node: &str, answer: Option<&str>, look: &Look) -> LedgerRow {
    LedgerRow::new(
        LedgerKind::ContextReleaseLook,
        None,
        None,
        json!({
            "company_turn": turn, "node": node, "answer": answer, "mode": "shadow",
            "outcome": look.outcome, "found": look.found, "ids": look.ids,
        }),
    )
}

/// After the trigger's judgment of the message `node` that opened `turn`, in the trigger's own task: the look
/// when it said the message leans on something not said in its place, and the one row either way.
pub async fn after_trigger(core: Arc<Core>, turn: String, node: String, answer: Option<String>) {
    let look = match answer.as_deref() {
        Some(LEANS_ELSEWHERE) => match message_of(&core, &node) {
            Some(text) => look(&core, &text).await,
            None => Look::bare(Outcome::NotAsked),
        },
        _ => Look::bare(Outcome::NotAsked),
    };
    let row = record(&turn, &node, answer.as_deref(), &look);
    let store = core.store.clone();
    let wrote = theseus_store::blocking(move || store.append_ledger(&row));
    if let Err(e) = wrote {
        tracing::warn!(error = %format!("{e:#}"), "the release card's look was not recorded");
    }
}

/// The words of the message `node`.
fn message_of(core: &Core, node: &str) -> Option<String> {
    let (_, n) = core.store.get_node(node).ok()??;
    match n.body {
        crate::node::Body::UserMessage { text, .. } => Some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
