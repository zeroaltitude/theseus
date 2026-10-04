//! The freeze (M7 43a, design §2.7 step 1): a proposed server's directory,
//! copied into `<state>/extensions/<name>/<digest>/`, addressed by the
//! tree's SHA-256. What the operator acks is that copy, read-only: a later
//! edit in the workspace changes nothing that runs.
//!
//! The digest is over the tree's regular files and directories, in sorted
//! order: each one's path from the root, its kind, whether a file is
//! executable (as git keeps a mode: 755 or 644, so the read-only copy
//! digests as its source does), and a file's length and bytes. A symbolic link, or anything else that is not
//! a file or a directory, is refused, since a link could point back into
//! the workspace and run what an edit there writes. So the same tree always
//! digests the same, wherever it lies, and any change to a name, a mode, or
//! a byte is a new digest.

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The most files and bytes a proposed tree may hold: a small server, not a
/// project.
pub const MAX_FILES: usize = 512;
pub const MAX_BYTES: u64 = 16 << 20;

/// One entry of a tree, as the digest reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// From the tree's root, `/`-separated; never empty.
    rel: String,
    dir: bool,
    /// 755 for a directory or an executable file, else 644.
    mode: u32,
    len: u64,
}

/// What a freeze made: the digest, and the frozen copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frozen {
    /// The tree's SHA-256, in hex.
    pub digest: String,
    /// `<root>/<name>/<digest>`.
    pub dir: PathBuf,
    pub files: usize,
    pub bytes: u64,
}

/// The tree under `root`, sorted by path: every entry, or why it cannot be
/// frozen.
fn walk(root: &Path) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    let mut bytes = 0u64;
    while let Some(rel) = stack.pop() {
        let dir = root.join(&rel);
        let rd = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for item in rd {
            let item = item.map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                return Err(format!(
                    "{} has a name that is not UTF-8",
                    item.path().display()
                ));
            };
            let path = rel.join(name);
            let md = fs::symlink_metadata(item.path())
                .map_err(|e| format!("{}: {e}", item.path().display()))?;
            let kind = md.file_type();
            let shown = path.to_string_lossy().into_owned();
            if kind.is_symlink() {
                return Err(format!(
                    "{shown} is a symbolic link: a frozen tree holds files and directories only, \
                     so nothing in it can point back to what an edit changes"
                ));
            }
            if !kind.is_file() && !kind.is_dir() {
                return Err(format!("{shown} is neither a file nor a directory"));
            }
            let len = if kind.is_file() { md.len() } else { 0 };
            bytes += len;
            out.push(Entry {
                rel: shown,
                dir: kind.is_dir(),
                mode: if kind.is_dir() || md.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                },
                len,
            });
            if out.len() > MAX_FILES {
                return Err(format!(
                    "it holds more than {MAX_FILES} files and directories: propose the server \
                     alone, not its project"
                ));
            }
            if bytes > MAX_BYTES {
                return Err(format!(
                    "it holds more than {} MiB: propose the server alone, not its project",
                    MAX_BYTES >> 20
                ));
            }
            if kind.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

/// The SHA-256 of the tree under `root`, and its entries: the same tree
/// always digests the same.
fn digest_of(root: &Path) -> Result<(String, Vec<Entry>), String> {
    let entries = walk(root)?;
    let mut h = Sha256::new();
    for e in &entries {
        let head = format!(
            "{}\0{}\0{:o}\0{}\0",
            e.rel,
            if e.dir { "d" } else { "f" },
            e.mode,
            e.len
        );
        h.update(head.as_bytes());
        if !e.dir {
            let p = root.join(&e.rel);
            let mut f = fs::File::open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let mut buf = [0u8; 64 * 1024];
            let mut read = 0u64;
            loop {
                let n = f
                    .read(&mut buf)
                    .map_err(|e| format!("{}: {e}", p.display()))?;
                if n == 0 {
                    break;
                }
                read += n as u64;
                h.update(&buf[..n]);
            }
            if read != e.len {
                return Err(format!("{} changed while it was read", e.rel));
            }
        }
    }
    Ok((hex::encode(h.finalize()), entries))
}

/// The digest of the tree under `root`.
pub fn digest(root: &Path) -> Result<String, String> {
    digest_of(root).map(|(d, _)| d)
}

