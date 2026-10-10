//! The daemon's day ceiling, the core's side (theseus-kp20). The counter is
//! the kernel's (`theseus_kernel::day_ceiling`): a provider call's
//! reservation holds on it in the kernel's plan, and this module gives the
//! rest of the daemon its part:
//!
//! - **The start's read** (`TurnRunner::seed_day_ceiling`, before serving):
//!   today's rows that carry model spend, read once through the ledger's
//!   pages by kind from local midnight (a window of time is one stretch of
//!   positions), and today's `spend.ceiling` row, so a restart never resets
//!   the day and never posts its notice twice. While the index's shape is
//!   built (an older build wrote last) the pages are not there: the read is
//!   then made after serving, once they are, and until it lands the day
//!   counts from this start alone (`Seeded::later`).
//! - **A refusal** (`TurnRunner::day_refused`): the day's first writes one
//!   `spend.ceiling` row and, when a DM with the owner is bound, one post
//!   there (never a shared place), in one frame. Every refusal is logged.
//!
//! What counts, by source, read back at the start from:
//! - a turn's and a task's provider calls, compaction's summary among them
//!   (`action.succeeded`, `action.failed` and `action.outcome_unknown` rows
//!   whose producer is a provider's, or whose cost is a booked reservation);
//!   a task's cost carried to its parent's budget writes no row of its own,
//!   so it counts once;
//! - speech (`speech.transcribed`, `speech.synthesized`, `Kernel::book_spend`);
//! - the judge's point judgments (its day budget's META record, `judge.budget`,
//!   whose `spent_micros` is today's when its `day` is);
//! - consolidation (`synthesis.proposed`), and the owner's learning runs
//!   (`judge.replay`, `judge.audit`, `judge.backfill`'s `cost_usd`), and the
//!   learning loop's writer (`judge.proposal`'s `writer_usd`), and the
//!   people's extractor (`people.extracted`, theseus-wy7y), and the keep-warm
//!   reads (`keep_warm`, theseus-ezeg, booked to their sessions).

use std::time::Instant;

use crate::ledger::LedgerRow;
use serde_json::json;
use theseus_kernel::{usd_to_micros, Micros, Reached, Seed};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::fact::limits::DayCeilingReached;
use crate::store::Store;
use crate::turn::TurnRunner;

/// The kinds whose rows of today carry model spend (the module's list).
pub const SPEND_KINDS: [&str; 12] = [
    "action.succeeded",
    "action.failed",
    "action.outcome_unknown",
    "speech.transcribed",
    "speech.synthesized",
    "synthesis.proposed",
    "judge.replay",
    "judge.audit",
    "judge.backfill",
    "judge.proposal",
    "people.extracted",
    "keep_warm",
];

/// The once-a-day row's kind.
pub const CEILING_KIND: &str = "spend.ceiling";

/// What the start's read found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seeded {
    pub seed: Seed,
    /// Rows decoded.
    pub rows: u64,
    /// The read waited for the index's shape: it lands after serving.
    pub later: bool,
}

/// A row's model spend, by its kind: `None` for a row that is not.
fn spend_of(row: &LedgerRow) -> Option<Micros> {
    let x = &row.data;
    let cost = match row.kind.as_str() {
        "judge.proposal" => x["writer_usd"].as_f64()?,
        _ => x["cost_usd"].as_f64()?,
    };
    let model = match row.kind.as_str() {
        "action.succeeded" | "action.failed" | "action.outcome_unknown" => {
            x["producer"]
                .as_str()
                .is_some_and(|p| p.starts_with("provider:"))
                || x["cost_basis"] == "reservation"
        }
        _ => true,
    };
    model.then(|| usd_to_micros(cost.max(0.0)))
}

/// Today's spend and its `spend.ceiling` row, from `since_ms` (local
/// midnight) on: `None` while the index's shape is built. A row written
/// under a lower ceiling than `limit` is not today's stop: the owner raised
/// the ceiling and restarted, which lifts it, and a refusal at the new one
/// is said again.
pub fn read_today(
    store: &Store,
    since_ms: u64,
    day: &str,
    limit: Micros,
) -> anyhow::Result<Option<Seeded>> {
    let mut tags: Vec<String> = SPEND_KINDS.iter().map(|k| ledger_kind(k)).collect();
    tags.push(ledger_kind(CEILING_KIND));
    let mut out = Seeded::default();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: None,
            limit: 512,
        };
        let Some(got) = store.ledger_page(&page)? else {
            return Ok(None);
        };
        for r in &got.records {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            out.rows += 1;
            // The kind's clock may hold a row a minute early: its own time decides.
            if row.at_unix_ms < since_ms {
                continue;
            }
            if row.kind == CEILING_KIND {
                let under = row.data["limit_usd"]
                    .as_f64()
                    .is_none_or(|l| usd_to_micros(l) >= limit);
                if row.data["day"] == day && under {
                    let at = out.seed.reached_at_ms.get_or_insert(row.at_unix_ms);
                    *at = (*at).min(row.at_unix_ms);
                }
                continue;
            }
            if let Some(m) = spend_of(&row) {
                out.seed.spent = out.seed.spent.saturating_add(m);
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => break,
        }
    }
    // The judge's day budget keeps today's settled spend in one record.
    if let Ok(Some(j)) =
        store.get_meta::<crate::judge::spend::Stored>(crate::judge::spend::META_KEY)
    {
        if j.day == day {
            out.seed.spent = out.seed.spent.saturating_add(j.spent_micros);
        }
    }
    Ok(Some(out))
}

