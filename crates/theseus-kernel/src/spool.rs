//! The completion spool (§3.16, §6): a directory on the same disk as the WAL
//! where job wrappers write results before any delivery attempt. Startup
//! drains it before accepting events; the reconciler reads it every heartbeat.
//!
//! Layout under the spool dir:
//! - `<correlation_id>.json`        a `Completion`, written tmp+rename
//! - `<correlation_id>.json.tmp`    in flight; never read
//! - `pids/<correlation_id>`        the wrapper's pid while it runs
//! - `lingering/<correlation_id>`   the wrapper's pid while it waits, its command done,
//!   for descendants that outlived the command (theseus-6qy)
//! - `results/<correlation_id>.out` captured output the completion's `result_ref` points at
//! - `malformed/`                   files that did not parse, moved aside and surfaced

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::types::{Completion, CorrelationId};

#[derive(Debug, Clone)]
pub struct Spool {
    dir: PathBuf,
}

#[derive(Debug, Default)]
pub struct Drained {
    pub completions: Vec<(PathBuf, Completion)>,
    pub malformed: u64,
}

impl Spool {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        fs::create_dir_all(dir.join("pids"))?;
        fs::create_dir_all(dir.join("lingering"))?;
        fs::create_dir_all(dir.join("results"))?;
        fs::create_dir_all(dir.join("malformed"))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn completion_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
    pub fn pid_path(&self, id: &str) -> PathBuf {
        self.dir.join("pids").join(id)
    }
    pub fn result_path(&self, id: &str) -> PathBuf {
        self.dir.join("results").join(format!("{id}.out"))
    }

