//! The daemon's side of the ladder: the job at an exchange's end (its `did`
//! line, then its place's fold step) in a place assembled as a view, where a
//! place's latest `/new` is, the person's lines that made a standing record,
//! and the build of a place's history ([`Core::ladder_build`]), with a dry
//! run that counts and prices, and a cap.

use std::sync::Arc;

use theseus_store::blocking;

use super::did::{self, Outcome};
use super::fold::Flags;
use super::read::{self, Step};
use super::writer::{Ask, ModelWriter, Work, Writer};
use super::DayPurse;
use super::FOLD_STEP;
use crate::exchange::{self, Root};
use crate::judge::categorize::ExchangeEnd;
use crate::judge::JudgeService;
use crate::rpc::Core;
use crate::view::Pos;

/// The person's lines a standing record was made from (by the record's
/// source), and those the judge's `classify` called a correction (the flag
/// its sink writes, [`super::corrected`]).
pub struct Standing<'a> {
    pub sources: Vec<String>,
    pub store: &'a crate::store::Store,
}

impl Flags for Standing<'_> {
    /// The message that opened `turn`, when the judge called it a correction.
    /// A flag that does not read is not a correction: the line is counted,
    /// never shown as more than it is.
    fn correction(&self, turn: &str, node: &str) -> bool {
        let ask = crate::exchange::exchange(self.store, turn)
            .ok()
            .flatten()
            .and_then(|e| e.ask);
        ask.as_deref() == Some(node) && super::corrected(self.store, turn).unwrap_or(false)
    }
    fn standing(&self, node: &str) -> bool {
        self.sources.iter().any(|s| s == node)
    }
}

impl JudgeService {
    /// An exchange's end: its `did` line and its place's fold step are
    /// written in a task of their own. Returns at once.
    pub fn ladder_at_exchange_end(&self, end: &ExchangeEnd, task: bool) {
        if task {
            return;
        }
        let Some(core) = self.core() else { return };
        if !core.ladder_writer {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let turn = end.turn_id.clone();
        let weak = Arc::downgrade(&core);
        drop(core);
        rt.spawn(async move {
            let Some(core) = weak.upgrade() else { return };
            if let Err(e) = core.ladder_after(&turn).await {
                tracing::warn!(turn = %turn, error = %format!("{e:#}"), "ladder: an exchange was not summarised");
            }
        });
    }
}

/// What a build found, would do or did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Built {
    pub dry_run: bool,
    pub place: String,
    /// The place's latest `/new`, if it has one.
    pub since: Option<Pos>,
    /// A place whose name holds the key separator: no ladder, nothing done.
    pub skipped: bool,
    /// The place's exchanges since its latest `/new`.
    pub exchanges: u64,
    /// Those with a line already, and those a line would be (was) written for.
    pub lines_had: u64,
    pub lines: u64,
    /// First-level nodes it would make (made); and the nodes above, by code.
    pub folds: u64,
    pub above: u64,
    /// The worst case of its calls, what is left of the day's line, whether
    /// the run would need leave to pass it, and what was spent (failed calls
    /// included).
    pub estimated_usd: f64,
    pub day_left_usd: f64,
    pub needs_over_day_line: bool,
    pub spent_usd: f64,
    pub stopped: Option<String>,
}

/// A build's money: the most it may spend, and leave to pass the day's line.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Allowance {
    pub cap_usd: Option<f64>,
    pub over_day_line: bool,
}

impl Core {
    /// Write `place`'s `/new` mark: from now, its ladder and window count only
    /// what comes after it. The position is now, and the place's newest
    /// exchange when it began in this same millisecond.
    pub fn ladder_mark_new(&self, place: &str) -> anyhow::Result<()> {
        let now = theseus_protocol::now_unix_ms();
        let newest = exchange::newest(&self.store, Root::Place(place), 1)?;
        let id = newest
            .first()
            .filter(|t| t.at_ms >= now)
            .map(|t| t.exchange.clone())
            .unwrap_or_default();
        let at = Pos {
            at_ms: now.max(newest.first().map_or(0, |t| t.at_ms)),
            id,
        };
        self.store.append(&[super::new_mark_record(place, &at)?])?;
        Ok(())
    }

