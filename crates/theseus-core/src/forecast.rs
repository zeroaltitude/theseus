//! A task's forecast (theseus-zz752): before a task runs, what finished tasks
//! like it cost and how long they took ("about $0.60 and 8 minutes, at most
//! $1.50"), and a word once when it passes the 90th percentile.
//!
//! - **The table** is the newest `KEEP` (2,000) finished tasks, in memory:
//!   each one's class, its brief's terms, its dollars, minutes and loops. The
//!   board feeds it as a task ends (`Book::ended`), and a thread after
//!   serving fills it from the store (`read_history`: one scan of the task
//!   executions and the task graph; nothing is stored for it). Nothing is a
//!   model call, and nothing waits on the index: a forecast is a memory read.
//! - **Like it** is a task of the same class (`class_of`: how much its
//!   arrangement says it is done by), then the nearest brief by the words its
//!   title and objective share (Jaccard over hashed words), the newest on a
//!   tie, at most `K` (25). Fewer than `MIN_LIKE` (5) is no forecast. The
//!   index tender answers over a socket, and a brief's terms are not a memory
//!   read there; its embeddings are not asked for (no model call).
//! - **Numbers only** leave this module: the percentiles and a count. No
//!   brief, title or id of the tasks they come from rides a view, so the
//!   place rule has nothing to hold back.
//! - **The calibration** (`Table::calibrate`) is a backtest: each task that
//!   finished in the window gets the forecast it would have had when it
//!   opened, from the tasks that had finished before, and is counted inside
//!   or outside its p90 of dollars and of minutes.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use theseus_protocol::forecast::{
    CalibrationClass, CalibrationWindow, ForecastCalibrationResult, MIN_LIKE,
};
use theseus_protocol::tasks::TaskRecord;

/// Finished tasks kept.
pub const KEEP: usize = 2_000;

/// The nearest finished tasks a forecast is made from, at most.
pub const K: usize = 25;

/// A task that ran longer than this is not one the minutes learn from: it
/// slept (a deferred start) or was left open.
pub const MAX_MINUTES: f64 = 24.0 * 60.0;

/// Terms kept for a brief.
const TERMS: usize = 48;

/// Memoized forecasts kept; the oldest go when the map is full.
const MEMO: usize = 4_096;

const DAY_MS: u64 = 86_400_000;

/// One finished task.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Its task record's id (`tsk_…`).
    pub task: String,
    pub class: String,
    /// The hashed words of its title and objective, sorted.
    pub terms: Vec<u64>,
    pub usd: f64,
    pub min: f64,
    /// Model loops, when the board counted them as the task ran.
    pub loops: Option<u32>,
    pub started_ms: u64,
    pub ended_ms: u64,
}

/// A forecast: the percentiles of the tasks it is made from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forecast {
    pub usd_p50: f64,
    pub usd_p90: f64,
    pub min_p50: f64,
    pub min_p90: f64,
    /// Over the rows that counted loops, when any did.
    pub loops_p50: Option<f64>,
    pub loops_p90: Option<f64>,
    pub n: u32,
}

impl Forecast {
    /// The wire's form, not yet past anything.
    pub fn wire(&self) -> theseus_protocol::forecast::WorkForecast {
        theseus_protocol::forecast::WorkForecast {
            usd_p50: self.usd_p50,
            usd_p90: self.usd_p90,
            min_p50: self.min_p50,
            min_p90: self.min_p90,
            n: self.n,
            past: false,
            read: None,
            read_p: None,
        }
    }
}

/// What asking for a forecast found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// The table has not been read from the store yet.
    Loading,
    /// This many are like it, which is too few.
    Few(u32),
    Some(Forecast),
}

impl Outcome {
    /// The forecast, when there is one.
    pub fn forecast(&self) -> Option<Forecast> {
        match self {
            Outcome::Some(f) => Some(*f),
            _ => None,
        }
    }

    /// `forecast 8 min (p90 14) · $0.60 (p90 $1.50) · from 23 like it`, or
    /// why there is none.
    pub fn words(&self) -> String {
        match self {
            Outcome::Some(f) => f.wire().words(),
            Outcome::Few(n) => theseus_protocol::forecast::none_words(*n),
            Outcome::Loading => "no forecast yet: history is still being read".into(),
        }
    }
}

/// A task's class: what its arrangement says it is done by. A task with no
/// acceptance line is `open`, one or two lines `checked`, three or more
/// `specified`: the objective and acceptance a task's record holds are its
/// arrangement's pieces, so the class reads the same for a task started now
/// and one started before the board existed.
pub fn class_of(t: &TaskRecord) -> &'static str {
    match t.acceptance.iter().filter(|a| !a.trim().is_empty()).count() {
        0 => "open",
        1 | 2 => "checked",
        _ => "specified",
    }
}

