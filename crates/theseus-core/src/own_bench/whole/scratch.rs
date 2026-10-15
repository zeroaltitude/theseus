//! The scratch copy (own-benchmark part (c)): the turn's directory copied to a
//! directory of its own, owner-only, removed when the replay ends (its guard
//! drops), so what the replay writes is discarded. Never a symbolic link that
//! leaves the tree, a device, a socket or a pipe; never `target/`,
//! `node_modules/` or the session's `.theseus-files/` (named in the caveats);
//! never a path the gate keeps behind a question or the floor (`skip`);
//! never more than the cap, which refuses the replay rather than copying
//! part.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::own_bench::rebuild::NotRebuildable;

/// Directory names left out of the copy: build output and installed
/// dependencies, large and rebuilt by the programs that need them.
pub const OMITTED: [&str; 3] = ["target", "node_modules", ".theseus-files"];

/// The most entries a copy visits, so a directory that is not a project (a
/// home, a root) is refused by count as well as by size.
const MAX_ENTRIES: u64 = 200_000;

/// A copy, and what it holds.
#[derive(Debug)]
pub struct Scratch {
    pub dir: PathBuf,
    pub files: u64,
    pub bytes: u64,
    /// What was left out, in words.
    pub caveats: Vec<String>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn no(why: impl Into<String>) -> NotRebuildable {
    NotRebuildable {
        part: "directory",
        why: why.into(),
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty, owner-only directory under `parent`.
fn fresh(parent: &Path, tag: &str) -> std::io::Result<PathBuf> {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = parent.join(format!(
        "theseus-replay-{}-{}-{n}",
        std::process::id(),
        tag.chars()
            .filter(char::is_ascii_alphanumeric)
            .take(12)
            .collect::<String>()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}

/// `orig` copied under `parent`, at most `max_bytes` of file content.
pub fn copy(
    orig: &Path,
    parent: &Path,
    tag: &str,
    (max_bytes, skip): (u64, &[PathBuf]),
) -> Result<Scratch, NotRebuildable> {
    if !orig.is_dir() {
        return Err(no(format!("{} is not a directory now", orig.display())));
    }
    let dir = fresh(parent, tag).map_err(|e| no(format!("no scratch directory: {e}")))?;
    let mut s = Scratch {
        dir,
        files: 0,
        bytes: 0,
        caveats: Vec::new(),
    };
    let mut seen = 0u64;
    let dest = s.dir.clone();
    walk(orig, &dest, orig, (max_bytes, skip), &mut seen, &mut s)?;
    Ok(s)
}

fn walk(
    from: &Path,
    to: &Path,
    root: &Path,
    (max, skip): (u64, &[PathBuf]),
    seen: &mut u64,
    s: &mut Scratch,
) -> Result<(), NotRebuildable> {
    let entries = std::fs::read_dir(from)
        .map_err(|e| no(format!("{} cannot be read: {e}", from.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| no(format!("{} cannot be read: {e}", from.display())))?;
        *seen += 1;
        if *seen > MAX_ENTRIES {
            return Err(no(format!(
                "{} holds more than {MAX_ENTRIES} entries: it is not a project directory",
                root.display()
            )));
        }
        let name = entry.file_name();
        if skip.iter().any(|k| entry.path().starts_with(k)) {
            s.caveats.push(format!(
                "{} was not copied (the gate asks before it is read)",
                rel(&entry.path(), root)
            ));
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        let ty = entry
            .file_type()
            .map_err(|e| no(format!("{} cannot be read: {e}", src.display())))?;
        if ty.is_dir() {
            if OMITTED.iter().any(|o| name == *o) {
                s.caveats
                    .push(format!("{} was not copied", rel(&src, root)));
                continue;
            }
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&dst)
                .map_err(|e| no(format!("{} cannot be made: {e}", dst.display())))?;
            walk(&src, &dst, root, (max, skip), seen, s)?;
        } else if ty.is_file() {
            let len = entry.metadata().map_or(0, |m| m.len());
            s.bytes += len;
            if s.bytes > max {
                return Err(no(format!(
                    "{} holds more than {} MB to copy ([route] whole_tree_mb)",
                    root.display(),
                    max / (1024 * 1024)
                )));
            }
            std::fs::copy(&src, &dst)
                .map_err(|e| no(format!("{} cannot be copied: {e}", src.display())))?;
            // Owner-only whatever the source's mode, its execute bits kept.
            let mode = entry.metadata().map_or(0o600, |m| m.permissions().mode());
            let _ = std::fs::set_permissions(
                &dst,
                std::fs::Permissions::from_mode(mode & 0o700 | 0o600),
            );
            s.files += 1;
        } else if ty.is_symlink() {
            // A link that stays inside the tree by a relative path is kept;
            // any other would reach outside the copy.
            match std::fs::read_link(&src) {
                Ok(t) if t.is_relative() && !t.components().any(|c| c.as_os_str() == "..") => {
                    let _ = std::os::unix::fs::symlink(&t, &dst);
                }
                _ => s
                    .caveats
                    .push(format!("the link {} was not copied", rel(&src, root))),
            }
        }
    }
    Ok(())
}

fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

/// `text` with every `from` that starts a path (at a boundary) replaced by
/// `to`: a model's paths into the copy, and the copy's back out. Only a
/// boundary is replaced: `/work/proj` in `/work/project` is left.
pub fn remap(text: &str, from: &Path, to: &Path) -> String {
    let (from, to) = (from.to_string_lossy(), to.to_string_lossy());
    if from.is_empty() || from == to {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from.as_ref()) {
        let after = &rest[at + from.len()..];
        let boundary = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.')));
        out.push_str(&rest[..at]);
        if boundary {
            out.push_str(&to);
        } else {
            out.push_str(&from);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// A JSON value with `remap` applied to every string in it.
pub fn remap_json(v: &serde_json::Value, from: &Path, to: &Path) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) => Value::String(remap(s, from, to)),
        Value::Array(a) => Value::Array(a.iter().map(|x| remap_json(x, from, to)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), remap_json(x, from, to)))
                .collect(),
        ),
        other => other.clone(),
    }
}
