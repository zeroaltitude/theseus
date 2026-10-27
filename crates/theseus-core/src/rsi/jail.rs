//! Where a self branch's code runs (theseus-pw1q.21.1; RSI loop 1, §4.2):
//! in L1, always. A branch is unreviewed code until the join accepts it, and
//! its gate, its tests, its `build.rs` and its scripts run as the operator's
//! uid; at L0 that code could read a sealed holdout set (theseus-pw1q.5) or
//! the store. [`Jail`] is the one launcher: the branch's gate, the planted
//! revert's test runs, the merge gate with its benches, auto-revert's gate and
//! the install's build all come through it, over [`Sandbox::self_build_view`]
//! (the tree and its build tree read-write, no network, the floor hidden).
//!
//! A [`Place`] carries a jail in a daemon (`Board::place`); a test's place has
//! none and runs its stand-in gate at L0, as before. Nothing falls back: a
//! host that cannot run L1 builds nothing (`rsi::pr` asks first), and a job
//! that cannot start in L1 is an error here, never an L0 run.
//!
//! Main's `may-build.sh` still runs at L0 beside the daemon: it is main's own
//! reviewed script, and it only reads the machine's load.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use theseus_kernel::job::L1;
use theseus_kernel::probe_run::{self, Logged};

use super::tree::{Place, Tree};
use crate::sandbox::Sandbox;
use crate::sandbox::{self_build_env, SelfBuild};

/// L1 for a daemon's self builds.
#[derive(Clone)]
pub struct Jail {
    sandbox: Arc<Sandbox>,
}

impl std::fmt::Debug for Jail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Jail")
    }
}

/// One command of a self build.
pub struct Job<'a> {
    pub tree: &'a Tree,
    /// More read-only files the command needs (main's copy of the gate).
    pub ro: Vec<PathBuf>,
    pub argv: Vec<OsString>,
    /// Variables beside `sandbox::self_build_env`'s.
    pub env: Vec<(String, String)>,
    pub log: &'a Path,
    pub limit: Duration,
}

impl Jail {
    pub fn new(sandbox: Arc<Sandbox>) -> Self {
        Self { sandbox }
    }

    /// Why L1 cannot run a self build on this host, or `None`: the same
    /// refusal health shows for L1 (a daemon that runs as root, theseus-pv6i).
    pub fn refusal(&self) -> Option<String> {
        self.sandbox.refusal().map(str::to_string)
    }

    /// The view and the environment `j` runs in: the self-build view over
    /// this daemon's floor, and the whole of the command's environment.
    pub fn prepare(&self, p: &Place, j: &Job<'_>) -> (L1, Vec<(String, String)>) {
        let repo = theseus_tools::paths::canonical_best_effort(&p.repo);
        let home = crate::config::expand("~");
        let view = self.sandbox.self_build_view(&SelfBuild {
            tree: &j.tree.dir,
            target: &j.tree.target,
            repo: &repo,
            ro: &j.ro,
            home: &home,
        });
        let mut env = self_build_env(&home, &j.tree.target);
        env.extend(j.env.iter().cloned());
        env.extend(p.env.iter().map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        }));
        (view, env)
    }

    /// Run the command in L1 and wait, on the calling thread (which lives as
    /// long as the job: the init dies with the thread that spawned it). Its
    /// exit code; a deadline or a start that failed is an error.
    pub fn run_blocking(&self, p: &Place, j: &Job<'_>) -> Result<Option<i32>> {
        let (view, env) = self.prepare(p, j);
        let argv: Vec<String> = j
            .argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let log = std::fs::File::create(j.log).context("opening the build's log")?;
        let ms = u64::try_from(j.limit.as_millis()).unwrap_or(u64::MAX);
        let Logged {
            code,
            timed_out,
            failed,
            silent_ms,
            ..
        } = probe_run::run_logged(&view, &argv, &j.tree.dir, env, log, ms);
        if timed_out {
            match silent_ms {
                Some(_) => bail!(
                    "the build printed nothing for {} minutes and was stopped",
                    j.limit.as_secs() / 60
                ),
                None => bail!(
                    "the build was still printing at its ceiling, {} minutes, and was stopped",
                    j.limit.as_secs() * u64::from(theseus_kernel::patience::CEILING_FACTOR) / 60
                ),
            }
        }
        match (code, failed) {
            (Some(c), _) => Ok(Some(c)),
            (None, Some(why)) => bail!("{why}"),
            (None, None) => Ok(None),
        }
    }

    /// `run_blocking` on the blocking pool.
    pub async fn run(&self, p: &Place, j: Job<'_>) -> Result<Option<i32>> {
        let (jail, place) = (self.clone(), p.clone());
        let (tree, log) = (j.tree.clone(), j.log.to_path_buf());
        let owned = (j.ro, j.argv, j.env, j.limit);
        tokio::task::spawn_blocking(move || {
            let job = Job {
                tree: &tree,
                ro: owned.0,
                argv: owned.1,
                env: owned.2,
                log: &log,
                limit: owned.3,
            };
            jail.run_blocking(&place, &job)
        })
        .await
        .context("the build's thread failed")?
    }
}
