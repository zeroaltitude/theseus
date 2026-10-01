//! Limits through a delegated cgroup (design §2.2, "Limits"). Where the
//! daemon's cgroup is delegated (a systemd unit with `Delegate=yes`), each
//! job gets its own, `<daemon's cgroup>/jobs/<corr>`, with `memory.max` and
//! `pids.max`, and `Spec::cgroup` names it. Where it is not, the namespaces
//! and seccomp still hold, and the job's processes are still capped by
//! `RLIMIT_NPROC`.
//!
//! These are the pieces; 17b decides when (lazily, at the first L1 job,
//! never on the start path) and whether a cgroup is really delegated.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const ROOT: &str = "/sys/fs/cgroup";

/// The controllers a job's cgroup uses, where its parent offers them.
const CONTROLLERS: [&str; 2] = ["memory", "pids"];

/// This process's cgroup (v2), as a directory.
pub fn own() -> io::Result<PathBuf> {
    let s = fs::read_to_string("/proc/self/cgroup")?;
    let rel = s
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or_else(|| io::Error::other("no cgroup v2 line in /proc/self/cgroup"))?;
    Ok(Path::new(ROOT).join(rel.trim_start_matches('/')))
}

/// Readies a delegated cgroup `dir` for jobs, and returns `dir/jobs`.
///
/// cgroup v2 puts no process in an inner node, so every process in `dir`
/// moves to the leaf `dir/daemon` first. Then `dir`'s children get the
/// memory and pids controllers it offers, and so do the children of
/// `dir/jobs`. Only for a cgroup this process owns: never a unit's cgroup
/// that systemd did not delegate.
pub fn delegate(dir: &Path) -> io::Result<PathBuf> {
    let leaf = dir.join("daemon");
    make(&leaf)?;
    for pid in fs::read_to_string(dir.join("cgroup.procs"))?.lines() {
        // A process that has exited since the read is no error.
        match fs::write(leaf.join("cgroup.procs"), pid) {
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {}
            r => r?,
        }
    }
    let offered = fs::read_to_string(dir.join("cgroup.controllers"))?;
    let enable: Vec<String> = CONTROLLERS
        .iter()
        .filter(|c| offered.split_whitespace().any(|o| o == **c))
        .map(|c| format!("+{c}"))
        .collect();
    let enable = enable.join(" ");
    fs::write(dir.join("cgroup.subtree_control"), &enable)?;
    let jobs = dir.join("jobs");
    make(&jobs)?;
    fs::write(jobs.join("cgroup.subtree_control"), &enable)?;
    Ok(jobs)
}

fn make(p: &Path) -> io::Result<()> {
    match fs::create_dir(p) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        r => r,
    }
}

/// One job's cgroup.
#[derive(Debug)]
pub struct JobCgroup {
    path: PathBuf,
}

impl JobCgroup {
    /// Makes `jobs/<name>` with its limits.
    pub fn create(
        jobs: &Path,
        name: &str,
        memory_mb: Option<u64>,
        pids: Option<u64>,
    ) -> io::Result<Self> {
        let path = jobs.join(name);
        fs::create_dir(&path)?;
        let cg = Self { path };
        let set = |file: &str, value: String| fs::write(cg.path.join(file), value);
        if let Some(mb) = memory_mb {
            set("memory.max", (mb << 20).to_string())?;
        }
        if let Some(n) = pids {
            set("pids.max", n.to_string())?;
        }
        Ok(cg)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `cgroup.kill`: every process in it, at once.
    pub fn kill(&self) -> io::Result<()> {
        fs::write(self.path.join("cgroup.kill"), "1")
    }

    /// Whether any process is still in it (`cgroup.events`).
    pub fn populated(&self) -> io::Result<bool> {
        let events = fs::read_to_string(self.path.join("cgroup.events"))?;
        Ok(events.lines().any(|l| l == "populated 1"))
    }

    /// How many forks `pids.max` refused (`pids.events`).
    pub fn pids_refused(&self) -> io::Result<u64> {
        let events = fs::read_to_string(self.path.join("pids.events"))?;
        Ok(events
            .lines()
            .find_map(|l| l.strip_prefix("max "))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0))
    }

    /// Removes it; it must be empty.
    pub fn remove(self) -> io::Result<()> {
        fs::remove_dir(&self.path)
    }
}
