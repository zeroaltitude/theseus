//! `bench jobs` (M4 17b; design §2.10): what a job's start costs, by class.
//! Each run starts `/bin/true` through the real job wrapper (`theseusd
//! job-wrapper`, detached, as the daemon starts every job), at L0 or in L1
//! over a workspace root and an empty HOME, and waits for its completion in
//! the spool. Three numbers per run:
//! - `start`: an L1 job's spawn, from the wrapper's call to the command's
//!   exec (`detail.sandbox.start_us`), against §2.2's target of a p95 under
//!   25 ms;
//! - `total`: from the dispatch to the completion's file, the wrapper's own
//!   start and its report included, at either class;
//! - `notified`: from the dispatch to the wrapper's poke on its notify socket,
//!   which is when a daemon hears of the completion: the completion's file is
//!   seen at its rename, and what the wrapper syncs after the rename shows
//!   only here (theseus-yxiv).
//!
//! `--class l1-egress` (18c) gives the L1 job an egress list, so its start
//! includes the listener's handoff and its end the proxy's stop.
//!
//! Each job carries an operator's umask (022), as the daemon's do. `--hold-mb
//! N` touches N MB before the runs and holds them, as a daemon grown to that
//! size would (theseus-ypqg): a spawn that copies its spawner's page tables
//! (a fork) costs more with every megabyte, and one that shares them doesn't.

use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use theseus_kernel::job::{self, WrapperArgs, L1};
use theseus_kernel::Spool;

use crate::history;
use crate::lifecycle::{percentile, Verdict};

/// §2.2's target for an L1 start's p95.
const START_P95_MS: f64 = 25.0;

#[derive(clap::Args, Debug)]
pub struct JobsArgs {
    /// The binary whose `job-wrapper` and `job-sandbox` roles run the jobs
    /// (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// `l0`, `l1`, `l1-egress` (an L1 job with an egress list, so with its
    /// listener and proxy: 18c), or several.
    #[arg(long, value_delimiter = ',', default_value = "l0,l1")]
    class: Vec<String>,
    /// Measured jobs of each class, after two that are not.
    #[arg(long, default_value_t = 50)]
    runs: usize,
    /// Exit 1 when an L1 start's p95 is over 25 ms.
    #[arg(long)]
    check: bool,
    /// The busy allowance, a percentage of the 25 ms (theseus-lew7): a p95
    /// over it by no more than this passes --check, and says so. The gate
    /// gives it only when its settle step found no quiet window.
    #[arg(long, default_value_t = 0)]
    allowance: u32,
    /// Megabytes this process touches and holds while it dispatches, as a
    /// daemon of that size.
    #[arg(long, default_value_t = 0)]
    hold_mb: usize,
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
    let rig = Rig::new(theseusd, tmp.path())?;
    let mut miss = false;
    // Every page written, so each is resident and mapped.
    let held = vec![1u8; a.hold_mb << 20];
    println!(
        "bench jobs: /bin/true through `{} job-wrapper`, {} runs a class, holding {} MB",
        rig.theseusd.display(),
        a.runs,
        a.hold_mb
    );
    let ws = rig.ws.clone();
    for class in &a.class {
        let l1 = match class.as_str() {
            "l0" => None,
            "l1" | "l1-egress" => Some(L1 {
                workspace: vec![ws.clone()],
                // `[sandbox]`'s defaults (design §2.12).
                limits: job::SandboxLimits::default(),
                // 18c: a list, so the init opens the listener and the
                // wrapper runs the proxy, which the job never uses.
                egress: if class == "l1-egress" {
                    vec!["bench.test:443".into()]
                } else {
                    Vec::new()
                },
                ..Default::default()
            }),
            other => bail!("--class {other}: l0, l1, or l1-egress"),
        };
        let (mut start, mut total, mut notified) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..a.runs + 2 {
            let r = one(&rig, l1.clone(), &format!("{class}-{i}"))?;
            if i >= 2 {
                start.extend(r.start);
                total.push(r.total);
                notified.push(r.notified);
            }
        }
        start.sort_by(f64::total_cmp);
        total.sort_by(f64::total_cmp);
        notified.sort_by(f64::total_cmp);
        let p = |v: &[f64], q| percentile(v, q);
        let poke = format!(
            "notified p50 {:.2} ms, p95 {:.2} ms",
            p(&notified, 50.0),
            p(&notified, 95.0)
        );
        if start.is_empty() {
            println!(
                "  {class}: total p50 {:.2} ms, p95 {:.2} ms; {poke}",
                p(&total, 50.0),
                p(&total, 95.0)
            );
        } else {
            let p95 = p(&start, 95.0);
            let v = Verdict {
                phase: "l1_start".to_string(),
                p95,
                budget: START_P95_MS,
                margin: 0.0,
                ok: p95 < START_P95_MS,
            };
            miss |= !history::allowed(&v, a.allowance);
            println!(
                "  {class}: start p50 {:.2} ms, p95 {p95:.2} ms (target under {START_P95_MS} ms: {}); total p50 {:.2} ms, p95 {:.2} ms; {poke}",
                p(&start, 50.0),
                if v.ok { "ok" } else { "MISSED" },
                p(&total, 50.0),
                p(&total, 95.0),
            );
            for line in history::allowance_applied("jobs", &[v], a.allowance) {
                println!("{line}");
            }
        }
    }
    std::hint::black_box(&held);
    if a.check && miss {
        std::process::exit(1);
    }
    Ok(())
}

