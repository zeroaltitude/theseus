//! What laying results kept by reference reads, per turn (issue 52), on a synthetic store in a temp dir: a window of
//! look backs, each quoting messages stored among others. Counted: the newest records read to check erase
//! ([`ERASED_READS`]), when the window is laid and at each call (`erased_since`); and the quotes read, each a newest
//! record and a record by position (`quotes::read`, through the renderer's own loop). Timed: the window's lay and a
//! call's check. `the_read_cost_of_laying_by_reference` is a bench (`#[ignore]`; run it with `--ignored
//! --nocapture`), and `docs/benchmarks/2026-11-25-references-read-cost.md` holds its numbers. Every path it opens is
//! inside its temp dir. All words invented.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use super::*;
use crate::compiler::tests_view::result;
use crate::quotes::{Form, Quote};

/// A store in `dir`, and nowhere else.
fn store_in(dir: &tempfile::TempDir) -> Store {
    let path = dir.path().join("store");
    assert!(path.starts_with(dir.path()));
    Store::open(&path).unwrap()
}

/// `m` messages stored after `others` that no look back quotes: each quoted one's id, position and text length.
fn messages(s: &Store, m: usize, others: usize) -> Vec<(String, u64, u32)> {
    let mut out = Vec::new();
    let all: Vec<Node> = (0..others + m)
        .map(|i| {
            let text = format!("message {i}: the grey heron nests by the old weir");
            Node::user("ses_bench", Some(&format!("t{i}")), "cli", &text)
        })
        .collect();
    for chunk in all.chunks(500) {
        let records: Vec<_> = chunk.iter().map(|n| n.record().unwrap()).collect();
        let at = s.append(&records).unwrap();
        for (n, p) in chunk.iter().zip(at) {
            if let Body::UserMessage { text, .. } = &n.body {
                out.push((n.id.clone(), p, text.len() as u32));
            }
        }
    }
    out.split_off(others)
}

/// A look back that quotes `quoted`, a page line each.
fn look_back(turn: &str, call: &str, quoted: &[(String, u64, u32)]) -> Node {
    let mut parts = Vec::new();
    for (node, position, len) in quoted {
        parts.push(Part::Code("- ".into()));
        parts.push(Part::Quote(Quote {
            node: node.clone(),
            position: *position,
            field: Field::Text,
            from: 0,
            to: *len,
            form: Form::Flat1,
        }));
        parts.push(Part::Code(" \u{b7} t0\n".into()));
    }
    let mut n = result(turn, call, "");
    if let Body::ToolResult { tool, parts: p, .. } = &mut n.body {
        *tool = "context.history".into();
        *p = parts;
    }
    n
}

/// A window of `n` look backs of `q` messages each, stored among `others` messages: none quoted twice, or, `same`,
/// each look back quoting the same `q`.
fn window(s: &Store, n: usize, q: usize, others: usize, same: bool) -> Vec<(u64, Stub)> {
    let quoted = messages(s, if same { q } else { n * q }, others);
    (0..n)
        .map(|i| {
            let mine = if same { 0..q } else { i * q..(i + 1) * q };
            let node = look_back("t0", &format!("c{i}"), &quoted[mine]);
            (10_000 + i as u64, Stub::from(node))
        })
        .collect()
}

/// What one lay of `window` and one call's check read and took.
struct Cost {
    /// Newest records read to check erase while the window was laid.
    erase: usize,
    /// Quotes read: each a newest record and a record by position.
    quotes: usize,
    /// Newest records read by a call's check of what the lay read.
    call: usize,
    /// Distinct nodes the lay says it read, over the whole window (the call checks each).
    read: usize,
    lay_time: Duration,
    call_time: Duration,
}

fn cost(s: &Store, window: &[(u64, Stub)]) -> Cost {
    let none = BTreeSet::new();
    let since = Since::new(s, &none);
    ERASED_READS.with(|c| c.set(0));
    let mut read = Vec::new();
    let t = Instant::now();
    let laid = lay_window(s, window.to_vec(), &since, &mut read).unwrap();
    // As the frozen view keeps them (`turn::view_step`'s window): distinct over all the window's results.
    read.sort();
    read.dedup();
    let lay_time = t.elapsed();
    let erase = ERASED_READS.with(|c| c.replace(0));
    assert_eq!(laid.len(), window.len());
    let t = Instant::now();
    assert!(!erased_since(s, &read));
    let call_time = t.elapsed();
    let call = ERASED_READS.with(|c| c.replace(0));
    let mut quoted = 0;
    for (_, n) in window {
        let parts = parts_of(n).expect("kept by reference");
        quotes::render_with(parts, |q| {
            quoted += 1;
            quotes::read(s, q)
        });
    }
    Cost {
        erase,
        quotes: quoted,
        call,
        read: read.len(),
        lay_time,
        call_time,
    }
}

/// The counts the bench prints are what laying reads: one erase check per quote of a message (before the nodes are
/// made distinct), one read of each quote, and at a call one check per distinct node the window read; three look
/// backs quoting the same four messages are checked at a call as four.
#[test]
fn laying_a_window_reads_what_the_measure_counts() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(&dir);
    let w = window(&s, 3, 4, 20, false);
    let c = cost(&s, &w);
    assert_eq!((c.erase, c.quotes, c.read, c.call), (12, 12, 12, 12));
    let w = window(&s, 3, 4, 20, true);
    let c = cost(&s, &w);
    assert_eq!((c.erase, c.quotes, c.read, c.call), (12, 12, 4, 4));
}

/// The bench: windows of 1 to 40 look backs of 10 to 50 messages each, among 2,000 other messages, each quoting
/// messages no other quotes or (`same`) all quoting the same ones; the counts, and the median and first wall time of
/// 20 lays and checks each.
#[test]
#[ignore = "a bench: run with --ignored --nocapture"]
fn the_read_cost_of_laying_by_reference() {
    const RUNS: usize = 20;
    eprintln!("looks quotes | erase quotes store | call | lay first, median | call median");
    let unique =
        [(1, 10), (1, 50), (10, 10), (10, 50), (40, 10), (40, 50)].map(|(n, q)| (n, q, false));
    for (n, q, same) in unique.into_iter().chain([(10, 10, true), (40, 50, true)]) {
        let dir = tempfile::tempdir().unwrap();
        let s = store_in(&dir);
        let w = window(&s, n, q, 2_000, same);
        let runs: Vec<Cost> = (0..RUNS).map(|_| cost(&s, &w)).collect();
        let median = |f: fn(&Cost) -> Duration| {
            let mut t: Vec<Duration> = runs.iter().map(f).collect();
            t.sort();
            t[RUNS / 2]
        };
        let c = &runs[0];
        assert!(runs
            .iter()
            .all(|r| (r.erase, r.quotes, r.call) == (c.erase, c.quotes, c.call)));
        eprintln!(
            "{n} {q}{} | {} {} {} | {} | {:?} {:?} | {:?}",
            if same { " same" } else { "" },
            c.erase,
            c.quotes,
            c.erase + 2 * c.quotes,
            c.call,
            c.lay_time,
            median(|r| r.lay_time),
            median(|r| r.call_time),
        );
        assert_eq!(c.read, c.call);
    }
}
