//! `bench jobs` (M4 17b; design §2.10): what a job's start costs, by class.
//! Each run starts `/bin/true` through the real job wrapper (`theseusd
//! job-wrapper`, detached, as the daemon starts every job), at L0 or in L1
//! over a workspace root and an empty HOME, and waits for its completion in
//! the spool. Two numbers per run:
//! - `start`: an L1 job's spawn, from the wrapper's call to the command's
//!   exec (`detail.sandbox.start_us`), against §2.2's target of a p95 under
//!   25 ms;
//! - `total`: from the dispatch to the completion's file, the wrapper's own
//!   start and its report included, at either class.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use theseus_kernel::job::{self, WrapperArgs, L1};
use theseus_kernel::Spool;

use crate::lifecycle::percentile;

/// §2.2's target for an L1 start's p95.
const START_P95_MS: f64 = 25.0;

#[derive(clap::Args, Debug)]
pub struct JobsArgs {
    /// The binary whose `job-wrapper` and `job-sandbox` roles run the jobs
    /// (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// `l0`, `l1`, or both.
    #[arg(long, value_delimiter = ',', default_value = "l0,l1")]
    class: Vec<String>,
    /// Measured jobs of each class, after two that are not.
    #[arg(long, default_value_t = 50)]
    runs: usize,
    /// Exit 1 when an L1 start's p95 is over 25 ms.
    #[arg(long)]
    check: bool,
}

pub fn jobs_cmd(a: JobsArgs) -> Result<()> {
    let theseusd = match a.theseusd {
        Some(p) => p,
        None => std::env::current_exe()?
            .parent()
            .context("locating this binary's directory")?
            .join("theseusd"),
    };
    let tmp = tempfile::tempdir()?;
    let spool = Spool::open(&tmp.path().join("spool"))?;
    let ws = tmp.path().join("work");
    std::fs::create_dir_all(&ws)?;
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home)?;
    let mut miss = false;
    println!(
        "bench jobs: /bin/true through `{} job-wrapper`, {} runs a class",
        theseusd.display(),
        a.runs
    );
    for class in &a.class {
        let l1 = match class.as_str() {
            "l0" => None,
            "l1" => Some(L1 {
                workspace: vec![ws.clone()],
                // `[sandbox]`'s defaults (design §2.12).
                limits: job::SandboxLimits::default(),
                memory_mb: 2048,
                ..Default::default()
            }),
            other => bail!("--class {other}: l0 or l1"),
        };
        let (mut start, mut total) = (Vec::new(), Vec::new());
        for i in 0..a.runs + 2 {
            let (s, t) = one(
                &theseusd,
                &spool,
                &ws,
                &home,
                l1.clone(),
                &format!("{class}-{i}"),
            )?;
            if i >= 2 {
                start.extend(s);
                total.push(t);
            }
        }
        start.sort_by(f64::total_cmp);
        total.sort_by(f64::total_cmp);
        let p = |v: &[f64], q| percentile(v, q);
        if start.is_empty() {
            println!(
                "  {class}: total p50 {:.2} ms, p95 {:.2} ms",
                p(&total, 50.0),
                p(&total, 95.0)
            );
        } else {
            let p95 = p(&start, 95.0);
            let ok = p95 < START_P95_MS;
            miss |= !ok;
            println!(
                "  {class}: start p50 {:.2} ms, p95 {p95:.2} ms (target under {START_P95_MS} ms: {}); total p50 {:.2} ms, p95 {:.2} ms",
                p(&start, 50.0),
                if ok { "ok" } else { "MISSED" },
                p(&total, 50.0),
                p(&total, 95.0),
            );
        }
    }
    if a.check && miss {
        std::process::exit(1);
    }
    Ok(())
}

/// One job: its L1 start in ms (none at L0), and its dispatch to its
/// completion in ms.
fn one(
    theseusd: &Path,
    spool: &Spool,
    ws: &Path,
    home: &Path,
    sandbox: Option<L1>,
    id: &str,
) -> Result<(Option<f64>, f64)> {
    let args = WrapperArgs {
        spool_dir: spool.dir().to_path_buf(),
        correlation_id: format!("bench-{id}"),
        deadline_ms: 30_000,
        notify_socket: None,
        argv: vec!["/bin/true".into()],
        cwd: Some(ws.to_path_buf()),
        env: vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), home.display().to_string()),
        ],
        umask: None,
        redact: vec![],
        output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
        sandbox,
    };
    let t0 = Instant::now();
    job::spawn_detached(theseusd, &[job::WRAPPER_MODE], spool, &args)?;
    let c = loop {
        if let Some(c) = spool.read_completion(&args.correlation_id)? {
            break c;
        }
        if t0.elapsed() > Duration::from_secs(30) {
            bail!("job {id}: no completion in 30 s");
        }
        std::thread::sleep(Duration::from_micros(200));
    };
    let total = t0.elapsed().as_secs_f64() * 1000.0;
    let detail = c.detail.unwrap_or_default();
    if let Some(e) = detail.pointer("/sandbox/error") {
        bail!("job {id} could not start in L1: {e}");
    }
    if c.outcome != theseus_kernel::Outcome::Succeeded {
        bail!("job {id} failed: {detail}");
    }
    spool.remove(&spool.completion_path(&args.correlation_id))?;
    let _ = spool.remove_result(&spool.result_path(&args.correlation_id));
    let start = detail
        .pointer("/sandbox/start_us")
        .and_then(serde_json::Value::as_u64)
        .map(|us| us as f64 / 1000.0);
    Ok((start, total))
}
