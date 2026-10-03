//! Context files (theseus-58a; in two levels since theseus-c48): the system
//! level, `[context] files`, which every session gets, then the files of the
//! persona in play, `[personas.<name>] files`. They are compiled into the
//! system block after the persona, the tools note, and `system`, each under a
//! header that names it and its level (spec §4.4).
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

use serde::{Deserialize, Serialize};

use crate::compiler::ContextFileRef;

/// A context file as `[context] files` and `[personas.<name>] files` name it
/// (M4 19a): its path, or a table with its path and its readers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContextEntry {
    Path(String),
    Table(ContextFileEntry),
}

/// `{ path = "~/notes.md", readers = "public" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextFileEntry {
    pub path: String,
    #[serde(default)]
    pub readers: ContextReaders,
}

/// Who may read a context file: the owner (the default), or anyone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextReaders {
    #[default]
    Owner,
    Public,
}

impl ContextEntry {
    pub fn path(&self) -> &str {
        match self {
            ContextEntry::Path(p) => p,
            ContextEntry::Table(t) => &t.path,
        }
    }

    pub fn readers(&self) -> ContextReaders {
        match self {
            ContextEntry::Path(_) => ContextReaders::Owner,
            ContextEntry::Table(t) => t.readers,
        }
    }
}

impl From<String> for ContextEntry {
    fn from(path: String) -> Self {
        ContextEntry::Path(path)
    }
}

/// The most of one file the system block carries.
pub const MAX_BYTES: usize = 64 * 1024;

/// A file changed this shortly before it was read is read again next time.
const RACY: Duration = Duration::from_secs(2);

/// A context file as the config names it, with its level (theseus-c48).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPath {
    pub path: String,
    /// The persona whose file it is; None: the system level.
    pub persona: Option<String>,
    /// Its entry says `readers = "public"` (M4 19a); otherwise the owner
    /// alone may read it.
    pub public: bool,
}

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

/// What a file held when it was last read, whatever its level: its record,
/// and the text its section carries under the header.
#[derive(Debug)]
struct Held {
    file: ContextFileRef,
    body: String,
}

struct Entry {
    stamp: Stamp,
    racy: bool,
    held: Arc<Held>,
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
    pub fn load(&self, paths: &[ContextPath]) -> (Vec<ContextFile>, Vec<Unreadable>) {
        let mut files = Vec::with_capacity(paths.len());
        let mut unreadable = Vec::new();
        for p in paths {
            let path = crate::config::expand(&p.path);
            let shown = path.display().to_string();
            let read = match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => self.read(&path, &shown, Stamp::of(&m)),
                Ok(_) => Err("not a regular file".to_string()),
                Err(e) => Err(reason(&e)),
            };
            let held = match read {
                Ok(h) => h,
                Err(error) => {
                    if self.warned.lock().unwrap().insert(path) {
                        unreadable.push(Unreadable {
                            path: shown.clone(),
                            error: error.clone(),
                        });
                    }
                    Arc::new(missing(&shown, &error))
                }
            };
            files.push(ContextFile {
                file: ContextFileRef {
                    persona: p.persona.clone(),
                    readers: p.public.then_some(theseus_protocol::Readers::Public),
                    ..held.file.clone()
                },
                section: format!("{}\n\n{}", header(&shown, p.persona.as_deref()), held.body),
            });
        }
        (files, unreadable)
    }

    /// Files read since the daemon started (a reused file is not read).
    pub fn reads(&self) -> u64 {
        self.reads.load(Ordering::Relaxed)
    }

    fn read(&self, path: &Path, shown: &str, stamp: Stamp) -> Result<Arc<Held>, String> {
        if let Some(e) = self.cache.lock().unwrap().get(path) {
            if e.stamp == stamp && !e.racy {
                return Ok(e.held.clone());
            }
        }
        let read_at = SystemTime::now();
        let mut buf = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_BYTES as u64 + 1).read_to_end(&mut buf))
            .map_err(|e| reason(&e))?;
        self.reads.fetch_add(1, Ordering::Relaxed);
        let held = Arc::new(section(shown, buf));
        let racy = stamp
            .mtime
            .is_none_or(|t| !matches!(read_at.duration_since(t), Ok(age) if age >= RACY));
        self.cache.lock().unwrap().insert(
            path.to_path_buf(),
            Entry {
                stamp,
                racy,
                held: held.clone(),
            },
        );
        Ok(held)
    }
}

/// Leave out each file whose readers do not cover the session's audience
/// (M4 19a, §2.7): its section becomes its header and why, so the block still
/// says the file is there, and the manifest records it as withheld. A file is
/// the owner's unless its entry says `readers = "public"`.
pub fn withhold(files: &mut [ContextFile], judge: &crate::labels::Judge) {
    for f in files {
        if f.file.withheld.is_some() {
            continue;
        }
        let readers = f
            .file
            .readers
            .clone()
            .unwrap_or(theseus_protocol::Readers::Owner);
        if judge.covers(&readers) {
            continue;
        }
        f.section = format!(
            "{} — withheld: {}, and this session's audience is {}",
            header(&f.file.path, f.file.persona.as_deref()),
            readers.describe(),
            judge.audience.describe()
        );
        f.file.withheld = Some(readers.describe());
        f.file.digest = None;
        f.file.bytes = 0;
        f.file.cut = false;
    }
}

