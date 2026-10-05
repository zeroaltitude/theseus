//! Backfill (M5 25d; design §2.9): a pack's judged points rebuilt from the
//! recorded history since a date, and judged in shadow, so the first holdout
//! exists on day one.
//!
//! - **Consent.** A backfill sends the owner's history to Jev, so it runs
//!   only under his recorded consent, `[judge] backfill_consent = true`, a
//!   line of his config note, which agents can't write (§2.7). Without it
//!   the run is refused, naming the line. Each run's `judge.backfill` row
//!   records the consent it ran under: the running config's sha256.
//! - **The events** are the points the live pack would have judged: for
//!   `loop.v1`, each turn the baseline ended with no tool calls (its
//!   `turn.ended` row), in the pack's shadow sample. Each input is rebuilt
//!   with the live point's own input function at the event's time as `now`
//!   (`rebuild`); a pack whose input the store can't rebuild is refused with
//!   the reason.
//! - **Judged in shadow**, with the live point's context fields (session,
//!   execution, turn, the baseline's decision, the class), so 25c's system
//!   labels reach them, plus `purpose: backfill`, the run, and
//!   `event_at_ms`, which the report's holdout split reads as the
//!   judgment's time. Each is keyed by its event (the pack, session, and
//!   turn hashed), so an event that version judged (live, or by an earlier
//!   backfill) is skipped, and a second run writes nothing.
//! - **Spend** is a replay's: its estimate is checked first against
//!   `[judge] replay_limit_usd`, and refused with the numbers.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::json;
use theseus_judge::price::{micros_to_usd, usd_to_micros};
use theseus_judge::{Ask, Input, Judgment, Mode, Pack};
use theseus_protocol::judge_runs::{JudgeBackfillParams, JudgeBackfillResult, ReplayLeftOut};
use theseus_store::NewRecord;

use super::audit::pack_named;
use super::read_scope;
use super::rebuild::{loop_input, unrebuildable, TurnEnd};
use super::replay::Caller;
use crate::fact;
use crate::node::Node;
use crate::rpc::Core;

