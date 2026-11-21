//! The exchange ledger at the RPC layer (issue 44, step 1; `crate::exchange`):
//! the owner's mark of a submitted message, made where the live proof is, and
//! `exchange.report` and `exchange.rebuild`, which are read and asked from the
//! terminal or the web UI alone.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::exchange::{
    ExchangeLadderParams, ExchangeLadderResult, ExchangeRebuildParams, ExchangeReportParams,
};
use theseus_protocol::{error_code, method, TurnSubmitParams};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::approval::Surface;
use crate::exchange::{ledger, rebuild, OwnerMark, Peer};

/// The methods `rpc_exchanges` answers.
pub(super) const OWN: &[&str] = &[
    method::EXCHANGE_REPORT,
    method::EXCHANGE_REBUILD,
    method::EXCHANGE_LADDER,
];

impl Core {
    /// A message's mark (issue 44), made from what `turn.submit` knows of its
    /// sender before it runs: whether `places::owner_in_person_of` holds for
    /// it, whether its chat author holds an owner handle, the process behind
    /// it, and `place`, where it arrived (at `home`, the place's own session)
    /// before routing chose `runs_in`. Staged for the inbound point, which
    /// writes it beside the message's node. Records only: it decides nothing.
    pub(super) fn stage_arrival(
        &self,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
        (home, place): (&str, String),
        runs_in: &str,
    ) {
        if !self.cfg.context.ledger_on() || p.prompt.is_some() {
            return;
        }
        let discord = p.speaking.as_ref().map(|s| s.discord.clone());
        let who = conn.answerer(p.author.clone(), discord.clone());
        let rule = &self.runner.place_rule;
        let in_person = crate::places::owner_in_person_of(&who, rule, &self.cfg, "", "").is_ok();
        let owner_handle = (conn.surface == Surface::Discord).then(|| {
            matches!(
                crate::places::owner_anywhere(&who, rule, &self.cfg),
                Ok(Some(_))
            )
        });
        let surface = match p.surface.as_deref() {
            Some("voice") => "voice",
            _ => conn.surface.as_str(),
        };
        let mark = OwnerMark {
            node: String::new(),
            in_person,
            owner_handle,
            peer: Peer::of(conn.surface, conn.peer.behind()),
            agent: who.agent,
            surface: surface.into(),
            place,
            arrived_session: home.into(),
            ran_session: runs_in.into(),
            author: discord.map(|d| format!("discord:{}", d.user_id)),
            at_ms: 0,
        };
        self.runner
            .exchanges
            .arrivals
            .stage(runs_in, &p.input, mark);
    }

    pub(super) async fn rpc_exchanges(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if !conn.surface.reads_private() {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                "the exchange ledger is read from your own terminal or the web UI",
            ));
        }
        let out = match name {
            method::EXCHANGE_REPORT => {
                let p: ExchangeReportParams = match params.is_null() {
                    true => ExchangeReportParams::default(),
                    false => parse(params)?,
                };
                let core = self.clone();
                let r = super::import::blocking(move || ledger::report(&core.store, &p)).await?;
                serde_json::to_value(r)
            }
            method::EXCHANGE_REBUILD => {
                let p: ExchangeRebuildParams = match params.is_null() {
                    true => ExchangeRebuildParams::default(),
                    false => parse(params)?,
                };
                let index = self.runner.pass.index();
                let r = rebuild::run(&self.store, &self.runner.place_rule, &self.cfg, index, &p)
                    .await
                    .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))?;
                serde_json::to_value(r)
            }
            method::EXCHANGE_LADDER => {
                let p: ExchangeLadderParams = parse(params)?;
                let b = self
                    .ladder_build(&p.place, p.dry_run, p.cap_usd)
                    .await
                    .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))?;
                serde_json::to_value(ExchangeLadderResult {
                    dry_run: b.dry_run,
                    place: b.place,
                    since_ms: b.since_ms,
                    exchanges: b.exchanges,
                    lines_had: b.lines_had,
                    lines: b.lines,
                    folds: b.folds,
                    above: b.above,
                    estimated_usd: b.estimated_usd,
                    spent_usd: b.spent_usd,
                    stopped: b.stopped,
                })
            }
            other => {
                return Err(RpcFailure::new(
                    error_code::METHOD_NOT_FOUND,
                    format!("unknown method {other:?}"),
                ))
            }
        };
        out.map_err(|e| RpcFailure::invalid(e.into()))
    }
}
