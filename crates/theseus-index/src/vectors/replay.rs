//! `theseus-index replay`: a file of query vectors put to the exact scan and to the tiered search
//! over an index's vectors, and the shares printed (issue 21 of 1.0.7).
//!
//! The owner's check of the hot set: the real recalls of the last 30 days, their queries
//! embedded, replayed against a **private copy** of the index (the tool writes the tier file and
//! the scan file into the copy's `vectors/` directory, and must not be pointed at the live index
//! of a running tender). It reads the vector files as a start does, makes the layout the tender
//! makes under the budget it is given (`--resident-mb`, or `--hot-records` to size the hot region
//! directly), whether or not that layout passes its own check (it says which), and for every
//! query puts it to both searches and counts how many of the exact search's best 6 and best 10
//! the tiered one kept, overall and for the results that lie in the cold part.
//!
//! The query file is text, one query a line: 768 numbers separated by spaces or commas, or a JSON
//! array of them, or a JSON object `{"vector": [...], "as_of": <position>, "exclude_sessions":
//! [...]}` (the two filters a recall puts on its query). Lines that are empty or begin with `#`
//! are skipped.

use std::path::PathBuf;
use std::time::Instant;

use super::*;
use crate::engine::{open_or_create, Engine};

/// What to replay, and under which budget.
#[derive(Debug, Clone)]
pub struct Options {
    /// A private copy of an index directory (`<state>/index`).
    pub index: PathBuf,
    /// The file of query vectors.
    pub queries: PathBuf,
    /// The budget the layout is made under (`[index] resident_mb`).
    pub resident_mb: u64,
    /// Or the hot region's size in records, over the budget.
    pub hot_records: Option<usize>,
}

struct Query {
    vector: Vec<f32>,
    as_of: Option<u64>,
    exclude: Vec<String>,
}

fn numbers(v: &serde_json::Value) -> anyhow::Result<Vec<f32>> {
    v.as_array()
        .context("a vector is an array of numbers")?
        .iter()
        .map(|x| x.as_f64().map(|f| f as f32).context("a number"))
        .collect()
}

fn parse(line: &str, full: usize) -> anyhow::Result<Option<Query>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let q = if line.starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(line)?;
        Query {
            vector: numbers(v.get("vector").context("no `vector`")?)?,
            as_of: v.get("as_of").and_then(serde_json::Value::as_u64),
            exclude: v
                .get("exclude_sessions")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        }
    } else if line.starts_with('[') {
        Query {
            vector: numbers(&serde_json::from_str(line)?)?,
            as_of: None,
            exclude: Vec::new(),
        }
    } else {
        let vector = line
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<f32>()
                    .with_context(|| format!("not a number: {s:?}"))
            })
            .collect::<anyhow::Result<Vec<f32>>>()?;
        Query {
            vector,
            as_of: None,
            exclude: Vec::new(),
        }
    };
    anyhow::ensure!(
        q.vector.len() == full,
        "a query of {} numbers where the stamp's vectors have {full}",
        q.vector.len()
    );
    Ok(Some(q))
}

pub(super) fn median_p95(v: &mut [f64]) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    (
        v[v.len() / 2],
        v[((v.len() as f64 * 0.95) as usize).min(v.len() - 1)],
    )
}

