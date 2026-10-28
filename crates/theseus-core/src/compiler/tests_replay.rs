//! A model-free replay of a long bug hunt through the real compiler
//! (theseus-5lzox): about 207 calls on a 1M window, ~270 reads of 12-25K
//! tokens over ~65 files read in a first sweep and read again, each answer
//! ~450 tokens, a ~7.5K system block. No model, no key: the stand-in is the
//! script below, and the numbers are the request the compiler would send
//! each call and the cache it would read and write at Sonnet 5.5's prices.
//!
//! It cannot show what a model does with a stub (the re-read share): that
//! takes a model, and the verify rerun reads it from the run's own record.

use serde_json::{json, Value};

use super::tests_ring::{answer, result, spec};
use super::*;
use crate::stub::Stub;

/// Calls the hunt makes, and files it opens in its first sweep.
const CALLS: usize = 207;
const FILES: usize = 65;
/// The system block, and each answer, in tokens.
const SYSTEM: u64 = 7_500;
const ANSWER: u64 = 450;
/// Sonnet 5.5's prices, dollars per million tokens, as the catalog holds them
/// (theseus-vzzcz): cache read, cache write, output. The replay held its own
/// copy ($0.10 a million for the read) until the catalog moved to $0.20
/// (theseus-rko44) and the two disagreed. The figures it is held against, the
/// 20-exchange rule's $17.67 and the owner's hunt's ~$16, were priced at
/// $0.10; at $0.20 they are $23.91 (its 62,350,444 cache-read tokens add
/// $6.24) and about $22 (a mean context of 318K against the rule's 321K).
fn prices() -> (f64, f64, f64) {
    let catalog = crate::catalog::Catalog::builtin();
    let p = catalog.get("claude-sonnet-5-5").expect("a catalog row");
    (
        p.cache_read_per_mtok,
        p.cache_write_per_mtok,
        p.output_per_mtok,
    )
}

/// A seeded stream: the same hunt every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// What a replay measured.
#[derive(Debug, Default)]
pub(super) struct Stats {
    pub calls: usize,
    pub passes: usize,
    /// Other recompiles (the ring), which also rewrite the cache.
    pub rings: usize,
    pub mean: u64,
    pub max: u64,
    /// Tokens written to the cache by a pass, and the dearest single pass.
    pub rewrite: u64,
    pub rewrite_max: u64,
    pub read: u64,
    pub write: u64,
    pub dollars: f64,
    /// Each call's estimate, and each pass's call, tokens before and after.
    pub series: Vec<u64>,
    pub pass_log: Vec<(usize, u64, u64)>,
}

fn per_byte(bytes: usize) -> f64 {
    let nodes = vec![(
        1,
        Node::user("s", None, "web", &"word ".repeat(bytes / 5)).into(),
    )];
    let c = compile_with(&nodes, None, 1_000_000, 0);
    c.est_tokens as f64 / c.request.json_bytes() as f64
}

fn compile_with(
    nodes: &[(u64, Stub)],
    current: Option<&Compilation>,
    window: u64,
    system_bytes: usize,
) -> Compiled {
    let mut s = spec();
    s.model = "claude-sonnet-5-5".into();
    s.max_tokens = 128_000;
    s.system_text = "S".repeat(system_bytes.max(1));
    let last = nodes.last().map_or(0, |(p, _)| *p);
    compile(CompileInput {
        session_id: "s",
        current,
        nodes,
        last_position: last,
        spec: &s,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: Some(window),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    })
}

/// A numbered read of `lines` rows of about `tokens` tokens.
fn read_of(tokens: u64, per_byte: f64, lines: usize, seed: u64) -> String {
    let width = (((tokens as f64 / per_byte) / lines as f64) as usize).max(12);
    (1..=lines)
        .map(|i| {
            let body: String = (0..width.saturating_sub(8))
                .map(|k| (b'a' + ((i as u64 * 7 + k as u64 * 13 + seed) % 26) as u8) as char)
                .collect();
            format!("{i:>6}\t{body}\n")
        })
        .collect()
}

fn call(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}

/// The hunt on a window of `window` tokens.
pub(super) fn replay(window: u64) -> Stats {
    replay_sized(window, 2_000, 6_000)
}

/// The hunt with reads of `lo` to `lo + span` tokens.
pub(super) fn replay_sized(window: u64, lo: u64, span: u64) -> Stats {
    replay_with(window, lo, span, 0)
}

