//! The exchange ledger at the RPC layer (issue 44, step 1; `crate::exchange`):
//! the owner's mark of a submitted message, made where the live proof is, and
//! `exchange.report` and `exchange.rebuild`, which are read and asked from the
//! terminal or the web UI alone.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::exchange::{
    ExchangeAdoptParams, ExchangeLadderParams, ExchangeLadderResult, ExchangeRebuildParams,
    ExchangeReportParams,
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
    method::EXCHANGE_ADOPT,
];

impl Core {
    /// A submission's mark (issue 44), made from what `turn.submit` knows of
    /// its sender before it runs: whether `places::owner_in_person_of` holds
    /// for it, whether its chat author holds an owner handle, the process
    /// behind it, and `place`, where it arrived (at `home`, the place's own
    /// session) before routing chose `runs_in`. It rides this submission's
    /// own sink to the node it becomes, a steer's included; a submission
    /// refused before its node is written takes it with it. Records only: it
    /// decides nothing.
    pub(super) fn arrival_of(
        &self,
        p: &TurnSubmitParams,
        conn: Conn<'_>,
        (home, place): (&str, String),
        runs_in: &str,
    ) -> Option<OwnerMark> {
        if !self.cfg.context.ledger_on() || p.prompt.is_some() {
            return None;
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
        Some(OwnerMark {
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
        })
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
                let mut r =
                    super::import::blocking(move || ledger::report(&core.store, &p)).await?;
                r.places_bound = ledger::bound_now(&self.runner.place_rule.health(&self.cfg));
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
                // It spends money: the owner in person only, never an agent's
                // process at the terminal.
                let who = conn.answerer(None, None);
                let rule = &self.runner.place_rule;
                if let Err(why) = crate::places::owner_in_person_of(
                    &who,
                    rule,
                    &self.cfg,
                    "building a ladder spends money",
                    "",
                ) {
                    return Err(RpcFailure::new(
                        error_code::REFUSED,
                        format!("not built: {why}"),
                    ));
                }
                let money = crate::ladder::build::Allowance {
                    cap_usd: p.cap_usd,
                    over_day_line: p.over_day_line,
                };
                let b = self
                    .ladder_build(&p.place, p.dry_run, money)
                    .await
                    .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))?;
                serde_json::to_value(ExchangeLadderResult {
                    dry_run: b.dry_run,
                    place: b.place,
                    since_ms: b.since.map_or(0, |p| p.at_ms),
                    assembly: match b.assembly {
                        crate::config::assembly::Assembly::View => "view".into(),
                        crate::config::assembly::Assembly::Transcript => "transcript".into(),
                    },
                    skipped: b.skipped,
                    day_left_usd: b.day_left_usd,
                    needs_over_day_line: b.needs_over_day_line,
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
            method::EXCHANGE_ADOPT => {
                let p: ExchangeAdoptParams = parse(params)?;
                self.clone().adopt(p, conn).await?
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

impl Core {
    /// `exchange.adopt`: the owner's own act, so the owner in person only,
    /// as `exchange.ladder` is: never an agent's process at the terminal, and
    /// never a job. Each session's seal is the gate's (`view_gate::seal_of`),
    /// and anything not proven clear is left.
    async fn adopt(
        self: Arc<Self>,
        p: ExchangeAdoptParams,
        conn: Conn<'_>,
    ) -> Result<serde_json::Result<Value>, RpcFailure> {
        let who = conn.answerer(None, None);
        let rule = &self.runner.place_rule;
        if let Err(why) = crate::places::owner_in_person_of(
            &who,
            rule,
            &self.cfg,
            "adopting history into a place is the owner's own act",
            "",
        ) {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!("not adopted: {why}"),
            ));
        }
        let private =
            rule.class(&self.cfg, Some(p.place.trim())) == crate::places::PlaceClass::Private;
        let surface = conn.surface.as_str().to_string();
        let core = self.clone();
        let r = super::import::blocking(move || {
            let sealed = |sid: &str| {
                crate::view_gate::seal_of(&core.runner, None, sid)
                    != theseus_memory::gate::Seal::Clear
            };
            // The threads unread: every node is held out.
            let board = core.runner.threads.all(&core.store).ok();
            let node_sealed = |sid: &str, node: &str| {
                board.as_ref().is_none_or(|b| {
                    crate::view_gate::node_seal(&core.store, b, None, sid, node)
                        != theseus_memory::gate::Seal::Clear
                })
            };
            let a = crate::exchange::adopt::Asker {
                surface: &surface,
                private,
                sealed: &sealed,
                node_sealed: &node_sealed,
                now_ms: theseus_protocol::time::now_unix_ms(),
            };
            match p.undo.clone() {
                Some(id) => crate::exchange::adopt::undo(&core.store, &a, &p, &id),
                None => crate::exchange::adopt::adopt(&core.store, &a, &p),
            }
        })
        .await?;
        Ok(serde_json::to_value(r))
    }
}
