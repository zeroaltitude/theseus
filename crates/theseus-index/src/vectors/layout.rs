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
//! does not place (those embedded since; and, until the groups exist, the cold part), then
//! (step three) the nearest groups. A record's rows are found by its text and each is filtered as
//! the exact scan filters it, so a forgotten, dead or filtered-out row is never returned from any
//! region: a record is a name for rows, and only alive rows answer.
//!
//! **Explicit searches are exact** ([`Reach::Exact`]): the memory search tool, the neighbours
//! lookup and anything that does not ask for [`Reach::Fast`] scan everything, hot and cold.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
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

    /// The layout pass's reading of the table: every record of the current stamp's scan file with
    /// its best row's score. `None` where a layout cannot be made now (no scan file, a stamp
    /// being re-embedded, the rows not every chunk yet).
    pub(super) fn plan_layout(&self, now_s: u32) -> Option<Plan> {
        if !self.settled() || self.caches.len() != 1 {
            return None;
        }
        let c = &self.caches[0];
        let scan = c.scan.clone()?;
        let n = scan.len();
        let mut scores = vec![f32::NEG_INFINITY; n];
        for i in 0..self.row_count() as u32 {
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
        Some(Plan {
            generation: self.generation,
            epoch: c.epoch,
            tier_path: c.tier_path(),
            cut: c.cut,
            scan,
            n,
            scores,
        })
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
    /// by their int8 cuts, each record's alive rows filtered by `keep`. The same candidates, with
    /// the same scores and the same ties, as the exact scan gives over those records.
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
                consider(tier.entry(p), dot_i8(tier.q(p), q) as f32 * tier.scale(p));
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

/// What a layout pass decided about the store.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct Layouts {
    pub count: u64,
    pub last_ms: f64,
    pub last_hot: u64,
    pub last_cold: u64,
    pub last_at_ms: u64,
}

/// A layout's state beside the table: when it may be tried again, and what it last did.
#[derive(Debug, Default)]
pub(super) struct LayoutState {
    pub retry_at: Option<Instant>,
    pub stats: Layouts,
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
    fn cap(&self, cut: usize) -> usize {
        hot_cap(self.cfg.resident_mb, cut)
    }

    /// Why a layout is due now, if it is: the store does not fit the budget and has no layout,
    /// or the layout is of a store that has grown past it. Never while the rows are not every
    /// chunk, a stamp is being re-embedded, or a query is embedding.
    pub(super) fn layout_due(&self) -> Option<&'static str> {
        if self
            .layouts
            .lock()
            .unwrap()
            .retry_at
            .is_some_and(|at| Instant::now() < at)
            || !self.ahead.may_backfill()
        {
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
            (None, false) => Some("over the budget"),
            (Some(_), true) => Some("back under the budget"),
            (Some(tier), false) => {
                let grown = c.len().saturating_sub(tier.covers());
                if grown >= super::scan::FOLD.max(tier.covers() / 20) {
                    return Some("grown");
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

    /// Make the layout: score the records, choose the hot set, write the tier file, and put it
    /// in place. The table is read once for the scores and written once to swap the file in; the
    /// work between does not hold its lock, so no query waits for it.
    pub(super) fn layout_now(&self, why: &'static str) {
        let t0 = Instant::now();
        let now_s = u32::try_from(now_unix_ms() / 1000).unwrap_or(u32::MAX);
        let plan = self.table.read().unwrap().plan_layout(now_s);
        let Some(plan) = plan else {
            return;
        };
        let r = build_layout(&plan, self.cap(plan.cut));
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        match r {
            Ok((tier, hot, cold)) => {
                let mut t = self.table.write().unwrap();
                let same = t.generation == plan.generation
                    && t.caches.first().is_some_and(|c| c.epoch == plan.epoch);
                if !same {
                    // A forget or a rebuild renumbered the records while this was made: its
                    // numbers mean other records now, and the file with them is dropped.
                    drop(t);
                    let _ = fs::remove_file(&plan.tier_path);
                    return;
                }
                t.tier = tier.map(Arc::new);
                drop(t);
                let mut s = self.layouts.lock().unwrap();
                s.retry_at = None;
                s.stats = Layouts {
                    count: s.stats.count + 1,
                    last_ms: ms,
                    last_hot: hot as u64,
                    last_cold: cold as u64,
                    last_at_ms: now_unix_ms(),
                };
                tracing::info!(why, hot, cold, ms, "index: the vector records laid out");
            }
            Err(e) => {
                self.layouts.lock().unwrap().retry_at = Some(Instant::now() + BACKSTOP);
                self.error(format!("laying out the vector records: {e:#}"));
            }
        }
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
        &plan.tier_path,
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
