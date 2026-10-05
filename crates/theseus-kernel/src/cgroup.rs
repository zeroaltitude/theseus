//! A job's cgroup (theseus-a5nv; the Linux survey's card 5). Where the
//! daemon's own cgroup is delegated to it (a systemd unit with
//! `Delegate=yes`), each L0 job's command is born in a cgroup of its own,
//! `<the daemon's cgroup>/job-<correlation id>`. It gives the job a process
//! cap (`pids.max`, threads counted), its CPU time (`cpu.stat`), and an exact
//! stop: every process in it, whatever tree, group, or session it left.
//!
//! The daemon stays in its unit's cgroup, so a restart while a job runs
//! starts as any start does: the design cut at theseus-gyin moved the daemon
//! into a leaf, and systemd then could not start the next one in the unit's
//! cgroup (219/CGROUP). With the daemon in it and `pids` on for its children,
//! the kernel takes the unit's cgroup for a thread root, whose children must
//! be threaded. A threaded cgroup has `pids.max`, `cpu.stat`, and
//! `cgroup.events`, but no `cgroup.kill`, so a stop lowers `pids.max` to 0,
//! which no new process gets past, and kills what `cgroup.threads` lists until
//! `cgroup.events` says `populated 0`.
//!
//! The command is born inside by `clone3` (`crate::spawn`), never moved in: a
//! move waits for an RCU grace period. Without delegation (a daemon under
//! another unit, `cargo test`) a job has no cgroup, and a stop finds its
//! processes by its tree (`crate::tree`), as before.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Seek};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::tree::{self, Stopped};

/// Where a daemon's jobs get their cgroups, and the cap each gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jobs {
    /// The daemon's own cgroup: its unit's, delegated to it.
    pub dir: PathBuf,
    /// Each job's `pids.max`: its processes and threads at once; 0, none.
    pub pids_max: u64,
}

/// Each job's cap unless the config says otherwise (`[tools] job_pids_max`):
/// far above what a build or a test run starts, far below the unit's own
/// (systemd's `TasksMax`, 15% of the kernel's), so a fork bomb in one job
/// leaves the daemon room.
pub const DEFAULT_PIDS_MAX: u64 = 4096;

static READY: OnceLock<Jobs> = OnceLock::new();

/// Where this daemon's jobs get their cgroups, once `ready` has readied its
/// own.
pub fn ready_jobs() -> Option<&'static Jobs> {
    READY.get()
}

/// This process's cgroup (v2), as a directory.
pub fn own() -> io::Result<PathBuf> {
    let s = fs::read_to_string("/proc/self/cgroup")?;
    let rel = s
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or_else(|| io::Error::other("no cgroup v2 line in /proc/self/cgroup"))?;
    Ok(Path::new("/sys/fs/cgroup").join(rel.trim_start_matches('/')))
}

/// Ready `jobs.dir`, the daemon's delegated cgroup, for jobs: `pids` on for
/// its children, which makes it a thread root with the daemon still in it,
/// and the empty job cgroups an earlier daemon left removed. From then on
/// each job this daemon starts gets its own.
pub fn ready(jobs: Jobs) -> io::Result<()> {
    fs::write(jobs.dir.join("cgroup.subtree_control"), "+pids")?;
    for e in fs::read_dir(&jobs.dir)?.flatten() {
        if e.file_name().to_string_lossy().starts_with("job-") {
            // One whose job still runs stays.
            let _ = fs::remove_dir(e.path());
        }
    }
    let _ = READY.set(jobs);
    Ok(())
}

/// One job's cgroup, made by its wrapper before the command.
pub(crate) struct Job {
    dir: PathBuf,
    fd: OwnedFd,
}

impl Job {
    /// `<jobs.dir>/job-<id>`: threaded, with its cap.
    pub(crate) fn make(jobs: &Jobs, id: &str) -> io::Result<Job> {
        let dir = jobs.dir.join(format!("job-{id}"));
        fs::create_dir(&dir)?;
        let cap = match jobs.pids_max {
            0 => "max".to_string(),
            n => n.to_string(),
        };
        let made = fs::write(dir.join("cgroup.type"), "threaded")
            .and_then(|()| fs::write(dir.join("pids.max"), cap))
            .and_then(|()| fs::File::open(&dir));
        match made {
            Ok(f) => Ok(Job { dir, fd: f.into() }),
            Err(e) => {
                let _ = fs::remove_dir(&dir);
                Err(e)
            }
        }
    }

