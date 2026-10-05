//! The daemon's own cgroup, found and readied for its jobs after serving
//! (theseus-a5nv): each L0 job then gets a cgroup of its own, with a process
//! cap and an exact stop (`theseus_kernel::cgroup`). Only a cgroup systemd
//! delegated to the daemon (`Delegate=yes`, as `theseusd install` writes) is
//! touched, and only systemd can say it is: the daemon asks it once. Health's
//! `cgroup` phase holds the answer, `delegated`, or what jobs fall back to and
//! why; until it is ready, a job stops by its process tree.

use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_kernel::cgroup::{self, Jobs};
use theseus_kernel::children::{self, Kind};

use crate::startup::StartupLog;

/// Ask, as long after serving as the index tender waits to start, so no
/// daemon that lives a moment, as each of the lifecycle bench's does, spawns
/// `systemctl` beside its measured stop; and ready the cgroup if it is
/// delegated. Each job gets `pids_max` (`[tools] job_pids_max`).
pub fn find_after_serving(log: Arc<StartupLog>, pids_max: u64) {
    tokio::spawn(async move {
        tokio::time::sleep(crate::tender::START_AFTER).await;
        let phase = log.begin("cgroup", true, Instant::now());
        let detail = match find(pids_max).await {
            Ok(jobs) => json!({"state": "delegated", "dir": jobs.dir, "pids_max": jobs.pids_max}),
            Err(why) => json!({"state": "none", "why": why}),
        };
        log.end(phase, detail);
    });
}

async fn find(pids_max: u64) -> Result<Jobs, String> {
    let dir = cgroup::own().map_err(|e| format!("no cgroup v2 here ({e})"))?;
    let unit = dir
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|u| u.ends_with(".service") || u.ends_with(".scope"))
        .ok_or_else(|| {
            format!(
                "the daemon runs in {}, not in a unit of its own (`theseusd install --user` makes one)",
                dir.display()
            )
        })?
        .to_string();
    let delegate = delegate(&unit, dir.to_string_lossy().contains("/user@")).await?;
    if delegate != "yes" {
        return Err(format!(
            "{unit} is not delegated (Delegate={delegate}): its unit needs Delegate=yes, as `theseusd \
             install` writes"
        ));
    }
    let jobs = Jobs { dir, pids_max };
    let readied = jobs.clone();
    tokio::task::spawn_blocking(move || cgroup::ready(readied))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("readying {} for jobs: {e}", jobs.dir.display()))?;
    Ok(jobs)
}

/// systemd's `Delegate` for `unit`, by `systemctl [--user] show`.
async fn delegate(unit: &str, user: bool) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    cmd.args(["show", "--property=Delegate", "--value", unit])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = children::spawn(Kind::Owned, || cmd.spawn(), tokio::process::Child::id)
        .map_err(|e| format!("could not ask systemd about {unit}: {e}"))?;
    let out = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .map_err(|_| format!("systemd gave no answer about {unit} in 5 s"))?
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "`systemctl show {unit}` exited with {}",
            out.status
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
