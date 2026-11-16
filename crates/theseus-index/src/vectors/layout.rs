//! The hot set under a budget (issue 21 of 1.0.7): which records a search reads first, how many
//! the budget holds, and the tiered search over what a layout (`tier.rs`) placed.
//!
//! **The budget** is `[index] resident_mb` (128 by default): the index's resident memory, the
//! model unloaded, anonymous and file-backed together, at rest and after a run of ordinary
//! recalls, whatever the store's size. A quarter of it is the fixed part (the word index, the
//! groups' centres, the table's bookkeeping); three quarters is the **hot region**, at a cut, a
//! scale and an entry number a record. At today's size and the default budget every record fits,
//! so nothing is laid out, nothing is read differently, and a search is the exact scan it always
//! was. Past it the highest scores ([`super::heat`]) are hot and the rest is cold.
//!
//! **What a search reads** under a layout: the whole hot region, then every record the layout
//! does not place (those embedded since, and those no alive row held when it was made), then the
//! promoted records, then the nearest eighth of the groups. A record's rows are found by its text
//! and each is filtered as the exact scan filters it, so a forgotten, dead or filtered-out row is
//! never returned from any region: a record is a name for rows, and only alive rows answer.
//!
//! **Explicit searches are exact** ([`Reach::Exact`]): the memory search tool, the neighbours
//! lookup and anything that does not ask for [`Reach::Fast`] scan everything, hot and cold.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use super::groups::{self, Cuts};
use super::heat::{self, Sig, NEVER_HOT};
use super::scan::ScanFile;
use super::tier::{Group, Layout, TierFile};
use super::*;

/// The default of `[index] resident_mb`.
pub const DEFAULT_RESIDENT_MB: u64 = 128;

/// The least `[index] resident_mb` that holds the parts that do not shrink: the word index, the
/// groups' centres and the table's bookkeeping (about 32 MiB at the size the budget is for), and a
/// hot region worth having (16 MiB).
pub const MIN_RESIDENT_MB: u64 = 48;

/// A value of `[index] resident_mb` the index can keep, or why not.
pub fn check_resident_mb(mb: u64) -> Result<(), String> {
    if mb < MIN_RESIDENT_MB {
        return Err(format!(
            "[index] resident_mb = {mb} is too small: the word index, the groups' centres and the \
             table's own bookkeeping need about 32 MiB, and the hot set at least 16 MiB more; \
             the least that holds them is {MIN_RESIDENT_MB}"
        ));
    }
    Ok(())
}

/// How many records the hot region may hold under `resident_mb`: three quarters of it, at a cut,
/// a scale and an entry number a record.
pub fn hot_cap(resident_mb: u64, cut: usize) -> usize {
    let bytes = resident_mb.saturating_mul(1 << 20) / 4 * 3;
    usize::try_from(bytes / (cut as u64 + 8)).unwrap_or(usize::MAX)
}

/// Promoted records held at once; past this a layout makes them hot.
pub(super) const PROMOTED_MAX: usize = 2048;

/// A layout is put in place only if, on the check it makes of itself, a search by it keeps at
/// least this share of the exact search's best ten.
pub(super) const KEEP_BAR: f32 = 0.95;

/// The bar when too few real queries have been served to check with: records' own vectors are a
/// kinder test than a turn's words, which fall between groups, so the bar is higher.
pub(super) const KEEP_BAR_RECORDS: f32 = 0.97;

/// Queries a layout is checked with, at most, and the most the index remembers of those it served.
const CHECK_QUERIES: usize = 64;
const RECENT: usize = 256;

/// Real queries needed before a layout is checked on them and not on records' own vectors.
const REAL_MIN: usize = 32;

/// A failed layout is tried again after this long, or when the store has grown a tenth.
const RETRY_AFTER: Duration = Duration::from_secs(24 * 3600);

/// Rows scored under one hold of the table's lock.
const SLICE: usize = 32_768;

/// A search by meaning reads the nearest group of every this many (an eighth), after the hot region.
pub const PROBE_DIVISOR: usize = 8;

/// The records the groups' centres are made from, however large the store.
const SAMPLE: usize = 30_000;

/// Rounds of the clustering.
const ROUNDS: usize = 8;

