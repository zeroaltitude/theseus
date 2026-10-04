//! The exam over the real recall pipeline (row 55, step 34b's wire-in): one
//! scratch daemon per arm, each on its own copy of the exam's store, as one
//! command (`theseus-exam run`).
//!
//! 1. **The oracle's notes** are read from the exam's store before any daemon
//!    serves a copy of it (`render.rs`).
//! 2. **Prepare.** For each daemon (`none`, `bm25`, `baseline`; `oracle`'s
//!    cells go to `none`'s): the store copied to
//!    `<work>/prepared/<arm>/state/store`, its config written from the base
//!    config (`daemon::config_for`), the daemon started and waited for until
//!    it serves and its tender holds the whole store (and has embedded it,
//!    when it has the model), then stopped. Its state, index included, is
//!    the arm's snapshot, marked `READY`; a resumed run reuses it.
//! 3. **Each run** starts fresh daemons from the snapshots, in
//!    `<work>/run-<n>/<arm>`, runs that run's cells (`drive::run`: pairing,
//!    the spend cap, resume), and stops them. A daemon indexes the turns it
//!    serves, so a daemon that lived across runs would recall an item's
//!    earlier answer in its next run; a fresh one per run never does. Within
//!    a run, a cell may still recall another item's cell of that run.
//! 4. **Nothing is left running**: each daemon is stopped as the run ends,
//!    on an error too (a dropped `Daemon` stops), and at the end no process
//!    names the work directory.
//!
//! A directory this command finds half made (a run that was stopped) is
//! moved aside as `<name>.bad-<ms>`, never deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};

use crate::daemon::{self, Daemon};
use crate::drive::{self, Arm, Plan, Summary};
use crate::fixture::Manifest;
use crate::item::{Exam, Item};

pub struct ArmsPlan {
    pub theseusd: PathBuf,
    pub base_config: toml::Table,
    /// The exam's store, as `write-store` wrote it.
    pub store: PathBuf,
    /// A directory of the command's own: snapshots, runs, logs.
    pub work: PathBuf,
    pub arms: Vec<Arm>,
    pub runs: u32,
    /// Item ids; empty: every item.
    pub items: Vec<String>,
    pub limit_usd: f64,
    pub workers: usize,
    pub profile: String,
    pub seed: u64,
    pub timeout: Duration,
    pub out: PathBuf,
    /// How long a tender may take to hold the store.
    pub settle: Duration,
    /// Each daemon's environment, changed: a value set, or, with `None`,
    /// taken out (tests: a fake `op` on `PATH`).
    pub env: Vec<(OsString, Option<OsString>)>,
}

/// What the command did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArmsSummary {
    pub daemons: Vec<String>,
    pub runs: Vec<Summary>,
    pub spent_usd: f64,
    pub stopped_at_cap: bool,
    /// Processes a stop had to kill (none, when every stop was clean).
    pub killed: Vec<String>,
}

/// Copy `src` into `dst` (made), files and directories; sockets and other
/// special files are left out.
pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("creating {}", dst.display()))?;
    for e in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let e = e?;
        let t = e.file_type()?;
        let to = dst.join(e.file_name());
        if t.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else if t.is_file() {
            std::fs::copy(e.path(), &to)
                .with_context(|| format!("copying {}", e.path().display()))?;
        }
    }
    Ok(())
}

/// Move `p` aside as `<p>.bad-<ms>`, if it is there.
fn aside(p: &Path) -> Result<()> {
    if p.exists() {
        let mut name = p.as_os_str().to_owned();
        name.push(format!(".bad-{}", theseus_protocol::now_unix_ms()));
        std::fs::rename(p, &name).with_context(|| format!("moving {} aside", p.display()))?;
    }
    Ok(())
}

/// The daemons `arms` need, by their `[memory] arm`, in order.
pub fn daemons_for(arms: &[Arm]) -> Vec<&'static str> {
    let set: BTreeSet<&str> = arms.iter().map(|a| a.daemon()).collect();
    ["none", "bm25", "baseline"]
        .into_iter()
        .filter(|d| set.contains(d))
        .collect()
}

