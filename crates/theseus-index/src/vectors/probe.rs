//! The vector table's search on a real index, measured and written out (issue 21
//! of 1.0.7): a tool beside the tests, as `heap.rs`'s `fill` and `held.rs`'s
//! `tool` are.
//!
//! The tender stopped, `THESEUS_PROBE_INDEX=<state>/index` (or a copy) names an
//! index whose vector files hold vectors of the pinned model's stamp (the
//! `fill_an_index_with_random_vectors` tool gives a synthetic store's index
//! them). The table is built as a start builds it, and a fixed set of queries,
//! random unit vectors of a seeded generator, are put to it under several
//! filters; each one's best results are written to `THESEUS_PROBE_OUT`, one line
//! a result (node, chunk, position, and the score's bits), so two builds'
//! files can be compared with `cmp`: the same index must answer the same, bit
//! for bit. The neighbours of a few nodes are written too.
//!
//! The times go to stderr. The vector files are read through first, so the warm
//! search is a tender that has been running's: each filter's median, the scan
//! alone against the re-score of its hundred, and the page faults. The cold
//! search is the first one after the process's own copy of the table's pages was
//! let go and the kernel was told to drop every mapped file's and the vector
//! files' (`posix_fadvise` `DONTNEED`): what a tender finds when the kernel has
//! needed its memory for something else.
//!
//! ```text
//! THESEUS_PROBE_INDEX=<index> THESEUS_PROBE_OUT=<file> cargo nextest run --release \
//!   -p theseus-index --run-ignored only -E 'test(probe_the_search)' --no-capture
//! ```

use super::*;
use crate::engine::{open_or_create, Engine};
use crate::weights::SplitMix;
use std::fmt::Write as _;
use std::os::fd::AsRawFd;

const QUERIES: usize = 60;
const FETCH: usize = 30;

type Filtered = (Option<u64>, Vec<String>, Filters);

fn unit(rng: &mut SplitMix, n: usize) -> Vec<f32> {
    let raw: Vec<f32> = (0..n).map(|_| rng.unit()).collect();
    embedder::l2_normalize(&raw)
}

/// The `.vec` and `.scan` files of `dir`.
fn vector_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("vec" | "scan")))
        .collect()
}

/// Read every `.vec` and `.scan` file of `dir` through, so the kernel holds it.
fn warm_file_cache(dir: &Path) {
    for e in vector_files(dir) {
        if let Ok(mut f) = File::open(e) {
            let _ = io::copy(&mut f, &mut io::sink());
        }
    }
}

/// Ask the kernel to drop every cached page of the `.vec` and `.scan` files of `dir`.
fn drop_file_cache(dir: &Path) -> usize {
    let mut n = 0;
    for e in vector_files(dir) {
        if let Ok(f) = File::open(e) {
            // SAFETY: a valid descriptor; advice only.
            unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
            n += 1;
        }
    }
    n
}

/// This process's (minor, major) page faults so far.
fn faults() -> (u64, u64) {
    let stat = fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // After the command's closing parenthesis: state is field 3, minflt 10, majflt 12.
    let rest: Vec<&str> = stat
        .rsplit_once(')')
        .map_or("", |x| x.1)
        .split_whitespace()
        .collect();
    let at = |n: usize| rest.get(n - 3).and_then(|v| v.parse().ok()).unwrap_or(0);
    (at(10), at(12))
}