/// How many of `groups` a search reads: the nearest eighth, at least one.
pub(super) fn probe_count(groups: usize) -> usize {
    groups.div_ceil(PROBE_DIVISOR).max(1)
}

/// How far a search reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reach {
    /// Every record, hot and cold: a search somebody asked for by name.
    #[default]
    Exact,
    /// The hot region, what no layout places, and the nearest groups: a turn's automatic recall.
    Fast,
}

/// The scan file's cuts, for the clustering and the layout to read.
pub(super) struct ScanCuts(pub Arc<ScanFile>);

impl Cuts for ScanCuts {
    fn cut(&self, e: u32) -> &[i8] {
        self.0.q(e)
    }

    fn scale(&self, e: u32) -> f32 {
        self.0.scale(e)
    }
}

/// What the layout pass reads off the table, under its read lock, to work from without it.
pub(super) struct Plan {
    pub generation: u64,
    pub epoch: u64,
    pub scan: Arc<ScanFile>,
    pub tier_path: PathBuf,
    pub cut: usize,
    /// Records the scan file holds.
    pub n: usize,
    /// Each record's best score, or `NEG_INFINITY` where no alive row holds its text.
    pub scores: Vec<f32>,
}

impl Plan {
    /// Where a layout is written until it has passed its check: a name nothing opens, so a kill
    /// in the middle of the check leaves no layout that was never checked.
    pub fn cand_path(&self) -> PathBuf {
        self.tier_path.with_extension("tier.cand")
    }
}

impl Table {
    /// The signals of the node of row `i`.
    fn sig_of(&self, i: u32) -> Sig {
        if self.signals.is_empty() {
            return Sig::default();
        }
        match (&self.base, self.in_tail(i)) {
            (Some(b), None) => self.signals.get(b.node_id(b.key(i as usize).node)),
            (_, Some(k)) => self.signals.get(&self.rows[k].node),
            _ => Sig::default(),
        }
    }

    /// The start of the layout pass's reading of the table: the current stamp's scan file and the
    /// numbers a layout made from it must still find. `None` where a layout cannot be made now
    /// (no scan file, a stamp being re-embedded, the rows not every chunk yet). Its scores are
    /// empty until [`Table::score_rows`] fills them.
    pub(super) fn plan_head(&self) -> Option<Plan> {
        if !self.settled() || self.caches.len() != 1 {
            return None;
        }
        let c = &self.caches[0];
        let scan = c.scan.clone()?;
        let n = scan.len();
        Some(Plan {
            generation: self.generation,
            epoch: c.epoch,
            tier_path: c.tier_path(),
            cut: c.cut,
            scan,
            n,
            scores: vec![f32::NEG_INFINITY; n],
        })
    }

    /// Fold the scores of rows `rows` into `scores`: each record keeps its best row's.
    pub(super) fn score_rows(&self, rows: Range<u32>, now_s: u32, scores: &mut [f32]) {
        for i in rows {
            if !self.is_alive(i) {
                continue;
            }
            let Some((0, e)) = self.row_vec(i) else {
                continue;
            };
            let Some(slot) = scores.get_mut(e as usize) else {
                continue;
            };
            let s = heat::score(now_s, self.row_time(i), self.sig_of(i));
            if s > *slot {
                *slot = s;
            }
        }
    }

    /// The whole plan at once, under one hold of the table (the tender's own pass reads it in
    /// slices, [`Vectors::plan`]).
    #[cfg(test)]
    pub(super) fn plan_layout(&self, now_s: u32) -> Option<Plan> {
        let mut plan = self.plan_head()?;
        self.score_rows(0..self.row_count() as u32, now_s, &mut plan.scores);
        Some(plan)
    }

    /// Put a layout in place (or none), with the promotions it makes needless gone and those the
    /// signals still call for found again.
    pub(super) fn install_tier(&mut self, tier: Option<Arc<TierFile>>) {
        self.tier = tier;
        self.rederive_promotions();
    }

    /// The cold records of node `node` are searched with the hot set from now until a layout:
    /// the operator vouched for it or a turn used it. At most [`PROMOTED_MAX`] are held.
    pub(super) fn promote_node(&mut self, node: &str) {
        let Some(tier) = self.tier.clone() else {
            return;
        };
        for i in self.rows_of(node) {
            if self.promoted.len() >= PROMOTED_MAX {
                break;
            }
            if let Some((0, e)) = self.row_vec(i) {
                if tier.places(e) && !tier.is_hot(e) {
                    self.promoted.insert(e);
                }
            }
        }
    }