/// Start the daemon of `arm` in `dir` (whose `state` holds its store), and
/// wait until it serves and its tender holds the store.
fn bring_up(
    plan: &ArmsPlan,
    dir: &Path,
    arm: &str,
    m: &Manifest,
    log: &mut dyn FnMut(&str),
) -> Result<Daemon> {
    let mut d = daemon::start(
        &plan.theseusd,
        dir,
        arm,
        &daemon::config_for(&plan.base_config, arm),
        &plan.env,
    )?;
    d.wait_serving()?;
    if let Some(s) = d.wait_indexed(m.last_position, plan.settle, log)? {
        log(&format!(
            "{arm}: the tender holds the store through @{} ({} mode, {} chunks, {} vectors)",
            s["position"], s["mode"], s["vectors"]["chunks"], s["vectors"]["vectors"]
        ));
    }
    Ok(d)
}

/// Stop every daemon in `ds`, keeping what had to be killed.
fn stop_all(ds: &mut Vec<Daemon>, killed: &mut Vec<String>) -> Result<()> {
    for mut d in ds.drain(..) {
        killed.extend(d.stop()?);
    }
    Ok(())
}

/// Each daemon's snapshot: its state once its tender holds the store.
fn prepare(
    plan: &ArmsPlan,
    daemons: &[&str],
    m: &Manifest,
    killed: &mut Vec<String>,
    log: &mut dyn FnMut(&str),
) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    let mut up = Vec::new();
    for d in daemons {
        let dir = plan.work.join("prepared").join(d);
        out.insert(d.to_string(), dir.join("state"));
        if dir.join("READY").is_file() {
            log(&format!("{d}: its snapshot is ready ({})", dir.display()));
            continue;
        }
        aside(&dir)?;
        copy_dir(&plan.store, &dir.join("state").join("store"))?;
        log(&format!("{d}: preparing its snapshot in {}", dir.display()));
        up.push((d.to_string(), dir));
    }
    // Every daemon starts before any is waited for, so their tenders read
    // (and embed) the store at once.
    let mut started = Vec::new();
    for (d, dir) in &up {
        started.push(daemon::start(
            &plan.theseusd,
            dir,
            d,
            &daemon::config_for(&plan.base_config, d),
            &plan.env,
        )?);
    }
    for d in &mut started {
        d.wait_serving()?;
        d.wait_indexed(m.last_position, plan.settle, log)?;
    }
    stop_all(&mut started, killed)?;
    for (d, dir) in &up {
        std::fs::write(dir.join("READY"), format!("{d}\n"))?;
    }
    Ok(out)
}

