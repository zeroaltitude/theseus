//! The daemon's own binary, as health reads it (review 2's consideration 3).
//! At L0 a job runs as the daemon's user, so a binary that user can write, or
//! one in a directory that user can write, is one a job can replace, and the
//! next start runs whatever it finds there. Health says so; running the
//! builder as its own user (`theseusd install --separate`) is the fix.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use theseus_protocol::BinaryStatus;

/// The running binary's status: `current_exe` as this process sees it.
pub fn status() -> BinaryStatus {
    match std::env::current_exe() {
        Ok(p) => status_of(&p),
        Err(e) => BinaryStatus {
            path: String::new(),
            state: "unknown".into(),
            detail: format!("the running binary's path could not be read: {e}"),
        },
    }
}

/// Whether this process's user can write `exe`, or put another file in its
/// place: `jobs_can_write`, `ok`, or `unknown`.
pub fn status_of(exe: &Path) -> BinaryStatus {
    // A binary replaced since this process started reads as `… (deleted)`;
    // the path is what the next start runs.
    let shown = exe.to_string_lossy();
    let (path, replaced) = match shown.strip_suffix(" (deleted)") {
        Some(p) => (PathBuf::from(p), true),
        None => (exe.to_path_buf(), false),
    };
    let status = |state: &str, detail: String| BinaryStatus {
        path: path.display().to_string(),
        state: state.into(),
        detail,
    };
    let dir = path.parent().unwrap_or(Path::new("/"));
    let (file, folder) = match (std::fs::metadata(&path), std::fs::metadata(dir)) {
        (Ok(f), Ok(d)) => (f, d),
        (Err(e), _) | (_, Err(e)) => {
            return status(
                "unknown",
                format!("{} could not be read: {e}", path.display()),
            )
        }
    };
    let uid = unsafe { libc::geteuid() };
    let since = if replaced {
        " (it was replaced since this daemon started)"
    } else {
        ""
    };
    if writable(&path) {
        return status(
            "jobs_can_write",
            format!(
                "the file is writable by this daemon's user (uid {uid}), as which jobs run, so a \
                 job can rewrite it, and the next start runs what it finds{since}"
            ),
        );
    }
    // A directory the user can write lets a job rename another file over it,
    // unless the directory is sticky and the user owns neither.
    let sticky =
        folder.mode() & libc::S_ISVTX != 0 && file.uid() != uid && folder.uid() != uid && uid != 0;
    if writable(dir) && !sticky {
        return status(
            "jobs_can_write",
            format!(
                "its directory {} is writable by this daemon's user (uid {uid}), as which jobs \
                 run, so a job can put another file in its place, and the next start runs it{since}",
                dir.display()
            ),
        );
    }
    status(
        "ok",
        format!("neither the file nor its directory is writable by this daemon's user{since}"),
    )
}

/// The effective user may write `p` (`faccessat` with `AT_EACCESS`): its
/// mode, its owner, and a read-only mount all count.
fn writable(p: &Path) -> bool {
    let Ok(c) = CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::faccessat(libc::AT_FDCWD, c.as_ptr(), libc::W_OK, libc::AT_EACCESS) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn chmod(p: &Path, mode: u32) {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Review 2's consideration 3: health says when a job can write the
    /// binary: the file itself, or its directory; and not when neither is
    /// writable. Root writes anything, so as root only the first holds.
    #[test]
    fn health_says_when_jobs_can_write_the_binary() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("bin");
        std::fs::create_dir(&dir).unwrap();
        let exe = dir.join("theseusd");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        chmod(&exe, 0o755);
        let s = status_of(&exe);
        assert_eq!(s.state, "jobs_can_write", "{s:?}");
        assert!(s.detail.starts_with("the file is writable"), "{s:?}");
        assert_eq!(s.path, exe.display().to_string());
        chmod(&exe, 0o555);
        let s = status_of(&exe);
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(s.state, "jobs_can_write", "{s:?}");
            assert!(s.detail.starts_with("its directory"), "{s:?}");
            chmod(&dir, 0o555);
            let s = status_of(&exe);
            assert_eq!(s.state, "ok", "{s:?}");
            chmod(&dir, 0o755);
        }
        let gone = PathBuf::from(format!("{} (deleted)", exe.display()));
        let s = status_of(&gone);
        assert_eq!(
            s.path,
            exe.display().to_string(),
            "the path the next start runs"
        );
        assert!(
            s.detail
                .ends_with("(it was replaced since this daemon started)"),
            "{s:?}"
        );
        let s = status_of(&d.path().join("missing"));
        assert_eq!(s.state, "unknown", "{s:?}");
    }
}