    /// The promotions again, from the signals: after a layout is put in place or opened.
    pub(super) fn rederive_promotions(&mut self) {
        self.promoted.clear();
        for node in self.signals.hot_nodes() {
            self.promote_node(&node);
        }
    }

    /// The tier a search of this reach reads, if one is good and the table is in one stamp.
    pub(super) fn tier_for(&self, reach: Reach) -> Option<Arc<TierFile>> {
        match reach {
            Reach::Exact => None,
            Reach::Fast if self.caches.len() == 1 => self.tier.clone(),
            Reach::Fast => None,
        }
    }

    /// The scan, then the re-score ([`Table::search`]), over the tier when the reach allows it.
    pub(super) fn search_reach(
        &self,
        full: &[f32],
        cut: usize,
        f: &RowFilter,
        fetch: usize,
        reach: Reach,
    ) -> io::Result<Vec<(usize, f64)>> {
        self.search_with(full, cut, f, fetch, self.tier_for(reach))
    }

    /// The best `n` rows of the tier's regions: the hot region, every record it does not place,
    /// the promoted records and the nearest groups, by their int8 cuts, each record's alive rows
    /// filtered by `keep`. The same candidates, with the same scores and the same ties, as the
    /// exact scan gives over those records.
    pub(super) fn top_tiered(
        &self,
        tier: &TierFile,
        q: &[i8],
        n: usize,
        keep: &dyn Fn(usize) -> bool,
    ) -> Vec<(usize, f32)> {
        if n == 0 {
            return Vec::new();
        }
        let c0 = &self.caches[0];
        let mut heap: BinaryHeap<Reverse<Scored>> = BinaryHeap::with_capacity(n + 1);
        let mut consider = |e: u32, s: f32| {
            // A record that cannot beat the worst kept row (and cannot win a tie by an earlier
            // row: equal scores are looked at) is not opened.
            if heap.len() == n && heap.peek().is_some_and(|w| s < w.0 .0) {
                return;
            }
            for i in self.holders(c0.hash_of(e)) {
                if self.row_vec(i) != Some((0, e)) || !keep(i as usize) {
                    continue;
                }
                let x = Scored(s, i as usize);
                if heap.len() < n {
                    heap.push(Reverse(x));
                } else if heap.peek().is_some_and(|w| x > w.0) {
                    heap.pop();
                    heap.push(Reverse(x));
                }
            }
        };
        for p in 0..tier.hot_n() {
            consider(tier.entry(p), dot_i8(tier.q(p), q) as f32 * tier.scale(p));
        }
        for e in 0..c0.len() as u32 {
            if !tier.places(e) {
                consider(e, dot_i8(c0.q(e), q) as f32 * c0.scale_of(e));
            }
        }
        // Cold records a signal has promoted since the layout, searched with the hot ones (and
        // so left out of their group's pass below: a record is scanned once).
        for &e in &self.promoted {
            if tier.places(e) && !tier.is_hot(e) {
                consider(e, dot_i8(c0.q(e), q) as f32 * c0.scale_of(e));
            }
        }
        // The nearest groups of the cold part, by their centres.
        let mut near: Vec<(f32, usize)> = (0..tier.groups())
            .map(|g| {
                let (c, s) = tier.centre(g);
                (dot_i8(q, c) as f32 * s, g)
            })
            .collect();
        near.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, g) in near.iter().take(probe_count(near.len())) {
            for p in tier.group(g) {
                let e = tier.entry(p);
                if !self.promoted.contains(&e) {
                    consider(e, dot_i8(tier.q(p), q) as f32 * tier.scale(p));
                }
            }
        }
        let mut out: Vec<Scored> = heap.into_iter().map(|r| r.0).collect();
        out.sort_by(|a, b| b.cmp(a));
        out.into_iter().map(|s| (s.1, s.0)).collect()
    }

    /// Let the pages the table's cold reads touched go from the process: the scan file's own
    /// columns and the rows' keys (the hot region, which the tier file holds, stays).
    pub(super) fn release_cold_pages(&self) {
        if let Some(b) = &self.base {
            b.release();
        }
        self.caches
            .iter()
            .for_each(super::cache::Cache::release_pages);
        if let Some(t) = &self.tier {
            t.release_groups();
        }
    }
}

