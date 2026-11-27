//! The join lock (theseus-pw1q.9): a directory made with `mkdir`, which
//! is atomic, so of two joiners exactly one makes it. Its `owner` file names
//! the holder on its first line and, once released, ends with a `done:`
//! line: the format the outer loop's join locks use, so `[self] join_lock`
//! can name the outer loop's own lock. In-process joins and reverts are
//! serialized here; cross-process exclusion during reclaim still requires
//! a shared protocol with the outer loop's scripts.
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

/// Serializes directory creation, stale reclaim, and release in this daemon.
static DOOR: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A lock directory with no `owner` file older than this is an orphan.
pub const ORPHAN: Duration = Duration::from_secs(60);

/// A held join lock. Released by [`Held::release`]; a drop without one
/// leaves it held, as a crash would, for the owner to see. A drop after this
/// daemon's orderly stop has begun (the runtime's end drops a join cut in the
/// middle) keeps it in [`CUT`] instead, and the stop releases it
/// ([`release_cut_by_stop`], issue 63).
#[derive(Debug)]
pub struct Held {
    pub dir: PathBuf,
    holder: String,
    pid: u32,
}

/// The join locks this daemon took and still holds: each one's directory,
/// holder and pid, from its take to its release or a drop before any stop.
static CUT: std::sync::Mutex<Vec<(PathBuf, String, u32)>> = std::sync::Mutex::new(Vec::new());

fn held_list() -> std::sync::MutexGuard<'static, Vec<(PathBuf, String, u32)>> {
    CUT.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Held {
    fn key(&self) -> (PathBuf, String, u32) {
        (self.dir.clone(), self.holder.clone(), self.pid)
    }

    fn forget(&self) {
        let key = self.key();
        held_list().retain(|k| *k != key);
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // Released, or dropped outside a stop: the list forgets it, and a lock
        // still on disk stays held, as a crash leaves it.
        if !crate::startup::stop_has_begun() {
            self.forget();
        }
    }
}

/// At an orderly stop or restart, after the runtime has ended (issue 63): each
/// join lock this daemon took and still holds is released by the lock's own
/// release, its holder and pid checked, with a `done:` line saying a stop cut
/// it, so the next start's join can take it. A lock another holder now owns is
/// left untouched. How many were released.
pub fn release_cut_by_stop() -> usize {
    let cut: Vec<_> = std::mem::take(&mut *held_list());
    let mut released = 0;
    for (dir, holder, pid) in cut {
        let at = dir.display().to_string();
        match (Held { dir, holder, pid }).release("cut by the daemon's stop") {
            Ok(()) => released += 1,
            Err(e) => tracing::warn!(lock = %at, error = %format!("{e:#}"),
                "stop: a join lock a stop cut could not be released; it stays held"),
        }
    }
    released
}

/// How long the stop waits for its release of the join locks it cut.
pub const STOP_RELEASE_BOUND: Duration = Duration::from_secs(5);

/// What the stop's release of the join locks did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StopRelease {
    /// Locks the stop cut, released.
    pub released: usize,
    /// Locks whose `Held` was still alive (a join still running): left held.
    pub left_held: usize,
    /// Locks the stop cut whose release failed on the disk: left held.
    pub failed: usize,
    /// The kind of the first such failure.
    pub kind: Option<std::io::ErrorKind>,
    /// Locks another holder now owns: left untouched (the release says so).
    pub another_holder: usize,
}

