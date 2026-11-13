//! The owner's override of a held call (theseus-p5kln): `undo.list` reads
//! the holds, `undo.allow` lets one of them run as it was first proposed.
//!
//! **The decision this builds is the owner's own**, 2026-11-12: *"i would
//! like it wired and switched on, but I want to ensure that in the case that
//! the irreversibility flag is flipped and the model is notified, that i have
//! the power to say it's OK to run what was originally suggested"*. The
//! reshapeable arm IS that case — the flag is flipped and the model is told
//! to re-plan — and until this it had no surface to override from: the
//! `irreversible_final` arm had one already, since answering its card runs
//! the original call as proposed. Three things the decision puts out of
//! reach, and none of them is touched here: `[judge] reversibility` stays on
//! by default; a hold nobody overrides still makes no card and costs no
//! attention; and the override runs the ORIGINAL call, never a re-planned one
//! (`holds::Holds::allowance_for` matches the input the owner was shown).
//!
//! **Who may take it.** This is a route PAST a safety hold, so it is the
//! owner's and nobody else's: `judge_act(Act::UndoAllow)`, which asks
//! `places::owner_in_person` — the bar theseus-me9fc set for granting a
//! repository's trust. No agent's process, and on a transport a DM with an
//! owner alone. `undo.list` is a read and takes the ordinary private-place
//! rule, since a hold's summary is the session's own text.
//!
//! **What the override does not widen.** The allowance admits the call into
//! the §3.9 order, which keeps every say it had: the place rule, the mode,
//! the floor, a grant, a glide. A call the order would have refused is still
//! refused. And one allowance admits one call (`Holds::spend`), so the owner's
//! word is spent, not standing.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;
use theseus_protocol::method;
use theseus_protocol::work::undo::{
    allowed_words, Allowance, HeldCall, UndoAllowParams, UndoAllowResult, UndoListParams,
    UndoListResult,
};

use super::confirms::Act;
use super::projects::failure;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::fact::tool::ToolHoldAllowed;
use crate::toolrun::holds::Holds;

/// The override's methods, routed by one dispatch arm.
pub(super) const OWN: [&str; 2] = [method::UNDO_LIST, method::UNDO_ALLOW];

/// Whether `name` is one of the two acts judged by `places::owner_in_person`:
/// a repository's trust (theseus-me9fc) and a held call's override
/// (theseus-p5kln). One predicate and one dispatch arm, because
/// `rpc_prefixed` sits at clippy's 100-line ceiling.
pub(super) fn owner_in_person_method(name: &str) -> bool {
    super::project_trust::OWN.contains(&name) || OWN.contains(&name)
}

impl Core {
    /// The two `owner_in_person` acts' methods, by name.
    pub(super) fn rpc_owner_in_person(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match OWN.contains(&name) {
            true => self.rpc_undo_allow(name, params, conn),
            false => self.rpc_project_trust(name, params, conn),
        }
    }

    /// `undo.list` (a read) and `undo.allow` (the owner's act), by name.
    pub(super) fn rpc_undo_allow(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name == method::UNDO_LIST {
            return route(params, |p: UndoListParams| {
                let held = self.undo_list(&p).map_err(RpcFailure::invalid)?;
                Ok(UndoListResult { held })
            });
        }
        route(params, |p: UndoAllowParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            let hold = self.undo_allow(&p.hold, who).map_err(failure)?;
            Ok(UndoAllowResult {
                words: allowed_words(&hold),
                hold,
            })
        })
    }

    /// The held calls, newest first. A surface that reads no private text
    /// gets none: a hold's summary is the conversation's own words.
    pub fn undo_list(&self, p: &UndoListParams) -> Result<Vec<HeldCall>> {
        Ok(Holds::read(&self.store)?.list(p.session_id.as_deref(), p.all))
    }

    /// Let the held call `hold` run as it was first proposed. One frame: the
    /// holds' record with the allowance on it, and its `tool.hold_allowed`
    /// row. The call itself runs when the model proposes it again unchanged;
    /// the gate spends the allowance at 0d (`toolrun::reversible`).
    pub fn undo_allow(&self, hold: &str, who: impl Into<Answerer>) -> Result<HeldCall> {
        let who = who.into();
        let mut held = Holds::read(&self.store)?;
        let id = Self::one_hold(&held, hold)?;
        // Judged after the hold is found, so a name that matches nothing is
        // an error about the name and not a refusal about the place.
        self.judge_act(
            &who,
            Act::UndoAllow {
                what: &format!("Letting the held call {id} run as it was first proposed"),
            },
        )?;
        let (by, via) = (who.who(), who.via());
        let at = Allowance {
            by: by.clone(),
            via: via.clone(),
            at_ms: theseus_protocol::now_unix_ms(),
        };
        let h = held
            .allow(&id, at)
            .ok_or_else(|| anyhow!("{id} is not a held call"))?
            .clone();
        let fact = ToolHoldAllowed {
            hold: &h.id,
            tool: &h.tool,
            judgment: &h.judgment,
            summary: &h.summary,
            by: &by,
            via: &via,
        };
        let rec = self.rec(Some(&h.session_id));
        self.store.append(&[rec.row(&fact)?, held.record()?])?;
        rec.announce(&fact);
        Ok(h)
    }

    /// The one hold `name` means. A name that names two is refused with both,
    /// as `work.answer`'s id does: an override of a safety hold is never
    /// guessed.
    pub(crate) fn one_hold(held: &Holds, name: &str) -> Result<String> {
        let all = held.list(None, true);
        let found = theseus_protocol::work::undo::pick(&all, name);
        match found.len() {
            1 => Ok(found[0].id.clone()),
            0 => Err(anyhow!(
                "no held call is named {name}: `theseus undo list` says what is held"
            )),
            _ => Err(anyhow!(
                "{name} names {} held calls ({}): say which",
                found.len(),
                found
                    .iter()
                    .map(|h| h.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}