/// What the layout passes did.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct Layouts {
    pub count: u64,
    pub last_ms: f64,
    pub last_hot: u64,
    pub last_cold: u64,
    pub last_groups: u64,
    pub last_at_ms: u64,
    /// What the last layout kept of the exact search's best ten, on its own check, and on what.
    pub last_share: Option<f32>,
    pub last_checked: Option<Checked>,
}

/// What a layout is checked on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Checked {
    /// Real queries the tender served (recall's own).
    Queries,
    /// Cold records' own vectors, while too few queries have been served.
    Records,
}

impl Checked {
    pub fn bar(self) -> f32 {
        match self {
            Checked::Queries => KEEP_BAR,
            Checked::Records => KEEP_BAR_RECORDS,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Checked::Queries => "queries",
            Checked::Records => "records",
        }
    }
}

/// A layout that did not pass its check: searches stay exact until one does.
#[derive(Debug, Clone, Copy)]
pub(super) struct Failed {
    pub share: f32,
    pub bar: f32,
    pub at: Instant,
    /// Records with an alive row, then.
    pub alive: usize,
}

/// A layout's state beside the table: when it may be tried again, what it last did, and whether
/// the last one failed its check.
#[derive(Debug, Default)]
pub(super) struct LayoutState {
    pub retry_at: Option<Instant>,
    pub stats: Layouts,
    pub failed: Option<Failed>,
}

/// Which of `scores`' records are hot under `cap`, the highest scores first, never one a doubted
/// label keeps out; `None` where every record with an alive row fits (nothing is laid out).
pub(super) fn choose_hot(scores: &[f32], cap: usize) -> Option<Vec<u32>> {
    let alive = scores.iter().filter(|s| **s > f32::NEG_INFINITY).count();
    if alive <= cap {
        return None;
    }
    let mut eligible: Vec<u32> = (0..scores.len() as u32)
        .filter(|&e| scores[e as usize] > NEVER_HOT)
        .collect();
    let by_score = |a: &u32, b: &u32| {
        scores[*b as usize]
            .total_cmp(&scores[*a as usize])
            .then(a.cmp(b))
    };
    if eligible.len() > cap {
        eligible.select_nth_unstable_by(cap, by_score);
        eligible.truncate(cap);
    }
    eligible.sort_unstable();
    Some(eligible)
}

impl Vectors {
    /// The hot region's room under this tender's budget.
    pub(super) fn cap(&self, cut: usize) -> usize {
        self.cfg
            .hot_records
            .unwrap_or_else(|| hot_cap(self.cfg.resident_mb, cut))
    }

