//! The nightly pass (theseus-pw1q.11): [`run`] turns candidates into counted
//! items, pure of the core but for the reviewer, the sealed sink and the
//! shelf it is handed; [`Core::self_exams_nightly`] is the daemon's side.
//!
//! The order, per candidate: scrub into an item; skip one already decided;
//! the cap; split; a sealed item with no sink is held and dropped with a
//! count (it costs no review); the gate again; the review; then, for a fair
//! item, its write and its `self.exam.counted` row, or, for an unfair one, its
//! `self.exam.rejected` row. A reviewer that fails leaves the item unwritten
//! and uncounted. A sealed item's text appears in no row and no log.

use std::collections::HashSet;

use serde_json::json;
use theseus_protocol::LedgerKind;

use super::review::CoreReviewer;
use super::{
    half, Family, Half, Item, ItemId, Raw, Reviewer, Scrub, SealedSink, Shelf, SplitKey, Verdict,
    Writer, KEY_META, LAST_META,
};
use crate::fact::rsi_exams::{ExamCounted, ExamPass, ExamRejected};
use crate::rpc::Core;
use crate::rsi::{gate, Gate};

/// One candidate's end.
#[derive(Debug, Clone, PartialEq)]
pub enum Fate {
    Counted,
    Rejected(String),
    /// A sealed item held for want of a sealed store.
    Dropped,
    /// The reviewer did not answer, or the write failed.
    Unreviewed(String),
}

/// One candidate and what became of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub id: ItemId,
    pub family: Family,
    pub sealed: bool,
    pub fate: Fate,
    pub reviewer: Option<String>,
    /// An open item's task; never a sealed item's.
    pub task: Option<String>,
}

/// What a pass was given.
pub struct Deps<'a> {
    pub reviewer: &'a dyn Reviewer,
    pub sink: Option<&'a dyn SealedSink>,
    pub shelf: &'a Shelf,
    pub key: &'a SplitKey,
    pub scrub: &'a Scrub<'a>,
    /// Items already counted or rejected, by id.
    pub seen: &'a HashSet<ItemId>,
    /// Reviews left under the day's cap.
    pub room: u32,
    /// The sealed set the night's items go to.
    pub set: &'a str,
    /// The kill switch and the mode, asked before each review.
    pub allowed: &'a (dyn Fn() -> bool + Sync),
    /// Called as each candidate ends, before the next begins.
    pub record: &'a (dyn Fn(&Outcome) + Sync),
}

/// What a pass did, in numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PassReport {
    pub candidates: u64,
    pub outcomes: Vec<Outcome>,
    pub already: u64,
    pub capped: u64,
    /// The gate closed before the pass was done.
    pub stopped: bool,
}

impl PassReport {
    fn n(&self, f: impl Fn(&Fate) -> bool) -> u64 {
        self.outcomes.iter().filter(|o| f(&o.fate)).count() as u64
    }
    pub fn counted(&self) -> u64 {
        self.n(|f| *f == Fate::Counted)
    }
    pub fn rejected(&self) -> u64 {
        self.n(|f| matches!(f, Fate::Rejected(_)))
    }
    pub fn sealed_dropped(&self) -> u64 {
        self.n(|f| *f == Fate::Dropped)
    }
    pub fn unreviewed(&self) -> u64 {
        self.n(|f| matches!(f, Fate::Unreviewed(_)))
    }
}

/// Run the pass over `raws`, oldest first.
pub async fn run(d: &Deps<'_>, raws: Vec<Raw>) -> PassReport {
    let mut rep = PassReport::default();
    let (mut room, mut batch) = (d.room, HashSet::new());
    for raw in raws {
        let item = Item::scrubbed(&raw, d.scrub);
        if d.seen.contains(&item.id) || !batch.insert(item.id.clone()) {
            rep.already += 1;
            continue;
        }
        rep.candidates += 1;
        let side = half(d.key, item.family, &item.id);
        let sealed = side == Half::Sealed;
        let out = |fate, reviewer: Option<String>| Outcome {
            id: item.id.clone(),
            family: item.family,
            sealed,
            fate,
            reviewer,
            task: (!sealed).then(|| item.task.clone()),
        };
        if sealed && d.sink.is_none() {
            let o = out(Fate::Dropped, None);
            (d.record)(&o);
            rep.outcomes.push(o);
            continue;
        }
        if room == 0 {
            rep.capped += 1;
            continue;
        }
        if !(d.allowed)() {
            rep.stopped = true;
            break;
        }
        room -= 1;
        let o = match d.reviewer.review(&item).await {
            Err(e) => out(Fate::Unreviewed(format!("{e:#}")), None),
            Ok(v) if !v.fair() => {
                let why = reason(&v, sealed);
                out(Fate::Rejected(why), Some(v.reviewer))
            }
            Ok(v) => match keep(d, &item, side) {
                Ok(()) => out(Fate::Counted, Some(v.reviewer)),
                Err(e) => out(Fate::Unreviewed(format!("{e:#}")), Some(v.reviewer)),
            },
        };
        (d.record)(&o);
        rep.outcomes.push(o);
    }
    rep
}

