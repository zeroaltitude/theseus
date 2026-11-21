//! `route.correct` (theseus-q31l): the owner's correction of a turn's
//! routing from a reaction, a footer's control, the cockpit, or the CLI,
//! judged as `judge.label` is (`judge_act`: the owner, from a private place;
//! the CLI refuses it inside a job). The owner's words in a conversation are
//! never read for one.

use anyhow::Context as _;
use serde_json::{json, Value};
use theseus_protocol::route::{RouteCorrectParams, RouteCorrectResult};
use theseus_protocol::{error_code, method};

use super::server::{Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::{Answerer, Refusal};
use crate::correction;
use crate::judge::inbound::ROUTE_PACK;

/// The methods `rpc_route` answers.
pub(super) const ROUTE: [&str; 1] = [method::ROUTE_CORRECT];

impl Core {
    /// `route.correct`: the owner's correction of the turn `turn_id` names,
    /// or the session's last turn routing decided. Its label and its
    /// `route.corrected` row in one frame; the session's next turn runs where
    /// the owner said.
    pub fn route_correct(
        &self,
        p: &RouteCorrectParams,
        who: impl Into<Answerer>,
    ) -> anyhow::Result<RouteCorrectResult> {
        let who = who.into();
        let corrections = &self.runner.judge.corrections;
        let last = match &p.turn_id {
            Some(t) => corrections.turn(t),
            None => corrections.last_of(&p.session_id),
        }
        .filter(|l| l.session == p.session_id)
        .with_context(|| {
            format!(
                "no turn of {} that routing decided is known since the daemon started{}",
                p.session_id,
                p.turn_id
                    .as_deref()
                    .map(|t| format!(" by the id {t}"))
                    .unwrap_or_default()
            )
        })?;
        let names = correction::names(&self.cfg, &self.personas());
        let to = correction::to_of(&p.to, &names).with_context(|| {
            format!(
                "{:?} names no profile or model: name a configured profile, `stronger`, or `cheaper`",
                p.to
            )
        })?;
        let what = format!("{} on {} (to {})", ROUTE_PACK, last.turn, p.to);
        self.judge_act(&who, Act::RouteCorrect { what: &what })?;
        let via = p.via.clone().unwrap_or_else(|| who.via());
        let provenance = p.provenance.clone().unwrap_or_default();
        let c = self
            .runner
            .correcting(last, &to, who.who(), &via, provenance)?;
        self.store.append(&c.records()?)?;
        c.announce(&self.rec(Some(&c.last.session)));
        self.runner.corrected(&c);
        if let Some(s) = c.steering() {
            corrections.set_pending(&c.last.session, s);
        }
        Ok(c.result())
    }

    /// The owner's correction of routing (theseus-q31l).
    pub(super) fn rpc_route(&self, params: Value, conn: Conn<'_>) -> Result<Value, RpcFailure> {
        let p = serde_json::from_value(params).map_err(|e| RpcFailure::invalid(e.into()))?;
        serde_json::to_value(self.rpc_route_correct(p, conn)?)
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
    }

    fn rpc_route_correct(
        &self,
        p: RouteCorrectParams,
        conn: Conn<'_>,
    ) -> Result<RouteCorrectResult, RpcFailure> {
        // Only the binding names a reaction's or a press's place.
        let who = conn.answerer(None, p.discord.clone());
        self.route_correct(&p, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "a correction from {} does not count: {}. Nothing was written.",
                        r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }
}
