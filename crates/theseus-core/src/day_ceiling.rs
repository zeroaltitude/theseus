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
//!   Under `daily_spend_ceiling_mode = "notify"` (theseus-9e02) the ceiling
//!   refuses nothing: the refusal is the runaway line's, with its own words,
//!   and the ceiling's own mark is a warning (`TurnRunner::day_warned`, in
//!   `turn/ceiling_step.rs`): a `spend.day_warning` row, the owner's DM and a
//!   narrative line, once at the ceiling and once at each step after it.
//!
//! - **The stored total** (theseus-vqcgq): the read above is today's rows up to
//!   a ledger position, kept in the META record `day_ceiling.total` (`Total`):
//!   spent, the warning level, each distinct limit's `spend.ceiling` time. A
//!   start reads only the rows past that position and stores it again, and
//!   `Core::keep_day_total` does so every minute, so a start reads about a
//!   minute's rows however long the day's ledger is. No usable total (none,
//!   another day, another rule or kinds list, a log shorter than it) makes
//!   one full read, once. `read_today` is that full read; the two must agree.
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
//!   people's extractor (`people.extracted`, theseus-wy7y), and a thread
//!   import's cards (`thread.imported`, theseus-71ib), and the card's observer
//!   (`card.observed`, theseus-6f8t0), and the keep-warm reads (`keep_warm`,
//!   theseus-ezeg, booked to their sessions).

use std::time::Instant;

use crate::ledger::LedgerRow;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_kernel::{usd_to_micros, Micros, Reached, Seed};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::fact::limits::DayCeilingReached;
use crate::store::Store;
use crate::turn::TurnRunner;

/// The kinds whose rows of today carry model spend (the module's list).
pub const SPEND_KINDS: [&str; 17] = [
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
    "thread.imported",
    "card.proposed",
    "card.observed",
    "keep_warm",
    "initiative.spoke",
    "onto.named",
];

/// The once-a-day row's kind.
pub const CEILING_KIND: &str = "spend.ceiling";

/// The warning's row kind (theseus-9e02): one a level a day.
pub const WARNING_KIND: &str = "spend.day_warning";

/// What the start's read found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seeded {
    pub seed: Seed,
    /// Rows decoded.
    pub rows: u64,
    /// The read waited for the index's shape: it lands after serving.
    pub later: bool,
    /// The read began from the stored total (`read_today_total`): `rows` are
    /// the rows past it.
    pub resumed: bool,
}

/// A row's model spend, by its kind: `None` for a row that is not.
pub(crate) fn spend_of(row: &LedgerRow) -> Option<Micros> {
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

/// The stored total's key (a META record): today's spend read so far, up to
/// a ledger position, so a start reads only the rows past it (theseus-vqcgq).
pub const TOTAL_KEY: &str = "day_ceiling.total";

/// Bumped when `spend_of` or the fold changes what a row counts: a total
/// written under another rule is not trusted, and the start scans once.
const TOTAL_RULE: u32 = 1;

/// A `spend.ceiling` row today: the limit it was written under, and when.
/// One entry a distinct limit (the earliest time), since whether a row is
/// today's stop depends on the limit the daemon starts with.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CeilingAt {
    limit_usd: Option<f64>,
    at_ms: u64,
}

/// Today's rows folded: what a full read of the day would have found, up to
/// ledger position `through`. Stored in META, extended by the rows past it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Total {
    rule: u32,
    /// `SPEND_KINDS`, as the build that wrote this read them: a kind added
    /// since is not in the sum.
    kinds: String,
    day: String,
    since_ms: u64,
    /// The newest position the fold has seen of the kinds it reads: the
    /// next read pages after it. `None`: nothing read yet.
    through: Option<u64>,
    spent: Micros,
    warned: u64,
    ceilings: Vec<CeilingAt>,
}

impl Total {
    fn new(since_ms: u64, day: &str) -> Self {
        Self {
            rule: TOTAL_RULE,
            kinds: SPEND_KINDS.join(","),
            day: day.to_string(),
            since_ms,
            ..Self::default()
        }
    }