/// Replay `o.queries` against `o.index` and describe what the tiered search kept.
pub fn run(o: &Options) -> anyhow::Result<String> {
    let (index, fields) = open_or_create(&crate::state::Paths::new(&o.index).bm25())?;
    let engine = Engine::new(index, fields)?;
    let mut cfg = VectorConfig::new(Some(o.index.join("no-weights")));
    cfg.resident_mb = o.resident_mb;
    cfg.hot_records = o.hot_records;
    let (cut, full) = (cfg.spec.cut, cfg.spec.config.hidden);
    let v = Vectors::new(cfg, &o.index);
    v.open_files()?;
    v.reconcile(&engine)?;
    v.on_caught_up();
    let text = fs::read_to_string(&o.queries)
        .with_context(|| format!("reading {}", o.queries.display()))?;
    let mut queries = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if let Some(q) = parse(line, full).with_context(|| format!("line {}", n + 1))? {
            queries.push(q);
        }
    }
    anyhow::ensure!(!queries.is_empty(), "no queries in {}", o.queries.display());
    // The layout the tender makes, put in place whether or not it passes its own check, so the
    // shares can be read either way.
    let now_s = u32::try_from(now_unix_ms() / 1000).unwrap_or(u32::MAX);
    let plan = v.plan(now_s).context(
        "the table is not ready for a layout (no scan file, or a stamp being re-embedded)",
    )?;
    let t0 = Instant::now();
    let (tier, hot, cold) = layout::build_layout(&plan, v.cap(plan.cut))?;
    let made_ms = t0.elapsed().as_secs_f64() * 1e3;
    let tier = tier.map(Arc::new);
    let kept = tier.as_ref().and_then(|t| v.self_check(t, &plan));
    let (kept, how) = (kept.map(|k| k.0), kept.map(|k| k.1));
    v.table.write().unwrap().install_tier(tier.clone());
    let mut out = String::new();
    out.push_str(&format!(
        "replay of {} queries on {}\n",
        queries.len(),
        o.index.display()
    ));
    out.push_str(&match (&tier, kept) {
        (None, _) => format!(
            "the records fit a budget of {} MiB: every record is hot and a search is the exact one, so the shares are 1 by construction ({hot} records)\n",
            o.resident_mb
        ),
        (Some(t), kept) => format!(
            "layout in {made_ms:.0} ms: {hot} hot, {cold} cold in {} groups, the nearest 1 in {} read; its own check kept {}{}\n",
            t.groups(),
            layout::PROBE_DIVISOR,
            kept.map_or("nothing to check".to_string(), |k| format!("{:.1}% of the exact best ten", k * 100.0)),
            if kept.zip(how).is_some_and(|(k, h)| k < h.bar()) {
                " (UNDER the bar: the tender would serve the exact scan; replayed anyway)"
            } else {
                ""
            }
        ),
    });
    let t = v.table.read().unwrap();
    let mut shares = Shares::default();
    for q in &queries {
        let f = t.filter(q.as_of, &q.exclude, &Filters::default());
        shares.put(&t, &tier, &q.vector, cut, &f)?;
    }
    out.push_str(&shares.describe());
    Ok(out)
}

/// What the tiered search kept of the exact search's best 6 and best 10 over a run of queries
/// (found, wanted), overall and for the exact results in the cold part, and each search's times.
#[derive(Default)]
pub(super) struct Shares {
    all: [(usize, usize); 2],
    cold: [(usize, usize); 2],
    pub fast_ms: Vec<f64>,
    pub exact_ms: Vec<f64>,
}

impl Shares {
    /// Put one query to both searches and count.
    pub(super) fn put(
        &mut self,
        t: &Table,
        tier: &Option<Arc<tier::TierFile>>,
        q: &[f32],
        cut: usize,
        f: &RowFilter,
    ) -> anyhow::Result<()> {
        let t1 = Instant::now();
        let want = t.search_with(q, cut, f, 10, None)?;
        self.exact_ms.push(t1.elapsed().as_secs_f64() * 1e3);
        let t2 = Instant::now();
        let got = t.search_with(q, cut, f, 10, tier.clone())?;
        self.fast_ms.push(t2.elapsed().as_secs_f64() * 1e3);
        for (k, take) in [6usize, 10].into_iter().enumerate() {
            let found: HashSet<usize> = got.iter().take(take).map(|x| x.0).collect();
            for (i, _) in want.iter().take(take) {
                let is_cold = tier
                    .as_ref()
                    .is_some_and(|tr| t.row_vec(*i as u32).is_some_and(|(_, e)| !tr.is_hot(e)));
                let hit = usize::from(found.contains(i));
                self.all[k].1 += 1;
                self.all[k].0 += hit;
                if is_cold {
                    self.cold[k].1 += 1;
                    self.cold[k].0 += hit;
                }
            }
        }
        Ok(())
    }

    /// The shares and the times, in lines.
    pub(super) fn describe(&mut self) -> String {
        let frac = |(a, b): (usize, usize)| {
            if b == 0 {
                "-".to_string()
            } else {
                format!("{:.3}", a as f64 / b as f64)
            }
        };
        let (em, ep) = median_p95(&mut self.exact_ms);
        let (fm, fp) = median_p95(&mut self.fast_ms);
        format!(
            "kept of the exact search's best 6 / best 10, all results: {} / {}\n\
             kept of the best 6 / best 10, results in the cold part ({} and {} of them): {} / {}\n\
             search time, warm: exact median {em:.1} / p95 {ep:.1} ms; tiered median {fm:.1} / p95 {fp:.1} ms\n",
            frac(self.all[0]),
            frac(self.all[1]),
            self.cold[0].1,
            self.cold[1].1,
            frac(self.cold[0]),
            frac(self.cold[1]),
        )
    }
}
