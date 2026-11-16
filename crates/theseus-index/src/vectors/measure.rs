//! The memory measure on a synthetic store (issue 21 of 1.0.7): a tool beside the tests, as
//! `probe.rs` and `heap.rs`'s `fill` are. **Its figures are synthetic**: the vectors are drawn
//! round a few hundred centres (random vectors have no similar groups, so nothing could be
//! grouped), and the queries near stored records, which is what a recall's are.
//!
//! Two phases, in two processes so that the second's memory is the table's and not the first's
//! build:
//!
//! ```text
//! THESEUS_MEASURE_DIR=<dir> THESEUS_MEASURE_NODES=266000 THESEUS_MEASURE_PHASE=build \
//!   cargo nextest run --release -p theseus-index --run-ignored only -E 'test(measure_the_hot_set)' --no-capture
//! THESEUS_MEASURE_DIR=<dir> THESEUS_MEASURE_NODES=266000 THESEUS_MEASURE_MB=128 \
//!   cargo nextest run --release -p theseus-index --run-ignored only -E 'test(measure_the_hot_set)' --no-capture
//! ```
//!
//! `build` writes the vector file (one record a chunk text). The measure reads it as a start
//! does, streams the rows into their file, gives some nodes signals, makes the layout the tender
//! makes, and prints: the process's anonymous and file-backed resident memory at rest and after
//! 300 searches against the budget; the recall of the tiered search against the exact one (best
//! 6 and 10, overall and for the exact results that are cold); search times warm and the first
//! after the kernel dropped the files' pages; the layout's time; and what a promotion costs and
//! that a query is not held by a layout. `THESEUS_MEASURE_NOISE` (default 0.6) sets how tightly
//! the records gather round their centres, `THESEUS_MEASURE_CENTRES` (default 400) how many.

use super::*;
use crate::weights::SplitMix;
use std::os::fd::AsRawFd;

fn var<T: std::str::FromStr>(k: &str, default: T) -> T {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const SESSIONS: u64 = 11_084;

/// Node `i` of `n`: one or two chunks, a text of its own each; its own time spread over sixty
/// days with a seventh of the nodes old (an import), and its place in a session.
fn node(i: u64, n: u64, now_ms: u64) -> NodeChunks {
    let mut rng = SplitMix(i ^ 0x5EED);
    let old = i % 7 == 3;
    let age_days = if old {
        120.0 + 300.0 * (i % 97) as f64 / 97.0
    } else {
        60.0 * (n - i) as f64 / n as f64
    };
    NodeChunks {
        node_id: format!("nd_{i:09}"),
        position: i * 3,
        time_ms: now_ms.saturating_sub((age_days * 86_400_000.0) as u64),
        session: format!("ses_{}", i % SESSIONS),
        kind: ["user_message", "tool_result", "assistant_message"][(i % 3) as usize].into(),
        external: i.is_multiple_of(4),
        chunks: (0..1 + (rng.next_u64() % 2))
            .map(|c| ChunkKey {
                chunk: c as u32,
                hash: u128::from(i) << 8 | u128::from(c) | 1 << 100,
                tokens: 60,
            })
            .collect(),
    }
}

fn anchors(centres: usize, full: usize) -> Vec<Vec<f32>> {
    let mut rng = SplitMix(4242);
    (0..centres)
        .map(|_| (0..full).map(|_| rng.unit()).collect())
        .collect()
}

fn anon_file_mib() -> (f64, f64) {
    let rollup = fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
    let kb = |k: &str| {
        rollup
            .lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.trim().strip_suffix("kB"))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map_or(0.0, |kb| kb / 1024.0)
    };
    let (rss, anon) = (kb("Rss:"), kb("Anonymous:"));
    (anon, rss - anon)
}

/// A line of the report, on stderr as it is made (a run cut short still says what it found) and
/// kept in `out`.
macro_rules! emit {
    ($out:expr, $($arg:tt)*) => {{
        let line = format!($($arg)*);
        eprintln!("{line}");
        $out.push_str(&line);
        $out.push('\n');
    }};
}

/// A stage reached, on stderr, so a long run says where it is.
fn stage(what: &str) {
    eprintln!(
        "[measure {:.0} s] {what}",
        START.get_or_init(Instant::now).elapsed().as_secs_f64()
    );
}

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn drop_cache(dir: &Path) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        if let Ok(f) = File::open(e.path()) {
            // SAFETY: a valid descriptor; advice only.
            unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
        }
    }
}