    /// Why a layout is due now, if it is: the store does not fit the budget and has no layout (and
    /// none failed its check, or not lately), or the layout is of a store that has grown, drifted
    /// or filled its promotions, or the store is back under the budget. Never while the rows are
    /// not every chunk, a stamp is being re-embedded, or a query is embedding.
    pub(super) fn layout_due(&self) -> Option<&'static str> {
        if !self.ahead.may_backfill() {
            return None;
        }
        let (retry_at, failed) = {
            let l = self.layouts.lock().unwrap();
            (l.retry_at, l.failed)
        };
        if retry_at.is_some_and(|at| Instant::now() < at) {
            return None;
        }
        let t = self.table.read().unwrap();
        if !t.settled() || t.caches.len() != 1 {
            return None;
        }
        let c = &t.caches[0];
        c.scan.as_ref()?;
        let alive = c.len() - c.dead();
        let fits = alive <= self.cap(c.cut);
        match (&t.tier, fits) {
            (None, true) => None,
            (None, false) => match failed {
                Some(f) if f.at.elapsed() < RETRY_AFTER && alive * 10 < f.alive * 11 => None,
                _ => Some("over the budget"),
            },
            (Some(_), true) => Some("back under the budget"),
            (Some(tier), false) => {
                let grown = c.len().saturating_sub(tier.covers());
                if grown >= super::scan::FOLD.max(tier.covers() / 20) {
                    return Some("grown");
                }
                if t.promoted.len() >= PROMOTED_MAX {
                    return Some("promoted");
                }
                // The cold part has moved a fifth from what the groups' centres were made of.
                let (cold, was) = (
                    alive.saturating_sub(tier.hot_n()),
                    tier.made_from() as usize,
                );
                (cold.abs_diff(was) * 5 >= was.max(1)).then_some("drifted")
            }
        }
    }

    /// The layout pass's reading of the table: every record of the current stamp's scan file with
    /// its best row's score, read a slice of rows at a time so no query or commit waits long for
    /// the lock.
    pub(super) fn plan(&self, now_s: u32) -> Option<Plan> {
        let mut plan = self.table.read().unwrap().plan_head()?;
        let mut at = 0u32;
        loop {
            let t = self.table.read().unwrap();
            if t.generation != plan.generation
                || t.caches.first().is_none_or(|c| c.epoch != plan.epoch)
            {
                return None;
            }
            let end = (at as usize + SLICE).min(t.row_count()) as u32;
            t.score_rows(at..end, now_s, &mut plan.scores);
            at = end;
            if at as usize >= t.row_count() {
                return Some(plan);
            }
        }
    }

    /// Make the layout: score the records, choose the hot set, group the rest, write the tier
    /// file, check it against the exact search, and put it in place if it keeps enough. The table
    /// is read in short slices for the scores and written once to swap the file in; the work
    /// between does not hold its lock, so no query waits for it.
    pub(super) fn layout_now(&self, why: &'static str) {
        let t0 = Instant::now();
        let now_s = u32::try_from(now_unix_ms() / 1000).unwrap_or(u32::MAX);
        let Some(plan) = self.plan(now_s) else {
            return;
        };
        match build_layout(&plan, self.cap(plan.cut)) {
            Ok((tier, hot, cold)) => {
                self.settle_layout(why, t0, &plan, tier.map(Arc::new), (hot, cold))
            }
            Err(e) => {
                self.layouts.lock().unwrap().retry_at = Some(Instant::now() + BACKSTOP);
                self.error(format!("laying out the vector records: {e:#}"));
            }
        }
    }

    /// A layout is made: check it, and put it in place, or leave the search exact and say why.
    fn settle_layout(
        &self,
        why: &'static str,
        t0: Instant,
        plan: &Plan,
        cand: Option<Arc<TierFile>>,
        (hot, cold): (usize, usize),
    ) {
        let checked = cand.as_ref().map(|c| self.self_check(c, plan));
        let drop_cand = || {
            let _ = fs::remove_file(plan.cand_path());
        };
        if matches!(checked, Some(None)) {
            // Nothing could be checked (every sample skipped, or the table moved on): a layout
            // nothing has checked is not put in place, and is tried again soon.
            drop_cand();
            self.layouts.lock().unwrap().retry_at = Some(Instant::now() + BACKSTOP);
            tracing::info!(
                "index: a layout of the vector records could not be checked, and is not used"
            );
            return;
        }
        let share = checked.flatten();
        let kept = share.is_none_or(|(s, how)| s >= how.bar());
        let mut t = self.table.write().unwrap();
        if t.generation != plan.generation || t.caches.first().is_none_or(|c| c.epoch != plan.epoch)
        {
            // A forget or a rebuild renumbered the records while this was made: its numbers
            // mean other records now, and the file with them is dropped.
            drop(t);
            drop_cand();
            return;
        }
        let groups = cand.as_ref().map_or(0, |c| c.groups());
        let tier = if kept {
            cand.and_then(|c| name_checked(plan, c))
        } else {
            None
        };
        let placed = tier.is_some() || share.is_none();
        t.install_tier(tier);
        drop(t);
        if !placed {
            drop_cand();
        }
        self.note_layout(
            why,
            t0.elapsed().as_secs_f64() * 1e3,
            kept && placed,
            share,
            (hot, cold, groups),
        );
    }

    /// What a layout pass did, for the status and the log.
    fn note_layout(
        &self,
        why: &'static str,
        ms: f64,
        kept: bool,
        share: Option<(f32, Checked)>,
        (hot, cold, groups): (usize, usize, usize),
    ) {
        let mut l = self.layouts.lock().unwrap();
        l.retry_at = None;
        if kept {
            l.failed = None;
            l.stats = Layouts {
                count: l.stats.count + 1,
                last_ms: ms,
                last_hot: hot as u64,
                last_cold: cold as u64,
                last_groups: groups as u64,
                last_at_ms: now_unix_ms(),
                last_share: share.map(|x| x.0),
                last_checked: share.map(|x| x.1),
            };
            tracing::info!(
                why,
                hot,
                cold,
                groups,
                ?share,
                ms,
                "index: the vector records laid out"
            );
            return;
        }
        let (share, how) = share.unwrap_or((0.0, Checked::Records));
        l.failed = Some(Failed {
            share,
            bar: how.bar(),
            at: Instant::now(),
            alive: hot + cold,
        });
        drop(l);
        tracing::warn!(
            kept = share,
            needs = how.bar(),
            checked_on = how.word(),
            ms,
            "index: a layout of the vector records kept too little of the exact search's best ten: searches stay exact until a rebuild passes"
        );
    }

    /// Remember the vector of a query a turn's recall served: layouts are checked on these.
    pub(super) fn remember_query(&self, full: &[f32]) {
        let mut r = self.recent.lock().unwrap();
        if r.len() == RECENT {
            r.pop_front();
        }
        r.push_back(full.to_vec());
    }

    /// What a search by the candidate layout keeps of the exact search's best ten. On real
    /// queries the tender served, when there are enough ([`REAL_MIN`]): up to [`CHECK_QUERIES`] of
    /// them, spread through those it remembers. Else on cold records' own vectors (each leaves its
    /// own node out, so a record cannot find itself), spread through the groups, against the
    /// higher bar. `None` where nothing could be checked, or the table moved on while it ran.
    pub(super) fn self_check(&self, cand: &Arc<TierFile>, plan: &Plan) -> Option<(f32, Checked)> {
        let real: Vec<Vec<f32>> = {
            let r = self.recent.lock().unwrap();
            let stride = (r.len() / CHECK_QUERIES).max(1);
            r.iter()
                .step_by(stride)
                .take(CHECK_QUERIES)
                .cloned()
                .collect()
        };
        let live = |t: &Table| {
            t.generation == plan.generation
                && t.caches.len() == 1
                && t.caches[0].epoch == plan.epoch
        };
        let (mut sum, mut n) = (0f32, 0usize);
        let mut share = |t: &Table, full: &[f32], f: &RowFilter| {
            let cut = t.caches[0].cut;
            let (Ok(want), Ok(got)) = (
                t.search_with(full, cut, f, 10, None),
                t.search_with(full, cut, f, 10, Some(cand.clone())),
            ) else {
                return;
            };
            if want.is_empty() {
                return;
            }
            let want: HashSet<usize> = want.iter().map(|x| x.0).collect();
            sum += got.iter().filter(|x| want.contains(&x.0)).count() as f32 / want.len() as f32;
            n += 1;
        };
        if real.len() >= REAL_MIN {
            for full in &real {
                let t = self.table.read().unwrap();
                if !live(&t) {
                    return None;
                }
                share(&t, full, &t.filter(None, &[], &Filters::default()));
            }
            return (n > 0).then(|| (sum / n as f32, Checked::Queries));
        }
        let cold = cand.assigned() - cand.hot_n();
        if cold == 0 {
            return None;
        }
        let stride = (cold / CHECK_QUERIES).max(1);
        for p in (cand.hot_n()..cand.assigned())
            .step_by(stride)
            .take(CHECK_QUERIES)
        {
            let e = cand.entry(p);
            let t = self.table.read().unwrap();
            if !live(&t) || e as usize >= t.caches[0].len() {
                return None;
            }
            let c0 = &t.caches[0];
            let Some(row) = t
                .holders(c0.hash_of(e))
                .into_iter()
                .find(|&i| t.row_vec(i) == Some((0, e)))
            else {
                continue;
            };
            let Ok(Some(full)) = t.full_of(row as usize) else {
                continue;
            };
            let mut f = t.filter(None, &[], &Filters::default());
            f.skip_node = Some(t.row_node(row));
            share(&t, &full, &f);
        }
        (n > 0).then(|| (sum / n as f32, Checked::Records))
    }

    /// How the vector side reads now, for the status.
    pub(super) fn tier_status(&self) -> Option<theseus_protocol::index::IndexTierStatus> {
        if !self.enabled() {
            return None;
        }
        let t = self.table.try_read().ok()?;
        let c = t.caches.first()?;
        let alive = c.len().saturating_sub(c.dead()) as u64;
        let cap = self.cap(c.cut) as u64;
        let l = self.layouts.lock().unwrap();
        let (mode, why_exact) = match (&t.tier, &l.failed) {
            (Some(_), _) => ("tiered", None),
            (None, Some(f)) => (
                "exact",
                Some(format!(
                    "the last layout of the records kept {:.0}% of the exact search's best ten on its own check, and it must keep {:.0}%",
                    f.share * 100.0,
                    f.bar * 100.0
                )),
            ),
            (None, None) if alive <= cap => ("all_hot", None),
            (None, None) => (
                "exact",
                Some("the store is over the budget and its first layout is made when the machine is quiet".to_string()),
            ),
        };
        let hot = t.tier.as_ref().map_or(alive, |x| x.hot_n() as u64);
        Some(theseus_protocol::index::IndexTierStatus {
            mode: mode.into(),
            resident_mb: self.cfg.resident_mb,
            hot,
            cold: alive.saturating_sub(hot),
            groups: t.tier.as_ref().map_or(0, |x| x.groups() as u64),
            promoted: t.promoted.len() as u64,
            kept: l
                .failed
                .map(|f| f64::from(f.share))
                .or(l.stats.last_share.map(f64::from)),
            checked_on: l.stats.last_checked.map(|c| c.word().to_string()),
            layouts: l.stats.count,
            last_layout_ms: l.stats.last_ms,
            last_layout_at_ms: l.stats.last_at_ms,
            why_exact,
        })
    }

    /// Take the signals the tender read off the log: the hot set follows them at the next
    /// layout, and they are kept beside the vectors across a restart.
    pub fn on_signals(&self, signals: &[heat::Signal]) {
        if !self.enabled() || signals.is_empty() {
            return;
        }
        let mut changed: Vec<heat::Signal> = Vec::new();
        {
            let mut t = self.table.write().unwrap();
            for s in signals {
                if t.signals.apply(s) {
                    changed.push(s.clone());
                    // A cold record a turn used, or the operator vouched for, is searched with
                    // the hot ones from now, and made hot by the next layout.
                    if t.signals.get(s.node()).wants_hot() {
                        t.promote_node(s.node());
                    }
                }
            }
        }
        if changed.is_empty() {
            return;
        }
        if let Err(e) = heat::Signals::append(&self.dir.join(HEAT_FILE), &changed) {
            tracing::warn!(error = %e, "index: the hot set's signals were not kept");
        }
    }

    /// Nodes forgotten (or erased): nothing of their signals stays, in memory or in the file.
    pub(super) fn forget_signals(&self, nodes: &[String]) {
        let gone = {
            let mut t = self.table.write().unwrap();
            nodes
                .iter()
                .map(|n| t.signals.forget(n))
                .fold(false, |a, b| a | b)
        };
        if gone {
            self.save_signals();
        }
    }

    pub(super) fn save_signals(&self) {
        let t = self.table.read().unwrap();
        if let Err(e) = t.signals.save(&self.dir.join(HEAT_FILE)) {
            tracing::warn!(error = %e, "index: the hot set's signals were not kept");
        }
    }

    /// Read the signals kept beside the vectors (and write them compact when the file has grown
    /// well past what it says).
    pub(super) fn load_signals(&self) {
        let path = self.dir.join(HEAT_FILE);
        match heat::Signals::load(&path) {
            Ok((s, lines)) => {
                let compact = lines > 2 * s.len() + 1000;
                self.table.write().unwrap().signals = s;
                if compact {
                    self.save_signals();
                }
            }
            Err(e) => tracing::warn!(error = %e, "index: the hot set's signals were not read"),
        }
    }
}

