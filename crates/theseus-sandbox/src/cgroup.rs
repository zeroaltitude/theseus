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

/// Undoes [`delegate`] at a stop: every controller `dir/jobs` and `dir`
/// enable for their children is turned off, `jobs` first. A unit that keeps
/// its jobs across a stop (`KillMode=process`) needs it: systemd starts the
/// next daemon in `dir` itself, and cgroup v2 puts no process in a cgroup
/// whose children have controllers, so without it a restart while a job runs
/// fails (`status=219/CGROUP`, EBUSY) until the job ends. A job still
/// running keeps its cgroup but loses its limits; the next daemon's first L1
/// job readies `dir` again. What it turned off, in order.
pub fn release(dir: &Path) -> io::Result<Vec<String>> {
    let mut off = Vec::new();
    for d in [dir.join("jobs"), dir.to_path_buf()] {
        let file = d.join("cgroup.subtree_control");
        let on = match fs::read_to_string(&file) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            r => r?,
        };
        for c in on.split_whitespace() {
            fs::write(&file, format!("-{c}"))?;
            off.push(format!("{} -{c}", d.display()));
        }
    }
    Ok(off)
}

/// The unit whose stop hook this process is, from its own cgroup: systemd
/// runs a delegated unit's `ExecStopPost=` in `<unit>/.control`. Only a
/// service that a daemon readied (`daemon` or `jobs` in it), so a run by
/// hand anywhere else never touches a cgroup it does not own.
pub fn stop_hook_unit(own: &Path) -> Option<PathBuf> {
    if own.file_name()? != ".control" {
        return None;
    }
    let unit = own.parent()?;
    let readied = unit.join("jobs").is_dir() || unit.join("daemon").is_dir();
    (unit.file_name()?.to_str()?.ends_with(".service") && readied).then(|| unit.to_path_buf())
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

    /// The processes in it now (`cgroup.procs`), by pid.
    pub fn procs(&self) -> io::Result<Vec<u32>> {
        let procs = fs::read_to_string(self.path.join("cgroup.procs"))?;
        Ok(procs
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The stop hook acts only from a readied service's `.control`.
    #[test]
    fn the_stop_hook_finds_only_a_readied_services_cgroup() {
        let d = tempfile::tempdir().unwrap();
        let unit = d.path().join("theseusd.service");
        let control = unit.join(".control");
        fs::create_dir_all(&control).unwrap();
        assert_eq!(stop_hook_unit(&control), None, "never readied");
        fs::create_dir(unit.join("jobs")).unwrap();
        assert_eq!(stop_hook_unit(&control), Some(unit.clone()));
        assert_eq!(stop_hook_unit(&unit.join("daemon")), None);
        let scope = d.path().join("session-2.scope").join(".control");
        fs::create_dir_all(scope.parent().unwrap().join("jobs")).unwrap();
        assert_eq!(stop_hook_unit(&scope), None, "not a service");
    }

    /// `jobs` lets go before the service, or the kernel refuses the
    /// service's own (a child still has the controller).
    #[test]
    fn release_turns_off_jobs_first() {
        let d = tempfile::tempdir().unwrap();
        let jobs = d.path().join("jobs");
        fs::create_dir(&jobs).unwrap();
        for p in [&jobs, &d.path().to_path_buf()] {
            fs::write(p.join("cgroup.subtree_control"), "memory pids\n").unwrap();
        }
        let off = release(d.path()).unwrap();
        let want: Vec<String> = [(&jobs, "memory"), (&jobs, "pids")]
            .into_iter()
            .chain([
                (&d.path().to_path_buf(), "memory"),
                (&d.path().to_path_buf(), "pids"),
            ])
            .map(|(p, c)| format!("{} -{c}", p.display()))
            .collect();
        assert_eq!(off, want);
        let bare = tempfile::tempdir().unwrap();
        assert!(
            release(bare.path()).unwrap().is_empty(),
            "nothing to turn off"
        );
    }
}
