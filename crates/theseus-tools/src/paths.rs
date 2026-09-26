//! Path resolution the gate can trust: lexical normalization first (so `..`
//! cannot climb out unnoticed), then symlink resolution of the longest prefix
//! that exists (so a link inside a root cannot point outside it unnoticed).

use std::path::{Component, Path, PathBuf};

/// Normalize `.` and `..` without touching the filesystem.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalize the longest existing ancestor and append the rest.
pub fn canonical_best_effort(p: &Path) -> PathBuf {
    let p = normalize(p);
    let mut existing = p.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = existing.canonicalize() {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (
            existing.file_name().map(|s| s.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return p,
        }
    }
}

pub fn resolve(cwd: &Path, p: &str) -> PathBuf {
    let raw = if let Some(rest) = p.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest)
    } else {
        PathBuf::from(p)
    };
    let joined = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };
    canonical_best_effort(&joined)
}

/// Is `p` equal to or under `root`? Both must already be resolved.
pub fn within(p: &Path, root: &Path) -> bool {
    p == root || p.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotdot_and_symlinks_cannot_escape() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        assert_eq!(resolve(&root, "a/b/../../x.txt"), root.join("x.txt"));
        assert!(!within(&resolve(&root, "../../etc/passwd"), &root));
        assert_eq!(resolve(&root, "a/new/file.rs"), root.join("a/new/file.rs"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();
            let r = resolve(&root, "link/passwd");
            assert!(!within(&r, &root), "{r:?} must resolve outside the root");
        }
    }
}
