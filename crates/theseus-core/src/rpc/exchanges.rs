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
    method::EXCHANGE_OFFER,
    method::EXCHANGE_RELEASE,
    method::EXCHANGE_RELEASES,
    method::EXCHANGE_WITHDRAW,
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
            method::EXCHANGE_OFFER
            | method::EXCHANGE_RELEASE
            | method::EXCHANGE_RELEASES
            | method::EXCHANGE_WITHDRAW => return self.rpc_release(name, params, conn).await,
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferParams {
    /// A message of the owner's in company, by its node.
    message: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseParams {
    /// The exchange, by its turn.
    exchange: String,
    /// The place it is released to, as the ledger names places.
    to: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WithdrawParams {
    id: String,
}

impl Core {
    /// The release methods (`crate::release`): the owner in person, by the proof his other acts ask, before
    /// anything is read; an agent's process, the MCP server and an unnamed connection are refused.
    async fn rpc_release(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let who = conn.answerer(None, None);
        if let Err(why) = crate::places::owner_in_person_of(
            &who,
            &self.runner.place_rule,
            &self.cfg,
            "a piece of a private conversation is released by the owner alone",
            "",
        ) {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!("refused: {why}"),
            ));
        }
        self.release_act(name, params, conn.surface.as_str())
            .await
            .map_err(|e| RpcFailure::new(error_code::REFUSED, format!("{e:#}")))
    }

    async fn release_act(
        self: Arc<Self>,
        name: &str,
        params: Value,
        surface: &str,
    ) -> anyhow::Result<Value> {
        Ok(match name {
            method::EXCHANGE_OFFER => {
                let p: OfferParams = serde_json::from_value(params)?;
                return self.clone().release_offer(&p.message).await;
            }
            method::EXCHANGE_RELEASE => {
                let p: ReleaseParams = serde_json::from_value(params)?;
                let moved = crate::view_gate::Moved::read(&self.runner, None);
                let held = |ex: &crate::exchange::Exchange| {
                    self.ladder_hidden(&ex.session_id) || moved.holds(ex)
                };
                let r = crate::release::grant(&self.store, &p.exchange, &p.to, surface, &held)?;
                serde_json::json!({"release": r, "label": crate::release::LABEL})
            }
            method::EXCHANGE_WITHDRAW => {
                let p: WithdrawParams = serde_json::from_value(params)?;
                let e = crate::release::withdraw(&self.store, &p.id, surface)?;
                serde_json::json!({"ended": e, "note": crate::release::UNSENT})
            }
            _ => {
                let all = crate::release::all(&self.store)?;
                let rows: Vec<Value> = all
                    .into_iter()
                    .map(|(r, end)| serde_json::json!({"release": r, "ended": end}))
                    .collect();
                serde_json::json!({"releases": rows})
            }
        })
    }

    /// What the release card's look would offer for the owner's message `node` in company: its place, the
    /// outcome, and each candidate's words (shown to him alone, at his own terminal).
    async fn release_offer(self: Arc<Self>, node: &str) -> anyhow::Result<Value> {
        let mark = crate::exchange::mark(&self.store, node)?
            .ok_or_else(|| anyhow::anyhow!("no message {node} with the owner's mark"))?;
        if mark.person().as_deref() != Some(crate::exchange::OWNER) {
            anyhow::bail!("message {node} is not the owner's");
        }
        let (_, n) = self
            .store
            .get_node(node)?
            .ok_or_else(|| anyhow::anyhow!("no message {node}"))?;
        let crate::node::Body::UserMessage { text, .. } = &n.body else {
            anyhow::bail!("{node} is not a message");
        };
        let look = crate::release_card::look(&self, text).await;
        let candidates: Vec<Value> = look
            .ids
            .iter()
            .filter_map(|t| crate::exchange::exchange(&self.store, t).ok().flatten())
            .map(|ex| {
                let words = ex
                    .ask
                    .as_deref()
                    .and_then(|a| self.store.get_node(a).ok().flatten())
                    .map(|(_, n)| crate::recall::text_of(&n))
                    .unwrap_or_default();
                serde_json::json!({"exchange": ex.turn_id, "from": ex.place,
                                   "at_ms": ex.start_ms, "words": words})
            })
            .collect();
        Ok(serde_json::json!({
            "message": node, "place": mark.place, "outcome": look.outcome,
            "found": look.found, "candidates": candidates,
        }))
    }
}
