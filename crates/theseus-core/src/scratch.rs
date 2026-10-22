//! The directories the core's tests make under the temp dir (theseus-5hr6i).
//! Nextest runs each test in a process of its own, and a test hands its core
//! to spawned tasks that never end, so no `Drop` is certain to run: the
//! directories are removed when the process exits, and a process that is
//! killed leaves them for the next one to prune once they are old.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, PoisonError};
use std::time::{Duration, SystemTime};

/// What every test directory's name begins with, so one prune finds them.
pub(crate) const PREFIX: &str = "theseus-test-";
/// A test is killed at two minutes, so a directory this old belongs to no test.
const OLD: Duration = Duration::from_secs(6 * 3600);

static MADE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A new test directory's path under the temp dir, to be made by the test and
/// removed at the process's exit. `tag` names what it is for.
pub(crate) fn dir(tag: &str) -> PathBuf {
    make(&std::env::temp_dir(), tag)
}

/// A new test file's path, as `dir` makes a directory's.
pub(crate) fn file(tag: &str, ext: &str) -> PathBuf {
    let path = named(&std::env::temp_dir(), tag).with_extension(ext);
    keep_track(path.clone());
    path
}

/// A `tempfile::tempdir()` for a test whose fixture may be leaked: it drops
/// as any does, and is removed at exit when it never does.
pub(crate) fn tempdir() -> tempfile::TempDir {
    let dir = tempfile::Builder::new().prefix(PREFIX).tempdir().unwrap();
    keep_track(dir.path().to_path_buf());
    dir
}

/// `dir` under `base`.
fn make(base: &Path, tag: &str) -> PathBuf {
    let path = named(base, tag);
    keep_track(path.clone());
    path
}

fn named(base: &Path, tag: &str) -> PathBuf {
    base.join(format!("{PREFIX}{}", crate::new_id(tag)))
}

/// Removes a directory or a file, and says nothing of one that is gone. A tree
/// a test froze read-only (an extension's copy, a sealed runner's binaries) is
/// made writable first and removed again.
fn remove(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if std::fs::remove_dir_all(path).is_err() {
        thaw(path);
        let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
    }
}

/// Gives every directory at and under `path` the owner's `rwx`, parents first
/// so the walk can enter each. A file is deleted by its directory's mode.
fn thaw(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    let mode = meta.permissions().mode() | 0o700;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
        thaw(&e.path());
    }
}

/// Notes `path` for removal at exit. The first one in a process also hooks
/// the exit and prunes the old directories under the temp dir.
fn keep_track(path: PathBuf) {
    static HOOK: Once = Once::new();
    extern "C" fn at_exit() {
        sweep();
    }
    HOOK.call_once(|| {
        prune(&std::env::temp_dir(), OLD);
        // SAFETY: `at_exit` takes no arguments and only removes directories.
        unsafe {
            libc::atexit(at_exit);
        }
    });
    MADE.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(path);
}

/// Removes every directory this process made.
fn sweep() {
    let made = std::mem::take(&mut *MADE.lock().unwrap_or_else(PoisonError::into_inner));
    made.into_iter().for_each(remove);
}

/// Removes the test directories under `base` that nothing has touched for
/// `age`, and nothing else: `.tmp*` and other programs' files are not ours.
fn prune(base: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let cutoff = SystemTime::now().checked_sub(age);
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| cutoff.is_some_and(|c| t < c));
        if old && e.file_name().to_string_lossy().starts_with(PREFIX) {
            remove(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aged(path: &Path, ago: Duration) {
        std::fs::File::open(path)
            .unwrap()
            .set_modified(SystemTime::now() - ago)
            .unwrap();
    }

    #[test]
    fn the_prune_removes_only_old_theseus_test_dirs() {
        let base = tempfile::tempdir().unwrap();
        let hour = Duration::from_secs(3600);
        let mk = |name: &str, ago: Duration| {
            let p = base.path().join(name);
            std::fs::create_dir(&p).unwrap();
            aged(&p, ago);
            p
        };
        let (fresh, old) = (
            mk(&format!("{PREFIX}fresh"), hour),
            mk(&format!("{PREFIX}old"), 7 * hour),
        );
        let (other, tmp) = (mk("somebody-elses-old", 7 * hour), mk(".tmpOLD", 7 * hour));
        prune(base.path(), OLD);
        assert!(fresh.exists() && other.exists() && tmp.exists());
        assert!(!old.exists());
    }

    #[test]
    fn a_made_dir_is_swept() {
        let base = tempfile::tempdir().unwrap();
        let p = make(base.path(), "s");
        std::fs::create_dir_all(p.join("store")).unwrap();
        assert!(p.starts_with(base.path()));
        sweep();
        assert!(!p.exists());
    }

    #[test]
    fn a_read_only_tree_is_removed() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let p = make(base.path(), "ro");
        let inner = p.join("store/sealed");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("frozen"), b"x").unwrap();
        for d in [&inner, &p.join("store"), &p] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o555)).unwrap();
        }
        remove(&p);
        assert!(!p.exists());
    }
}