    /// Durably write a completion: tmp file, fsync, rename, fsync dir.
    pub fn write(&self, c: &Completion) -> Result<PathBuf> {
        let final_path = self.completion_path(&c.correlation_id);
        let tmp = self.dir.join(format!("{}.json.tmp", c.correlation_id));
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(&serde_json::to_vec(c)?)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &final_path)?;
        if let Ok(d) = File::open(&self.dir) {
            let _ = d.sync_all();
        }
        Ok(final_path)
    }

    /// Every completion currently spooled, oldest first by file mtime.
    /// Unparseable files move to `malformed/` and are counted.
    pub fn drain(&self) -> Result<Drained> {
        let mut out = Drained::default();
        let mut entries: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") && p.is_file() {
                let mt = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                entries.push((mt, p));
            }
        }
        entries.sort();
        for (_, p) in entries {
            match fs::read(&p)
                .context("read spool file")
                .and_then(|b| serde_json::from_slice::<Completion>(&b).context("parse"))
            {
                Ok(c) => out.completions.push((p, c)),
                Err(_) => {
                    let name = p.file_name().unwrap().to_owned();
                    let _ = fs::rename(&p, self.dir.join("malformed").join(name));
                    out.malformed += 1;
                }
            }
        }
        Ok(out)
    }

    /// Remove a settled completion file (after its frame committed).
    pub fn remove(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Remove a job's raw output once its result is absorbed (theseus-wz2).
    /// Only a file in this spool's `results/` is touched, and one already
    /// gone is fine. Whether a file went.
    pub fn remove_result(&self, path: &Path) -> Result<bool> {
        if path.parent() != Some(self.dir.join("results").as_path()) {
            return Ok(false);
        }
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub fn has_completion(&self, id: &str) -> bool {
        self.completion_path(id).exists()
    }

    pub fn read_completion(&self, id: &str) -> Result<Option<Completion>> {
        let p = self.completion_path(id);
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&fs::read(p)?)?))
    }

    pub fn write_pid(&self, id: &str, pid: u32) -> Result<()> {
        fs::write(self.pid_path(id), pid.to_string())?;
        Ok(())
    }
    pub fn read_pid(&self, id: &str) -> Option<u32> {
        fs::read_to_string(self.pid_path(id))
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    /// The jobs with a pid file, as (job, wrapper pid): the wrappers whose
    /// command still runs, since the file is removed at the wrapper's report
    /// (theseus-ht82). One directory read, so the heartbeat can ask it every
    /// beat; no process is looked at, and a file a killed wrapper left is
    /// listed until its job settles.
    pub fn running(&self) -> Vec<(String, u32)> {
        let Ok(dir) = fs::read_dir(self.dir.join("pids")) else {
            return vec![];
        };
        let mut out: Vec<(String, u32)> = dir
            .flatten()
            .filter_map(|e| {
                let pid = fs::read_to_string(e.path()).ok()?.trim().parse().ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), pid))
            })
            .collect();
        out.sort();
        out
    }
    pub fn remove_pid(&self, id: &CorrelationId) {
        let _ = fs::remove_file(self.pid_path(id));
    }

    fn lingering_path(&self, id: &str) -> PathBuf {
        self.dir.join("lingering").join(id)
    }

    /// A wrapper whose command has exited waits for its descendants
    /// (theseus-6qy).
    pub fn write_lingering(&self, id: &str, pid: u32) -> Result<()> {
        fs::create_dir_all(self.dir.join("lingering"))?;
        fs::write(self.lingering_path(id), pid.to_string())?;
        Ok(())
    }

    pub fn remove_lingering(&self, id: &str) {
        let _ = fs::remove_file(self.lingering_path(id));
    }

    /// The pid a job's `lingering` marker names, if it has one.
    pub fn read_lingering(&self, id: &str) -> Option<u32> {
        fs::read_to_string(self.lingering_path(id))
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    /// Whether job `id`'s wrapper still lives (theseus-5wgd): the pid file's
    /// while its command runs, or the lingering marker's once the command has
    /// exited and a process it started holds the output open. The wrapper
    /// removes its pid file at its report, so a reader that stops there reads
    /// a lingering wrapper as gone, and takes the file it is still writing.
    /// Nothing is removed: a marker a killed wrapper left is `lingering()`'s.
    pub fn wrapper_lives(&self, id: &str) -> bool {
        [self.read_pid(id), self.read_lingering(id)]
            .into_iter()
            .flatten()
            .any(|pid| crate::job::wrapper_alive(pid, id))
    }

    /// The wrappers lingering now, as (job, wrapper pid): each marker whose
    /// pid is still that job's wrapper. A marker a killed wrapper left
    /// behind is removed.
    pub fn lingering(&self) -> Vec<(String, u32)> {
        let Ok(dir) = fs::read_dir(self.dir.join("lingering")) else {
            return vec![];
        };
        let mut out = Vec::new();
        for e in dir.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            let pid = fs::read_to_string(e.path())
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
            match pid {
                Some(pid) if crate::job::wrapper_job(pid).as_deref() == Some(id.as_str()) => {
                    out.push((id, pid));
                }
                _ => {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Outcome;

    #[test]
    fn write_drain_remove_and_malformed() {
        let d = tempfile::tempdir().unwrap();
        let sp = Spool::open(d.path()).unwrap();
        let c = Completion {
            correlation_id: "act_1".into(),
            outcome: Outcome::Succeeded,
            result_ref: None,
            external_op_id: None,
            started_at_ms: 1,
            finished_at_ms: 2,
            producer: "test".into(),
            signature: None,
            cost_micros: None,
            detail: None,
        };
        sp.write(&c).unwrap();
        fs::write(d.path().join("junk.json"), b"{not json").unwrap();
        fs::write(d.path().join("act_2.json.tmp"), b"half").unwrap();
        let dr = sp.drain().unwrap();
        assert_eq!(dr.completions.len(), 1);
        assert_eq!(dr.malformed, 1);
        assert!(d.path().join("malformed/junk.json").exists());
        sp.remove(&dr.completions[0].0).unwrap();
        assert!(sp.drain().unwrap().completions.is_empty());
        sp.remove(Path::new("/nonexistent/x.json")).unwrap();
    }
}