/// The hunt with reads of `lo` to `lo + span` tokens, `small_pct` percent of
/// its calls returning a result of 1,500 characters (never cleared).
pub(super) fn replay_with(window: u64, lo: u64, span: u64, small_pct: u64) -> Stats {
    let pb = per_byte(200_000);
    let system_bytes = ((SYSTEM as f64 / pb) as usize).saturating_sub(300);
    let mut rng = Rng(7);
    // Each file's size is its own, whenever it is read.
    let size: Vec<(u64, usize)> = (0..FILES)
        .map(|_| (lo + rng.next(span + 1), 300 + rng.next(501) as usize))
        .collect();
    let mut nodes: Vec<(u64, Stub)> = vec![(
        1,
        Node::user("s", None, "web", "find and fix the bugs in this workspace").into(),
    )];
    let mut stats = Stats::default();
    let (read_price, write_price, out_price) = prices();
    let (mut prev, mut sum, mut opened, mut id) = (0u64, 0u64, 0usize, 0usize);
    let mut current: Option<Compilation> = None;
    let mut sys = SYSTEM;
    for k in 0..CALLS {
        let c = compile_with(&nodes, current.as_ref(), window, system_bytes);
        if k == 0 {
            sys = c.est_tokens;
        }
        let est = c.est_tokens;
        let recompiled = k > 0 && c.new_compilation;
        let (read, write) = match (k, recompiled) {
            (0, _) => (0, est),
            (_, true) => (sys, est.saturating_sub(sys)),
            _ => (prev, est.saturating_sub(prev)),
        };
        stats.series.push(est);
        if let Some(p) = c.cleared {
            stats.pass_log.push((k, p.tokens_before, p.tokens_after));
        }
        if c.cleared.is_some() {
            stats.passes += 1;
            stats.rewrite += write;
            stats.rewrite_max = stats.rewrite_max.max(write);
        } else if recompiled {
            stats.rings += 1;
        }
        stats.read += read;
        stats.write += write;
        stats.dollars +=
            (read as f64 * read_price + write as f64 * write_price + ANSWER as f64 * out_price)
                / 1e6;
        stats.max = stats.max.max(est);
        sum += est;
        prev = est;
        current = Some(c.compilation);
        // The model's answer: its text and one or two calls; the first sweep
        // opens a new file each read, later reads open one of the 65 again.
        let n_calls = 1 + (rng.next(100) < 45) as usize;
        let mut blocks = vec![json!({"type": "text", "text": format!(
            "Checked the module around call {k} and compared it with the earlier notes; {}",
            "the logic holds here and the next file is where the mismatch may be. ".repeat(18)
        )})];
        let mut results = Vec::new();
        for _ in 0..n_calls {
            id += 1;
            let tid = format!("c{id}");
            if small_pct > 0 && rng.next(100) < small_pct {
                blocks.push(call(
                    &tid,
                    "fs_read",
                    json!({"path": format!("crates/x/src/s{id}.rs")}),
                ));
                results.push(result(&tid, &format!("{id:>6}\t{}", "y".repeat(1_490))));
            } else if rng.next(100) < 90 {
                let f = if opened < FILES {
                    opened += 1;
                    opened - 1
                } else {
                    rng.next(FILES as u64) as usize
                };
                blocks.push(call(
                    &tid,
                    "fs_read",
                    json!({"path": format!("crates/x/src/f{f}.rs")}),
                ));
                let (tokens, lines) = size[f];
                results.push(result(&tid, &read_of(tokens, pb, lines, f as u64)));
            } else {
                blocks.push(call(
                    &tid,
                    "fs_grep",
                    json!({"pattern": format!("fn m{id}")}),
                ));
                results.push(result(&tid, &read_of(900, pb, 40, id as u64)));
            }
        }
        let p = nodes.len() as u64;
        nodes.push((p + 1, answer(blocks).into()));
        for (i, r) in results.into_iter().enumerate() {
            nodes.push((p + 2 + i as u64, r.into()));
        }
    }
    stats.calls = CALLS;
    stats.mean = sum / CALLS as u64;
    stats
}

fn print(row: &str, s: &Stats) {
    println!(
        "{row:<28} calls {} passes {} rings {} mean {} max {} rewrite {} (max {}) read {} write {} ${:.2}",
        s.calls, s.passes, s.rings, s.mean, s.max, s.rewrite, s.rewrite_max, s.read, s.write, s.dollars
    );
}