    /// Whether this stored total is today's under this build's rules, and
    /// not ahead of the log (a store restored from an older copy).
    fn fits(&self, since_ms: u64, day: &str, last_position: u64) -> bool {
        self.rule == TOTAL_RULE
            && self.kinds == SPEND_KINDS.join(",")
            && self.day == day
            && self.since_ms == since_ms
            && self.through.is_none_or(|t| t <= last_position)
    }

    /// One row of the day, as the full read counts it.
    fn fold(&mut self, row: &LedgerRow) {
        // The kind's clock may hold a row a minute early: its own time decides.
        if row.at_unix_ms < self.since_ms {
            return;
        }
        if row.kind == WARNING_KIND {
            if row.data["day"] == self.day.as_str() {
                let level = row.data["level"].as_u64().unwrap_or(1);
                self.warned = self.warned.max(level);
            }
            return;
        }
        if row.kind == CEILING_KIND {
            if row.data["day"] == self.day.as_str() {
                let limit_usd = row.data["limit_usd"].as_f64();
                match self.ceilings.iter_mut().find(|c| c.limit_usd == limit_usd) {
                    Some(c) => c.at_ms = c.at_ms.min(row.at_unix_ms),
                    None => self.ceilings.push(CeilingAt {
                        limit_usd,
                        at_ms: row.at_unix_ms,
                    }),
                }
            }
            return;
        }
        if let Some(m) = spend_of(row) {
            self.spent = self.spent.saturating_add(m);
        }
    }

    /// The seed for a start under `limit`. A row written under a lower
    /// ceiling than `limit` is not today's stop: the owner raised the
    /// ceiling and restarted, which lifts it, and a refusal at the new one
    /// is said again.
    fn seed(&self, limit: Micros) -> Seed {
        let reached = self
            .ceilings
            .iter()
            .filter(|c| c.limit_usd.is_none_or(|l| usd_to_micros(l) >= limit))
            .map(|c| c.at_ms)
            .min();
        Seed {
            spent: self.spent,
            warned: self.warned,
            reached_at_ms: reached,
        }
    }
}

/// The tags of the kinds the day's read pages by.
fn day_tags() -> Vec<String> {
    let mut tags: Vec<String> = SPEND_KINDS.iter().map(|k| ledger_kind(k)).collect();
    tags.push(ledger_kind(CEILING_KIND));
    tags.push(ledger_kind(WARNING_KIND));
    tags
}

/// Folds the day's rows into `t`: every one from midnight (`resume` is
/// `None`, newest page first), or only those past `resume` (oldest first).
/// Returns the rows decoded, or `None` while the index's shape is built.
fn scan_day(store: &Store, t: &mut Total, resume: Option<u64>) -> anyhow::Result<Option<u64>> {
    let tags = day_tags();
    let (mut rows, mut cursor) = (0u64, resume);
    let mut through = t.through;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: cursor.filter(|_| resume.is_some()),
            before: cursor.filter(|_| resume.is_none()),
            since_ms: Some(t.since_ms),
            until_ms: None,
            limit: 512,
        };
        let Some(got) = store.ledger_page(&page)? else {
            return Ok(None);
        };
        through = through.max(got.last);
        for r in &got.records {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            rows += 1;
            t.fold(&row);
        }
        match (resume.is_some(), got.more, got.first, got.last) {
            (true, true, _, Some(last)) => cursor = Some(last),
            (false, true, Some(first), _) => cursor = Some(first),
            _ => break,
        }
    }
    t.through = through;
    Ok(Some(rows))
}

/// The judge's day budget keeps today's settled spend in one record.
fn judge_spent(store: &Store, day: &str) -> Micros {
    match store.get_meta::<crate::judge::spend::Stored>(crate::judge::spend::META_KEY) {
        Ok(Some(j)) if j.day == day => j.spent_micros,
        _ => 0,
    }
}