    /// For `clone3`'s `CLONE_INTO_CGROUP`.
    pub(crate) fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Its CPU time so far, in µs (`cpu.stat`), and how many new processes
    /// its cap has refused (`pids.events`).
    pub(crate) fn usage(&self) -> (Option<u64>, u64) {
        let field = |file: &str, key: &str| -> Option<u64> {
            fs::read_to_string(self.dir.join(file))
                .ok()?
                .lines()
                .find_map(|l| l.strip_prefix(key)?.trim().parse().ok())
        };
        (
            field("cpu.stat", "usage_usec "),
            field("pids.events", "max ").unwrap_or(0),
        )
    }

    /// Stop every process in it, in `tree::stop`'s phases: SIGTERM to each,
    /// and up to `grace` for it to empty; then `pids.max` 0, so nothing new
    /// starts, and SIGKILL to each until it is empty, up to `tree::KILL_WAIT`.
    /// Each wait is woken by `cgroup.events`. `reap` runs at the end: a zombie
    /// has left the cgroup, but its parent, the wrapper, still reaps it.
    pub(crate) fn stop(&self, grace: Duration, reap: &mut dyn FnMut() -> tree::Left) -> Stopped {
        let t0 = Instant::now();
        let mut met = BTreeSet::new();
        self.signal(libc::SIGTERM, &mut met);
        let mut empty = self.wait_empty(grace);
        if !empty {
            let _ = fs::write(self.dir.join("pids.max"), "0");
            let until = Instant::now() + tree::KILL_WAIT;
            while !empty && Instant::now() < until {
                self.signal(libc::SIGKILL, &mut met);
                let left = until.saturating_duration_since(Instant::now());
                empty = self.wait_empty(left.min(Duration::from_millis(50)));
            }
        }
        let _ = reap();
        let survivors = if empty {
            vec![]
        } else {
            let left: BTreeSet<u32> = self
                .tasks()
                .into_iter()
                .map(|t| tgid(t).unwrap_or(t))
                .collect();
            left.into_iter()
                .map(|p| (p, tree::stat(p).map_or('?', |s| s.state)))
                .collect()
        };
        Stopped {
            killed: met.len() as u32,
            survivors,
            unseen: false,
            ms: t0.elapsed().as_millis() as u64,
        }
    }

    /// Its tasks now: a threaded cgroup lists threads, never processes.
    fn tasks(&self) -> Vec<u32> {
        fs::read_to_string(self.dir.join("cgroup.threads"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect()
    }

    /// `sig` to each process with a task in it; `met` counts each process
    /// once, by its thread group.
    fn signal(&self, sig: libc::c_int, met: &mut BTreeSet<u32>) {
        for p in self.tasks().into_iter().map(|t| tgid(t).unwrap_or(t)) {
            if met.insert(p) || sig == libc::SIGKILL {
                // SAFETY: kill(2) on a process in this job's cgroup, which the
                // cap keeps from growing; no memory is shared.
                unsafe { libc::kill(p as libc::pid_t, sig) };
            }
        }
    }

    /// Up to `limit` for `populated 0`, woken by `cgroup.events`, which polls
    /// as changed once read: whether it came.
    fn wait_empty(&self, limit: Duration) -> bool {
        let Ok(mut f) = fs::File::open(self.dir.join("cgroup.events")) else {
            return true;
        };
        let until = Instant::now() + limit;
        loop {
            let mut s = String::new();
            if f.rewind().and_then(|()| f.read_to_string(&mut s)).is_err() {
                return false;
            }
            if s.lines().any(|l| l == "populated 0") {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let mut p = libc::pollfd {
                fd: f.as_raw_fd(),
                events: libc::POLLPRI,
                revents: 0,
            };
            // SAFETY: one pollfd, for the call's length.
            unsafe { libc::poll(&mut p, 1, left.as_millis().clamp(1, 60_000) as i32) };
        }
    }

    /// Remove it, once its last process has gone; one that is not empty stays,
    /// for the next daemon's `ready` to take.
    pub(crate) fn remove(&self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

/// A task's thread group: its process.
fn tgid(task: u32) -> Option<u32> {
    fs::read_to_string(format!("/proc/{task}/status"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("Tgid:")?.trim().parse().ok())
}