fn build(dir: &Path, n: u64, centres: usize, noise: f32) {
    let stamp = VectorConfig::new(None).spec.stamp(&embedder::engine_tag());
    let [cut, full] = stamp.dims;
    let anchor = anchors(centres, full);
    let vdir = dir.join("vectors");
    fs::create_dir_all(&vdir).unwrap();
    let mut cache = Cache::open(&vdir, &stamp).unwrap();
    let mut rng = SplitMix(2026);
    let now = now_unix_ms();
    let t0 = Instant::now();
    let mut batch: Vec<(u128, Vector)> = Vec::new();
    for i in 0..n {
        for c in &node(i, n, now).chunks {
            let a = &anchor[(c.hash as usize >> 3) % centres];
            let raw: Vec<f32> = a.iter().map(|x| x + noise * rng.unit()).collect();
            let (f, ct) = embedder::finish(&raw, cut);
            batch.push((
                c.hash,
                Vector {
                    full: f,
                    cut: ct,
                    tokens: 1,
                    windowed: false,
                    truncated: false,
                },
            ));
        }
        if batch.len() >= 2000 || i + 1 == n {
            let refs: Vec<(u128, &Vector)> = batch.iter().map(|(h, v)| (*h, v)).collect();
            cache.append(&refs).unwrap();
            batch.clear();
        }
    }
    eprintln!(
        "{} records written in {:.0} s",
        cache.len(),
        t0.elapsed().as_secs_f64()
    );
}

/// The table a start makes, over the vector file `build` wrote, and the signals some nodes carry.
fn start(dir: &Path, n: u64, now_ms: u64) -> Vectors {
    let cfg = {
        let mut c = VectorConfig::new(Some(dir.join("no-weights")));
        c.resident_mb = var("THESEUS_MEASURE_MB", DEFAULT_RESIDENT_MB);
        c.hot_records = std::env::var("THESEUS_MEASURE_HOT")
            .ok()
            .and_then(|v| v.parse().ok());
        c
    };
    let v = Vectors::new(cfg, dir);
    v.open_files().unwrap();
    {
        let mut t = v.table.write().unwrap();
        t.start_rows(n as usize, (2 * n) as usize, true);
        let mut batch = Vec::new();
        for i in 0..n {
            batch.push(node(i, n, now_ms));
            if batch.len() == 4096 {
                t.write_nodes(&mut batch).unwrap();
            }
        }
        t.write_nodes(&mut batch).unwrap();
        t.finish_rows().unwrap();
        t.reconciled = true;
        t.caught_up = true;
        t.recount();
    }
    // Signals: a fortieth of the nodes were used by a turn, a hundredth labelled useful, a
    // twentieth of high durability.
    let mut sig = Vec::new();
    for i in (0..n).step_by(40) {
        sig.push(heat::Signal::Used {
            node: format!("nd_{i:09}"),
            at_ms: now_ms - 86_400_000 * (i % 30),
        });
    }
    for i in (5..n).step_by(100) {
        sig.push(heat::Signal::Label {
            node: format!("nd_{i:09}"),
            label: "useful".into(),
        });
    }
    for i in (9..n).step_by(20) {
        sig.push(heat::Signal::Durability {
            node: format!("nd_{i:09}"),
            durability: "high".into(),
        });
    }
    {
        let mut t = v.table.write().unwrap();
        for s in &sig {
            t.signals.apply(s);
        }
    }
    release_memory();
    v
}

/// Queries near stored records: a stored vector and a little noise.
fn near_queries(t: &Table, n: usize) -> Vec<Vec<f32>> {
    let mut rng = SplitMix(77);
    let rows = t.row_count() as u64;
    (0..n as u64)
        .filter_map(|k| {
            let row = ((k * 2_654_435_761) % rows) as u32;
            let own = t.full_of(row as usize).ok()??;
            let noisy: Vec<f32> = own.iter().map(|x| x + 0.02 * rng.unit()).collect();
            Some(embedder::l2_normalize(&noisy))
        })
        .collect()
}

/// The memory, the recall and the times of the tiered search against the exact one.
fn search_lines(v: &Vectors, dir: &Path, out: &mut String) -> Vec<Vec<f32>> {
    let t = v.table.read().unwrap();
    let cut = v.cfg.spec.cut;
    let queries = near_queries(&t, 300);
    let f = t.filter(None, &[], &Filters::default());
    let tier = t.tier.clone();
    let mut shares = replay::Shares::default();
    for q in &queries {
        shares.put(&t, &tier, q, cut, &f).unwrap();
    }
    let (anon, file) = anon_file_mib();
    emit!(
        out,
        "{} queries near stored records\n{}",
        queries.len(),
        shares.describe()
    );
    emit!(
        out,
        "after the searches: anonymous {anon:.1} MiB, file-backed {file:.1} MiB, total {:.1} MiB; setting {} MiB",
        anon + file,
        v.cfg.resident_mb
    );
    // The first search after the kernel dropped the files' pages.
    t.evict_files();
    if let Some(tr) = &tier {
        tr.release(0..tr.assigned());
    }
    drop_cache(&dir.join("vectors"));
    let t4 = Instant::now();
    t.search_with(&queries[0], cut, &f, 10, tier).unwrap();
    let first_fast = ms(t4);
    t.evict_files();
    drop_cache(&dir.join("vectors"));
    let t5 = Instant::now();
    t.search_with(&queries[0], cut, &f, 10, None).unwrap();
    emit!(
        out,
        "first search after the files' pages were dropped: tiered {first_fast:.0} ms, exact {:.0} ms",
        ms(t5)
    );
    queries
}

