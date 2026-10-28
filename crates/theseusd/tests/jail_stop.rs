//! A job stopped at its limit in a daemon's jail names the phase its log was
//! last in (theseus-vv4km; theseus-ccik8 did it for the L0 stop only, and a
//! daemon never takes that one); a log with no phase line says so.
//!
//! `Jail::run_blocking` starts the job's init as `/proc/self/exe job-sandbox`,
//! the running binary (`theseusd`, in a daemon). A libtest binary reads that
//! word as a test filter, runs nothing and exits 0, so the init never reports
//! ("it exited before it started the command, exit status 0") and the stop is
//! never reached. So this is a test binary of its own (`harness = false`),
//! which takes the init's role first, as the sandbox's own test binaries do
//! (`theseus-sandbox/tests/common`). `self_build.rs` cannot hold it: its jobs
//! run through the wrapper, which is `theseusd`.
//!
//! Real L1, unprivileged: as root L1 refuses every job (theseus-pv6i), and the
//! case only checks that refusal.

#[path = "../../theseus-sandbox/tests/common/mod.rs"]
mod common;

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use common::{check, Case};
use theseus_core::rsi::jail::{Jail, Job};
use theseus_core::rsi::tree::{Place, Tree};

fn main() -> ! {
    common::main(
        &[Case {
            name: "a_stopped_jailed_job_names_its_phase",
            run: a_stopped_jailed_job_names_its_phase,
        }],
        &[],
        |_| {},
    )
}

/// A job that outlives its limit is stopped, and the error names its log's
/// last `gate: ` line, or says there is none.
fn a_stopped_jailed_job_names_its_phase() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        check(
            theseus_sandbox::refused_here().is_some(),
            "L1 accepted a root daemon's job",
        )?;
        eprintln!(
            "skipped: L1 refuses a root daemon's jobs (theseus-pv6i); run as an ordinary user"
        );
        return Ok(());
    }
    let dir = tempfile::tempdir().map_err(|e| format!("a temp dir: {e}"))?;
    let p = |s: &str| dir.path().join(s);
    for d in ["repo/state", "repo/sealed", "tree", "target"] {
        std::fs::create_dir_all(p(d)).map_err(|e| format!("{d}: {e}"))?;
    }
    let mut cfg = theseus_core::Config::example();
    cfg.self_improve.sealed.dir = Some(p("repo/sealed").to_string_lossy().into_owned());
    let sandbox = theseus_core::toolrun::sandbox_for(&cfg, &p("repo/state"));
    let tree = Tree {
        slug: "t".into(),
        branch: "self/t".into(),
        dir: p("tree"),
        target: p("target"),
    };
    let place = Place {
        repo: p("repo"),
        state: p("state"),
        env: Vec::new(),
        jail: None,
        notify: None,
        stop: None,
    };
    let jail = Jail::new(Arc::new(sandbox));
    for (i, (body, want)) in [
        (
            "echo 'gate: phase suite'; exec sleep 60",
            "gate: phase suite",
        ),
        ("exec sleep 60", "no `gate: ` line"),
    ]
    .into_iter()
    .enumerate()
    {
        let log = p(&format!("stopped-{i}.log"));
        let job = Job {
            tree: &tree,
            ro: Vec::new(),
            argv: ["sh", "-c", body].map(OsString::from).to_vec(),
            env: Vec::new(),
            log: &log,
            limit: Duration::from_secs(3),
        };
        let e = jail
            .run_blocking(&place, &job)
            .err()
            .ok_or("a job that outlived its limit was not stopped")?;
        let said = format!("{e:#}");
        check(said.contains("was stopped"), format!("not a stop: {said}"))?;
        check(said.contains(want), format!("no {want:?} in: {said}"))?;
    }
    Ok(())
}