/// The stop's release of the join locks, as [`release_cut_by_stop`] does it, on the calling thread.
pub fn release_cut_by_stop_within(_bound: Duration) -> Option<StopRelease> {
    let released = release_cut_by_stop();
    if released > 0 {
        tracing::info!(released, "stop: the join lock a stop cut is released");
    }
    Some(StopRelease {
        released,
        ..StopRelease::default()
    })
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
    let _door = DOOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                let held = Held {
                    dir: dir.into(),
                    holder: holder.into(),
                    pid: std::process::id(),
                };
                held_list().push(held.key());
                return Ok(Ok(held));
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
        self.release_with(done, || {})
    }

    fn release_with(self, done: &str, after_done: impl FnOnce()) -> Result<()> {
        self.forget();
        let _door = DOOR
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = std::fs::read_to_string(owner(&self.dir))?;
        let pid = text.lines().find_map(|line| line.strip_prefix("pid: "));
        if text.lines().next() != Some(self.holder.as_str())
            || pid != Some(self.pid.to_string().as_str())
        {
            tracing::warn!("self-join: the lock belongs to another holder; leaving it untouched");
            anyhow::bail!("the join lock belongs to another holder");
        }
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
        after_done();
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

#[cfg(test)]
mod release_tests {
    use super::*;

    #[test]
    fn a_release_keeps_the_door_until_its_directory_is_removed() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "a").unwrap().unwrap();
        let paused = std::sync::Barrier::new(2);
        let resume = std::sync::Barrier::new(2);
        std::thread::scope(|s| {
            let thread = s.spawn(|| {
                a.release_with("joined", || {
                    paused.wait();
                    resume.wait();
                })
            });
            paused.wait();
            let held = DOOR.try_lock().is_err();
            resume.wait();
            thread.join().unwrap().unwrap();
            assert!(
                held,
                "reclaim must wait until release finishes removing the old lock"
            );
            let b = try_take(&dir, "b").unwrap().unwrap();
            assert_eq!(
                try_take(&dir, "c").unwrap().unwrap_err(),
                Busy::Held("b".into())
            );
            b.release("joined").unwrap();
        });
    }

    /// KEY (issue 63): a join cut by an orderly stop drops its lock unreleased; the stop releases it by the
    /// lock's own release, with a `done:` line, and the next take gets it.
    #[test]
    fn a_lock_a_stop_cut_is_released_by_the_stop() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        crate::startup::stop_began();
        drop(a);
        assert!(dir.exists(), "the drop itself releases nothing");
        assert_eq!(release_cut_by_stop(), 1);
        assert!(!dir.exists(), "released");
        let b = try_take(&dir, "b").unwrap().unwrap();
        b.release("joined").unwrap();
        assert_eq!(release_cut_by_stop(), 0, "nothing is released twice");
    }

    /// A lock another holder owns by the time the stop comes is left exactly as it was.
    #[test]
    fn a_stop_leaves_a_lock_another_holder_owns() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        let theirs = "the outer loop's join\npid: 1\nstarted: then\n";
        std::fs::write(owner(&dir), theirs).unwrap();
        crate::startup::stop_began();
        drop(a);
        assert_eq!(release_cut_by_stop(), 0);
        assert_eq!(std::fs::read_to_string(owner(&dir)).unwrap(), theirs);
    }

    /// A drop with no stop begun keeps the crash's rule: the lock stays held, and no later stop releases it.
    #[test]
    fn a_drop_outside_a_stop_leaves_the_lock_held() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        drop(a);
        crate::startup::stop_began();
        assert_eq!(release_cut_by_stop(), 0);
        assert!(dir.exists(), "still held, for the owner to see");
        assert_eq!(
            try_take(&dir, "b").unwrap().unwrap_err(),
            Busy::Held("theseus self-join a".into())
        );
    }

    #[test]
    fn a_release_does_not_remove_a_different_holder() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "a").unwrap().unwrap();
        let replacement = format!("b\npid: {}\n", std::process::id());
        std::fs::write(owner(&dir), &replacement).unwrap();
        assert!(a.release("joined").is_err());
        assert_eq!(std::fs::read_to_string(owner(&dir)).unwrap(), replacement);
    }
}

#[cfg(test)]
mod stop_release_tests {
    use super::*;
    use crate::test_support::Log;
    use std::time::Instant;

