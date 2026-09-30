//! "Should have asked" and its undo (theseus-sgh, spec §3.9). A press records
//! that a tool asks first from now on; an undo returns the tool to what the
//! config says. Each is ledgered (`policy.tightened`, `policy.untightened`)
//! with who, when, the tool, and the call that prompted it, and each is
//! judged by `Core::judge_act`, the judgment an answer to a waiting call
//! gets. The undo loosens, so it takes the whole `[approval]` rule.

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::{
    error_code, notify, Message, Notification, PolicyTightenParams, PolicyUntightenParams,
    TightenResult, Tightening,
};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::policy::{Posture, PostureNow};

impl Core {
    /// "Should have asked": `tool` asks first from now on, on every surface.
    /// `correlation_id` names the call whose notice was pressed; its proposal
    /// digest and tool make the ledger row a labeled example for later
    /// judgment work (Jev, M5). A tool tightened already is left as it was.
    pub fn tighten(
        &self,
        tool: &str,
        correlation_id: Option<&str>,
        by: impl Into<Answerer>,
    ) -> Result<TightenResult> {
        let who = by.into();
        if self.tools.registry.get(tool).is_none() {
            let names: Vec<&str> = self.tools.registry.all().map(|t| t.name()).collect();
            bail!(
                "no tool is named {tool:?}; the tools are {}",
                if names.is_empty() {
                    "none (tools are off)".to_string()
                } else {
                    names.join(", ")
                }
            );
        }
        let call = match correlation_id {
            None => None,
            Some(c) => {
                let a = self
                    .kernel
                    .action(c)?
                    .ok_or_else(|| anyhow!("no call {c}"))?;
                if a.tool != tool {
                    bail!("{c} is a call of {}, not {tool}", a.tool);
                }
                Some(a)
            }
        };
        self.judge_act(&who, Act::Tighten { tool })?;
        let before = self.tools.posture_now(tool);
        let t = Tightening {
            tool: tool.into(),
            posture: Posture::Approve.as_str().into(),
            by: who.label.clone(),
            who: who.who(),
            via: who.via(),
            at_ms: theseus_protocol::now_unix_ms(),
            correlation_id: call.as_ref().map(|a| a.correlation_id.clone()),
            session_id: call.as_ref().map(|a| a.session_id.clone()),
            digest: call.as_ref().map(|a| a.args_digest.clone()),
        };
        let after = self
            .tools
            .policy
            .posture_now(tool, Some(crate::tighten::as_tightened(&t)));
        let changed = after.posture != before.posture;
        let row = LedgerRow::new(
            "policy.tightened",
            t.session_id.as_deref(),
            None,
            json!({"tool": tool, "by": t.by, "who": t.who, "via": t.via,
                   "correlation_id": t.correlation_id, "digest": t.digest,
                   "posture": after.posture.as_str(), "setting": after.setting,
                   "config_posture": after.config.as_str(), "config_setting": after.config_setting,
                   "changed": changed}),
        );
        if !self.tools.tightened.insert(&self.store, t.clone(), &row)? {
            // Tightened already, perhaps a moment ago from another surface.
            let t = self.tools.tightened.get(tool).unwrap_or(t);
            return Ok(result(
                tool,
                &who.label,
                t,
                self.tools.posture_now(tool),
                false,
                true,
            ));
        }
        if changed {
            narrate!(
                self.narrator,
                Approval,
                t.session_id.as_deref(),
                None,
                "{tool} now asks first: tightened by {}.",
                t.by
            );
        } else {
            narrate!(
                self.narrator,
                Approval,
                t.session_id.as_deref(),
                None,
                "{tool} already asks first ({}); tightened by {} as well, so it keeps asking if \
                 the config changes.",
                after.config_setting,
                t.by
            );
        }
        let r = result(tool, &who.label, t, after, changed, false);
        self.announce(notify::POLICY_TIGHTENED, &r);
        Ok(r)
    }

    /// Undo a tightening: `tool` goes back to what the config says. It
    /// loosens, so it is judged as an answer is: never from a Theseus job's
    /// process (theseus-6qy), and under `[approval]`, only from a trusted
    /// user through a trusted channel.
    pub fn untighten(&self, tool: &str, by: impl Into<Answerer>) -> Result<TightenResult> {
        let who = by.into();
        let Some(t) = self.tools.tightened.get(tool) else {
            bail!("{tool} is not tightened, so there is nothing to undo");
        };
        let asker = self.judge_act(&who, Act::Untighten { tool })?;
        let before = self.tools.posture_now(tool);
        let after = self.tools.policy.posture_now(tool, None);
        let changed = after.posture != before.posture;
        let row = LedgerRow::new(
            "policy.untightened",
            t.session_id.as_deref(),
            None,
            json!({"tool": tool, "by": who.label, "who": who.who(), "via": who.via(),
                   "tightened_by": t.by, "tightened_at_ms": t.at_ms,
                   "correlation_id": t.correlation_id, "digest": t.digest,
                   "posture": after.posture.as_str(), "setting": after.setting,
                   "changed": changed, "asker": asker.json()}),
        );
        if !self.tools.tightened.remove(&self.store, tool, &row)? {
            bail!("{tool} is not tightened, so there is nothing to undo");
        }
        if changed {
            narrate!(
                self.narrator,
                Approval,
                t.session_id.as_deref(),
                None,
                "{tool} is back to what the config says ({}, {}): {} undid the tightening by {}.",
                after.posture.as_str(),
                after.setting,
                who.label,
                t.by
            );
        } else {
            narrate!(
                self.narrator,
                Approval,
                t.session_id.as_deref(),
                None,
                "{} undid the tightening of {tool} by {}; the config still asks ({}).",
                who.label,
                t.by,
                after.setting
            );
        }
        let r = result(tool, &who.label, t, after, changed, false);
        self.announce(notify::POLICY_UNTIGHTENED, &r);
        Ok(r)
    }

    /// A tightening holds for every session, so every watching connection
    /// hears of it once.
    fn announce(&self, method: &str, r: &TightenResult) {
        self.bus
            .publish_all(&Message::Notification(Notification::new(method, r)));
    }

    pub(super) fn policy_tighten(
        &self,
        p: PolicyTightenParams,
        conn: Conn<'_>,
    ) -> Result<TightenResult, RpcFailure> {
        let who = conn.answerer(p.author, p.discord);
        self.tighten(&p.tool, p.correlation_id.as_deref(), who)
            .map_err(|e| failure(e, "the press", &format!("{} keeps its posture", p.tool)))
    }

    pub(super) fn policy_untighten(
        &self,
        p: PolicyUntightenParams,
        conn: Conn<'_>,
    ) -> Result<TightenResult, RpcFailure> {
        let who = conn.answerer(p.author, p.discord);
        self.untighten(&p.tool, who)
            .map_err(|e| failure(e, "the undo", &format!("{} keeps asking first", p.tool)))
    }
}

fn result(
    tool: &str,
    by: &str,
    tightening: Tightening,
    now: PostureNow,
    changed: bool,
    already: bool,
) -> TightenResult {
    TightenResult {
        tool: tool.into(),
        by: by.into(),
        tightening,
        posture: now.posture.as_str().into(),
        setting: now.setting,
        config_posture: now.config.as_str().into(),
        config_setting: now.config_setting,
        changed,
        already,
    }
}

/// A refusal is `REFUSED` with who, via, and why, as for an answer; anything
/// else is the caller's.
fn failure(e: anyhow::Error, what: &str, then: &str) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!("{what} from {} does not count: {}. {then}", r.who, r.why),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