fn pct(ms: &mut [f64], p: f64) -> f64 {
    ms.sort_by(f64::total_cmp);
    ms[((ms.len() as f64 * p) as usize).min(ms.len() - 1)]
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// The five filters the queries rotate through.
fn filters(t: &Table) -> Vec<Filtered> {
    let mid = t.row_position((t.row_count() / 2) as u32);
    let sessions: Vec<String> = [3u32, 40]
        .iter()
        .map(|&i| t.sessions.name(i).to_string())
        .collect();
    vec![
        (None, vec![], Filters::default()),
        (Some(mid), vec![], Filters::default()),
        (None, vec![sessions[0].clone()], Filters::default()),
        (
            None,
            vec![],
            Filters {
                sessions,
                ..Filters::default()
            },
        ),
        (
            None,
            vec![],
            Filters {
                kinds: vec![t.kinds.name(0).to_string()],
                external: Some(false),
                ..Filters::default()
            },
        ),
    ]
}

/// The queries put to the table: the answers as lines, and each search's time under its filter.
fn warm_searches(t: &Table, cut: usize, full: usize) -> (String, Vec<Vec<f64>>) {
    let filters = filters(t);
    let mut rng = SplitMix(7);
    let mut text = String::new();
    let mut by_filter: Vec<Vec<f64>> = vec![Vec::new(); filters.len()];
    let f0 = faults();
    for n in 0..QUERIES {
        let q = unit(&mut rng, full);
        let (as_of, exclude, f) = &filters[n % filters.len()];
        let rf = t.filter(*as_of, exclude, f);
        let t1 = Instant::now();
        let hits = t.search(&q, cut, &rf, FETCH).unwrap();
        by_filter[n % filters.len()].push(ms(t1));
        for (i, score) in hits {
            let r = t.row(i as u32);
            let bits = score.to_bits();
            writeln!(
                text,
                "q{n} {} {} {} {bits:016x}",
                r.node, r.chunk, r.position
            )
            .unwrap();
        }
    }
    let f1 = faults();
    eprintln!(
        "page faults over the {QUERIES} searches: {} minor, {} major",
        f1.0 - f0.0,
        f1.1 - f0.1
    );
    (text, by_filter)
}

/// One query, unfiltered, again and again: the scan alone, the re-score of its hundred alone.
fn scan_against_rescore(t: &Table, cut: usize, full: usize) {
    let mut rng = SplitMix(31);
    let q = unit(&mut rng, full);
    let (q8, _) = quantize(&embedder::l2_normalize(&q[..cut]));
    let rf = t.filter(None, &[], &Filters::default());
    let (mut scan, mut rescore): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    for _ in 0..30 {
        let t1 = Instant::now();
        let c = Flat(t).top(&q8, RESCORE, &|i| t.admits(&rf, i));
        scan.push(ms(t1));
        let t2 = Instant::now();
        for (i, _) in c {
            t.full_of(i).unwrap();
        }
        rescore.push(ms(t2));
    }
    eprintln!(
        "one unfiltered query, 30 times: the scan alone min {:.1} median {:.1} ms; the 768-d re-score of its {RESCORE} median {:.1} ms",
        pct(&mut scan.clone(), 0.0),
        pct(&mut scan, 0.5),
        pct(&mut rescore, 0.5)
    );
}

/// The neighbours of four nodes, as lines.
fn neighbour_lines(v: &Vectors) -> String {
    let mut text = String::new();
    let nodes: Vec<String> = {
        let t = v.table.read().unwrap();
        [11usize, 5_000, 77_777, 150_000]
            .iter()
            .map(|&i| t.row_node(((i * 3) % t.row_count()) as u32).to_string())
            .collect()
    };
    for node in nodes {
        match v.neighbours(&node, 10, None) {
            Ok((ns, _)) => {
                for n in ns {
                    let bits = n.score.to_bits();
                    writeln!(
                        text,
                        "n {node} {} {} {} {bits:016x}",
                        n.node_id, n.chunk, n.position
                    )
                    .unwrap();
                }
            }
            Err(e) => writeln!(text, "n {node} {e}").unwrap(),
        }
    }
    text
}

#[test]
#[ignore = "a tool: reads the index THESEUS_PROBE_INDEX names"]
fn probe_the_search() {
    let (Ok(dir), Ok(out)) = (
        std::env::var("THESEUS_PROBE_INDEX"),
        std::env::var("THESEUS_PROBE_OUT"),
    ) else {
        eprintln!("set THESEUS_PROBE_INDEX=<index> and THESEUS_PROBE_OUT=<file>");
        return;
    };
    let dir = PathBuf::from(dir);
    let (index, fields) = open_or_create(&crate::state::Paths::new(&dir).bm25()).unwrap();
    let engine = Engine::new(index, fields).unwrap();
    let v = Vectors::new(VectorConfig::new(Some(dir.join("no-weights"))), &dir);
    let t0 = Instant::now();
    v.open_files().unwrap();
    v.reconcile(&engine).unwrap();
    eprintln!("the table built in {:.0} ms", ms(t0));
    warm_file_cache(&dir.join("vectors"));
    let (cut, full) = (v.cfg.spec.cut, v.cfg.spec.config.hidden);
    let (mut text, mut by_filter) = {
        let t = v.table.read().unwrap();
        let r = warm_searches(&t, cut, full);
        scan_against_rescore(&t, cut, full);
        r
    };
    let mut all: Vec<f64> = by_filter.concat();
    for (k, m) in by_filter.iter_mut().enumerate() {
        eprintln!("  filter {k}: median {:.1} ms", pct(m, 0.5));
    }
    eprintln!(
        "warm search: median {:.1} ms, 95th percentile {:.1} ms over {QUERIES} queries",
        pct(&mut all.clone(), 0.5),
        pct(&mut all, 0.95)
    );
    text.push_str(&neighbour_lines(&v));
    fs::write(&out, &text).unwrap();
    // The first search after the pages were let go and the files dropped.
    v.table.read().unwrap().evict_files();
    let files = drop_file_cache(&dir.join("vectors"));
    let mut rng = SplitMix(99);
    let q = unit(&mut rng, full);
    let t = v.table.read().unwrap();
    let rf = t.filter(None, &[], &Filters::default());
    let t1 = Instant::now();
    t.search(&q, cut, &rf, FETCH).unwrap();
    let first = ms(t1);
    let t2 = Instant::now();
    t.search(&q, cut, &rf, FETCH).unwrap();
    eprintln!(
        "cold search ({files} files dropped from the kernel's cache): first {first:.0} ms, the same again {:.1} ms",
        ms(t2)
    );
}