/// An event's judgment id: the pack and the event hashed, so a backfill of
/// the same event is the same judgment.
pub fn event_id(pack: &str, session: &str, turn: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for part in [pack, "backfill", session, turn] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    let d = h.finalize();
    let hex: String = d[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("jdg_{hex}")
}

/// The local midnight that begins `date` (`2026-10-01`).
pub fn day_start(date: &str) -> anyhow::Result<u64> {
    let bad = || anyhow::anyhow!("--since is a local day such as 2026-10-01, not {date:?}");
    let mut it = date.trim().splitn(3, '-');
    let (y, m, d) = (
        it.next()
            .and_then(|x| x.parse::<i64>().ok())
            .ok_or_else(bad)?,
        it.next()
            .and_then(|x| x.parse::<i64>().ok())
            .ok_or_else(bad)?,
        it.next()
            .and_then(|x| x.parse::<i64>().ok())
            .ok_or_else(bad)?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return Err(bad());
    }
    // Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let utc = (days * 86_400_000) as u64;
    let offset = crate::wake::local(utc).offset_secs * 1000;
    Ok((utc as i64 - offset).max(0) as u64)
}

impl Core {
    /// `judge.backfill`: the owner's run, from a private place
    /// (`judge_act(Act::JudgeRun)`), under his recorded consent, on a thread
    /// of its own at low priority.
    pub async fn judge_backfill(
        self: &Arc<Self>,
        p: JudgeBackfillParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<JudgeBackfillResult> {
        let who = who.into();
        if !self.cfg.judge.enabled {
            bail!("the judge is off ([judge] enabled = false): nothing is backfilled");
        }
        let pack = pack_named(&p.pack)?;
        let since = day_start(&p.since)?;
        let what = format!("backfill of {} since {}", pack.name(), p.since.trim());
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::JUDGE_BACKFILL,
                what: &what,
            },
        )?;
        if !self.cfg.judge.backfill_consent {
            bail!(
                "a backfill sends your recorded history to Jev, so it runs only under your \
                 consent: `backfill_consent = true` under [judge] in your config note, which \
                 agents can't write. Nothing was sent."
            );
        }
        if let Some(why) = unrebuildable(pack.builder) {
            bail!("{} can't be backfilled: {why}", pack.name());
        }
        let core = Arc::downgrade(self);
        let rt = tokio::runtime::Handle::current();
        let (who_s, via) = (who.who(), who.via());
        let rx = super::tender::on_low_thread(move || match core.upgrade() {
            Some(c) => c.backfill_run(&rt, &pack, since, &who_s, &via),
            None => Err(anyhow::anyhow!(
                "the daemon stopped before the backfill ran"
            )),
        })?;
        rx.await.context("the backfill's thread ended")?.1
    }

    /// The events `pack`'s live point would have judged since `since_ms`:
    /// `loop.v1`'s turns the baseline ended with no tool calls, in its shadow
    /// sample.
    pub(crate) fn backfill_events(
        &self,
        pack: &Pack,
        since_ms: u64,
    ) -> anyhow::Result<Vec<TurnEnd>> {
        let share = self.cfg.judge.sample_of(&pack.name(), pack.sample);
        Ok(self
            .turn_ends(None, since_ms)?
            .into_iter()
            .filter(|e| e.stop_reason == "no_tool_calls")
            .filter(|e| crate::judge::sampled(&e.turn_id, share))
            .collect())
    }

    /// Each event's state, built as the live point builds it, from the
    /// session's nodes at the event's time.
    pub(crate) fn backfill_state(
        &self,
        pack: &Pack,
        e: &TurnEnd,
        nodes: &[Node],
    ) -> Result<theseus_judge::Prepared, String> {
        let input = Input::Loop(loop_input(nodes, e, self.is_task_session(&e.session_id)));
        theseus_judge::prepare(pack, &input, &self.runner.judge.scrub()).map_err(|x| x.to_string())
    }

    fn backfill_run(
        &self,
        rt: &tokio::runtime::Handle,
        pack: &Arc<Pack>,
        since_ms: u64,
        who: &str,
        via: &str,
    ) -> anyhow::Result<JudgeBackfillResult> {
        let _rt = rt.enter();
        let id = crate::new_id("bkf");
        let until_ms = theseus_protocol::now_unix_ms();
        let events = self.backfill_events(pack, since_ms)?;
        // What that version judged: live ones by their turn, backfilled ones
        // by their key.
        let mut scope = read_scope(&self.store, &pack.id)?;
        let live: BTreeSet<String> = scope
            .judgments
            .remove(&pack.name())
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.context("purpose") != Some("backfill"))
            .filter_map(|s| s.context("turn").map(str::to_string))
            .collect();
        let mut r = JudgeBackfillResult {
            id: id.clone(),
            pack: pack.name(),
            since_ms,
            until_ms,
            consent: crate::config_copy::sha256(&serde_json::to_string(&*self.cfg)?),
            events: events.len() as u32,
            limit_usd: self.cfg.judge.replay_limit_usd,
            ..Default::default()
        };
        let mut nodes: HashMap<String, Vec<Node>> = HashMap::new();
        let mut asks: Vec<Ask> = Vec::new();
        for e in &events {
            let key = event_id(&pack.name(), &e.session_id, &e.turn_id);
            if live.contains(&e.turn_id) || self.store.ledger_by_key(&key)?.is_some() {
                r.already += 1;
                continue;
            }
            let ns = nodes.entry(e.session_id.clone()).or_insert_with(|| {
                self.store
                    .session_nodes(&e.session_id)
                    .map(|v| v.into_iter().map(|(_, n)| n).collect())
                    .unwrap_or_default()
            });
            let prepared = match self.backfill_state(pack, e, ns) {
                Ok(p) => p,
                Err(reason) => {
                    r.left_out.push(ReplayLeftOut {
                        judgment: key,
                        reason,
                    });
                    continue;
                }
            };
            let blob = self.store.blobs().put(prepared.state.json.as_bytes())?;
            let task = self.is_task_session(&e.session_id);
            let context = json!({
                "session": e.session_id, "turn": e.turn_id, "loops": e.loops,
                "baseline": "until_no_tool_calls", "decision": e.stop_reason,
                "class": crate::judge::loop_end::class(task, e.tool_calls), "blob": blob,
                "on_path_ms": 0, "purpose": "backfill", "run": id, "event_at_ms": e.at_ms,
            });
            let mut ask = Ask::new(pack.clone(), &prepared, Mode::Shadow, context);
            ask.id = Some(key);
            asks.push(ask);
        }
        let built = self.runner.judge.jev()?;
        let mut caller = Caller::new(rt, &built, usd_to_micros(self.cfg.judge.replay_limit_usd));
        let estimate = caller.estimate(asks.iter())?;
        r.estimate_usd = micros_to_usd(estimate);
        if estimate > caller.limit {
            bail!(
                "the backfill of {} over {} events would reserve {}, past [judge] \
                 replay_limit_usd ({}); nothing was sent",
                pack.name(),
                asks.len(),
                crate::narrative::dollars(estimate),
                crate::narrative::dollars(caller.limit)
            );
        }
        for ask in asks {
            let key = ask.id.clone().unwrap_or_default();
            match caller.ask(ask) {
                Some(j) if j.outcome == theseus_judge::Outcome::Answered => r.judged += 1,
                Some(_) => r.failed += 1,
                None => r.left_out.push(ReplayLeftOut {
                    judgment: key,
                    reason: "the run reached [judge] replay_limit_usd".into(),
                }),
            }
        }
        r.cost_usd = micros_to_usd(caller.spent);
        if !caller.called.is_empty() {
            self.write_backfill(pack, &r, &caller.called, who, via)?;
        }
        Ok(r)
    }

    /// The run's frame: each judgment's `judge.call` row, scoped
    /// `judge:<pack id>` where the report reads it, and the `judge.backfill`
    /// row.
    fn write_backfill(
        &self,
        pack: &Pack,
        r: &JudgeBackfillResult,
        called: &[Judgment],
        who: &str,
        via: &str,
    ) -> anyhow::Result<()> {
        let scope = crate::rpc::judge::scope_of(&pack.id);
        let mut records: Vec<NewRecord> = Vec::new();
        for j in called {
            let f = fact::judge::JudgeCall {
                judgment: j,
                budget: "backfill",
            };
            let session = j.context.get("session").and_then(|v| v.as_str());
            let mut rec = fact::row(&f, session, None)?;
            rec.key = Some(j.id.clone());
            records.push(rec.scoped(&scope));
        }
        let f = fact::judge_runs::JudgeBackfilled {
            result: r,
            who,
            via,
        };
        let mut rec = fact::row(&f, None, None)?;
        rec.key = Some(r.id.clone());
        records.push(rec.scoped(&scope));
        self.store.append(&records)?;
        self.rec(None).announce(&f);
        Ok(())
    }
}
