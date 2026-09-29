//! Context files (theseus-58a): the files a profile names in `context_files`
//! (or `[model].context_files` for a profile that names none), compiled into
//! the system block after the persona, the tools note, and `system`, each
//! under a header that names it (spec §4.4).
//!
//! The compiler reads them, not a tool, so no posture or approval applies.
//! Their text is in the system block, so the system digest covers it: an
//! edit is one `system_changed` recompile, and an unchanged file appends.
//!
//! FAST: nothing is read at startup. Each compile stats each file and
//! rereads it only when its size, mtime, or inode changed, or when it
//! changed less than two seconds before it was read (a second edit inside
//! the filesystem's timestamp granularity would keep the same stamp). A
//! missing or unreadable file does not stop the turn: its section says it is
//! missing, and the caller warns once per file per daemon run.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::compiler::ContextFileRef;

/// The most of one file the system block carries.
pub const MAX_BYTES: usize = 64 * 1024;

/// A file changed this shortly before it was read is read again next time.
const RACY: Duration = Duration::from_secs(2);

/// One file as the system block carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFile {
    /// What the manifest records.
    pub file: ContextFileRef,
    /// The header and the text, or the header and the note that it is missing.
    pub section: String,
}

/// A file that could not be read, the first time this daemon run met it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime: Option<SystemTime>,
    ino: u64,
}

impl Stamp {
    fn of(m: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        let ino = std::os::unix::fs::MetadataExt::ino(m);
        #[cfg(not(unix))]
        let ino = 0;
        Self {
            len: m.len(),
            mtime: m.modified().ok(),
            ino,
        }
    }
}

struct Entry {
    stamp: Stamp,
    racy: bool,
    file: Arc<ContextFile>,
}

/// The daemon's context files: what each file held when it was last read,
/// and which unreadable files it has warned about.
#[derive(Default)]
pub struct ContextFiles {
    cache: Mutex<HashMap<PathBuf, Entry>>,
    warned: Mutex<HashSet<PathBuf>>,
    reads: AtomicU64,
}

impl ContextFiles {
    /// The sections for `paths`, in order, and the files this daemon run
    /// finds unreadable for the first time.
    pub fn load(&self, paths: &[String]) -> (Vec<Arc<ContextFile>>, Vec<Unreadable>) {
        let mut files = Vec::with_capacity(paths.len());
        let mut unreadable = Vec::new();
        for p in paths {
            let path = crate::config::expand(p);
            let shown = path.display().to_string();
            let read = match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => self.read(&path, &shown, Stamp::of(&m)),
                Ok(_) => Err("not a regular file".to_string()),
                Err(e) => Err(reason(&e)),
            };
            match read {
                Ok(f) => files.push(f),
                Err(error) => {
                    if self.warned.lock().unwrap().insert(path) {
                        unreadable.push(Unreadable {
                            path: shown.clone(),
                            error: error.clone(),
                        });
                    }
                    files.push(Arc::new(missing(&shown, &error)));
                }
            }
        }
        (files, unreadable)
    }

    /// Files read since the daemon started (a reused file is not read).
    pub fn reads(&self) -> u64 {
        self.reads.load(Ordering::Relaxed)
    }

    fn read(&self, path: &Path, shown: &str, stamp: Stamp) -> Result<Arc<ContextFile>, String> {
        if let Some(e) = self.cache.lock().unwrap().get(path) {
            if e.stamp == stamp && !e.racy {
                return Ok(e.file.clone());
            }
        }
        let read_at = SystemTime::now();
        let mut buf = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_BYTES as u64 + 1).read_to_end(&mut buf))
            .map_err(|e| reason(&e))?;
        self.reads.fetch_add(1, Ordering::Relaxed);
        let file = Arc::new(section(shown, buf));
        let racy = stamp
            .mtime
            .is_none_or(|t| !matches!(read_at.duration_since(t), Ok(age) if age >= RACY));
        self.cache.lock().unwrap().insert(
            path.to_path_buf(),
            Entry {
                stamp,
                racy,
                file: file.clone(),
            },
        );
        Ok(file)
    }
}

fn reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "not found".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}

fn header(shown: &str) -> String {
    format!("# Context file: {shown}")
}

