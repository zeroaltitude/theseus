//! The join lock (theseus-pw1q.9): a directory made with `mkdir`, which
//! is atomic, so of two joiners exactly one makes it. Its `owner` file names
//! the holder on its first line and, once released, ends with a `done:`
//! line: the format the outer loop's join locks use, so `[self] join_lock`
//! can name the outer loop's own lock and the two never join at once.
//!
//! A lock whose `owner` file ends with `done:` is free (a holder that died
//! between its `done:` line and the removal), and so is a lock directory
//! left without an `owner` file for longer than [`ORPHAN`] (a holder that
//! died between the `mkdir` and its first line). Any other is held, and a
//! joiner waits for it on tokio's timer.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

/// A lock directory with no `owner` file older than this is an orphan.
pub const ORPHAN: Duration = Duration::from_secs(60);

/// A held join lock. Released by [`Held::release`]; a drop without one
/// leaves it held, as a crash would, for the owner to see.
#[derive(Debug)]
pub struct Held {
    pub dir: PathBuf,
}

/// Why the lock was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    /// Another holds it: its `owner` file's first line.
    Held(String),
}

fn owner(dir: &Path) -> PathBuf {
    dir.join("owner")
}

/// Whether a lock left at `dir` is free to take over.
fn stale(dir: &Path) -> bool {
    match std::fs::read_to_string(owner(dir)) {
        Ok(t) => t
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.starts_with("done:")),
        Err(_) => std::fs::metadata(dir)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > ORPHAN),
    }
}

/// Take the lock now, or say who holds it. `holder` is the owner file's
/// first line.
pub fn try_take(dir: &Path, holder: &str) -> Result<Result<Held, Busy>> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("the join lock's parent {}", parent.display()))?;
    }
    for _ in 0..2 {
        match std::fs::create_dir(dir) {
            Ok(()) => {
                let mut f = std::fs::File::create(owner(dir))?;
                writeln!(f, "{holder}")?;
                writeln!(f, "pid: {}", std::process::id())?;
                writeln!(
                    f,
                    "started: {}",
                    crate::arrangement::utc(theseus_protocol::now_unix_ms())
                )?;
                f.sync_all()?;
                return Ok(Ok(Held { dir: dir.into() }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if !stale(dir) {
                    let first = std::fs::read_to_string(owner(dir))
                        .ok()
                        .and_then(|t| t.lines().next().map(str::to_string))
                        .unwrap_or_else(|| "(no owner yet)".into());
                    return Ok(Err(Busy::Held(first)));
                }
                // A released or orphaned lock: take it over once.
                let _ = std::fs::remove_file(owner(dir));
                let _ = std::fs::remove_dir(dir);
            }
            Err(e) => return Err(e).with_context(|| format!("the join lock {}", dir.display())),
        }
    }
    let first = std::fs::read_to_string(owner(dir)).unwrap_or_default();
    Ok(Err(Busy::Held(
        first.lines().next().unwrap_or("").to_string(),
    )))
}

/// Take the lock, waiting while another holds it: every `every`, at most
/// `wait`, and stopping when `stop` says why (a halt between tries).
pub async fn take(
    dir: &Path,
    holder: &str,
    wait: Duration,
    every: Duration,
    stop: impl Fn() -> Option<String>,
) -> Result<Result<Held, String>> {
    let until = tokio::time::Instant::now() + wait;
    loop {
        let (d, h) = (dir.to_path_buf(), holder.to_string());
        let got = theseus_store::blocking(move || try_take(&d, &h))?;
        let Busy::Held(by) = match got {
            Ok(held) => return Ok(Ok(held)),
            Err(b) => b,
        };
        if let Some(why) = stop() {
            return Ok(Err(why));
        }
        if tokio::time::Instant::now() >= until {
            return Ok(Err(format!(
                "the join lock {} stayed held by {by} for {} minutes",
                dir.display(),
                wait.as_secs() / 60
            )));
        }
        tokio::time::sleep(every).await;
    }
}

impl Held {
    /// Write the `done:` line, then remove the lock.
    pub fn release(self, done: &str) -> Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(owner(&self.dir))?;
        writeln!(
            f,
            "done: {done} at {}",
            crate::arrangement::utc(theseus_protocol::now_unix_ms())
        )?;
        f.sync_all()?;
        drop(f);
        std::fs::remove_file(owner(&self.dir))?;
        std::fs::remove_dir(&self.dir)?;
        Ok(())
    }

    /// The owner file's text, while held (a test's look).
    #[cfg(test)]
    pub fn text(&self) -> String {
        std::fs::read_to_string(owner(&self.dir)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time_and_a_done_line_frees_it() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("locks").join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        assert!(a.text().starts_with("theseus self-join a\npid: "));
        assert_eq!(
            try_take(&dir, "b").unwrap().unwrap_err(),
            Busy::Held("theseus self-join a".into())
        );
        a.release("joined").unwrap();
        assert!(!dir.exists());
        let b = try_take(&dir, "b").unwrap().unwrap();
        // The outer loop's lock, released with its done: line but left in place.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join("owner"))
            .unwrap();
        writeln!(f, "done: merged").unwrap();
        drop(f);
        std::mem::forget(b);
        let c = try_take(&dir, "c").unwrap().unwrap();
        assert!(c.text().starts_with("c\n"));
    }
}