/// Words too common to tell two briefs apart.
const STOP: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "then", "than", "are", "was",
    "were", "will", "all", "any", "not", "but", "its", "has", "have", "can", "should", "task",
    "report", "run", "make", "use", "using", "each", "also", "when", "what", "which",
];

/// The sorted hashed words of a title and an objective.
pub fn terms_of(title: &str, objective: &str) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for text in [title, objective] {
        for w in text.split(|c: char| !c.is_alphanumeric()) {
            if w.chars().count() < 3 {
                continue;
            }
            let w = w.to_lowercase();
            if STOP.contains(&w.as_str()) {
                continue;
            }
            // FNV-1a: no dependency, the same on every run.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in w.bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            out.push(h);
        }
    }
    out.sort_unstable();
    out.dedup();
    out.truncate(TERMS);
    out
}

/// How much two sorted term lists share, 0 to 1.
fn jaccard(a: &[u64], b: &[u64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut both) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                both += 1;
                i += 1;
                j += 1;
            }
        }
    }
    both as f64 / (a.len() + b.len() - both) as f64
}

/// The `p`th percentile of `v` (sorted in place), by linear interpolation
/// between the nearest ranks.
pub fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    match v.len() {
        0 => 0.0,
        1 => v[0],
        n => {
            let at = p * (n - 1) as f64;
            let (lo, hi) = (at.floor() as usize, at.ceil() as usize);
            v[lo] + (v[hi] - v[lo]) * (at - lo as f64)
        }
    }
}

/// What a forecast is asked about: a task's class and terms.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub class: String,
    pub terms: Vec<u64>,
}

impl Query {
    pub fn of(t: &TaskRecord) -> Self {
        Self {
            class: class_of(t).to_string(),
            terms: terms_of(&t.title, &t.objective),
        }
    }
}

/// The finished tasks, newest `KEEP`, oldest first.
#[derive(Debug, Default)]
pub struct Table {
    rows: VecDeque<Row>,
    ids: HashSet<String>,
    /// The store has been read.
    ready: bool,
}

impl Table {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// One more finished task; a task already held is not held twice.
    pub fn add(&mut self, row: Row) {
        if !self.ids.insert(row.task.clone()) {
            return;
        }
        // Rows stay in order of their end; a late one is inserted in place.
        let at = self.rows.partition_point(|r| r.ended_ms <= row.ended_ms);
        self.rows.insert(at, row);
        while self.rows.len() > KEEP {
            if let Some(old) = self.rows.pop_front() {
                self.ids.remove(&old.task);
            }
        }
    }

    /// The forecast for `q`: the tasks of its class, the nearest brief first
    /// and the newest on a tie, at most `K`; none under `MIN_LIKE`.
    pub fn forecast(&self, q: &Query) -> Outcome {
        self.forecast_among(q, |_| true)
    }

    fn forecast_among(&self, q: &Query, keep: impl Fn(&Row) -> bool) -> Outcome {
        // Nearest first; the newest first among equals (rows run oldest
        // first, so a later index is newer).
        let mut near: Vec<(f64, usize)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.class == q.class && keep(r))
            .map(|(i, r)| (jaccard(&q.terms, &r.terms), i))
            .collect();
        near.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.1.cmp(&a.1))
        });
        near.truncate(K);
        let n = near.len() as u32;
        if n < MIN_LIKE {
            return Outcome::Few(n);
        }
        let rows: Vec<&Row> = near.iter().map(|(_, i)| &self.rows[*i]).collect();
        let mut usd: Vec<f64> = rows.iter().map(|r| r.usd).collect();
        let mut min: Vec<f64> = rows.iter().map(|r| r.min).collect();
        let mut loops: Vec<f64> = rows.iter().filter_map(|r| r.loops.map(f64::from)).collect();
        let (lp50, lp90) = if loops.is_empty() {
            (None, None)
        } else {
            (
                Some(percentile(&mut loops, 0.5)),
                Some(percentile(&mut loops, 0.9)),
            )
        };
        Outcome::Some(Forecast {
            usd_p50: percentile(&mut usd, 0.5),
            usd_p90: percentile(&mut usd, 0.9),
            min_p50: percentile(&mut min, 0.5),
            min_p90: percentile(&mut min, 0.9),
            loops_p50: lp50,
            loops_p90: lp90,
            n,
        })
    }

    /// How often the forecast held: for each window, the tasks that finished
    /// in the last `days` days, by class, each judged against the forecast it
    /// would have had when it opened (the tasks that had finished before it
    /// started, itself never among them).
    pub fn calibrate(&self, now_ms: u64, windows: &[u32]) -> Vec<CalibrationWindow> {
        windows
            .iter()
            .map(|&days| {
                let since = now_ms.saturating_sub(u64::from(days) * DAY_MS);
                let mut by_class: std::collections::BTreeMap<&str, CalibrationClass> =
                    std::collections::BTreeMap::new();
                for r in self.rows.iter().filter(|r| r.ended_ms >= since) {
                    let c = by_class
                        .entry(r.class.as_str())
                        .or_insert_with(|| CalibrationClass {
                            class: r.class.clone(),
                            finished: 0,
                            forecast: 0,
                            inside: 0,
                        });
                    c.finished += 1;
                    let q = Query {
                        class: r.class.clone(),
                        terms: r.terms.clone(),
                    };
                    let known = |o: &Row| o.task != r.task && o.ended_ms <= r.started_ms;
                    if let Outcome::Some(f) = self.forecast_among(&q, known) {
                        c.forecast += 1;
                        if r.usd <= f.usd_p90 && r.min <= f.min_p90 {
                            c.inside += 1;
                        }
                    }
                }
                CalibrationWindow {
                    days,
                    classes: by_class.into_values().collect(),
                }
            })
            .collect()
    }
}

