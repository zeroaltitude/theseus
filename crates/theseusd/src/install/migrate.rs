//! `--migrate-state <dir>` (M4 §2.9, theseus-7hh): a stopped daemon's state,
//! copied into the separated layout's state dir under F4's rules. The source
//! is only read: never written, never deleted. A daemon answering in it, or a
//! store newer than this build, is refused before anything is copied, and a
//! store already in the layout is never overwritten.
//!
//! The copy runs as root out of the operator's directory, where the
//! operator's jobs can write. So every entry must be a regular file or a
//! directory owned by the directory's owner, checked on what was opened: a
//! symlink planted there cannot make root copy a file the operator could not
//! read into a place the daemon would serve it from.

use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::host::Host;
use super::layout::{answers, present, real, same_bytes, set, Act, Owner};

/// What a state dir holds that the separated daemon needs: the store (its
/// WAL, index, and blobs), the spool (results not yet read), the config's
/// last-known-good copy, and the Discord bindings.
pub(crate) const NAMES: [&str; 4] = ["store", "spool", "config.last-good.toml", "bindings.toml"];

/// Where a copy is made before it is renamed into place, inside the layout's
/// state dir, so a copy cut short is never taken for a store.
const STAGING: &str = ".migrate-staging";

#[derive(Debug, Clone)]
pub(crate) struct Migrate {
    /// The old state dir, on the machine itself: never under the root.
    pub from: PathBuf,
    /// The layout's state dir.
    pub to: PathBuf,
    pub owner: Owner,
}

/// A tree's entries, relative to its top: sockets are a running daemon's,
/// not state, and are neither copied nor compared.
#[derive(Debug, PartialEq, Eq)]
enum Node {
    Dir(PathBuf),
    File(PathBuf, u64),
}

impl Migrate {
    pub fn describe(&self) -> String {
        format!(
            "{} -> {} ({} 0700/0600)",
            self.from.display(),
            self.to.display(),
            self.owner
        )
    }

    pub fn inspect(&self, root: &Path) -> Result<Act> {
        Ok(match self.todo(root)? {
            Err(why) => Act::Refuse(why),
            Ok(todo) if todo.is_empty() => Act::InPlace,
            Ok(_) => Act::Create,
        })
    }

    /// The names still to copy, or why none may be.
    fn todo(&self, root: &Path) -> Result<std::result::Result<Vec<&'static str>, String>> {
        let from = &self.from;
        let shown = from.display();
        let Some(m) = present(from)? else {
            return Ok(Err(format!("{shown} does not exist")));
        };
        if m.file_type().is_symlink() || !m.is_dir() {
            return Ok(Err(format!("{shown} is not a directory")));
        }
        let dest = real(root, &self.to);
        let canon = std::fs::canonicalize(from)?;
        let dest_canon = std::fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone());
        if canon.starts_with(&dest_canon) || dest_canon.starts_with(&canon) {
            return Ok(Err(format!(
                "{shown} and {} overlap: copy from the old daemon's own state dir",
                self.to.display()
            )));
        }
        let mut found = vec![];
        let mut sockets = vec![];
        for e in std::fs::read_dir(from).with_context(|| format!("reading {shown}"))? {
            let e = e?;
            if e.file_type()?.is_socket() {
                sockets.push(e.path());
            }
        }
        for name in NAMES {
            if present(&from.join(name))?.is_some() {
                let nodes = match walk(&from.join(name), m.uid(), &mut sockets) {
                    Ok(n) => n,
                    Err(why) => return Ok(Err(format!("{why:#}"))),
                };
                found.push((name, nodes));
            }
        }
        if let Some(s) = sockets.iter().find(|s| answers(s)) {
            return Ok(Err(format!(
                "a daemon answers on {}: stop it first (`theseus shutdown`); only a stopped \
                 daemon's store is copied",
                s.display()
            )));
        }
        if found.is_empty() {
            return Ok(Err(format!(
                "{shown} holds none of {}: nothing to migrate",
                NAMES.join(", ")
            )));
        }
        if let Err(e) = theseus_store::store::check_manifest(&from.join("store")) {
            return Ok(Err(format!("{e:#}")));
        }
        let mut todo = vec![];
        for (name, nodes) in found {
            let there = dest.join(name);
            if present(&there)?.is_none() {
                todo.push(name);
                continue;
            }
            let same = match walk(
                &there,
                std::fs::symlink_metadata(&there)?.uid(),
                &mut vec![],
            ) {
                Ok(have) => have == nodes && same_files(&from.join(name), &there, &nodes)?,
                Err(_) => false,
            };
            if !same {
                return Ok(Err(format!(
                    "{}/{name} already holds something else: the installer never overwrites a \
                     store; move it aside first",
                    self.to.display()
                )));
            }
        }
        Ok(Ok(todo))
    }

    pub fn apply(
        &self,
        root: &Path,
        host: &mut dyn Host,
        log: &mut dyn FnMut(String),
    ) -> Result<()> {
        let todo = match self.todo(root)? {
            Ok(t) => t,
            Err(why) => bail!("not copying the state: {why}"),
        };
        let dest = real(root, &self.to);
        let staging = dest.join(STAGING);
        if let Some(m) = present(&staging)? {
            if m.file_type().is_symlink() || !m.is_dir() {
                bail!(
                    "{}/{STAGING} is not a directory: remove it first",
                    self.to.display()
                );
            }
            std::fs::remove_dir_all(&staging)?;
            log(format!(
                "removed {}/{STAGING}, a copy cut short",
                self.to.display()
            ));
        }
        std::fs::DirBuilder::new().mode(0o700).create(&staging)?;
        set(host, &staging, &self.owner, 0o700)?;
        let uid = std::fs::symlink_metadata(&self.from)?.uid();
        for name in todo {
            let (files, bytes) = copy(
                &self.from.join(name),
                &staging.join(name),
                uid,
                host,
                &self.owner,
            )?;
            host.rename(&staging.join(name), &dest.join(name))?;
            log(format!(
                "copied {}/{name} to {}/{name}: {files} file(s), {bytes} bytes ({} 0700/0600)",
                self.from.display(),
                self.to.display(),
                self.owner
            ));
        }
        std::fs::remove_dir(&staging)?;
        log(format!(
            "{} was only read: nothing in it changed",
            self.from.display()
        ));
        Ok(())
    }
}

