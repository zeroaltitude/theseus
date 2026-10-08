//! `route.correct` and `route.corrections` (theseus-q31l): the owner's
//! correction of a turn's routing from a reaction, a footer's control, the
//! cockpit, or the CLI, judged as `judge.label` is (`judge_act`: the owner,
//! from a private place; the CLI refuses it inside a job), and the live
//! layer, a read. The owner's words in a conversation are read here too, as
//! `turn.submit` takes them, and acted on by the turn (`route_step::correct`).

use anyhow::Context as _;
use serde_json::{json, Value};
use theseus_protocol::route::{
    RouteCorrectParams, RouteCorrectResult, RouteCorrectionInfo, RouteCorrectionsResult,
};
use theseus_protocol::TurnSubmitParams;
use theseus_protocol::{error_code, method};

use super::server::{Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::{Answerer, Refusal, Surface};
use crate::correction::{self, By};
use crate::judge::inbound::ROUTE_PACK;

/// The methods `rpc_route` answers.
pub(super) const ROUTE: [&str; 2] = [method::ROUTE_CORRECT, method::ROUTE_CORRECTIONS];

impl Core {
    /// `route.correct`: the owner's correction of the turn `turn_id` names,
    /// or the session's last turn routing decided. Its label and its
    /// `route.corrected` row in one frame; the session's next turn runs where
    /// the owner said, and the layer steers a close message the same way.
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
        let pack = self.runner.judge.pack(ROUTE_PACK);
        let names = correction::names(&self.cfg, pack.as_deref());
        let to = correction::to_of(&p.to, &names).with_context(|| {
            format!(
                "{:?} names no profile, model or mode: name a configured profile, a mode of {ROUTE_PACK} ({}), \
                 `stronger`, or `cheaper`",
                p.to,
                names.modes.join(", ")
            )
        })?;
        let what = format!("{} on {} (to {})", ROUTE_PACK, last.turn, p.to);
        self.judge_act(&who, Act::RouteCorrect { what: &what })?;
        let via = p.via.clone().unwrap_or_else(|| who.via());
        let provenance = p.provenance.clone().unwrap_or_default();
        let c = self
            .runner
            .correcting(last, &to, false, who.who(), &via, provenance)?;
        self.store.append(&c.records()?)?;
        c.announce(&self.rec(Some(&c.last.session)));
        self.runner.corrected(&c);
        if let Some(s) = c.steering(By::Owner) {
            corrections.set_pending(&c.last.session, s);
        }
        Ok(c.result())
    }

    /// The owner's correction of routing, and the layer (theseus-q31l).
    pub(super) fn rpc_route(
        &self,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let out = match m {
            method::ROUTE_CORRECT => {
                let p =
                    serde_json::from_value(params).map_err(|e| RpcFailure::invalid(e.into()))?;
                serde_json::to_value(self.rpc_route_correct(p, conn)?)
            }
            _ => serde_json::to_value(self.route_corrections()),
        };
        out.map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
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

    /// `route.corrections`: the layer, newest first, under the acting route
    /// pack version.
    pub fn route_corrections(&self) -> RouteCorrectionsResult {
        let cfg = &self.cfg.routing.corrections;
        let (entries, retired) = self.runner.judge.corrections.list(ROUTE_PACK);
        RouteCorrectionsResult {
            pack: ROUTE_PACK.into(),
            entries: entries
                .into_iter()
                .map(|e| RouteCorrectionInfo {
                    to: correction::steer_line(&e.steer),
                    id: e.id,
                    label: e.label,
                    pack: e.pack,
                    session_id: e.session,
                    turn_id: e.turn,
                    words: e.words,
                    at_ms: e.at_ms,
                })
                .collect(),
            max_entries: cfg.max_entries,
            similarity: cfg.similarity,
            retired,
            enabled: cfg.enabled,
        }
    }

    /// The layer, rebuilt from its rows once the socket answers (theseusd's
    /// `after_serving`), on the blocking pool. Nothing with the judge off.
    pub fn warm_corrections(self: &std::sync::Arc<Self>) {
        if !self.cfg.judge.enabled {
            return;
        }
        let core = self.clone();
        tokio::task::spawn_blocking(move || {
            let cfg = &core.cfg.routing.corrections;
            core.runner
                .judge
                .corrections
                .warm(&core.store, ROUTE_PACK, cfg);
        });
    }

    /// `turn.submit`'s part: the owner's words read as a correction, for the
    /// turn to take at its inbound point, which acts on them only in a
    /// private place. A turn a job sent (`opened_from`), one through the MCP
    /// server or an unnamed surface, a prompt's, or one with files is a
    /// message.
    pub(super) fn expect_correction(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &str) {
        let cfg = &self.cfg.routing.corrections;
        if !self.cfg.judge.enabled
            || !cfg.enabled
            || p.opened_from.is_some()
            || p.prompt.is_some()
            || !p.attachments.is_empty()
            || p.input.chars().count() > correction::words::MAX_CHARS
            || matches!(conn.surface, Surface::Mcp | Surface::Unnamed)
        {
            return;
        }
        let pack = self.runner.judge.pack(ROUTE_PACK);
        let names = correction::names(&self.cfg, pack.as_deref());
        if let Some(said) = correction::words::read(&p.input, &names) {
            self.runner
                .judge
                .corrections
                .expect(session, said, &p.input);
        }
    }
}
