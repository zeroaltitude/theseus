//! Scoped grants' methods (theseus-drjs); the matching and the gate's step
//! are `policy::grants`.
//!
//! - The offer on a question: `grant_offer_of`, computed from the call's
//!   stored gate record the way the turn computed it when it asked, so
//!   `confirm.list`, a card and the push say the same.
//! - The answer: `action.confirm { grant }` (`confirm_granting`). The daemon
//!   recomputes its own offer and takes the answer only when it echoes that
//!   offer's pattern exactly, so a client cannot widen it; the grant is the
//!   daemon's offer, never the client's words. Its `policy.granted` row rides
//!   in the answer's frame, after the answer's place rule (`judge_act`).
//! - `policy.grants`: the live grants beside the earned ones.
//! - `policy.revoke sgr_…`: one ends (a tightening: anyone's).
//! - A retired or superseded session's grants end in the frame that retires
//!   it (`grant_end_rows`).
//! - After serving, the ledger's `policy.granted` and `policy.grant_ended`
//!   rows are read once (`load_grants`), so a restart keeps them.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use theseus_protocol::grants::KIND_MODE;
use theseus_protocol::grants::{
    GrantAnswer, GrantOffer, PolicyGrantsParams, PolicyGrantsResult, SessionGrant,
};
use theseus_protocol::modes::SessionMode;
use theseus_protocol::{GateRecord, LedgerKind};
use theseus_store::{kinds, NewRecord};

use super::Core;
use crate::approval::Answerer;
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::node::{Body, Node};
use crate::policy::grants::{grant_of, offer, Asked};
use crate::session::SessionRecord;

/// How long the read after serving waits for the ledger's index, at most.
const LOAD_TRIES: u32 = 300;

impl Core {
    /// The offer of a question for a tool call, from its stored gate record.
    pub(super) fn grant_offer_of(
        &self,
        session: &SessionRecord,
        tool: &str,
        input: &Value,
        gate: Option<&GateRecord>,
    ) -> Option<GrantOffer> {
        let gate = gate?;
        let (decision, plan) = (gate.decision.as_ref()?, gate.plan.as_ref()?);
        let t = self.tools.registry.get(tool)?;
        let asked = Asked {
            approve: decision.posture.as_deref() == Some("approve"),
            reason: &decision.reason,
            floor: decision.floor,
            external: decision.external.is_some(),
            l1: decision.class.as_deref() == Some("l1"),
        };
        let class = self.runner.class_of(&session.session_id);
        offer(
            &self.tools,
            class,
            session.dir.as_deref(),
            t.as_ref(),
            plan,
            input,
            &asked,
        )
    }