/// A layout that passed its check gets its name (the candidate is renamed; the map is of the same
/// file), or is left out if it cannot.
fn name_checked(plan: &Plan, cand: Arc<TierFile>) -> Option<Arc<TierFile>> {
    match fs::rename(plan.cand_path(), &plan.tier_path) {
        Ok(()) => Some(cand),
        Err(e) => {
            tracing::warn!(error = %e, "index: a checked layout could not be put in place");
            None
        }
    }
}

/// The tier file a plan makes under a hot region of at most `cap` records (none where everything
/// fits, and any old one is removed), the hot records it holds and the cold ones it does not.
pub(super) fn build_layout(
    plan: &Plan,
    cap: usize,
) -> anyhow::Result<(Option<TierFile>, usize, usize)> {
    let alive = plan
        .scores
        .iter()
        .filter(|s| **s > f32::NEG_INFINITY)
        .count();
    let Some(hot) = choose_hot(&plan.scores, cap) else {
        match fs::remove_file(&plan.tier_path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("removing a tier file the store no longer needs"),
        }
        return Ok((None, alive, 0));
    };
    let scan = &plan.scan;
    let mut is_hot = vec![false; plan.n];
    hot.iter().for_each(|&e| is_hot[e as usize] = true);
    let cold: Vec<u32> = (0..plan.n as u32)
        .filter(|&e| plan.scores[e as usize] > f32::NEG_INFINITY && !is_hot[e as usize])
        .collect();
    let groups = group_cold(&ScanCuts(scan.clone()), &cold, plan.cut);
    let layout = Layout {
        cut: plan.cut,
        hot: &hot,
        groups: &groups,
        made_from: cold.len() as u64,
    };
    let file = TierFile::write(
        &plan.cand_path(),
        &layout,
        &|e| (scan.scale(e), scan.q(e).to_vec()),
        &|e| scan.hash(e),
        plan.n,
    )
    .context("writing the tier file")?;
    Ok((Some(file), hot.len(), cold.len()))
}