/// A rejection's reason. A sealed item's names the failed questions only:
/// the reviewer's own words may quote the item.
fn reason(v: &Verdict, sealed: bool) -> String {
    if sealed {
        Verdict {
            reason: String::new(),
            ..v.clone()
        }
        .why()
    } else {
        v.why()
    }
}

/// Write a fair item to its half.
fn keep(d: &Deps<'_>, item: &Item, side: Half) -> anyhow::Result<()> {
    match side {
        Half::Open => d.shelf.put(&Writer::Pass, item),
        Half::Sealed => {
            let sink = d.sink.ok_or_else(|| anyhow::anyhow!("no sealed store"))?;
            sink.put(d.set, item.family.as_str(), &item.bytes()?)
                .map(drop)
        }
    }
}

const DAY_MS: u64 = 86_400_000;
/// How far back a pass reads when it has never run, or ran long ago.
const LOOKBACK_MS: u64 = 7 * DAY_MS;

impl Core {
    /// The split key: the store's, made at the first use.
    fn exam_key(&self) -> anyhow::Result<SplitKey> {
        if let Some(k) = self
            .store
            .get_meta::<serde_json::Value>(KEY_META)?
            .and_then(|v| v["key"].as_str().and_then(SplitKey::from_hex))
        {
            return Ok(k);
        }
        let k = SplitKey::fresh();
        self.store.put_meta(KEY_META, &json!({"key": k.hex()}))?;
        Ok(k)
    }

    /// Where sealed items go: the sealed store when `[self] sealed_dir` is
    /// set (theseus-pw1q.5), else none, and a sealed item is held in memory
    /// for the pass and dropped with a count.
    fn sealed_sink(&self) -> Option<&dyn SealedSink> {
        self.sealed().map(|s| &**s as &dyn SealedSink)
    }