    fn ladder_flags(&self) -> Standing<'_> {
        let all = self.runner.standing.all(&self.store).unwrap_or_default();
        Standing {
            sources: all.iter().map(|r| r.source.clone()).collect(),
            store: &self.store,
        }
    }

    /// The job at the end of exchange `turn`: only in a place whose requests
    /// are assembled as a view (`[context] assembly`), the one reader of what
    /// is written here; a place on the transcript spends nothing on it.
    async fn ladder_after(self: &Arc<Self>, turn: &str) -> anyhow::Result<()> {
        let Some(ex) = blocking(|| exchange::exchange(&self.store, turn))? else {
            return Ok(());
        };
        if !super::laddered(&ex.place) {
            return Ok(());
        }
        // A worker's turns (a task's, the self loop's) are not summarised: the day's purse is the owner's
        // conversations'. Its window is bounded instead (`read::ladder`), the rest reached by history.
        if blocking(|| crate::view_gate::worker(&self.runner, &ex.session_id)) {
            return Ok(());
        }
        let since = blocking(|| super::new_mark(&self.store, &ex.place))?;
        let writer = ModelWriter(self.clone());
        let purse = DayPurse::new(&self.store, None, false);
        match did::write(&self.store, &writer, turn, &purse).await? {
            Outcome::Failed { why, .. } => tracing::info!(turn, %why, "ladder: no did line"),
            Outcome::NotEnded => return Ok(()),
            _ => {}
        }
        let flags = self.ladder_flags();
        let step = Step {
            writer: &writer,
            flags: &flags,
            purse: &purse,
            floor: self.cfg.context.sizes().here,
        };
        let s = read::fold_step(&self.store, &ex.place, since.as_ref(), &step).await?;
        if let Some(why) = s.failed {
            tracing::info!(place = %ex.place, %why, "ladder: a fold waits");
        }
        Ok(())
    }

    /// Build `place`'s ladder from its history since its latest `/new`: the
    /// missing `did` lines, oldest first, then the folds, within the
    /// smaller of `money.cap_usd` and what is left of the day's line (the
    /// cap alone with `over_day_line`). A dry run writes nothing and says
    /// what it would do, its worst case and whether it would need that leave.
    pub async fn ladder_build(
        self: &Arc<Self>,
        place: &str,
        dry_run: bool,
        money: Allowance,
    ) -> anyhow::Result<Built> {
        let writer = ModelWriter(self.clone());
        self.ladder_build_with(&writer, place, dry_run, money).await
    }

    pub(crate) async fn ladder_build_with(
        &self,
        writer: &dyn Writer,
        place: &str,
        dry_run: bool,
        money: Allowance,
    ) -> anyhow::Result<Built> {
        let mut b = Built {
            dry_run,
            place: place.to_string(),
            ..Built::default()
        };
        if !super::laddered(place) {
            b.skipped = true;
            b.stopped = Some("the place's name holds a character no ladder key may".into());
            return Ok(b);
        }
        if money.over_day_line && money.cap_usd.is_none() {
            anyhow::bail!("passing the day's line needs a cap of its own (--cap-usd)");
        }
        b.since = blocking(|| super::new_mark(&self.store, place))?;
        let since = b.since.clone();
        let l = blocking(|| read::ladder(&self.store, place, since.as_ref()))?;
        let mut todo = Vec::new();
        b.exchanges = l.window.len() as u64;
        for e in &l.window {
            if blocking(|| did::did(&self.store, &e.turn_id))?.is_some() {
                b.lines_had += 1;
                continue;
            }
            let Some(r) = blocking(|| did::read(&self.store, &e.turn_id))? else {
                continue;
            };
            if r.ex.end_ms.is_some() && r.reply.is_some() {
                todo.push(r);
            }
        }
        let purse = DayPurse::new(&self.store, money.cap_usd, money.over_day_line);
        b.day_left_usd = DayPurse::new(&self.store, None, false).left()?;
        let (w, floor) = (l.window.len(), self.cfg.context.sizes().here);
        let folds = match crate::view::fold::fold_due_over(w, floor) {
            true => (w - floor) / FOLD_STEP,
            false => 0,
        };
        if dry_run {
            b.lines = todo.len() as u64;
            b.folds = folds as u64;
            b.estimated_usd = estimate(writer, &todo, folds);
            b.needs_over_day_line = b.estimated_usd > b.day_left_usd;
            return Ok(b);
        }
        let stopped = self.ladder_lines(writer, &todo, &purse, &mut b).await?;
        b.spent_usd = purse.spent();
        if stopped {
            // A run whose lines stopped (its allowance, a failed call) folds nothing.
            return Ok(b);
        }
        let flags = self.ladder_flags();
        let step = Step {
            writer,
            flags: &flags,
            purse: &purse,
            floor,
        };
        let s = read::fold_step(&self.store, place, since.as_ref(), &step).await?;
        b.spent_usd = purse.spent();
        b.folds = s.made.iter().filter(|n| n.level == 1).count() as u64;
        b.above = s.made.len() as u64 - b.folds;
        b.stopped = s.failed;
        Ok(b)
    }

    /// The missing lines, oldest first: whether the run stopped short.
    async fn ladder_lines(
        &self,
        writer: &dyn Writer,
        todo: &[did::Read],
        purse: &DayPurse<'_>,
        b: &mut Built,
    ) -> anyhow::Result<bool> {
        for r in todo {
            match did::write(&self.store, writer, &r.ex.turn_id, purse).await? {
                Outcome::Written(_) => b.lines += 1,
                Outcome::Failed { why, .. } => {
                    b.stopped = Some(why);
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }
}

/// The worst case of `todo`'s lines and of `folds` first-level nodes, each
/// priced as its own call (a fold's material at its most).
fn estimate(writer: &dyn Writer, todo: &[did::Read], folds: usize) -> f64 {
    let lines: f64 = todo
        .iter()
        .filter_map(|r| {
            writer.estimate(&Ask {
                work: Work::Did,
                text: did::material(r),
                sources: 1,
            })
        })
        .sum();
    let most =
        (super::fold::ASK_CHARS + super::fold::REPLY_CHARS + did::MAX_CHARS + 80) * FOLD_STEP;
    let fold = writer
        .estimate(&Ask {
            work: Work::Fold,
            text: "x".repeat(most),
            sources: FOLD_STEP as u32,
        })
        .unwrap_or(0.0);
    lines + fold * folds as f64
}