fn seeded(t: &Total, rows: u64, resumed: bool, store: &Store, day: &str, limit: Micros) -> Seeded {
    let mut seed = t.seed(limit);
    seed.spent = seed.spent.saturating_add(judge_spent(store, day));
    Seeded {
        seed,
        rows,
        later: false,
        resumed,
    }
}

/// Today's spend and its `spend.ceiling` row, from `since_ms` (local
/// midnight) on, by a full read of the day's rows: `None` while the index's
/// shape is built. The start does not use it (`read_today_total`): it is the
/// full read the stored total must equal.
pub fn read_today(
    store: &Store,
    since_ms: u64,
    day: &str,
    limit: Micros,
) -> anyhow::Result<Option<Seeded>> {
    let mut t = Total::new(since_ms, day);
    Ok(scan_day(store, &mut t, None)?.map(|rows| seeded(&t, rows, false, store, day, limit)))
}

/// Today's spend as `read_today` finds it, without reading the day's rows
/// again (theseus-vqcgq): the stored total (`TOTAL_KEY`) is today's rows up
/// to a position, and only the rows past it are read and folded in. With no
/// usable total (none yet, an older build's store, another day, or another
/// rule) one full read makes it. The total is stored again when rows were
/// added to it, so a start's cost follows the rows written since the last
/// one, and `keep_day_total` keeps that to a minute's.
pub fn read_today_total(
    store: &Store,
    since_ms: u64,
    day: &str,
    limit: Micros,
) -> anyhow::Result<Option<Seeded>> {
    let Some((t, rows, resumed)) = advance_total(store, since_ms, day)? else {
        return Ok(None);
    };
    Ok(Some(seeded(&t, rows, resumed, store, day, limit)))
}

/// The stored total, read forward to the log's end and stored again if that
/// moved it: the total, the rows read, and whether it began from a stored one.
fn advance_total(
    store: &Store,
    since_ms: u64,
    day: &str,
) -> anyhow::Result<Option<(Total, u64, bool)>> {
    let stored = match store.get_meta::<Total>(TOTAL_KEY) {
        Ok(Some(t)) if t.fits(since_ms, day, store.last_position()) => Some(t),
        _ => None,
    };
    let resumed = stored.as_ref().is_some_and(|t| t.through.is_some());
    let mut t = stored.unwrap_or_else(|| Total::new(since_ms, day));
    let was = t.through;
    let resume = t.through.filter(|_| resumed);
    let Some(rows) = scan_day(store, &mut t, resume)? else {
        return Ok(None);
    };
    if t.through != was {
        if let Err(e) = store.put_meta(TOTAL_KEY, &t) {
            tracing::warn!(error = %format!("{e:#}"), "day ceiling: today's total was not stored");
        }
    }
    Ok(Some((t, rows, resumed)))
}

