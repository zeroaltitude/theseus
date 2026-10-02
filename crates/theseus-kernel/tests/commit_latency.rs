//! The commit path under concurrent turns (theseus-vni9). Review 2's §S2 asked
//! for tail latency under load, which no bench measured: every kernel commit
//! is an append and its fdatasync, made from async code. A measurement, not a
//! check, so it is ignored by default:
//!
//! ```text
//! THESEUS_RIG_DIR=<a directory on the real disk> cargo nextest run --workspace \
//!     -E 'test(commit_latency_under_concurrent_turns)' --run-ignored only --no-capture
//! ```
//!
//! `THESEUS_RIG_TURNS` turns run at once (default 32), each for
//! `THESEUS_RIG_ROUNDS` rounds (default 20), on a runtime of
//! `THESEUS_RIG_WORKERS` workers (default 4), against a store that syncs
//! every frame, as the daemon's does. A round is a turn with one call, as the
//! kernel writes it: input wakes the execution, a turn is admitted, the call
//! is planned and dispatched, the turn parks on it, its completion settles,
//! and a second turn takes the result and parks on input: 7 frames. The calls
//! run back to back, a yield between them, so the turns' commits overlap.
//!
//! Beside them, a probe task wakes every 2 ms, spawns a task, and waits for
//! it: how late each round of that comes back is how long a task waited for a
//! worker, what a health request waits while every worker is in a commit. A
//! starved probe takes few samples, each late, so its count says it too.
//! Printed: the commit, round, and probe latencies (p50, p95, p99, max), the
//! frames, the syncs, and the frames each sync covered.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_kernel::types::{
    new_id, Authority, Completion, Outcome, RetryClass, SessionKind, Wake,
};
use theseus_kernel::{Kernel, KernelConfig, NoEvidence, Proposal, RealClock, TurnEnd};
use theseus_store::{Store, WalConfig, WalStore};

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn auth() -> Authority {
    Authority {
        principal: "rig".into(),
        delegated_by: None,
        ceilings: BTreeMap::from([("tools".into(), "fs".into())]),
    }
}

/// One timed kernel call.
fn timed<T>(lat: &mut Vec<u64>, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    lat.push(t.elapsed().as_micros() as u64);
    out
}

/// One round of one execution: 7 frames, each call timed.
fn round(k: &Kernel, exec: &str, commits: &mut Vec<u64>) {
    let p = Proposal {
        tool: "fs.read".into(),
        args: json!({"path": "a"}),
        resource: None,
        policy_context: json!({}),
    };
    timed(commits, || k.wake_input(exec).unwrap());
    let g = timed(commits, || k.admit(exec).unwrap());
    let a = timed(commits, || {
        k.plan_and_dispatch(&g, &p, RetryClass::SafeToRepeat, Some(60_000), 0, |_| {
            Ok(vec![])
        })
        .unwrap()
    });
    let wake = Wake::Actions {
        correlation_ids: vec![a.correlation_id.clone()],
    };
    timed(commits, || k.end_turn(g, TurnEnd::Wait { wake }).unwrap());
    let c = Completion {
        correlation_id: a.correlation_id,
        outcome: Outcome::Succeeded,
        result_ref: Some("node_rig".into()),
        external_op_id: None,
        started_at_ms: 1,
        finished_at_ms: 2,
        producer: "rig".into(),
        signature: None,
        cost_micros: None,
        detail: None,
    };
    timed(commits, || k.accept_completion(&c).unwrap());
    let g = timed(commits, || k.admit(exec).unwrap());
    k.take_results(&g).unwrap();
    timed(commits, || {
        k.end_turn(g, TurnEnd::Wait { wake: Wake::Input }).unwrap()
    });
}

