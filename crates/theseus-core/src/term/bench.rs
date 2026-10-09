//! Measures for theseus-ggqf's FAST check, ignored in the suite: the close of
//! four terminals with nothing in the background (the daemon's stop's), and
//! the daemon's own CPU through a 30 s wait on a busy terminal. Each uses
//! only `Terms`' calls that were there before theseus-ggqf, so the same file
//! measures the build before it. Run with
//! `cargo nextest run -p theseus-core --run-ignored only -E 'test(/term::bench/)' --no-capture`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_tools::ToolCtx;

use super::*;

fn terms() -> Arc<Terms> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    Arc::new(Terms::new(
        vec![("PATH".into(), path), ("HOME".into(), "/tmp".into())],
        Vec::new(),
    ))
}

async fn run(terms: &Arc<Terms>, tool: &str, input: serde_json::Value) -> String {
    let d = std::env::temp_dir();
    match terms.run(tool, "s1", &input, &ToolCtx::for_tests(&d)).await {
        Ok((o, _)) => o.text,
        Err(f) => panic!("{tool}: {}", f.message),
    }
}

/// This process's CPU time so far (user and system), in clock ticks.
fn cpu_ticks() -> u64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<&str> = s[s.rfind(')').unwrap() + 2..].split_whitespace().collect();
    f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
}

/// Four interactive bash terminals at their prompts.
async fn four(terms: &Arc<Terms>) {
    for i in 0..4 {
        run(
            terms,
            OPEN,
            json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "quiet_ms": 0}),
        )
        .await;
        let id = format!("t{}", i + 1);
        run(
            terms,
            SEND,
            json!({"terminal": id, "text": "PS1='r''eady# '\n"}),
        )
        .await;
        run(
            terms,
            READ,
            json!({"terminal": id, "until": "ready# ", "timeout_ms": 10_000}),
        )
        .await;
    }
}

fn line(what: &str, took: &mut [f64]) {
    took.sort_by(f64::total_cmp);
    println!(
        "{what}: median {:.1} ms, min {:.1}, max {:.1}",
        took[took.len() / 2],
        took[0],
        took[took.len() - 1]
    );
}

/// Four terminals with nothing in the background, closed together as the
/// daemon's stop closes them, and four closed one by one by `term.close`:
/// the times, over ten rounds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measure: run it by hand"]
async fn close_of_four_terminals() {
    let (mut stop, mut close) = (Vec::new(), Vec::new());
    for _ in 0..10 {
        let terms = terms();
        four(&terms).await;
        let t0 = Instant::now();
        let closed = tokio::task::spawn_blocking({
            let terms = terms.clone();
            move || terms.close_where(BY_DAEMON, |_| true)
        })
        .await
        .unwrap();
        stop.push(t0.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(closed.len(), 4);
        let terms = super::bench::terms();
        four(&terms).await;
        for i in 0..4 {
            let t0 = Instant::now();
            run(&terms, CLOSE, json!({"terminal": format!("t{}", i + 1)})).await;
            close.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
    line("the daemon's stop's close of 4 terminals", &mut stop);
    line("term.close of one of 4 terminals", &mut close);
}

/// A 30 s wait on a terminal whose shell runs `sleep 40` in front: the CPU
/// this process spent in it. `THESEUS_BENCH_WAIT` names the wait's input:
/// `quiet` (a `quiet_ms` wait of 40 s) or `idle` (`until_idle`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measure: run it by hand"]
async fn cpu_of_a_30_s_wait() {
    let how = std::env::var("THESEUS_BENCH_WAIT").unwrap_or_else(|_| "quiet".into());
    let terms = terms();
    run(
        &terms,
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "quiet_ms": 0}),
    )
    .await;
    run(
        &terms,
        SEND,
        json!({"terminal": "t1", "text": "sleep 40\n"}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let input = match how.as_str() {
        "idle" => json!({"terminal": "t1", "until_idle": true, "timeout_ms": 30_000}),
        _ => json!({"terminal": "t1", "quiet_ms": 40_000, "timeout_ms": 30_000}),
    };
    let (c0, t0) = (cpu_ticks(), Instant::now());
    let text = run(&terms, READ, input).await;
    let ticks = cpu_ticks() - c0;
    // SAFETY: sysconf reads a constant.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    println!(
        "{how} wait of {:.1} s: {:.0} ms of CPU ({:.2}% of a core)\n{}",
        t0.elapsed().as_secs_f64(),
        ticks as f64 * 1000.0 / hz,
        ticks as f64 / hz / t0.elapsed().as_secs_f64() * 100.0,
        text.lines().next().unwrap_or_default()
    );
    terms.close_where(BY_TOOL, |_| true);
}

/// theseus-ggqf's keeping close: four bash terminals, each with a `sleep &`
/// job in the background, closed together at the daemon's stop, the jobs
/// left and then ended. Only on a build with `keep_background`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measure: run it by hand"]
async fn keeping_close_of_four_terminals() {
    let mut took = Vec::new();
    for round in 0..10 {
        let terms = terms();
        four(&terms).await;
        let mark = format!("4798.{:07}{round}", std::process::id());
        for i in 0..4 {
            let id = format!("t{}", i + 1);
            let text = format!("sleep {mark} & echo bg\n");
            run(&terms, SEND, json!({"terminal": id, "text": text})).await;
            run(
                &terms,
                READ,
                json!({"terminal": id, "until": "bg\nready# ", "timeout_ms": 10_000}),
            )
            .await;
        }
        let t0 = Instant::now();
        let closed = tokio::task::spawn_blocking({
            let terms = terms.clone();
            move || terms.close_where(BY_DAEMON, |_| true)
        })
        .await
        .unwrap();
        took.push(t0.elapsed().as_secs_f64() * 1000.0);
        assert!(closed.iter().all(|c| c.left.len() == 1), "{closed:?}");
        tokio::task::spawn_blocking(move || terms.end_left("s1"))
            .await
            .unwrap();
    }
    line(
        "the daemon's stop's keeping close of 4 terminals",
        &mut took,
    );
}