impl TurnRunner {
    /// The start's read of today (before serving): seeds the kernel's
    /// counter, and says what it read and how long it took.
    pub fn seed_day_ceiling(&self) -> Seeded {
        let c = self.kernel.day_ceiling();
        let now = c.now();
        let (day, since, _) = c.day_of(now);
        let t = Instant::now();
        match read_today_total(&self.store, since, &day, c.bar()) {
            Ok(Some(s)) => {
                c.seed(now, &s.seed);
                tracing::info!(day = %day, spent_usd = theseus_kernel::micros_to_usd(s.seed.spent),
                    rows = s.rows, resumed = s.resumed, reached = s.seed.reached_at_ms.is_some(),
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

    /// Reads the rows written since the stored total and stores it again, so
    /// the next start's read is the rows written after this one (and the day
    /// turning starts a new total). Returns the rows read, or `None` while
    /// the index's shape is built.
    pub fn advance_day_total(&self) -> Option<u64> {
        let c = self.kernel.day_ceiling();
        let (day, since, _) = c.day_of(c.now());
        match advance_total(&self.store, since, &day) {
            Ok(Some((_, rows, _))) => Some(rows),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "day ceiling: today's total was not advanced");
                None
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
        _at_ms: u64,
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
        let text = match r.runaway {
            true => runaway_text(r, what),
            false => notice_text(r, what),
        };
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

/// The owner's post: what stopped, today's total, the ceiling, when the day
/// turns, that a refused turn or task ends (it does not wait), and that a
/// higher ceiling applies at the next start (theseus-l9on).
pub fn notice_text(r: &Reached, what: &str) -> String {
    stopped_text(r, what, "daily ceiling", "daily_spend_ceiling_usd")
}

/// The owner's post when the runaway line stopped a call (theseus-9e02): the
/// day's total, the line, when the day turns, and that a higher line applies
/// at the next start.
pub fn runaway_text(r: &Reached, what: &str) -> String {
    stopped_text(r, what, "daily runaway line", "daily_runaway_usd")
}

/// The two stops' words (theseus-x8b9k): one head, the line's name and key.
fn stopped_text(r: &Reached, what: &str, line: &str, key: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        "Model calls are stopped for today",
        &format!(
            "Today's model spend reached the {} {line} ({} spent).",
            crate::narrative::dollars(r.limit),
            crate::narrative::dollars(r.spent.saturating_add(r.held)),
        ),
        &format!(
            "A {what}'s call was not made, and no model call will be, however small, until the day \
             turns at {} local time. A refused turn or task ends; it does not wait for the day to \
             turn. A higher `[kernel] {key}` applies at the daemon's next start.",
            r.turns_at,
        ),
    )
}

/// The owner's warning (theseus-9e02): what was spent today, the ceiling and
/// the multiple, that the work goes on, where the stop is, and how to stop now.
pub fn warning_text(w: &theseus_kernel::Warning, stop_now: &str) -> String {
    use crate::narrative::dollars;
    let times = w.spent as f64 / w.limit.max(1) as f64;
    let next = match w.step {
        0 => String::new(),
        step => {
            let at = w.limit.saturating_add(step.saturating_mul(w.level));
            match w.runaway > 0 && at >= w.runaway {
                true => String::new(),
                false => format!(" The next warning comes at {}.", dollars(at)),
            }
        }
    };
    let stop = match w.runaway {
        0 => " No runaway line is set (`[kernel] daily_runaway_usd = 0`).".to_string(),
        line => format!(
            " Model calls stop at the {} runaway line (`[kernel] daily_runaway_usd`), and not \
             before, until the day turns at {} local time.",
            dollars(line),
            w.turns_at
        ),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        "Today's spend passed the daily ceiling, and the work goes on",
        &format!(
            "Today's model spend is {}, {times:.1}× the {} daily ceiling.",
            dollars(w.spent),
            dollars(w.limit),
        ),
        &format!(
            "The ceiling warns (`[kernel] daily_spend_ceiling_mode = \"notify\"`).{stop}{next} To \
             stop now: {stop_now}."
        ),
    )
}

/// How often `keep_day_total` stores today's total again.
const TOTAL_EVERY_SECS: u64 = 60;

impl crate::rpc::Core {
    /// Keeps the stored total near the log's end while the daemon runs
    /// (theseus-vqcgq), so a restart's read is a minute's rows, not the
    /// day's: a thread of its own, in the idle class, holding the core by
    /// `Weak`, which reads the rows written since its last pass.
    pub fn keep_day_total(self: &std::sync::Arc<Self>) {
        let weak = std::sync::Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("day-total".into())
            .spawn(move || {
                let _ = theseus_store::pressure::idle_this_thread();
                loop {
                    for _ in 0..TOTAL_EVERY_SECS {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        if weak.strong_count() == 0 {
                            return;
                        }
                    }
                    let Some(core) = weak.upgrade() else { return };
                    core.runner.advance_day_total();
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "day ceiling: today's total is not kept");
        }
    }

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
                        core.keep_day_total();
                        return;
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "day ceiling: today's read was not started");
        }
    }
}