/// The cold records in groups of similar material: about their square root in number, each
/// record in its nearest group, each group's entries in order. A group no record reached is not
/// kept.
fn group_cold(src: &ScanCuts, cold: &[u32], cut: usize) -> Vec<Group> {
    if cold.is_empty() {
        return Vec::new();
    }
    let centres = groups::make(
        src,
        cold,
        cut,
        groups::group_count(cold.len()),
        SAMPLE,
        ROUNDS,
    );
    let of = groups::assign(src, &centres, cold);
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); centres.len()];
    for (&e, &g) in cold.iter().zip(&of) {
        buckets[g as usize].push(e);
    }
    buckets
        .into_iter()
        .enumerate()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(g, entries)| {
            let (c, scale) = centres.centre(g);
            Group {
                entries,
                centre: c.to_vec(),
                scale,
            }
        })
        .collect()
}

/// The file of signals, beside the vector files.
pub(super) const HEAT_FILE: &str = "heat.log";

/// Open the tier file of `c`, if it is a good one.
pub(super) fn open_tier(c: &super::cache::Cache) -> Option<TierFile> {
    let path = c.tier_path();
    if !path.exists() {
        return None;
    }
    let r = TierFile::open(&path, c.cut, c.len(), &|e| c.hash_of(e));
    match r {
        Ok(t) => Some(t),
        Err(why) => {
            tracing::info!(file = %path.display(), why, "index: a tier file is left out and made again");
            let _ = fs::remove_file(&path);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_holds_what_it_should() {
        // 128 MiB: three quarters at 264 bytes a record is about 380,000 records, more than the
        // 291,000 the owner's store holds today.
        assert!(hot_cap(128, 256) > 291_000);
        assert!(hot_cap(128, 256) < 400_000);
        assert!(check_resident_mb(128).is_ok());
        assert!(check_resident_mb(47).unwrap_err().contains("too small"));
    }

    #[test]
    fn everything_fits_means_no_layout_and_otherwise_the_highest_scores_are_hot() {
        let scores = [0.5, f32::NEG_INFINITY, 2.0, -9.0, 1.0, 0.1];
        assert_eq!(choose_hot(&scores, 5), None);
        // Four records have an alive row and one of them is doubted: with room for two, the two
        // best; the doubted one is never hot however much room.
        let hot = choose_hot(&scores, 2).unwrap();
        assert_eq!(hot, vec![2, 4]);
        let roomy = choose_hot(&scores, 4).unwrap();
        assert_eq!(roomy, vec![0, 2, 4, 5]);
        assert!(!roomy.contains(&3));
    }
}