/// The command: prepare, then each run on fresh daemons; see the module's
/// note.
pub fn run(
    plan: &ArmsPlan,
    exam: &Exam,
    m: &Manifest,
    log: &mut dyn FnMut(&str),
) -> Result<ArmsSummary> {
    ensure!(
        m.digest == exam.digest,
        "the store was written from {} ({}), not this exam ({}): write it again",
        m.exam,
        m.digest,
        exam.digest
    );
    ensure!(!plan.arms.is_empty(), "no arms");
    let items: Vec<&Item> = if plan.items.is_empty() {
        exam.file.items.iter().collect()
    } else {
        plan.items
            .iter()
            .map(|id| exam.item(id).with_context(|| format!("no item {id}")))
            .collect::<Result<_>>()?
    };
    let notes = drive::oracle_notes(&plan.store, m, &items)?;
    let daemons = daemons_for(&plan.arms);
    std::fs::create_dir_all(&plan.work)?;
    let mut killed = Vec::new();
    let snapshots = prepare(plan, &daemons, m, &mut killed, log)?;
    let mut summary = ArmsSummary {
        daemons: daemons.iter().map(|d| d.to_string()).collect(),
        runs: Vec::new(),
        spent_usd: 0.0,
        stopped_at_cap: false,
        killed: Vec::new(),
    };
    for run in 1..=plan.runs {
        let mut dp = Plan {
            sockets: BTreeMap::new(),
            notes: notes.clone(),
            profile: plan.profile.clone(),
            arms: plan.arms.clone(),
            runs: plan.runs,
            items: items.iter().map(|i| i.id.clone()).collect(),
            limit_usd: plan.limit_usd,
            workers: plan.workers,
            seed: plan.seed,
            timeout: plan.timeout,
            out: plan.out.clone(),
            only_run: Some(run),
        };
        if left_in_run(&dp, exam)? == 0 {
            log(&format!("run {run}: every cell has its verdict"));
            continue;
        }
        let mut up = Vec::new();
        let rdir = plan.work.join(format!("run-{run}"));
        aside(&rdir)?;
        for d in &daemons {
            let dir = rdir.join(d);
            copy_dir(&snapshots[*d], &dir.join("state"))?;
            let daemon = bring_up(plan, &dir, d, m, log)?;
            dp.sockets.insert(d.to_string(), daemon.socket.clone());
            up.push(daemon);
        }
        log(&format!("run {run}: {}", drive::preflight(&dp, exam, m)?));
        let s = drive::run(&dp, exam, m);
        stop_all(&mut up, &mut killed)?;
        let s = s?;
        summary.spent_usd = s.spent_usd;
        summary.stopped_at_cap |= s.stopped_at_cap;
        let cap = s.stopped_at_cap;
        summary.runs.push(s);
        if cap {
            log(&format!("run {run}: stopped at the spend cap"));
            break;
        }
    }
    let left = daemon::processes_naming(&plan.work);
    if !left.is_empty() {
        bail!("processes still name {}: {left:?}", plan.work.display());
    }
    summary.killed = killed;
    Ok(summary)
}

/// The cells of `plan`'s run with no verdict in its output file yet.
fn left_in_run(plan: &Plan, exam: &Exam) -> Result<usize> {
    let items: Vec<&Item> = plan.items.iter().filter_map(|id| exam.item(id)).collect();
    let done: BTreeSet<(String, String, u32)> = drive::read_records(&plan.out)?
        .into_iter()
        .filter(|r| r.pass.is_some() && r.digest == exam.digest)
        .map(|r| (r.item, r.arm, r.run))
        .collect();
    Ok(drive::order(&items, &plan.arms, plan.runs, plan.seed)
        .into_iter()
        .filter(|(run, id, arm)| {
            Some(*run) == plan.only_run
                && !done.contains(&(id.clone(), arm.as_str().to_string(), *run))
        })
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_shares_nones_daemon_and_each_other_arm_has_its_own() {
        assert_eq!(daemons_for(&Arm::ALL), ["none", "bm25", "baseline"]);
        assert_eq!(daemons_for(&[Arm::Oracle]), ["none"]);
        assert_eq!(
            daemons_for(&[Arm::Baseline, Arm::Bm25]),
            ["bm25", "baseline"]
        );
    }

    #[test]
    fn a_copy_leaves_out_sockets_and_a_half_made_directory_is_moved_aside() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(src.join("index")).unwrap();
        std::fs::write(src.join("wal"), b"frames").unwrap();
        std::fs::write(src.join("index/cursor"), b"7").unwrap();
        let _l = std::os::unix::net::UnixListener::bind(src.join("index/sock")).unwrap();
        let dst = d.path().join("dst");
        copy_dir(&src, &dst).unwrap();
        assert_eq!(std::fs::read(dst.join("wal")).unwrap(), b"frames");
        assert_eq!(std::fs::read(dst.join("index/cursor")).unwrap(), b"7");
        assert!(!dst.join("index/sock").exists());
        aside(&dst).unwrap();
        assert!(!dst.exists());
        let moved: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("dst.bad-"))
            .collect();
        assert_eq!(moved.len(), 1);
    }
}
