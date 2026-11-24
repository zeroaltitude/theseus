//! The join's tree as the count found it and as it left it, read from the file system alone (issue 45). No git reads
//! the tree's files for the count: git reads a working file through whatever filter the tree's own `.gitattributes`
//! selects, and the count's checks run outside L1. So what the counter changed is the difference of two snapshots, and
//! undoing it writes back what the first one held.
//!
//! A snapshot is every path under the tree, walked without following a link: a directory and its permission bits, a
//! regular file, its permission bits and its content's SHA-256, a link and what it names, anything else and its bits.
//! The tree's own `.git` file is in it like any file, and a repository of its own is a path like any other.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// One path of a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Entry {
    /// A directory, and its permission bits.
    Dir(u32),
    /// A regular file: its permission bits and its content's SHA-256.
    File(u32, [u8; 32]),
    /// A symbolic link, and what it names.
    Link(Vec<u8>),
    /// A fifo, a socket or a device, and its bits.
    Other(u32),
}

/// Every path under a tree (relative, as bytes), with what stands there.
pub(super) type Snapshot = BTreeMap<Vec<u8>, Entry>;

/// The tree under `root` as it stands now.
pub(super) fn snapshot(root: &Path) -> Result<Snapshot> {
    let mut out = Snapshot::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        let dir = root.join(&rel);
        let entries =
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for e in entries {
            let e = e?;
            let rel = rel.join(e.file_name());
            let path = root.join(&rel);
            let m = std::fs::symlink_metadata(&path)?;
            let bits = m.permissions().mode() & 0o7777;
            let entry = if m.file_type().is_symlink() {
                Entry::Link(std::fs::read_link(&path)?.as_os_str().as_bytes().to_vec())
            } else if m.is_dir() {
                todo.push(rel.clone());
                Entry::Dir(bits)
            } else if m.is_file() {
                Entry::File(bits, digest(&path)?)
            } else {
                Entry::Other(bits)
            };
            out.insert(rel.as_os_str().as_bytes().to_vec(), entry);
        }
    }
    Ok(out)
}

/// A regular file's content's SHA-256, read without following a link.
fn digest(path: &Path) -> Result<[u8; 32]> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// The paths whose entry differs between two snapshots, in order: added, removed or changed.
pub(super) fn changed(before: &Snapshot, after: &Snapshot) -> Vec<Vec<u8>> {
    let mut paths: Vec<Vec<u8>> = before
        .iter()
        .filter(|(p, e)| after.get(*p) != Some(*e))
        .map(|(p, _)| p.clone())
        .chain(after.keys().filter(|p| !before.contains_key(*p)).cloned())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// A path as words, for a refusal.
pub(super) fn shown(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

/// Whether `path` lies under `dir` (a strict descendant).
fn under(path: &[u8], dir: &[u8]) -> bool {
    path.len() > dir.len() && path.starts_with(dir) && path[dir.len()] == b'/'
}

/// Put the tree under `root` back as `before` held it, from `now`: what the counter added is removed, and what it
/// changed or removed is written again. A file's content comes from `content(path)`, which must hash to what `before`
/// held, or nothing is written for it and this fails; so does a path `before` held that is no file, link or directory.
/// The caller then takes a snapshot again and compares.
pub(super) fn put_back(
    root: &Path,
    before: &Snapshot,
    now: &Snapshot,
    content: impl Fn(&[u8]) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    let paths = changed(before, now);
    // A directory that stays a directory, whose bits the counter changed (made read-only, say): its owner may write
    // in it and pass through it while what it holds is put back, a parent before what it holds; its own bits are
    // applied last, below.
    for p in &paths {
        if let (Some(Entry::Dir(_)), Some(Entry::Dir(mode))) = (before.get(p), now.get(p)) {
            let at = root.join(OsStr::from_bytes(p));
            std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode | 0o700))
                .with_context(|| format!("opening {}", at.display()))?;
        }
    }
    // What stands where it should not: removed, a directory with all it holds (never through a link).
    let mut gone: Vec<&[u8]> = Vec::new();
    for p in &paths {
        let Some(is) = now.get(p) else { continue };
        if gone.iter().any(|g| under(p, g)) {
            continue;
        }
        let same_kind = matches!(
            (before.get(p), is),
            (Some(Entry::Dir(_)), Entry::Dir(_)) | (Some(Entry::File(..)), Entry::File(..))
        );
        if same_kind {
            continue;
        }
        let at = root.join(OsStr::from_bytes(p));
        match is {
            Entry::Dir(_) => std::fs::remove_dir_all(&at),
            _ => std::fs::remove_file(&at),
        }
        .with_context(|| format!("removing {}", at.display()))?;
        gone.push(p);
    }
    // What should stand: written again, a directory before what it holds; a directory's bits last, so a directory
    // that is to be read-only is written into first.
    let mut bits = Vec::new();
    for p in &paths {
        let Some(was) = before.get(p) else { continue };
        let at = root.join(OsStr::from_bytes(p));
        match was {
            Entry::Dir(mode) => {
                if std::fs::symlink_metadata(&at).is_err() {
                    std::fs::create_dir(&at).with_context(|| format!("making {}", at.display()))?;
                }
                bits.push((at, *mode));
            }
            Entry::File(mode, sum) => {
                let Some(text) = content(p)? else {
                    bail!(
                        "{} cannot be written again: git holds no copy of it",
                        shown(p)
                    );
                };
                if <[u8; 32]>::from(Sha256::digest(&text)) != *sum {
                    bail!(
                        "{} cannot be written again: git's copy is not what the tree held",
                        shown(p)
                    );
                }
                if std::fs::symlink_metadata(&at).is_ok() {
                    std::fs::remove_file(&at)
                        .with_context(|| format!("removing {}", at.display()))?;
                }
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&at)
                    .and_then(|mut f| std::io::Write::write_all(&mut f, &text))
                    .with_context(|| format!("writing {}", at.display()))?;
                std::fs::set_permissions(&at, std::fs::Permissions::from_mode(*mode))?;
            }
            Entry::Link(to) => {
                std::os::unix::fs::symlink(OsStr::from_bytes(to), &at)
                    .with_context(|| format!("linking {}", at.display()))?;
            }
            Entry::Other(_) => bail!(
                "{} cannot be made again: it is no file, link or directory",
                shown(p)
            ),
        }
    }
    for (at, mode) in bits.into_iter().rev() {
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}