/// A tree's entries, sorted, each checked: a regular file or a directory,
/// owned by `uid`. Sockets go to `sockets`, and are not entries.
fn walk(top: &Path, uid: u32, sockets: &mut Vec<PathBuf>) -> Result<Vec<Node>> {
    let mut nodes = vec![];
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let at = if rel.as_os_str().is_empty() {
            top.to_path_buf()
        } else {
            top.join(&rel)
        };
        let m =
            std::fs::symlink_metadata(&at).with_context(|| format!("reading {}", at.display()))?;
        let t = m.file_type();
        if t.is_socket() {
            sockets.push(at);
            continue;
        }
        if t.is_symlink() || !(t.is_dir() || t.is_file()) {
            bail!(
                "{} is not a regular file or a directory (a symlink, a fifo, or a device): a \
                 state dir holds none, so nothing is copied",
                at.display()
            );
        }
        if m.uid() != uid {
            bail!(
                "{} is owned by uid {}, not by the state dir's owner (uid {uid}): nothing is copied",
                at.display(),
                m.uid()
            );
        }
        if t.is_file() {
            nodes.push(Node::File(rel, m.len()));
            continue;
        }
        nodes.push(Node::Dir(rel.clone()));
        for e in std::fs::read_dir(&at).with_context(|| format!("reading {}", at.display()))? {
            stack.push(rel.join(e?.file_name()));
        }
    }
    nodes.sort_by(|a, b| key(a).cmp(key(b)));
    Ok(nodes)
}

fn key(n: &Node) -> &Path {
    match n {
        Node::Dir(p) | Node::File(p, _) => p,
    }
}

fn same_files(a: &Path, b: &Path, nodes: &[Node]) -> Result<bool> {
    for n in nodes {
        if let Node::File(rel, _) = n {
            let (fa, fb) = if rel.as_os_str().is_empty() {
                (a.to_path_buf(), b.to_path_buf())
            } else {
                (a.join(rel), b.join(rel))
            };
            if !same_bytes(&fa, &fb)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Copy a tree: directories `0700`, files `0600`, each owned by `owner`.
/// A file is opened without following a symlink and without blocking on a
/// fifo, and what was opened is checked again.
fn copy(
    src: &Path,
    dst: &Path,
    uid: u32,
    host: &mut dyn Host,
    owner: &Owner,
) -> Result<(u64, u64)> {
    let m = std::fs::symlink_metadata(src).with_context(|| format!("reading {}", src.display()))?;
    let t = m.file_type();
    if t.is_socket() {
        return Ok((0, 0));
    }
    if m.uid() != uid || t.is_symlink() {
        bail!("{} changed while it was copied: stopping", src.display());
    }
    if t.is_dir() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dst)
            .with_context(|| format!("creating {}", dst.display()))?;
        set(host, dst, owner, 0o700)?;
        let mut names: Vec<_> = std::fs::read_dir(src)?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<std::io::Result<_>>()?;
        names.sort();
        let (mut files, mut bytes) = (0, 0);
        for n in names {
            let (f, b) = copy(&src.join(&n), &dst.join(&n), uid, host, owner)?;
            files += f;
            bytes += b;
        }
        return Ok((files, bytes));
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(src)
        .with_context(|| format!("opening {}", src.display()))?;
    let opened = f.metadata()?;
    if !opened.is_file() || opened.uid() != uid {
        bail!(
            "{} changed while it was copied (not a regular file of the state dir's owner): stopping",
            src.display()
        );
    }
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dst)
        .with_context(|| format!("creating {}", dst.display()))?;
    let n = std::io::copy(&mut f, &mut out)?;
    out.sync_all()?;
    drop(out);
    set(host, dst, owner, 0o600)?;
    Ok((1, n))
}