/// Leave out each file a shared place may not carry (the place rule,
/// theseus-nbsh): every file whose entry does not say `readers = "public"`.
/// Its section becomes its header and why, and the manifest records it as
/// withheld.
pub fn withhold_shared(files: &mut [ContextFile]) {
    for f in files {
        let public = f.file.readers == Some(theseus_protocol::Readers::Public);
        if public || f.file.withheld.is_some() {
            continue;
        }
        f.section = format!(
            "{} — withheld: this place is shared, and the file is not marked readers = \"public\"",
            header(&f.file.path, f.file.persona.as_deref()),
        );
        f.file.withheld = Some(SHARED_PLACE.into());
        f.file.digest = None;
        f.file.bytes = 0;
        f.file.cut = false;
    }
}

/// Why a shared place's request left a context file out (the manifest's
/// `withheld`).
pub const SHARED_PLACE: &str = "not public, in a shared place";

fn reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "not found".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}

/// A file's header names it and its level, so a reader of the system block
/// sees which files are every session's and which the persona's.
fn header(shown: &str, persona: Option<&str>) -> String {
    match persona {
        None => format!("# Context file (system): {shown}"),
        Some(name) => format!("# Context file (persona {name}): {shown}"),
    }
}

/// A file's record and text from up to `MAX_BYTES + 1` bytes of it.
fn section(shown: &str, mut buf: Vec<u8>) -> Held {
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
    let mut body = text.trim_end().to_string();
    if cut {
        body.push_str(&format!(
            "\n\n[Cut: only the first {} bytes of this file are included.]",
            crate::narrative::thousands(MAX_BYTES as u64)
        ));
    }
    Held {
        file: ContextFileRef {
            path: shown.to_string(),
            digest: Some(digest),
            bytes: text.len() as u64,
            cut,
            missing: None,
            persona: None,
            readers: None,
            withheld: None,
        },
        body,
    }
}

fn missing(shown: &str, error: &str) -> Held {
    Held {
        file: ContextFileRef {
            path: shown.to_string(),
            digest: None,
            bytes: 0,
            cut: false,
            missing: Some(error.to_string()),
            persona: None,
            readers: None,
            withheld: None,
        },
        body: format!("[Missing: the file could not be read ({error}).]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system level's paths.
    fn system(paths: &[&Path]) -> Vec<ContextPath> {
        paths
            .iter()
            .map(|p| ContextPath {
                path: p.to_string_lossy().into_owned(),
                persona: None,
                public: false,
            })
            .collect()
    }

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
        let paths = system(&[&p]);
        let (a, w) = cf.load(&paths);
        assert!(w.is_empty());
        assert_eq!(cf.reads(), 1);
        let (b, _) = cf.load(&paths);
        assert_eq!(cf.reads(), 1, "a stat, and no read");
        assert_eq!(a, b);
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
        let (f, _) = ContextFiles::default().load(&system(&[&p]));
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
        let paths = system(&[&dir.path().join("GONE.md"), dir.path()]);
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

    /// The system level's files come first, then the persona's, each under a
    /// header naming its level. A file named at both levels is read once and
    /// carried twice, labeled each time; its record says which level named it.
    #[test]
    fn each_section_says_its_level_and_a_file_is_read_once_for_both() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("USER.md"), dir.path().join("PERSONA.md"));
        std::fs::write(&a, "who\n").unwrap();
        std::fs::write(&b, "voice\n").unwrap();
        aged(&a, 60);
        aged(&b, 60);
        let at = |p: &Path, persona: Option<&str>| ContextPath {
            path: p.to_string_lossy().into_owned(),
            persona: persona.map(str::to_string),
            public: false,
        };
        let cf = ContextFiles::default();
        let (f, _) = cf.load(&[
            at(&a, None),
            at(&b, Some("theseus")),
            at(&a, Some("theseus")),
        ]);
        let sections: Vec<&str> = f.iter().map(|f| f.section.as_str()).collect();
        assert_eq!(
            sections,
            [
                format!("# Context file (system): {}\n\nwho", a.display()),
                format!("# Context file (persona theseus): {}\n\nvoice", b.display()),
                format!("# Context file (persona theseus): {}\n\nwho", a.display()),
            ]
        );
        let levels: Vec<Option<&str>> = f.iter().map(|f| f.file.persona.as_deref()).collect();
        assert_eq!(levels, [None, Some("theseus"), Some("theseus")]);
        assert_eq!(f[0].file.digest, f[2].file.digest);
        assert_eq!(cf.reads(), 2, "USER.md is read once");
    }
}
