//! A job's cgroup (theseus-a5nv), where systemd delegated the daemon's own
//! (`Delegate=yes`): the real wrapper's command is born in a cgroup of its own
//! there. A stop of a fork loop that ignores SIGTERM ends every process in
//! it, none left (`verified_by: cgroup`); a job past its cap is refused new
//! processes, and its completion says so; and a unit restarts while a job
//! lives in its cgroup, the case the cut design failed (219/CGROUP). Each
//! needs systemd's user manager (`systemd-run --user`), and says on stderr
//! when it skips.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use theseus_kernel::cgroup::{self, Jobs};
use theseus_kernel::job::{self, WrapperArgs};
use theseus_kernel::Spool;

fn wait_for<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(t0.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `systemctl --user show -p NAME --value UNIT`.
fn show(unit: &str, name: &str) -> String {
    let out = Command::new("systemctl")
        .args(["--user", "show", "-p", name, "--value", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    out.unwrap_or_default()
}

/// A unit's cgroup, once systemd has made it.
fn unit_dir(unit: &str) -> Option<PathBuf> {
    let cg = show(unit, "ControlGroup");
    let dir = Path::new("/sys/fs/cgroup").join(cg.trim_start_matches('/'));
    (!cg.is_empty() && dir.join("cgroup.procs").exists()).then_some(dir)
}

/// A delegated scope that holds one `sleep`: its cgroup stands in for a
/// daemon's unit's. Stopped, with everything in it, when dropped.
struct Scope {
    unit: String,
    dir: PathBuf,
    holder: std::process::Child,
}

impl Scope {
    fn start(name: &str) -> Option<Scope> {
        let unit = format!("theseus-test-{name}-{}.scope", std::process::id());
        let mut holder = Command::new("systemd-run")
            .args([
                "--user",
                "--scope",
                "-q",
                "-p",
                "Delegate=yes",
                "--unit",
                &unit,
            ])
            .args(["sleep", "600"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let t0 = Instant::now();
        loop {
            if let Some(dir) = unit_dir(&unit) {
                return Some(Scope { unit, dir, holder });
            }
            if holder.try_wait().ok().flatten().is_some() || t0.elapsed() > Duration::from_secs(10)
            {
                let _ = holder.kill();
                let _ = holder.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Its cgroup, readied for jobs as a delegated daemon readies its own.
    fn jobs(&self, pids_max: u64) -> Jobs {
        let jobs = Jobs {
            dir: self.dir.clone(),
            pids_max,
        };
        cgroup::ready(jobs.clone()).unwrap();
        jobs
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &self.unit])
            .status();
        let _ = self.holder.kill();
        let _ = self.holder.wait();
        // Systemd leaves its cgroup when a job's was still there as it
        // stopped: empty now, it goes.
        let _ = std::fs::remove_dir(&self.dir);
    }
}

fn skip(why: &str) {
    eprintln!("skipped: {why}: no delegated scope (systemd-run --user) here");
}

/// A spool, and `sh -c script` under a detached wrapper with `jobs` as its
/// cgroup, in the rig's directory, which is `$1`.
struct Rig {
    dir: tempfile::TempDir,
    spool: Spool,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let spool = Spool::open(&dir.path().join("spool")).unwrap();
        Self { dir, spool }
    }

    fn start(&self, jobs: &Jobs, id: &str, script: &str) -> u32 {
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: id.into(),
            deadline_ms: 60_000,
            notify_socket: None,
            argv: vec![
                "sh".into(),
                "-c".into(),
                script.into(),
                "sh".into(),
                self.dir.path().display().to_string(),
            ],
            cwd: Some(self.dir.path().to_path_buf()),
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
            umask: Some(0o022),
            redact: vec![],
            output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: None,
            cgroup: Some(jobs.clone()),
        };
        let theseusd = Path::new(env!("CARGO_BIN_EXE_theseusd"));
        job::spawn_detached(theseusd, &[job::WRAPPER_MODE], &self.spool, &args).unwrap()
    }
}

/// The tasks a cgroup lists now.
fn tasks(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("cgroup.threads"))
        .map(|s| s.split_whitespace().count())
        .unwrap_or(0)
}

/// Survey card 5's gate: a fork loop that ignores SIGTERM, with 40 sleepers,
/// one in its own session and one double-forked, is stopped whole by its
/// cgroup: SIGTERM, the grace, then no new process let in and SIGKILL to
/// each, with none left. The wrapper then removes the job's cgroup.
#[test]
fn a_fork_loop_that_ignores_sigterm_is_stopped_whole_by_its_cgroup() {
    let Some(scope) = Scope::start("stop") else {
        return skip("a_fork_loop_that_ignores_sigterm_is_stopped_whole_by_its_cgroup");
    };
    let jobs = scope.jobs(512);
    let rig = Rig::new();
    let wrapper = rig.start(
        &jobs,
        "act_loop",
        "trap '' TERM; setsid sleep 60 < /dev/null > /dev/null 2>&1 & (sleep 60 &); \
         i=0; while [ $i -lt 40 ]; do sleep 60 & i=$((i+1)); done; : > up; \
         while [ -d \"$1\" ]; do sleep 0.01 & done",
    );
    let job = scope.dir.join("job-act_loop");
    wait_for("the loop", Duration::from_secs(20), || {
        rig.dir.path().join("up").exists().then_some(())
    });
    assert!(tasks(&job) >= 43, "{} tasks", tasks(&job));
    let v = job::terminate(&rig.spool, wrapper, "act_loop", Duration::from_millis(300));
    assert!(v.verified(), "{v:?}");
    assert_eq!(v.verified_by, theseus_kernel::VerifiedBy::Cgroup, "{v:?}");
    assert_eq!(
        (v.survivors, v.scope.as_deref()),
        (Some(0), Some("cgroup")),
        "{v:?}"
    );
    assert!(v.killed.unwrap_or(0) >= 43, "{v:?}");
    assert_eq!(tasks(&job), 0, "nothing is left in the job's cgroup");
    wait_for(
        "the job's cgroup to be removed",
        Duration::from_secs(10),
        || (!job.exists()).then_some(()),
    );
}

/// The cap holds: a job allowed 10 processes and threads that starts 30
/// sleepers is refused the rest, and its completion counts the refusals, with
/// its cap and its CPU time, for its result's words.
#[test]
fn a_job_past_its_cap_is_refused_new_processes_and_its_completion_says_so() {
    let Some(scope) = Scope::start("cap") else {
        return skip("a_job_past_its_cap_is_refused_new_processes_and_its_completion_says_so");
    };
    let jobs = scope.jobs(10);
    let rig = Rig::new();
    rig.start(
        &jobs,
        "act_cap",
        "i=0; while [ $i -lt 30 ]; do sleep 2 & i=$((i+1)); done; exit 0",
    );
    let c = wait_for("the completion", Duration::from_secs(20), || {
        rig.spool.read_completion("act_cap").unwrap()
    });
    let d = c.detail.unwrap_or_default();
    assert!(d["pids_refused"].as_u64().unwrap_or(0) > 0, "{d}");
    assert_eq!(d["pids_max"], 10, "{d}");
    assert!(d["cpu_us"].as_u64().is_some(), "{d}");
    // Its sleepers end within 2 s, and its wrapper then removes its cgroup.
    let job = scope.dir.join("job-act_cap");
    wait_for(
        "the job's cgroup to be removed",
        Duration::from_secs(20),
        || (!job.exists()).then_some(()),
    );
}

/// The 219 case (theseus-gyin's cut, survey card 5): a unit as `theseusd
/// install` writes it (`Delegate=yes`, `KillMode=process`,
/// `Restart=on-failure`) whose daemon is killed while a job lives in its
/// cgroup starts again: the daemon stayed in the unit's cgroup, and only
/// threaded children with the threaded `pids` controller are below it.
#[test]
fn a_unit_restarts_while_a_job_lives_in_its_cgroup() {
    let dir = tempfile::tempdir().unwrap();
    let path = |p: &str| dir.path().join(p);
    let unit = format!("theseus-test-restart-{}.service", std::process::id());
    let sock = path("sock");
    if !daemon_unit(dir.path(), &unit) {
        return skip("a_unit_restarts_while_a_job_lives_in_its_cgroup");
    }
    let stop = Stop(unit.clone());
    let log = || std::fs::read_to_string(path("theseusd.log")).unwrap_or_default();
    // Its cgroup's phase, once the daemon has asked systemd and readied it.
    let phase = || -> Option<serde_json::Value> {
        let h = call(&sock, "health").ok()?;
        let p = h["startup"]
            .as_array()?
            .iter()
            .find(|p| p["name"] == "cgroup")?
            .clone();
        p["end_us"].as_u64().map(|_| p["detail"].clone())
    };
    let d = wait_for("the daemon's cgroup phase", Duration::from_secs(20), &phase);
    assert_eq!(d["state"], "delegated", "{d}\n{}", log());
    let jobs = Jobs {
        dir: PathBuf::from(d["dir"].as_str().unwrap()),
        pids_max: 64,
    };
    // A job in its cgroup, started as the daemon's launcher starts one.
    let rig = Rig::new();
    let wrapper = rig.start(&jobs, "act_alive", "echo $$ > main.pid; exec sleep 60");
    let main: u32 = wait_for("the job", Duration::from_secs(20), || {
        std::fs::read_to_string(rig.dir.path().join("main.pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
    });
    assert_eq!(tasks(&jobs.dir.join("job-act_alive")), 1);
    // The daemon crashes: systemd starts the next one in the unit's cgroup.
    let first: i32 = show(&unit, "MainPID").parse().unwrap();
    // SAFETY: a signal to the unit's main process, this test's daemon.
    unsafe { libc::kill(first, libc::SIGKILL) };
    wait_for("the restart", Duration::from_secs(20), || {
        let again = show(&unit, "MainPID").parse::<i32>().ok()?;
        (show(&unit, "NRestarts") == "1"
            && show(&unit, "SubState") == "running"
            && again != first
            && again != 0)
            .then_some(())
    });
    let d = wait_for(
        "the next daemon's cgroup phase",
        Duration::from_secs(20),
        &phase,
    );
    assert_eq!(d["state"], "delegated", "{d}\n{}", log());
    assert_ne!(show(&unit, "ExecMainStatus"), "219", "{}", log());
    assert!(
        std::fs::metadata(format!("/proc/{main}")).is_ok(),
        "the job lives on"
    );
    let v = job::terminate(&rig.spool, wrapper, "act_alive", Duration::from_millis(300));
    assert!(v.verified(), "{v:?}");
    let job = jobs.dir.join("job-act_alive");
    wait_for(
        "the job's cgroup to be removed",
        Duration::from_secs(20),
        || (!job.exists()).then_some(()),
    );
    drop(stop);
}

/// `theseusd` on `common::safe_note` in `dir`, as a transient user service
/// named `unit` with the lines `theseusd install` writes that matter here:
/// `Delegate=yes`, `KillMode=process`, `Restart=on-failure` (its socket
/// `sock`, its log `theseusd.log`). Whether systemd started it.
fn daemon_unit(dir: &Path, unit: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let path = |p: &str| dir.join(p);
    std::fs::create_dir_all(path("bin")).unwrap();
    std::fs::create_dir_all(path("projects")).unwrap();
    std::fs::write(path("bin/op"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let note = common::safe_note(&theseusd, &path("projects"), 100.0);
    std::fs::write(path("config.toml"), note).unwrap();
    let props = [
        "Delegate=yes",
        "KillMode=process",
        "Restart=on-failure",
        "RestartSec=100ms",
    ];
    Command::new("systemd-run")
        .args(["--user", "-q", "--unit", unit])
        .args(props.iter().map(|p| format!("--property={p}")))
        .arg(format!(
            "--property=StandardError=file:{}",
            path("theseusd.log").display()
        ))
        .arg(format!(
            "--setenv=PATH={}:{}",
            path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        ))
        .arg("--setenv=OP_SERVICE_ACCOUNT_TOKEN=test-not-a-token")
        .arg("--")
        .arg(&theseusd)
        .arg("--config")
        .arg(path("config.toml"))
        .arg("--state-dir")
        .arg(path("state"))
        .arg("--socket")
        .arg(path("sock"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Stops the transient unit, and forgets it, when dropped.
struct Stop(String);

impl Drop for Stop {
    fn drop(&mut self) {
        let dir = unit_dir(&self.0);
        for verb in ["stop", "reset-failed"] {
            let _ = Command::new("systemctl")
                .args(["--user", verb, &self.0])
                .stderr(Stdio::null())
                .status();
        }
        // As a scope's: a cgroup systemd left, now empty.
        if let Some(d) = dir {
            let _ = std::fs::remove_dir(d);
        }
    }
}

/// One JSON-RPC call on a daemon's socket: its result, or its error.
fn call(sock: &Path, method: &str) -> Result<serde_json::Value, String> {
    use std::io::{BufRead, Write};
    let s = std::os::unix::net::UnixStream::connect(sock).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": null});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    for line in std::io::BufReader::new(&s).lines() {
        let v: serde_json::Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if v["id"] == 1 {
            return Ok(v["result"].clone());
        }
    }
    Err("the connection closed".into())
}