    /// KEY (issue 63): the stop's release is bounded. Another thread holds the lock module's own mutex past the
    /// bound; the release returns to the stop within it, says so in one warning, and the lock stays on disk.
    #[test]
    fn a_release_held_past_its_bound_returns_to_the_stop() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        crate::startup::stop_began();
        drop(a);
        let held = std::sync::Barrier::new(2);
        let log = Log::default();
        std::thread::scope(|s| {
            s.spawn(|| {
                let _door = DOOR.lock().unwrap();
                held.wait();
                std::thread::sleep(Duration::from_secs(3));
            });
            held.wait();
            let t0 = Instant::now();
            let r = log.capture(|| release_cut_by_stop_within(Duration::from_millis(300)));
            let took = t0.elapsed();
            assert!(took < Duration::from_secs(2), "the stop waited {took:?}");
            assert_eq!(r, None);
            assert!(dir.exists(), "the lock stays on disk");
        });
        let text = log.text();
        assert!(
            text.contains("the join locks' release did not end within its bound"),
            "{text}"
        );
        assert!(
            !text.contains(&*d.path().to_string_lossy()),
            "a path: {text}"
        );
    }

    /// A join still running (its `Held` alive) when the stop's release runs keeps its lock, as a crash leaves it,
    /// and the warning counts it.
    #[test]
    fn a_lock_whose_held_is_alive_is_left_held_and_counted() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        crate::startup::stop_began();
        let log = Log::default();
        let r = log
            .capture(|| release_cut_by_stop_within(STOP_RELEASE_BOUND))
            .expect("the release ended");
        assert!(dir.exists(), "a live join's lock stays held");
        assert_eq!((r.released, r.left_held), (0, 1), "{r:?}");
        let text = log.text();
        assert!(
            text.contains("whose join had not ended stays held"),
            "{text}"
        );
        assert!(text.contains("left=1"), "{text}");
        assert!(
            !text.contains(&*d.path().to_string_lossy()),
            "a path: {text}"
        );
        drop(a);
    }

    /// A release that fails on the disk says so in fixed words: the error's kind, never its text and never the
    /// lock's directory.
    #[test]
    fn a_failed_release_names_no_path_and_no_error_text() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        crate::startup::stop_began();
        drop(a);
        std::fs::remove_file(owner(&dir)).unwrap();
        let log = Log::default();
        let r = log.capture(|| release_cut_by_stop_within(STOP_RELEASE_BOUND));
        let text = log.text();
        assert!(
            !text.contains(&*d.path().to_string_lossy()),
            "a path: {text}"
        );
        assert!(!text.contains("No such file"), "an error's text: {text}");
        assert!(
            text.contains("could not be released; it stays held"),
            "{text}"
        );
        assert!(text.contains("NotFound"), "{text}");
        let r = r.expect("the release ended");
        assert_eq!((r.released, r.failed), (0, 1), "{r:?}");
    }

    /// A lock another holder now owns is left untouched, and the stop adds no second warning for it.
    #[test]
    fn a_lock_another_holder_owns_gets_no_second_warning() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("join.lock");
        let a = try_take(&dir, "theseus self-join a").unwrap().unwrap();
        let theirs = "the outer loop's join\npid: 1\nstarted: then\n";
        std::fs::write(owner(&dir), theirs).unwrap();
        crate::startup::stop_began();
        drop(a);
        let log = Log::default();
        let r = log.capture(|| release_cut_by_stop_within(STOP_RELEASE_BOUND));
        let text = log.text();
        assert!(
            !text.contains("could not be released"),
            "a second warning: {text}"
        );
        assert!(
            !text.contains(&*d.path().to_string_lossy()),
            "a path: {text}"
        );
        assert_eq!(std::fs::read_to_string(owner(&dir)).unwrap(), theirs);
        let r = r.expect("the release ended");
        assert_eq!((r.released, r.another_holder), (0, 1), "{r:?}");
    }
}
