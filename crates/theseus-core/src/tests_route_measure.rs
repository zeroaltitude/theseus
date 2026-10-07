//! The default bound's table (theseus-ddbi, step 4): against the fake Jev
//! with set latencies, each cell's routed turns, their wait after the first
//! compile (`wait_ms`) and their in-time rate (`late` false), at
//! `[routing] max_wait_ms` 200; and a fresh daemon's first message, cold
//! (no warm-up) and warm (`Core::warm_judge` first), under a set-up of
//! `SETUP`. A measurement, run by name:
//! `cargo nextest run -p theseus-core --run-ignored only --no-capture -E 'test(/tests_route_measure/)' --test-threads 1`.

use std::time::Duration;

use theseus_judge::fake::FakeJev;

use crate::tests_route::{decided, mode, rig, turn};

/// Messages a cell.
const TURNS: usize = 8;
/// A new connection's TCP and TLS, for the first-message rows.
const SETUP: Duration = Duration::from_millis(300);

/// The cell's `(wait_ms, late, the turn's wall ms)` per message, on one
/// fresh rig.
async fn cell(single: u64, batch: u64, warm: bool, n: usize) -> Vec<(u64, bool, u64)> {
    let jev = FakeJev::start().unwrap();
    jev.keep_alive(SETUP);
    jev.set_latency(Duration::from_millis(single), Duration::from_millis(batch));
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), n, |c| {
        c.routing.max_wait_ms = 200;
        c.judge.total_secs = 30;
    });
    if warm {
        r.core.warm_judge();
        while !r.core.runner.judge.jev_warm() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    let mut sid = None;
    let mut walls = Vec::new();
    for i in 0..n {
        let t0 = std::time::Instant::now();
        let res = turn(
            &r.core,
            sid.as_deref(),
            &format!("Weigh design {i} for the log."),
            None,
        )
        .await;
        walls.push(t0.elapsed().as_millis() as u64);
        sid = Some(res.session_id);
        // The next message after this one's requests are answered, as a
        // person's next message comes: 2 s apart, less here.
        tokio::time::sleep(Duration::from_millis(single.max(batch) + 50)).await;
    }
    decided(&r.core.store)
        .iter()
        .zip(walls)
        .map(|(d, w)| {
            (
                d["wait_ms"].as_u64().unwrap(),
                d["late"].as_bool().unwrap(),
                w,
            )
        })
        .collect()
}

/// The p50 and p95 of `xs`.
fn pct(mut xs: Vec<u64>) -> (u64, u64) {
    xs.sort_unstable();
    (
        xs[xs.len() / 2],
        xs[(xs.len() * 95 / 100).min(xs.len() - 1)],
    )
}

fn line(name: &str, rows: &[(u64, bool, u64)]) -> String {
    let (p50, p95) = pct(rows.iter().map(|r| r.0).collect());
    let (w50, w95) = pct(rows.iter().map(|r| r.2).collect());
    let mean = rows.iter().map(|r| r.0).sum::<u64>() as f64 / rows.len() as f64;
    let in_time = rows.iter().filter(|r| !r.1).count();
    format!(
        "| {name} | {mean:.0} | {p50} | {p95} | {in_time}/{} | {w50} | {w95} |",
        rows.len()
    )
}

/// The header, then one line a cell.
fn table(lines: Vec<String>) {
    let mut out = vec![
        "| fake Jev (ms) | wait mean ms | wait p50 | wait p95 | in time | turn p50 ms | turn p95 |"
            .to_string(),
        "|---|---|---|---|---|---|---|".to_string(),
    ];
    out.extend(lines);
    println!("{}", out.join("\n"));
}

/// Each single latency against a batch of `batch` ms, warm.
async fn by_batch(batch: u64) {
    let mut lines = Vec::new();
    for single in [80, 150, 300, 600] {
        let rows = cell(single, batch, true, TURNS).await;
        lines.push(line(&format!("single {single}, batch {batch}"), &rows));
    }
    table(lines);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measurement: run it by name"]
async fn the_bounds_table_batch_400() {
    by_batch(400).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measurement: run it by name"]
async fn the_bounds_table_batch_1000() {
    by_batch(1000).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measurement: run it by name"]
async fn the_bounds_table_batch_2000() {
    by_batch(2000).await;
}

/// A fresh daemon's first message, cold and warm, five rigs each.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measurement: run it by name"]
async fn the_bounds_table_first_message() {
    let mut lines = Vec::new();
    for single in [80, 150, 300] {
        let mut cold = Vec::new();
        let mut warm = Vec::new();
        for _ in 0..5 {
            cold.extend(cell(single, 1000, false, 1).await);
            warm.extend(cell(single, 1000, true, 1).await);
        }
        lines.push(line(
            &format!("first message, cold, single {single}"),
            &cold,
        ));
        lines.push(line(
            &format!("first message, warm, single {single}"),
            &warm,
        ));
    }
    table(lines);
}
