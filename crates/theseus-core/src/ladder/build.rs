//! The daemon's side of the ladder: the job at an exchange's end (its `did`
//! line, then its place's fold step) in a place assembled as a view, what is sealed or veiled, where a
//! place's latest `/new` is, the person's lines that made a standing record,
//! and the build of a place's history ([`Core::ladder_build`]), with a dry
//! run that counts and prices, and a cap.

use std::sync::Arc;

use theseus_store::blocking;

use super::did::{self, Outcome};
use super::fold::Flags;
use super::read::{self, Step};
use super::writer::{Ask, ModelWriter, Work, Writer};
use super::N;
use crate::config::assembly::Assembly;
use crate::exchange::{self, Root};
use crate::judge::categorize::ExchangeEnd;
use crate::judge::JudgeService;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The person's lines a standing record was made from: by the record's source.
pub struct Standing(pub Vec<String>);

impl Flags for Standing {
    /// The judge's answer of a correction is not kept per message today, so
    /// no line is called one (`docs/design/ladder.md`).
    fn correction(&self, _: &str) -> bool {
        false
    }
    fn standing(&self, node: &str) -> bool {
        self.0.iter().any(|s| s == node)
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
        if !core.cfg.context.ledger_on() {
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
    pub since_ms: u64,
    /// The place's exchanges since its latest `/new`.
    pub exchanges: u64,
    /// Those with a line already, and those a line would be (was) written for.
    pub lines_had: u64,
    pub lines: u64,
    /// First-level nodes it would make (made); and the nodes above, by code.
    pub folds: u64,
    pub above: u64,
    /// The worst case of its calls, and what was spent.
    pub estimated_usd: f64,
    pub spent_usd: f64,
    pub stopped: Option<String>,
}

impl Core {
    /// Whether session `sid` is sealed or veiled: a sealed or seal-pending
    /// thread's, or an imported conversation's (whose labels the ladder does
    /// not read, so fails closed).
    pub(crate) fn ladder_hidden(&self, sid: &str) -> bool {
        crate::import::is_imported(sid)
            || self
                .threads()
                .of_session(sid)
                .is_some_and(crate::compiler::place_digest::held)
    }

    /// When `place`'s latest `/new` was: the start of the session its
    /// binding runs on now; 0 for a place no binding moves (the terminal).
    pub(crate) fn ladder_since(&self, place: &str) -> u64 {
        let bare = place.strip_prefix("discord:").unwrap_or(place);
        let sid = self.outbox.place_session(bare).ok().flatten();
        sid.and_then(|s| self.store.get_session::<SessionRecord>(&s).ok().flatten())
            .map_or(0, |r| r.created_at_unix_ms)
    }

    fn ladder_flags(&self) -> Standing {
        let all = self.runner.standing.all(&self.store).unwrap_or_default();
        Standing(all.iter().map(|r| r.source.clone()).collect())
    }

    /// The job at the end of exchange `turn`: only in a place whose requests
    /// are assembled as a view (`[context] assembly`), the one reader of what
    /// is written here; a place on the transcript spends nothing on it.
    async fn ladder_after(self: &Arc<Self>, turn: &str) -> anyhow::Result<()> {
        let Some(ex) = blocking(|| exchange::exchange(&self.store, turn))? else {
            return Ok(());
        };
        if self.cfg.context.assembly_for(&ex.place) != Assembly::View {
            return Ok(());
        }
        let writer = ModelWriter(self.clone());
        let hidden = |sid: &str| self.ladder_hidden(sid);
        match did::write(&self.store, &writer, turn, &hidden, None).await? {
            Outcome::Failed(why) => tracing::info!(turn, %why, "ladder: no did line"),
            Outcome::NotEnded => return Ok(()),
            _ => {}
        }
        let flags = self.ladder_flags();
        let step = Step {
            writer: &writer,
            flags: &flags,
            hidden: &hidden,
            cap_usd: None,
        };
        let since = self.ladder_since(&ex.place);
        let s = read::fold_step(&self.store, &ex.place, since, &step).await?;
        if let Some(why) = s.failed {
            tracing::info!(place = %ex.place, %why, "ladder: a fold waits");
        }
        Ok(())
    }

    /// Build `place`'s ladder from its history since its latest `/new`: the
    /// missing `did` lines, oldest first, then the folds, all within
    /// `cap_usd` (the day's line when none). A dry run writes nothing and
    /// says what it would do and its worst case.
    pub async fn ladder_build(
        self: &Arc<Self>,
        place: &str,
        dry_run: bool,
        cap_usd: Option<f64>,
    ) -> anyhow::Result<Built> {
        let writer = ModelWriter(self.clone());
        self.ladder_build_with(&writer, place, dry_run, cap_usd)
            .await
    }

    pub(crate) async fn ladder_build_with(
        &self,
        writer: &dyn Writer,
        place: &str,
        dry_run: bool,
        cap_usd: Option<f64>,
    ) -> anyhow::Result<Built> {
        let since = self.ladder_since(place);
        let mut b = Built {
            dry_run,
            place: place.to_string(),
            since_ms: since,
            ..Built::default()
        };
        let mut todo = Vec::new();
        let touches = blocking(|| exchange::newest(&self.store, Root::Place(place), usize::MAX))?;
        for t in touches.iter().rev().filter(|t| t.at_ms >= since) {
            b.exchanges += 1;
            if blocking(|| did::did(&self.store, &t.exchange))?.is_some() {
                b.lines_had += 1;
                continue;
            }
            let Some(r) = blocking(|| did::read(&self.store, &t.exchange))? else {
                continue;
            };
            if r.ex.end_ms.is_some() && r.reply.is_some() && !self.ladder_hidden(&r.ex.session_id) {
                todo.push(r);
            }
        }
        let l = blocking(|| read::ladder(&self.store, place, since))?;
        let w = l.window.len();
        let folds = if w >= 2 * N { (w - N) / N } else { 0 };
        if dry_run {
            b.lines = todo.len() as u64;
            b.folds = folds as u64;
            b.estimated_usd = estimate(writer, &todo, folds);
            return Ok(b);
        }
        self.ladder_lines(writer, &todo, cap_usd, &mut b).await?;
        let flags = self.ladder_flags();
        let hidden = |sid: &str| self.ladder_hidden(sid);
        let step = Step {
            writer,
            flags: &flags,
            hidden: &hidden,
            cap_usd: cap_usd.map(|c| c - b.spent_usd),
        };
        let s = read::fold_step(&self.store, place, since, &step).await?;
        b.spent_usd += s.spent_usd;
        b.folds = s.made.iter().filter(|n| n.level == 1).count() as u64;
        b.above = s.made.len() as u64 - b.folds;
        b.stopped = b.stopped.take().or(s.failed);
        Ok(b)
    }

    async fn ladder_lines(
        &self,
        writer: &dyn Writer,
        todo: &[did::Read],
        cap_usd: Option<f64>,
        b: &mut Built,
    ) -> anyhow::Result<()> {
        let hidden = |sid: &str| self.ladder_hidden(sid);
        for r in todo {
            let cap = cap_usd.map(|c| c - b.spent_usd);
            match did::write(&self.store, writer, &r.ex.turn_id, &hidden, cap).await? {
                Outcome::Written(d) => {
                    b.lines += 1;
                    b.spent_usd += d.cost_usd;
                }
                Outcome::Failed(why) => {
                    b.stopped = Some(why);
                    return Ok(());
                }
                _ => {}
            }
        }
        Ok(())
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
    let most = (super::fold::ASK_CHARS + super::fold::REPLY_CHARS + did::MAX_CHARS + 80) * N;
    let fold = writer
        .estimate(&Ask {
            work: Work::Fold,
            text: "x".repeat(most),
            sources: N as u32,
        })
        .unwrap_or(0.0);
    lines + fold * folds as f64
}