    /// The held people's names, when the ontology is built (memory only).
    fn exam_names(&self) -> Vec<String> {
        self.runner
            .ontology
            .held()
            .map(|o| {
                o.categories()
                    .filter(|c| c.kind() == theseus_ontology::person::KIND)
                    .map(|c| c.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The ids of every item already counted or rejected.
    fn exam_seen(&self, now_ms: u64) -> anyhow::Result<HashSet<ItemId>> {
        let mut seen = HashSet::new();
        for kind in [LedgerKind::SelfExamCounted, LedgerKind::SelfExamRejected] {
            for (_, row) in super::harvest::rows_of(self, kind, 0, now_ms)? {
                if let Some(id) = row.data["item"].as_str() {
                    seen.insert(ItemId(id.to_string()));
                }
            }
        }
        Ok(seen)
    }

    /// Reviews already spent today.
    fn exam_used_today(&self, now_ms: u64) -> anyhow::Result<u32> {
        let midnight = crate::learning::local_midnight(now_ms);
        let mut n = 0;
        for kind in [LedgerKind::SelfExamCounted, LedgerKind::SelfExamRejected] {
            n += super::harvest::rows_of(self, kind, midnight, now_ms)?.len();
        }
        Ok(n as u32)
    }

    /// The nightly exam pass, after `rsi::gate` and after the backlog's. With
    /// the mode off, or the switch thrown, it reads and writes nothing.
    pub async fn self_exams_nightly(&self) -> Option<PassReport> {
        let ctx = self.self_ctx();
        if gate(&ctx) != Gate::Allowed {
            return None;
        }
        let rt_now = theseus_protocol::now_unix_ms();
        match self.self_exams_pass(rt_now, &CoreReviewer::new(self)).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self exams: the pass did not run");
                let fact = ExamPass::failed(&format!("{e:#}"));
                if let Ok(row) = crate::fact::row(&fact, None, None) {
                    let _ = self.store.append(&[row]);
                }
                None
            }
        }
    }

    /// One pass at `now_ms` with `reviewer`: its candidates read, reviewed
    /// and written. `None` when the day's cap leaves no room or nothing is new.
    pub async fn self_exams_pass(
        &self,
        now_ms: u64,
        reviewer: &dyn Reviewer,
    ) -> anyhow::Result<Option<PassReport>> {
        let day_max = self.cfg.self_improve.exams_day_max;
        let used = theseus_store::blocking(|| self.exam_used_today(now_ms))?;
        let room = day_max.saturating_sub(used);
        if room == 0 {
            return Ok(None);
        }
        let last = self
            .store
            .get_meta::<serde_json::Value>(LAST_META)?
            .and_then(|v| v["at_ms"].as_u64())
            .unwrap_or(0);
        let since = last.max(now_ms.saturating_sub(LOOKBACK_MS));
        let (raws, seen, key) = theseus_store::blocking(|| -> anyhow::Result<_> {
            Ok((
                self.exam_raws(since, now_ms, now_ms)?,
                self.exam_seen(now_ms)?,
                self.exam_key()?,
            ))
        })?;
        let shelf = Shelf::new(&self.cfg.state_dir());
        let names = self.exam_names();
        let scrubber = self.tools.scrubber.clone();
        let secrets = move |s: &str| scrubber.scrub(s).0;
        let scrub = Scrub {
            secrets: &secrets,
            names: &names,
        };
        let day = crate::wake::local(now_ms);
        let set = format!("self-exams-{:04}-{:02}-{:02}", day.year, day.month, day.day);
        let allowed = || gate(&self.self_ctx()) == Gate::Allowed;
        let record = |o: &Outcome| {
            if let Err(e) = self.record_exam(o, reviewer_name(o)) {
                tracing::warn!(error = %format!("{e:#}"), "self exams: a row was not written");
            }
        };
        let deps = Deps {
            reviewer,
            sink: self.sealed_sink(),
            shelf: &shelf,
            key: &key,
            scrub: &scrub,
            seen: &seen,
            room,
            set: &set,
            allowed: &allowed,
            record: &record,
        };
        let rep = run(&deps, raws).await;
        if rep.candidates > 0 || rep.already > 0 {
            let fact = ExamPass::of(&rep, &set);
            self.store.append(&[crate::fact::row(&fact, None, None)?])?;
            self.rec(None).announce(&fact);
        }
        if !rep.stopped {
            self.store.put_meta(LAST_META, &json!({"at_ms": now_ms}))?;
        }
        Ok(Some(rep))
    }

    /// Write an outcome's row, in one frame, and say it.
    fn record_exam(&self, o: &Outcome, reviewer: &str) -> anyhow::Result<()> {
        match &o.fate {
            Fate::Counted => {
                let f = ExamCounted {
                    item: &o.id.0,
                    family: o.family.as_str(),
                    sealed: o.sealed,
                    reviewer,
                };
                self.store.append(&[crate::fact::row(&f, None, None)?])?;
                self.rec(None).announce(&f);
            }
            Fate::Rejected(why) => {
                let f = ExamRejected {
                    item: &o.id.0,
                    family: o.family.as_str(),
                    sealed: o.sealed,
                    reviewer,
                    reason: why,
                };
                self.store.append(&[crate::fact::row(&f, None, None)?])?;
                self.rec(None).announce(&f);
            }
            Fate::Dropped | Fate::Unreviewed(_) => {}
        }
        Ok(())
    }
}

fn reviewer_name(o: &Outcome) -> &str {
    o.reviewer.as_deref().unwrap_or("")
}

/// The sealed store as the pass's sink (seam S5, a join fix with
/// theseus-pw1q.5): the item's task and check in the store's own format,
/// kept under the store's own id; the pass writes its counted row.
impl SealedSink for crate::rsi::sealed::Sealed {
    fn put(&self, set: &str, family: &str, bytes: &[u8]) -> anyhow::Result<ItemId> {
        let item: Item = serde_json::from_slice(bytes)?;
        let file = json!({
            "v": crate::rsi::sealed::ITEM_VERSION,
            "task": item.task,
            "check": item.check,
        });
        let id = crate::rsi::sealed::Sealed::put(self, set, family, &serde_json::to_vec(&file)?)?;
        Ok(ItemId(id.as_str().to_string()))
    }
}