/// Copy `src` into `<root>/<name>/<digest>/`, read-only, and return the
/// digest and the copy. A copy already there under the same digest is
/// checked and kept; the copy is made beside it and renamed into place, so
/// no reader sees half of one. The digest is of the copy as made, so a file
/// changed while it was read fails the freeze.
pub fn freeze(src: &Path, root: &Path, name: &str) -> Result<Frozen, String> {
    let (digest, entries) = digest_of(src)?;
    let files = entries.iter().filter(|e| !e.dir).count();
    let bytes = entries.iter().map(|e| e.len).sum();
    let parent = root.join(name);
    let dir = parent.join(&digest);
    if dir.is_dir() {
        if self::digest(&dir)? == digest {
            return Ok(Frozen {
                digest,
                dir,
                files,
                bytes,
            });
        }
        return Err(format!(
            "{} is there and holds another tree: it is left as it is",
            dir.display()
        ));
    }
    fs::create_dir_all(&parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let tmp = parent.join(format!(".{digest}.{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let made = copy(src, &tmp, &entries).and_then(|()| {
        seal(&tmp, &entries)?;
        if self::digest(&tmp)? != digest {
            return Err("the tree changed while it was copied: propose it again".into());
        }
        fs::rename(&tmp, &dir).map_err(|e| format!("{}: {e}", dir.display()))
    });
    if let Err(e) = made {
        let _ = make_writable(&tmp);
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    Ok(Frozen {
        digest,
        dir,
        files,
        bytes,
    })
}

fn copy(src: &Path, dst: &Path, entries: &[Entry]) -> Result<(), String> {
    fs::create_dir(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for e in entries {
        let to = dst.join(&e.rel);
        if e.dir {
            fs::create_dir(&to).map_err(|err| format!("{}: {err}", to.display()))?;
        } else {
            fs::copy(src.join(&e.rel), &to).map_err(|err| format!("{}: {err}", e.rel))?;
        }
        fs::set_permissions(&to, fs::Permissions::from_mode(e.mode | 0o700))
            .map_err(|err| format!("{}: {err}", to.display()))?;
    }
    Ok(())
}

/// Every entry to its mode with no write bit (555 or 444), deepest first,
/// then the root: nothing in the copy is writable, by its owner either.
fn seal(dst: &Path, entries: &[Entry]) -> Result<(), String> {
    for e in entries.iter().rev() {
        let p = dst.join(&e.rel);
        fs::set_permissions(&p, fs::Permissions::from_mode(e.mode & 0o555))
            .map_err(|err| format!("{}: {err}", p.display()))?;
    }
    fs::set_permissions(dst, fs::Permissions::from_mode(0o555))
        .map_err(|err| format!("{}: {err}", dst.display()))
}

/// Every directory under `dir` writable by its owner again, so it can be
/// removed: a failed copy's, and a test's.
pub(crate) fn make_writable(dir: &Path) -> std::io::Result<()> {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    for item in fs::read_dir(dir)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            make_writable(&item.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(at: &Path) {
        fs::create_dir_all(at.join("lib")).unwrap();
        fs::write(at.join("server.sh"), "#!/bin/sh\necho counting\n").unwrap();
        fs::set_permissions(at.join("server.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(at.join("lib/words.txt"), "alpha beta\n").unwrap();
    }

    /// The same tree digests the same wherever it lies; a changed byte, a
    /// changed mode, or a new name is a new digest.
    #[test]
    fn the_digest_is_stable_and_follows_every_byte_mode_and_name() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        tree(&a);
        tree(&b);
        let first = digest(&a).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(digest(&a).unwrap(), first, "stable");
        assert_eq!(digest(&b).unwrap(), first, "wherever it lies");
        fs::write(b.join("lib/words.txt"), "alpha betA\n").unwrap();
        let byte = digest(&b).unwrap();
        assert_ne!(byte, first);
        tree(&b);
        assert_eq!(digest(&b).unwrap(), first);
        fs::set_permissions(b.join("server.sh"), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(digest(&b).unwrap(), first, "executable either way");
        fs::set_permissions(b.join("server.sh"), fs::Permissions::from_mode(0o644)).unwrap();
        assert_ne!(digest(&b).unwrap(), first, "a mode");
        tree(&b);
        fs::write(b.join("extra"), "").unwrap();
        assert_ne!(digest(&b).unwrap(), first, "a name");
    }

    /// The copy is the tree, read-only, under its digest; an edit to the
    /// source after the freeze changes nothing in it; freezing the same tree
    /// again keeps the copy.
    #[test]
    fn the_copy_is_read_only_and_an_edit_after_it_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("work/wc");
        tree(&src);
        let state = d.path().join("state/extensions");
        let f = freeze(&src, &state, "wordcount").unwrap();
        assert_eq!(f.dir, state.join("wordcount").join(&f.digest));
        assert_eq!((f.files, f.bytes), (2, 35));
        assert_eq!(digest(&f.dir).unwrap(), f.digest);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&f.dir.join("server.sh")), 0o555);
        assert_eq!(mode(&f.dir.join("lib/words.txt")), 0o444);
        assert_eq!(mode(&f.dir) & 0o222, 0);
        fs::write(src.join("server.sh"), "#!/bin/sh\nexit 7\n").unwrap();
        assert_eq!(
            fs::read_to_string(f.dir.join("server.sh")).unwrap(),
            "#!/bin/sh\necho counting\n"
        );
        assert_eq!(digest(&f.dir).unwrap(), f.digest, "the copy is unchanged");
        assert_ne!(digest(&src).unwrap(), f.digest);
        // The same tree again: the same copy, kept.
        tree(&src);
        assert_eq!(freeze(&src, &state, "wordcount").unwrap(), f);
        // No temporary copy is left beside it.
        let left: Vec<_> = fs::read_dir(state.join("wordcount")).unwrap().collect();
        assert_eq!(left.len(), 1);
        make_writable(&state).unwrap();
    }

    /// A link is refused, so nothing frozen can point back into the
    /// workspace; and nothing is left behind.
    #[test]
    fn a_symbolic_link_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("wc");
        tree(&src);
        std::os::unix::fs::symlink(src.join("server.sh"), src.join("run.sh")).unwrap();
        let state = d.path().join("ext");
        let e = freeze(&src, &state, "wordcount").unwrap_err();
        assert!(e.contains("run.sh is a symbolic link"), "{e}");
        assert!(
            !state.join("wordcount").exists()
                || fs::read_dir(state.join("wordcount"))
                    .unwrap()
                    .next()
                    .is_none()
        );
    }
}
