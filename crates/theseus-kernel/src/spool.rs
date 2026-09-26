//! The completion spool (§3.16, §6): a directory on the same disk as the WAL
//! where job wrappers write results before any delivery attempt. Startup
//! drains it before accepting events; the reconciler reads it every heartbeat.
//!
//! Layout under the spool dir:
//! - `<correlation_id>.json`        a `Completion`, written tmp+rename
//! - `<correlation_id>.json.tmp`    in flight; never read
//! - `pids/<correlation_id>`        the wrapper's pid while it runs
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
    pub fn remove_pid(&self, id: &CorrelationId) {
        let _ = fs::remove_file(self.pid_path(id));
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
            usage_units: None,
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