/// The hunt on a window, the rule's trigger and keep tried at `(at, keep)`.
fn tried(at: u64, keep: u64) -> Stats {
    clearing::TRIED.set(Some((at, keep)));
    let s = replay(1_000_000);
    clearing::TRIED.set(None);
    s
}

/// The grid the shipped trigger and keep were chosen from (the owner's
/// ranges: trigger 120-200K, keep 40-80K), printed with `--nocapture`.
#[test]
#[ignore = "prints the grid the constants were chosen from"]
fn the_replays_grid() {
    for at in [120_000, 150_000, 200_000] {
        for keep in [40_000, 60_000, 80_000] {
            print(
                &format!("trigger {}K keep {}K", at / 1000, keep / 1000),
                &tried(at, keep),
            );
        }
    }
}

/// The hunt's context at every call and each pass, for the shipped rule, on
/// the owner's hunt and with a third of its results under 2,000 characters
/// (which no pass clears), printed with `--nocapture --run-ignored all`. The
/// context a pass leaves climbs through the hunt (59K at call 23, 189K at call
/// 205): what a pass cannot clear (the model's answers, call ids, stubs) is
/// the floor under the sawtooth, which is why the mean is 175K and not the
/// ~120K the fix list expected from the keep and the trigger alone. Neither
/// replay passes the percent line (the highest request is 316K of 434K), so
/// the join fix's retry never runs in them.
#[test]
#[ignore = "prints the replay's series"]
fn the_replays_series() {
    for (row, s) in [
        ("hunt", replay(1_000_000)),
        ("hunt, 30% small", replay_with(1_000_000, 2_000, 6_000, 30)),
    ] {
        print(row, &s);
        let means: Vec<u64> = s
            .series
            .chunks(52)
            .map(|c| c.iter().sum::<u64>() / c.len() as u64)
            .collect();
        let floor = s.series[100..].iter().min().copied().unwrap_or(0);
        let over = s.series.iter().filter(|&&e| e > 434_000).count();
        println!("  mean per 52 calls {means:?}; lowest after call 100: {floor}; calls over 434K: {over}");
        for (k, before, after) in &s.pass_log {
            println!("  pass at call {k}: {before} -> {after}");
        }
    }
}

/// Triggers and keeps below the owner's ranges (trigger 120-200K, keep
/// 40-80K), to see what the replay's mean would take: the trigger is the
/// context a pass lets build above the floor, so a mean near 120K needs a
/// trigger well under 150K, at the price of more passes (each one a cache
/// rewrite). Printed with `--nocapture --run-ignored all`.
#[test]
#[ignore = "prints cells below the owner's ranges"]
fn the_replays_low_grid() {
    for (at, keep) in [
        (100_000, 20_000),
        (80_000, 20_000),
        (60_000, 20_000),
        (60_000, 10_000),
        (40_000, 10_000),
    ] {
        print(
            &format!("trigger {}K keep {}K", at / 1000, keep / 1000),
            &tried(at, keep),
        );
    }
}

/// The hunt's shape through the compiler: a pass at the token line, a small
/// rewrite each, a mean context well under what the 20-exchange rule held.
/// The same script at the 20-exchange rule (b69236c1): 9 passes, mean
/// 321,508, max 433,338, rewrite 2,479,623, read 62,350,444, $17.67 at $0.10 a
/// million cache-read tokens ($23.91 at the catalog's $0.20); the owner's hunt
/// measured a mean of 318K and 7-8 passes of ~$16 at $0.10 (about $22 at
/// $0.20). The replay costs $15.20 at $0.20 ($11.88 at $0.10), and the bound
/// keeps the headroom it had over that.
#[test]
fn the_bug_hunts_replay_holds_its_context_near_the_keep() {
    let s = replay(1_000_000);
    print("shipped rule", &s);
    assert!(s.passes >= 6 && s.passes <= 30, "{s:?}");
    assert!(s.mean < 200_000, "mean context {}", s.mean);
    assert!(s.max < 360_000, "{s:?}");
    assert!(
        s.rewrite_max < 200_000,
        "a pass's rewrite {}",
        s.rewrite_max
    );
    assert!(s.dollars < 18.0, "{s:?}: the 20-exchange rule cost $23.91");
}