impl TurnRunner {
    /// The start's read of today (before serving): seeds the kernel's
    /// counter, and says what it read and how long it took.
    pub fn seed_day_ceiling(&self) -> Seeded {
        let c = self.kernel.day_ceiling();
        let now = c.now();
        let (day, since, _) = c.day_of(now);
        let t = Instant::now();
        match read_today(&self.store, since, &day, c.limit()) {
            Ok(Some(s)) => {
                c.seed(now, &s.seed);
                tracing::info!(day = %day, spent_usd = theseus_kernel::micros_to_usd(s.seed.spent),
                    rows = s.rows, reached = s.seed.reached_at_ms.is_some(),
                    took_us = t.elapsed().as_micros() as u64, "day ceiling: today's spend read");
                s
            }
            Ok(None) => Seeded {
                later: true,
                ..Seeded::default()
            },
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "day ceiling: today's spend was not read; the day counts from this start");
                Seeded::default()
            }
        }
    }

    /// Hold `need` on the day for a background call of `what` (consolidation,
    /// a learning run): the hold, settled at the call's cost; or, refused,
    /// the refusal said (`day_refused`) and its words.
    pub fn day_hold(&self, need: Micros, what: &str) -> Result<theseus_kernel::Hold, String> {
        let c = self.kernel.day_ceiling();
        c.hold_call(c.now(), need).map_err(|r| {
            self.day_refused(&r, what, None);
            r.to_string()
        })
    }

    /// What a provider's answer cost: at its served model's prices, else the
    /// asked model's, else `need` (the reservation).
    pub fn priced(
        &self,
        resp: &crate::provider::ModelResponse,
        asked: &str,
        need: Micros,
    ) -> Micros {
        self.catalog
            .get(&resp.model)
            .or_else(|| self.catalog.get(asked))
            .map_or(need, |e| e.cost_micros(&resp.usage))
    }

    /// A call the day ceiling refused: logged, and the day's first writes its
    /// `spend.ceiling` row and the owner's post, in one frame.
    pub fn day_refused(&self, r: &Reached, what: &str, session_id: Option<&str>) {
        tracing::warn!(what, day = %r.day, spent_usd = theseus_kernel::micros_to_usd(r.spent),
            limit_usd = theseus_kernel::micros_to_usd(r.limit), "day ceiling: a model call was not made");
        let c = self.kernel.day_ceiling();
        if !c.take_notice(c.now()) {
            return;
        }
        let text = notice_text(r, what);
        let dm = self.place_rule.owner_dm_bound();
        let written = (|| -> anyhow::Result<()> {
            let mut frame = Vec::new();
            let post = match dm {
                true => {
                    let body = json!({"kind": "spend_ceiling", "text": text});
                    let (a, records) = self.outbox.stage_to_operator(session_id, body)?;
                    frame.extend(records);
                    Some(a)
                }
                false => None,
            };
            let f = DayCeilingReached {
                reached: r,
                what,
                posted: post.is_some(),
                text: &text,
            };
            frame.push(crate::fact::row(&f, session_id, None)?);
            self.store.append(&frame)?;
            if let Some(a) = post {
                self.outbox.posted(&a);
            }
            Ok(())
        })();
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "day ceiling: the day's notice was not written");
        }
    }
}

/// The owner's post: what stopped, today's total, the ceiling, the key, and
/// when the day turns.
pub fn notice_text(r: &Reached, what: &str) -> String {
    format!(
        "⛔ Today's model spend reached the {} daily ceiling ({} spent): a {what}'s call was \
         not made, and no model call will be until the day turns at {} local time. Raise \
         `[kernel] daily_spend_ceiling_usd` to go on sooner.",
        crate::narrative::dollars(r.limit),
        crate::narrative::dollars(r.spent.saturating_add(r.held)),
        r.turns_at,
    )
}

impl crate::rpc::Core {
    /// The start's read, after serving, once the index's shape is built: on
    /// a thread of its own, holding the core by `Weak`, trying every second
    /// for up to an hour.
    pub fn seed_day_ceiling_later(self: &std::sync::Arc<Self>) {
        let weak = std::sync::Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("day-ceiling".into())
            .spawn(move || {
                for _ in 0..3600 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let Some(core) = weak.upgrade() else { return };
                    if !core.runner.seed_day_ceiling().later {
                        return;
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "day ceiling: today's read was not started");
        }
    }
}