fn quantiles(name: &str, unit: &str, v: &mut [u64]) -> String {
    v.sort_unstable();
    let q = |p: f64| v[((v.len() as f64 - 1.0) * p).round() as usize];
    format!(
        "{name:<8} n={:<6} p50={:>7} p95={:>7} p99={:>7} max={:>7} {unit}",
        v.len(),
        q(0.50),
        q(0.95),
        q(0.99),
        v[v.len() - 1]
    )
}

#[test]
#[ignore = "a measurement: run it alone, with THESEUS_RIG_DIR on the real disk"]
fn commit_latency_under_concurrent_turns() {
    let turns = env("THESEUS_RIG_TURNS", 32);
    let rounds = env("THESEUS_RIG_ROUNDS", 20);
    let workers = env("THESEUS_RIG_WORKERS", 4);
    let dir = match std::env::var("THESEUS_RIG_DIR") {
        Ok(d) => tempfile::tempdir_in(d).unwrap(),
        Err(_) => tempfile::tempdir().unwrap(),
    };
    let store: Arc<dyn Store> = Arc::new(
        WalStore::open_projected(
            &dir.path().join("store"),
            WalConfig::default(),
            &theseus_kernel::terms::PROJECTION,
        )
        .unwrap(),
    );
    let cfg = KernelConfig {
        admission_ceiling: turns as u32 + 1,
        ..KernelConfig::default()
    };
    let kernel = Arc::new(Kernel::new(store.clone(), Arc::new(RealClock), cfg));
    kernel.startup(None, &NoEvidence).unwrap();
    let execs: Vec<String> = (0..turns)
        .map(|_| {
            kernel
                .open_execution(
                    &new_id("ses"),
                    SessionKind::Conversation,
                    auth(),
                    Some(1_000_000_000),
                    None,
                )
                .unwrap()
                .id
        })
        .collect();
    let before = store.stats().unwrap();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap();
    let t0 = Instant::now();
    let (mut commits, mut rounds_ms, mut probes) = rt.block_on(async {
        let done = Arc::new(AtomicBool::new(false));
        let probe = {
            let done = done.clone();
            tokio::spawn(async move {
                let tick = Duration::from_millis(2);
                let mut late = Vec::new();
                while !done.load(Ordering::Relaxed) {
                    let t = Instant::now();
                    tokio::time::sleep(tick).await;
                    tokio::spawn(async {}).await.unwrap();
                    late.push(t.elapsed().saturating_sub(tick).as_micros() as u64);
                }
                late
            })
        };
        let mut tasks = Vec::new();
        for exec in execs {
            let k = kernel.clone();
            tasks.push(tokio::spawn(async move {
                let (mut commits, mut rounds_ms) = (Vec::new(), Vec::new());
                for _ in 0..rounds {
                    let t = Instant::now();
                    round(&k, &exec, &mut commits);
                    rounds_ms.push(t.elapsed().as_millis() as u64);
                    tokio::task::yield_now().await;
                }
                (commits, rounds_ms)
            }));
        }
        let (mut commits, mut rounds_ms) = (Vec::new(), Vec::new());
        for t in tasks {
            let (c, r) = t.await.unwrap();
            commits.extend(c);
            rounds_ms.extend(r);
        }
        done.store(true, Ordering::Relaxed);
        (commits, rounds_ms, probe.await.unwrap())
    });
    let wall = t0.elapsed();
    let after = store.stats().unwrap();
    let frames = after.frames_appended - before.frames_appended;
    let syncs = after.syncs - before.syncs;
    println!(
        "commit_latency: {turns} turns x {rounds} rounds on {workers} workers, {:.0} ms",
        wall.as_secs_f64() * 1000.0
    );
    println!("{}", quantiles("commit", "us", &mut commits));
    println!("{}", quantiles("round", "ms", &mut rounds_ms));
    println!("{}", quantiles("probe", "us", &mut probes));
    println!(
        "frames={frames} syncs={syncs} frames/sync={:.2} frames/s={:.0}",
        frames as f64 / syncs.max(1) as f64,
        frames as f64 / wall.as_secs_f64()
    );
    drop(rt);
}