#[test]
#[ignore = "a tool: reads and writes the directory THESEUS_MEASURE_DIR names"]
fn measure_the_hot_set() {
    let Ok(dir) = std::env::var("THESEUS_MEASURE_DIR") else {
        eprintln!("set THESEUS_MEASURE_DIR=<dir>, THESEUS_MEASURE_NODES=<n>, and THESEUS_MEASURE_PHASE=build for the first run");
        return;
    };
    let dir = PathBuf::from(dir);
    let n: u64 = var("THESEUS_MEASURE_NODES", 266_000);
    let centres: usize = var("THESEUS_MEASURE_CENTRES", 400);
    let noise: f32 = var("THESEUS_MEASURE_NOISE", 0.6);
    if std::env::var("THESEUS_MEASURE_PHASE").as_deref() == Ok("build") {
        build(&dir, n, centres, noise);
        return;
    }
    let now_ms = now_unix_ms();
    stage("start");
    let (anon0, file0) = anon_file_mib();
    let t0 = Instant::now();
    let v = start(&dir, n, now_ms);
    stage("table built");
    let (anon1, file1) = anon_file_mib();
    let mut out = String::new();
    emit!(out, "nodes {n}, centres {centres}, noise {noise}: start {:.1} s; anonymous {anon0:.1} -> {anon1:.1} MiB, file-backed {file0:.1} -> {file1:.1} MiB", t0.elapsed().as_secs_f64());
    let t1 = Instant::now();
    v.layout_now("measure");
    stage("layout made");
    emit!(
        out,
        "layout in {:.0} ms: {:?}",
        ms(t1),
        v.tier_status().unwrap()
    );
    let (anon2, file2) = anon_file_mib();
    emit!(
        out,
        "at rest after the layout: anonymous {anon2:.1} MiB, file-backed {file2:.1} MiB, total {:.1} MiB; setting {} MiB",
        anon2 + file2,
        v.cfg.resident_mb
    );
    let queries = search_lines(&v, &dir, &mut out);
    stage("searches done");
    // What a promotion costs, and a query held by a layout.
    let t6 = Instant::now();
    v.on_signals(&[heat::Signal::Label {
        node: "nd_000000001".into(),
        label: "useful".into(),
    }]);
    emit!(
        out,
        "a label's promotion: {:.3} ms under the table's write lock",
        ms(t6)
    );
    let f = v
        .table
        .read()
        .unwrap()
        .filter(None, &[], &Filters::default());
    let (held_ms, quiet_ms) = held_by_a_layout(&v, &queries, v.cfg.spec.cut, &f);
    emit!(
        out,
        "a search while a layout runs: median {held_ms:.1} ms (idle {quiet_ms:.1} ms)"
    );
    stage("done");
    drop(out);
}

/// Searches run while another thread lays the store out again: the time each takes against the
/// idle median.
fn held_by_a_layout(v: &Vectors, queries: &[Vec<f32>], cut: usize, f: &RowFilter) -> (f64, f64) {
    let one = |q: &Vec<f32>| {
        let t = v.table.read().unwrap();
        let t0 = Instant::now();
        t.search_reach(q, cut, f, 10, layout::Reach::Fast).unwrap();
        ms(t0)
    };
    let mut quiet: Vec<f64> = queries.iter().take(30).map(&one).collect();
    let done = AtomicBool::new(false);
    let mut during: Vec<f64> = Vec::new();
    std::thread::scope(|s| {
        s.spawn(|| {
            // However it ends (a panic too), the loop below is told.
            struct Done<'a>(&'a AtomicBool);
            impl Drop for Done<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _done = Done(&done);
            v.layout_now("measure again");
        });
        let mut k = 0;
        let began = Instant::now();
        while !done.load(Ordering::SeqCst) && began.elapsed().as_secs() < 90 {
            during.push(one(&queries[k % queries.len()]));
            k += 1;
        }
    });
    if during.is_empty() {
        during.push(f64::NAN);
    }
    (
        replay::median_p95(&mut during).0,
        replay::median_p95(&mut quiet).0,
    )
}