struct Inner {
    table: Table,
    /// A task record's forecast, said once: the open's line and the board's
    /// view are the same numbers.
    memo: HashMap<String, Outcome>,
    memo_order: VecDeque<String>,
}

/// The daemon's finished tasks, shared by the task tool (the first line of a
/// task) and the work board (the view, the notice).
pub struct Book {
    inner: Mutex<Inner>,
}

impl Default for Book {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                table: Table::default(),
                memo: HashMap::new(),
                memo_order: VecDeque::new(),
            }),
        }
    }
}

impl Book {
    /// A task's forecast, from memory. Said once per task record while the
    /// table is read: a task that ends meanwhile does not change what its
    /// open said.
    pub fn for_task(&self, t: &TaskRecord) -> Outcome {
        let mut g = self.inner.lock().unwrap();
        if !g.table.ready {
            return Outcome::Loading;
        }
        if let Some(o) = g.memo.get(&t.id) {
            return *o;
        }
        let o = g.table.forecast(&Query::of(t));
        if g.memo.len() >= MEMO {
            if let Some(old) = g.memo_order.pop_front() {
                g.memo.remove(&old);
            }
        }
        g.memo.insert(t.id.clone(), o);
        g.memo_order.push_back(t.id.clone());
        o
    }

    /// A task ended: its row joins the table, once.
    pub fn ended(&self, row: Row) {
        self.inner.lock().unwrap().table.add(row);
    }

    /// The store has been read: the history's rows, and from now a forecast
    /// is asked of the table.
    pub fn loaded(&self, rows: Vec<Row>) {
        let mut g = self.inner.lock().unwrap();
        for r in rows {
            g.table.add(r);
        }
        g.table.ready = true;
    }

    pub fn ready(&self) -> bool {
        self.inner.lock().unwrap().table.ready
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `forecast.calibration`'s answer.
    pub fn calibration(&self, now_ms: u64) -> ForecastCalibrationResult {
        // The table is copied out, so the backtest never holds the board's
        // lock.
        let (rows, ready) = {
            let g = self.inner.lock().unwrap();
            (g.table.rows.clone(), g.table.ready)
        };
        let table = Table {
            ids: HashSet::new(),
            rows,
            ready,
        };
        ForecastCalibrationResult {
            at_ms: now_ms,
            table: table.len() as u32,
            ready,
            windows: table.calibrate(now_ms, &[7, 30]),
        }
    }
}

/// The task's first line in the conversation: `⏳ task c91e "migrate
/// fixtures…" · forecast 8 min (p90 14) · $0.60 (p90 $1.50) · from 23 like
/// it`, or why there is no forecast.
pub fn first_line(short: &str, title: &str, o: &Outcome) -> String {
    let line = title.lines().next().unwrap_or("").trim();
    let title = if line.chars().count() > 40 {
        let cut: String = line.chars().take(39).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.to_string()
    };
    format!("⏳ task {short} \"{title}\" · {}", o.words())
}

mod history;
pub use history::{read_history, row_of};

impl crate::Core {
    /// Read the finished tasks once, after serving, on the blocking pool (a
    /// thread holding the core weakly), and say the forecast of each task
    /// that is open on the work board if it has been seeded. Called after
    /// serving and when the board warms; the second finds the table read and
    /// reads nothing.
    pub fn warm_forecasts(self: &std::sync::Arc<Self>) {
        let weak = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = weak.upgrade() else { return };
            let t0 = std::time::Instant::now();
            if !core.tools.forecasts.ready() {
                core.tools.forecasts.loaded(read_history(&core));
                tracing::info!(
                    rows = core.tools.forecasts.len(),
                    ms = t0.elapsed().as_millis() as u64,
                    "forecast: finished tasks read"
                );
            }
            core.push.forecast_open(&core);
        });
    }
}

#[cfg(test)]
mod tests;