/// A file's section from up to `MAX_BYTES + 1` bytes of it.
fn section(shown: &str, mut buf: Vec<u8>) -> ContextFile {
    let cut = buf.len() > MAX_BYTES;
    if cut {
        buf.truncate(MAX_BYTES);
        // Do not end inside a UTF-8 sequence.
        if let Err(e) = std::str::from_utf8(&buf) {
            if e.error_len().is_none() {
                buf.truncate(e.valid_up_to());
            }
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let digest = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(text.as_bytes()))[..16].to_string()
    };
    let mut section = format!("{}\n\n{}", header(shown), text.trim_end());
    if cut {
        section.push_str(&format!(
            "\n\n[Cut: only the first {} bytes of this file are included.]",
            crate::narrative::thousands(MAX_BYTES as u64)
        ));
    }
    ContextFile {
        file: ContextFileRef {
            path: shown.to_string(),
            digest: Some(digest),
            bytes: text.len() as u64,
            cut,
            missing: None,
        },
        section,
    }
}

fn missing(shown: &str, error: &str) -> ContextFile {
    ContextFile {
        file: ContextFileRef {
            path: shown.to_string(),
            digest: None,
            bytes: 0,
            cut: false,
            missing: Some(error.to_string()),
        },
        section: format!(
            "{}\n\n[Missing: the file could not be read ({error}).]",
            header(shown)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aged(path: &std::path::Path, secs: u64) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn an_unchanged_file_is_not_read_again_and_a_changed_one_is() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("RULES.md");
        std::fs::write(&p, "one\n").unwrap();
        aged(&p, 60);
        let cf = ContextFiles::default();
        let paths = vec![p.to_string_lossy().into_owned()];
        let (a, w) = cf.load(&paths);
        assert!(w.is_empty());
        assert_eq!(cf.reads(), 1);
        let (b, _) = cf.load(&paths);
        assert_eq!(cf.reads(), 1, "a stat, and no read");
        assert!(Arc::ptr_eq(&a[0], &b[0]));
        // Same size, older stamp: still a change.
        std::fs::write(&p, "two\n").unwrap();
        aged(&p, 30);
        let (c, _) = cf.load(&paths);
        assert_eq!(cf.reads(), 2);
        assert!(c[0].section.ends_with("\n\ntwo"), "{}", c[0].section);
        assert_ne!(c[0].file.digest, a[0].file.digest);
        // Written just now: read again next time, with the same digest.
        std::fs::write(&p, "six\n").unwrap();
        let (d, _) = cf.load(&paths);
        let (e, _) = cf.load(&paths);
        assert_eq!(
            cf.reads(),
            4,
            "a racy file is read until it is two seconds old"
        );
        assert_eq!(d[0].file.digest, e[0].file.digest);
    }

    #[test]
    fn a_long_file_is_cut_on_a_character_boundary_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("BIG.md");
        // 'é' is two bytes; an odd prefix puts one across the cap.
        let text = format!("x{}", "é".repeat(MAX_BYTES));
        std::fs::write(&p, &text).unwrap();
        let (f, _) = ContextFiles::default().load(&[p.to_string_lossy().into_owned()]);
        let f = &f[0];
        assert!(f.file.cut);
        assert_eq!(f.file.bytes, MAX_BYTES as u64 - 1);
        assert!(!f.section.contains('\u{fffd}'));
        assert!(f
            .section
            .ends_with("[Cut: only the first 65,536 bytes of this file are included.]"));
    }

    #[test]
    fn an_unreadable_file_says_it_is_missing_and_warns_once() {
        let dir = tempfile::tempdir().unwrap();
        let cf = ContextFiles::default();
        let paths = vec![
            dir.path().join("GONE.md").to_string_lossy().into_owned(),
            dir.path().to_string_lossy().into_owned(),
        ];
        let (f, w) = cf.load(&paths);
        assert_eq!(f[0].file.missing.as_deref(), Some("not found"));
        assert_eq!(f[0].file.digest, None);
        assert!(f[0]
            .section
            .ends_with("[Missing: the file could not be read (not found).]"));
        assert_eq!(f[1].file.missing.as_deref(), Some("not a regular file"));
        assert_eq!(w.len(), 2);
        let (_, w) = cf.load(&paths);
        assert!(w.is_empty(), "once per file per daemon run: {w:?}");
    }
}
