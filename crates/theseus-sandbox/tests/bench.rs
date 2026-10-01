//! The spawn micro-bench (design §2.2, "Cost and the probe"): 100 L1
//! starts of `/bin/true`, each as 17b will start a job (a workspace root
//! under an overlay, HOME, the system binds, `/proc`, `/sys`, `/dev`), timed
//! from the dispatch (`spawn`) to the command's exec, which is when `spawn`
//! returns. The target is a p95 under 25 ms.
//!
//! It prints p50, p95, and the max, with the init's own part split out, and
//! the same for an L0 start (a plain fork and exec) beside it. The case
//! fails only past ten times the target, so a loaded machine never fails the
//! gate; `THESEUS_SANDBOX_BENCH_STRICT=1` holds it to the target itself.

mod common;

use std::time::{Duration, Instant};

use common::{check, job, Case, Output};

const RUNS: usize = 100;
const TARGET: Duration = Duration::from_millis(25);

fn main() {
    common::main(
        &[Case {
            name: "spawn_100",
            run: bench,
        }],
        |_| {},
    );
}

fn pct(sorted: &[Duration], p: f64) -> Duration {
    let i = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[i]
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn line(what: &str, mut v: Vec<Duration>) -> Duration {
    v.sort();
    let p95 = pct(&v, 0.95);
    println!(
        "{what:<26} p50 {:>9}  p95 {:>9}  max {:>9}",
        ms(pct(&v, 0.50)),
        ms(p95),
        ms(*v.last().unwrap_or(&Duration::ZERO))
    );
    p95
}

fn bench() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spec = job(vec!["/bin/true".into()], ws.path());
    let out = Output::new();
    // One start to warm the page cache, untimed.
    common::spawn(&spec, &out)?
        .wait()
        .map_err(|e| e.to_string())?;
    let (mut start, mut init, mut whole) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut child = common::spawn(&spec, &out)?;
        start.push(t.elapsed());
        init.push(Duration::from_micros(child.started().setup_us));
        let exit = child.wait().map_err(|e| e.to_string())?;
        whole.push(t.elapsed());
        check(exit.success(), format!("/bin/true in L1: {exit:?}"))?;
    }
    let mut l0 = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut c = std::process::Command::new("/bin/true")
            .spawn()
            .map_err(|e| e.to_string())?;
        l0.push(t.elapsed());
        c.wait().map_err(|e| e.to_string())?;
    }
    println!(
        "{RUNS} starts of /bin/true (target: an L1 start's p95 under {})",
        ms(TARGET)
    );
    let p95 = line("L1 start (dispatch→exec)", start);
    line("  of which the init", init);
    line("L1 start to exit", whole);
    line("L0 start (fork+exec)", l0);
    let strict = std::env::var_os("THESEUS_SANDBOX_BENCH_STRICT").is_some();
    let bound = if strict { TARGET } else { TARGET * 10 };
    println!(
        "L1 p95 {} the target{}",
        if p95 <= TARGET { "meets" } else { "misses" },
        if strict { " (strict)" } else { "" }
    );
    check(
        p95 <= bound,
        format!("an L1 start's p95 is {}, past {}", ms(p95), ms(bound)),
    )
}