    /// `action.confirm` with `grant`: approve the call and take its offer.
    pub fn confirm_granting(
        &self,
        correlation_id: &str,
        note: Option<&str>,
        who: Answerer,
        trust: bool,
        answer: &GrantAnswer,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let a = self
            .kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("no action {correlation_id}"))?;
        let rec = self
            .store
            .get_session::<SessionRecord>(&a.session_id)?
            .ok_or_else(|| anyhow!("no session {}", a.session_id))?;
        let nodes = self.store.session_nodes(&a.session_id)?;
        let Some((tool, input, gate)) = call_of(&nodes, correlation_id) else {
            bail!("{correlation_id} is no tool call's question: it offers no grant");
        };
        let Some(offered) = self.grant_offer_of(&rec, &tool, &input, gate.as_deref()) else {
            bail!("{correlation_id} offers no grant: approve it without one. Nothing changed.");
        };
        if answer.pattern != offered.pattern {
            bail!(
                "the grant answered (`{}`) is not the one offered (`{}`). Nothing changed.",
                answer.pattern,
                offered.pattern
            );
        }
        // A plan's second yes is a mode, not a grant: the plan's own tool
        // moves the conversation once the call runs (theseus-lyxy).
        if offered.kind == KIND_MODE {
            let modes = &self.tools.earned.modes;
            modes.pick(correlation_id, SessionMode::Edits);
            let done = self.confirm_action_with(correlation_id, true, note, who, trust, &[]);
            if done.is_err() {
                modes.unpick(correlation_id);
            }
            return done;
        }
        let argv = gate
            .as_ref()
            .and_then(|g| g.plan.as_ref())
            .and_then(|p| p.argv.clone());
        let now = theseus_protocol::now_unix_ms();
        let g = grant_of(
            &offered,
            &a.session_id,
            correlation_id,
            argv.as_deref(),
            (&who.label, who.who(), who.via()),
            now,
        )
        .ok_or_else(|| anyhow!("{correlation_id}'s offer makes no grant. Nothing changed."))?;
        let row = LedgerRow::new(
            LedgerKind::PolicyGranted,
            Some(&a.session_id),
            None,
            serde_json::to_value(&g)?,
        );
        let extra = [NewRecord::json(kinds::LEDGER, None, &row)?];
        let mut done = self.confirm_action_with(correlation_id, true, note, who, trust, &extra)?;
        self.tools.earned.scoped.add(g.clone());
        narrate!(
            self.narrator,
            Approval,
            Some(&a.session_id),
            None,
            "{} runs with a notice for the rest of this conversation: {} granted it.",
            g.pattern,
            g.by
        );
        done.granted = Some(g);
        Ok(done)
    }

    /// `policy.grants`: the live grants, and the earned ones.
    pub fn grants_list(&self, p: &PolicyGrantsParams) -> PolicyGrantsResult {
        PolicyGrantsResult {
            session: self.tools.earned.scoped.list(p.session_id.as_deref()),
            earned: self.tools.earned.grants(),
        }
    }

    /// `policy.revoke` of a conversation's grant: it ends, in one frame
    /// with its row. A tightening, so anyone's.
    pub fn grant_revoke(&self, id: &str, who: &Answerer) -> Result<SessionGrant> {
        let Some(g) = self.tools.earned.scoped.get(id) else {
            bail!("{id} is no live grant; `theseus policy grants` lists them");
        };
        let row = end_row(&g, "revoked", &who.label);
        self.store
            .append(&[NewRecord::json(kinds::LEDGER, None, &row)?])?;
        self.tools.earned.scoped.end(id);
        narrate!(
            self.narrator,
            Approval,
            Some(&g.session_id),
            None,
            "{} asks first again: {} ended its grant.",
            g.pattern,
            who.label
        );
        Ok(g)
    }

    /// The rows that end a session's grants as it is retired, for its
    /// frame, and their ids, to end in memory once it is written.
    pub(crate) fn grant_end_rows(
        &self,
        session_id: &str,
        why: &str,
    ) -> Result<(Vec<NewRecord>, Vec<String>)> {
        let live = self.tools.earned.scoped.list(Some(session_id));
        let mut rows = Vec::new();
        for g in &live {
            rows.push(NewRecord::json(
                kinds::LEDGER,
                None,
                &end_row(g, why, "theseus"),
            )?);
        }
        Ok((rows, live.into_iter().map(|g| g.id).collect()))
    }

    pub(crate) fn grants_ended(&self, ids: &[String]) {
        for id in ids {
            self.tools.earned.scoped.end(id);
        }
    }

    /// Read the grants back from the ledger: every `policy.granted` row with
    /// no `policy.grant_ended` row for it, whose session is there and not
    /// retired since. False while the ledger's index is being built.
    pub fn load_grants(&self) -> Result<bool> {
        self.load_grants_into(&self.tools.earned.scoped)
    }

    /// `load_grants`, into `into`: a restart's read, as a test makes it.
    pub fn load_grants_into(&self, into: &crate::policy::grants::Grants) -> Result<bool> {
        let Some((granted, _)) = super::earned::rows_of(self, "policy.granted", 0)? else {
            return Ok(false);
        };
        // The ends are read after the grants, so an end written between the
        // two reads is seen.
        let Some((ended, _)) = super::earned::rows_of(self, "policy.grant_ended", 0)? else {
            return Ok(false);
        };
        let ended: std::collections::BTreeSet<String> = ended
            .iter()
            .filter_map(|r| r.data["id"].as_str().map(str::to_string))
            .collect();
        for r in granted {
            let Ok(g) = serde_json::from_value::<SessionGrant>(r.data) else {
                continue;
            };
            if ended.contains(&g.id) {
                continue;
            }
            let live = self
                .store
                .get_session::<SessionRecord>(&g.session_id)?
                .is_some_and(|s| {
                    s.retired.is_none() && s.reopened_ms.is_none_or(|at| at < g.at_ms)
                });
            if live {
                into.add(g);
            }
        }
        Ok(true)
    }

    /// `load_grants` after serving, off the start path, waiting on tokio's
    /// timer while the ledger's index is built.
    pub fn warm_grants(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            for _ in 0..LOAD_TRIES {
                let Some(c) = core.upgrade() else { return };
                let done = tokio::task::spawn_blocking(move || c.load_grants()).await;
                match done {
                    Ok(Ok(true)) => return,
                    Ok(Ok(false)) => {}
                    Ok(Err(e)) => {
                        tracing::warn!(error = %format!("{e:#}"), "the conversations' grants cannot be read");
                        return;
                    }
                    Err(_) => return,
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

/// A grant's end, as its row says it.
fn end_row(g: &SessionGrant, why: &str, by: &str) -> LedgerRow {
    LedgerRow::new(
        LedgerKind::PolicyGrantEnded,
        Some(&g.session_id),
        None,
        json!({"id": g.id, "pattern": g.pattern, "kind": g.kind, "why": why, "by": by,
               "granted_ms": g.at_ms}),
    )
}

/// The tool call a question names, from its session's nodes.
fn call_of(
    nodes: &[(u64, Node)],
    correlation_id: &str,
) -> Option<(String, Value, Option<Box<GateRecord>>)> {
    nodes.iter().find_map(|(_, n)| match &n.body {
        Body::ToolCall {
            tool,
            input,
            correlation_id: Some(c),
            gate,
            ..
        } if c == correlation_id => Some((tool.clone(), input.clone(), gate.clone())),
        _ => None,
    })
}
