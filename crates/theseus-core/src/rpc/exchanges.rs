//! The exchange ledger at the RPC layer (issue 44, step 1; `crate::exchange`):
//! the owner's mark of a submitted message, made where the live proof is, and
//! `exchange.report` and `exchange.rebuild`, which are read and asked from the
//! terminal or the web UI alone; and `exchange.topics` (issue 46), the owner's
//! act in person (`topics_judged`).

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::exchange::{
    ExchangeLadderParams, ExchangeLadderResult, ExchangeRebuildParams, ExchangeReportParams,
    ExchangeTopicsParams,
};
use theseus_protocol::{error_code, method, TurnSubmitParams};

use super::server::{parse, Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::{Refusal, Surface};
use crate::exchange::{ledger, rebuild, OwnerMark, Peer};

/// The methods `rpc_exchanges` answers.
pub(super) const OWN: &[&str] = &[
    method::EXCHANGE_REPORT,
    method::EXCHANGE_REBUILD,
    method::EXCHANGE_LADDER,
    method::EXCHANGE_TOPICS,
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
        if p.prompt.is_some() {
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
        // Kept on 2026-11-22 for the two that act (`exchange.rebuild` writes
        // records, `exchange.ladder` spends on model calls): the surface rule
        // is their input. The report is a read, and every surface reads it.
        if name != method::EXCHANGE_REPORT && !conn.surface.reads_private() {
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
            method::EXCHANGE_TOPICS => {
                let p: ExchangeTopicsParams = parse(params)?;
                self.topics_judged(conn)?;
                let r = self.topics_history(&p).await.map_err(|e| {
                    match e.is::<crate::judge::onto::history::Busy>() {
                        true => RpcFailure::new(error_code::LIMIT, e.to_string()),
                        false => RpcFailure::invalid(e),
                    }
                })?;
                serde_json::to_value(r)
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

    /// `exchange.topics` is the owner's act in person: the people backfill's
    /// own check (`import_judged`: the owner, from a private place), then the
    /// narrower rule the ladder's spend takes (`places::owner_in_person_of`:
    /// never an agent's process at the terminal). Either refusal is ledgered
    /// and narrated, as any refused act is.
    fn topics_judged(&self, conn: Conn<'_>) -> Result<(), RpcFailure> {
        let what = "filing your history under topics";
        self.import_judged(conn, method::EXCHANGE_TOPICS, what)?;
        let who = conn.answerer(None, None);
        let Err(why) = crate::places::owner_in_person_of(
            &who,
            &self.runner.place_rule,
            &self.cfg,
            "filing your history under topics sends your old messages to the judge and spends \
             money",
            "your history is filed under topics",
        ) else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why,
        };
        let act = Act::Import {
            method: method::EXCHANGE_TOPICS,
            what,
        };
        self.refused(act, &who, &r).map_err(RpcFailure::invalid)?;
        Err(RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "{what} from {} does not count: {}. Nothing was asked.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        })
    }
}