/// What every run shares: the wrapper's binary, the spool, the workspace and
/// HOME, and the notify socket the wrappers poke, as a daemon's.
struct Rig {
    theseusd: PathBuf,
    spool: Spool,
    ws: PathBuf,
    home: PathBuf,
    sock: PathBuf,
    listener: UnixListener,
}

impl Rig {
    fn new(theseusd: PathBuf, dir: &Path) -> Result<Self> {
        let spool = Spool::open(&dir.join("spool"))?;
        let ws = dir.join("work");
        std::fs::create_dir_all(&ws)?;
        let home = dir.join("home");
        std::fs::create_dir_all(&home)?;
        let sock = dir.join("notify.sock");
        let listener = UnixListener::bind(&sock)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            theseusd,
            spool,
            ws,
            home,
            sock,
            listener,
        })
    }

    /// When job `id`'s wrapper poked the socket, if it has: a connection
    /// accepted, and its line naming the job.
    fn poked(&self, id: &str) -> Result<Option<Instant>> {
        let mut s = match self.listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let at = Instant::now();
        s.set_nonblocking(false)?;
        s.set_read_timeout(Some(Duration::from_secs(1)))?;
        let mut line = String::new();
        std::io::Read::read_to_string(&mut s, &mut line)?;
        Ok((line.trim() == id).then_some(at))
    }
}

/// One job's numbers, in ms: its L1 start (none at L0), its dispatch to its
/// completion's file, and its dispatch to its wrapper's poke.
struct Run {
    start: Option<f64>,
    total: f64,
    notified: f64,
}

/// One job.
fn one(rig: &Rig, sandbox: Option<L1>, id: &str) -> Result<Run> {
    let spool = &rig.spool;
    let args = WrapperArgs {
        spool_dir: spool.dir().to_path_buf(),
        correlation_id: format!("bench-{id}"),
        deadline_ms: 30_000,
        notify_socket: Some(rig.sock.clone()),
        argv: vec!["/bin/true".into()],
        cwd: Some(rig.ws.clone()),
        env: vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), rig.home.display().to_string()),
        ],
        umask: Some(0o022),
        redact: vec![],
        output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
        sandbox,
    };
    let ms = |t0: Instant, at: Instant| (at - t0).as_secs_f64() * 1000.0;
    let t0 = Instant::now();
    job::spawn_detached(&rig.theseusd, &[job::WRAPPER_MODE], spool, &args)?;
    let (mut seen, mut poked) = (None, None);
    let c = loop {
        if seen.is_none() {
            if let Some(c) = spool.read_completion(&args.correlation_id)? {
                seen = Some((c, Instant::now()));
            }
        }
        if poked.is_none() {
            poked = rig.poked(&args.correlation_id)?;
        }
        if let (Some((c, at)), Some(p)) = (&seen, poked) {
            break (c.clone(), ms(t0, *at), ms(t0, p));
        }
        if t0.elapsed() > Duration::from_secs(30) {
            bail!("job {id}: no completion and poke in 30 s");
        }
        std::thread::sleep(Duration::from_micros(200));
    };
    let (c, total, notified) = c;
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
    Ok(Run {
        start,
        total,
        notified,
    })
}
