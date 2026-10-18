//! The night's cost (task: FAST). A realistic 14-day ledger, 20,000 calls
//! across 14 days, 840 runs, a few dozen shapes with the owner's flows among
//! them: the whole mining pass in CPU seconds and peak memory. Nothing about it
//! grows with history beyond the window.
//! `cargo nextest run -p theseus-core -E 'test(/a_fortnight/)' --no-capture`
//! prints the numbers.

use serde_json::json;

use super::fixture::{Land, Ledger, REPO};
use super::*;

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// About 20,000 calls: 840 runs of about 24 calls over 14 days, in three
/// repositories, each run a mix of reads, searches, edits, builds and, in a
/// fifth of them, one of the owner's flows.
fn fortnight() -> Ledger {
    let mut l = Ledger::new();
    let mut r = Lcg(7);
    let scopes = [REPO, "r:0000000000feed02", "r:0000000000feed03"];
    for day in 0..14u64 {
        for k in 0..60u64 {
            let scope = scopes[(k % 3) as usize];
            let run = l.run(
                &format!("s{day}-{k}"),
                &format!("2026-10-{:02}", day + 1),
                scope,
            );
            l.model(run);
            let mut calls = 0;
            while calls < 20 + r.next(10) {
                match r.next(8) {
                    0 | 1 => l.call(run, "fs.read", json!({"path": format!("crates/c{}/src/m{}.rs", r.next(30), r.next(9))}), 0, "text"),
                    2 => l.call(run, "fs.grep", json!({"pattern": format!("p{}", r.next(50)), "path": "crates"}), 0, "hit"),
                    3 => l.call(run, "fs.glob", json!({"pattern": format!("**/*.{}", ["rs", "toml", "md"][r.next(3) as usize])}), 0, "files"),
                    4 => l.argv(run, &["cargo", "check", "-p", &format!("crate{}", r.next(12))], (r.next(5) == 0) as i64, "out"),
                    5 => l.argv(run, &["git", "status", "--short"], 0, ""),
                    6 => {
                        l.edit(run, &format!("crates/c{}/src/m{}.rs", r.next(30), r.next(9)));
                    }
                    _ => l.argv(run, &["git", "diff", "--stat"], 0, "1 file changed"),
                }
                if r.next(4) == 0 {
                    l.model(run);
                }
                calls += 1;
            }
            if k % 5 == 0 {
                l.sync(run);
                l.land(
                    run,
                    &format!("feat/n{k}"),
                    [
                        Land::Clean,
                        Land::RerereResolved,
                        Land::Clean,
                        Land::RerereLeft,
                    ][(k % 4) as usize],
                );
            }
        }
    }
    l
}

#[test]
fn a_fortnight_of_a_busy_ledger_is_mined_in_seconds() {
    let l = fortnight();
    let calls: usize = l.runs.iter().map(|r| r.calls.len()).sum();
    assert!((18_000..26_000).contains(&calls), "{calls} calls");
    let before = rss_kb();
    let t = std::time::Instant::now();
    let m = mine(&l.runs, &l.vocab, &Params::default(), 20 * 86_400_000);
    let cpu = t.elapsed();
    let grew = rss_kb().saturating_sub(before);
    println!(
        "mined {} runs, {calls} calls in {:.2} s (debug build), {} motifs, peak memory grew {} MB (high-water {} MB)",
        l.runs.len(),
        cpu.as_secs_f64(),
        m.motifs.len(),
        grew / 1024,
        rss_kb() / 1024
    );
    assert!(cpu.as_secs() < 60, "{cpu:?}");
    assert!(grew < 600 * 1024, "{grew} kB");
    // The owner's flows are in there, in the repository that has them all.
    assert!(m
        .motifs
        .iter()
        .any(|x| x.steps.iter().any(|s| s.id == "merge")));
    assert!(m.motifs.len() <= MAX_MOTIFS);
}
