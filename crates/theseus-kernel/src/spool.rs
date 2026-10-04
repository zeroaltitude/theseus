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
//! - `stops/<correlation_id>`       a cancel's verdict, from the wrapper that stopped the job
//!   (M4 18a): a cancelled job writes it, and no completion
//! - `malformed/`                   files that did not parse, moved aside and surfaced

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::types::{Completion, CorrelationId, Verdict};

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
        fs::create_dir_all(dir.join("stops"))?;
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
        self.drain_with(|p| fs::read(p))
    }

    /// `drain`, its reads through `read`: the seam a test uses to take a
    /// file between the listing and the read, as a turn's `job_settled` can.
    fn drain_with(
        &self,
        mut read: impl FnMut(&Path) -> std::io::Result<Vec<u8>>,
    ) -> Result<Drained> {
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
            let parsed = match read(&p) {
                // The other consumer, a turn's `job_settled`, accepted and
                // removed it since the listing: it is taken, not malformed
                // (theseus-46ya).
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => Err(anyhow::Error::from(e).context("read spool file")),
                Ok(b) => serde_json::from_slice::<Completion>(&b).context("parse"),
            };
            match parsed {
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

    /// A job's spooled completion, or `None` when there is none. Two
    /// consumers take completions, the daemon's drain and a turn's
    /// `job_settled`, each accepting one before it removes the file, so a
    /// file gone at the read was taken, its completion already accepted: it
    /// reads as `None`, never as an error (theseus-46ya). There is no
    /// `exists()` first, which would leave the window between the check and
    /// the read in which the other consumer removes it.
    pub fn read_completion(&self, id: &str) -> Result<Option<Completion>> {
        self.read_completion_with(id, |p| fs::read(p))
    }

    /// `read_completion`, its read through `read`: the seam a test uses to
    /// take the file just before the read.
    fn read_completion_with(
        &self,
        id: &str,
        read: impl FnOnce(&Path) -> std::io::Result<Vec<u8>>,
    ) -> Result<Option<Completion>> {
        match read(&self.completion_path(id)) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
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

    fn stop_path(&self, id: &str) -> PathBuf {
        self.dir.join("stops").join(id)
    }

    /// A cancel's verdict, from the wrapper that stopped job `id` (M4 18a):
    /// tmp+rename, so a reader sees all of it or nothing.
    pub fn write_stop(&self, id: &str, v: &Verdict) -> Result<()> {
        fs::create_dir_all(self.dir.join("stops"))?;
        let tmp = self.dir.join("stops").join(format!("{id}.tmp"));
        fs::write(&tmp, serde_json::to_vec(v)?)?;
        fs::rename(&tmp, self.stop_path(id))?;
        Ok(())
    }

    /// The verdict a wrapper wrote for job `id`, if it wrote one.
    pub fn read_stop(&self, id: &str) -> Option<Verdict> {
        serde_json::from_slice(&fs::read(self.stop_path(id)).ok()?).ok()
    }

    /// Remove job `id`'s verdict once its cancel has read it.
    pub fn remove_stop(&self, id: &str) {
        let _ = fs::remove_file(self.stop_path(id));
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

    fn completion(id: &str) -> Completion {
        Completion {
            correlation_id: id.into(),
            outcome: Outcome::Succeeded,
            result_ref: None,
            external_op_id: None,
            started_at_ms: 1,
            finished_at_ms: 2,
            producer: "test".into(),
            signature: None,
            cost_micros: None,
            detail: None,
        }
    }

    /// A completion that is not there, or no longer, reads as `None`.
    #[test]
    fn a_completion_that_is_gone_reads_as_none() {
        let d = tempfile::tempdir().unwrap();
        let sp = Spool::open(d.path()).unwrap();
        assert!(sp.read_completion("act_never").unwrap().is_none());
        let p = sp.write(&completion("act_gone")).unwrap();
        assert!(sp.read_completion("act_gone").unwrap().is_some());
        sp.remove(&p).unwrap();
        assert!(sp.read_completion("act_gone").unwrap().is_none());
    }

    /// The race (theseus-46ya): the drain accepts and removes a completion
    /// between a turn's look and its read. The seam removes the file at the
    /// read itself, after any look, as the drain did: it reads as `None`,
    /// the other consumer having taken it, never as `No such file`.
    #[test]
    fn a_completion_the_drain_takes_at_the_read_reads_as_none() {
        let d = tempfile::tempdir().unwrap();
        let sp = Spool::open(d.path()).unwrap();
        let p = sp.write(&completion("act_taken")).unwrap();
        let got = sp
            .read_completion_with("act_taken", |path| {
                sp.remove(&p).unwrap();
                fs::read(path)
            })
            .unwrap();
        assert!(got.is_none());
        // Another error still is one.
        let err = sp.read_completion_with("act_taken", |_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert!(err.is_err());
    }

    /// The mirror race: a turn takes a completion between the drain's
    /// listing and its read. It is skipped, not counted malformed, and
    /// nothing moves to `malformed/`; the others drain as before.
    #[test]
    fn a_completion_a_turn_takes_during_the_drain_is_skipped() {
        let d = tempfile::tempdir().unwrap();
        let sp = Spool::open(d.path()).unwrap();
        let taken = sp.write(&completion("act_taken")).unwrap();
        sp.write(&completion("act_kept")).unwrap();
        let dr = sp
            .drain_with(|path| {
                if path == taken {
                    sp.remove(&taken).unwrap();
                }
                fs::read(path)
            })
            .unwrap();
        assert_eq!(dr.malformed, 0);
        let ids: Vec<_> = dr
            .completions
            .iter()
            .map(|(_, c)| c.correlation_id.as_str())
            .collect();
        assert_eq!(ids, ["act_kept"]);
        assert_eq!(fs::read_dir(d.path().join("malformed")).unwrap().count(), 0);
    }
}
