//! Context files (theseus-58a; in two levels since theseus-c48): the system
//! level, `[context] files`, which every session gets, then the files of the
//! persona in play, `[personas.<name>] files`. They are compiled into the
//! system block after the persona, the tools note, and `system`, each under a
//! header that names it and its level (spec §4.4).
//!
//! The compiler reads them, not a tool, so no posture or approval applies.
//! Their text is in the system block, so the system digest covers it: an
//! edit reaches the next turn (and starts a new chain inside a turn), and an
//! unchanged file keeps every byte.
//!
//! What a compile carries of the system level's files is the version the
//! owner last accepted of each (`project_context::held`), which this reader
//! reads and compares; the persona's are carried as read.
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

/// A context file as `[context] files` and `[personas.<name>] files` name it:
/// its path, or a table with its path and its readers (since M4 19a). Since
/// 2026-11-22 every place carries every file; `readers` is recorded only.
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

/// Who a context file's entry says may read it: the owner (the default), or
/// anyone. Recorded in the manifest; it withholds nothing since 2026-11-22.
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

/// Bytes read past [`MAX_BYTES`], so a known value that runs across the cap
/// is seen whole and the cut can end before it.
const MARGIN: usize = 4 * 1024;

/// A file changed this shortly before it was read is read again next time.
const RACY: Duration = Duration::from_secs(2);

/// Which part of the request a context file belongs to, and which header it
/// gets (theseus-c48, theseus-wadu). The first two are the layers' own; the
/// last two land in the session's block, block 3 (`project-context` §3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Level {
    /// `[context] files`: every session's, in the project layer.
    #[default]
    System,
    /// `[personas.<name>] files`, in the persona layer.
    Persona(String),
    /// The owner's own user-level instruction file (`project-context` §1).
    User,
    /// A repository's own `AGENTS.md` or `CLAUDE.md`.
    Project,
}

impl Level {
    /// The persona whose file it is; None at every other level. What the
    /// manifest's `ContextFileRef` records, unchanged since theseus-c48.
    pub fn persona(&self) -> Option<String> {
        match self {
            Level::Persona(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The manifest's word for the level (`ContextFileRef.level`).
    pub fn words(&self) -> theseus_protocol::ContextLevel {
        use theseus_protocol::ContextLevel as L;
        match self {
            Level::System => L::System,
            Level::Persona(_) => L::Persona,
            Level::User => L::User,
            Level::Project => L::Project,
        }
    }

    /// The level a stored record's `persona` names, for a reader that has
    /// only the record.
    pub fn of_persona(persona: Option<&str>) -> Self {
        match persona {
            Some(name) => Level::Persona(name.to_string()),
            None => Level::System,
        }
    }
}

/// A context file as the config names it, with its level (theseus-c48).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPath {
    pub path: String,
    /// Which part of the request carries it, and under which header.
    pub level: Level,
    /// Its entry says `readers = "public"`: recorded in the manifest, and
    /// nothing is withheld for its absence (every place carries every file).
    pub public: bool,
}

impl ContextPath {
    /// The persona whose file it is; None at every other level.
    pub fn persona(&self) -> Option<String> {
        self.level.persona()
    }
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
    /// The persona whose file it is; none: the system level's.
    pub persona: Option<String>,
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
    /// The texts of the accepted versions of the owner's own file, checked,
    /// by their record's key and the digest they were read for
    /// (`project_context::own_file`), so each is read from the store once a
    /// daemon run.
    accepted: Mutex<HashMap<String, Arc<crate::project_context::own_file::Kept>>>,
    /// What this daemon run could not confirm of the owner's own file
    /// (`project_context::own_file::Missed`): for the run, never stored.
    own_missed: Mutex<Option<crate::project_context::own_file::Missed>>,
    /// Where the daemon's start step that accepts the instruction files
    /// stands: a turn waits while it runs (`TurnRunner::instructions_ready`).
    adoption: Adoption,
}

/// Where the daemon's start step that accepts the owner's instruction files
/// (`Core::own_file_after_serving`) stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adopting {
    /// Done, or never begun: a core built without the daemon's start.
    Done,
    /// Begun before the daemon serves, and not yet done.
    Pending,
    /// The step ended without finishing (it panicked).
    Unfinished,
}

/// [`Adopting`], with a wait for its end.
struct Adoption(tokio::sync::watch::Sender<Adopting>);

impl Default for Adoption {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(Adopting::Done).0)
    }
}

impl ContextFiles {
    /// The sections for `paths`, in order, and the files this daemon run
    /// finds unreadable for the first time.
    pub fn load(&self, paths: &[ContextPath]) -> (Vec<ContextFile>, Vec<Unreadable>) {
        self.load_warning(paths, true)
    }

    /// The sections for `paths`, as `load` gives them, warning of nothing:
    /// a read that shows them (`context.explain`, theseus-7n3e) leaves the
    /// first warning of an unreadable file to the turn that meets it.
    pub fn peek(&self, paths: &[ContextPath]) -> Vec<ContextFile> {
        self.load_warning(paths, false).0
    }

    fn load_warning(
        &self,
        paths: &[ContextPath],
        warn: bool,
    ) -> (Vec<ContextFile>, Vec<Unreadable>) {
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
                    if warn && self.warned.lock().unwrap().insert(path) {
                        unreadable.push(Unreadable {
                            path: shown.clone(),
                            error: error.clone(),
                            persona: p.persona(),
                        });
                    }
                    Arc::new(missing(&shown, &error))
                }
            };
            files.push(ContextFile {
                file: ContextFileRef {
                    persona: p.persona(),
                    level: p.level.words(),
                    public: p.public,
                    ..held.file.clone()
                },
                section: format!("{}\n\n{}", header(&shown, &p.level), held.body),
            });
        }
        (files, unreadable)
    }

    /// The start step that accepts the instruction files has begun: a turn
    /// waits until [`ContextFiles::adoption_ends`].
    pub(crate) fn adoption_begins(&self) {
        self.adoption.0.send_replace(Adopting::Pending);
    }

    /// The start step has ended, as `how`.
    pub(crate) fn adoption_ends(&self, how: Adopting) {
        self.adoption.0.send_replace(how);
    }

    /// How the start step ended, once it has: at once when it is not
    /// running.
    pub(crate) async fn adopted(&self) -> Adopting {
        let mut rx = self.adoption.0.subscribe();
        rx.wait_for(|a| *a != Adopting::Pending)
            .await
            .map_or(Adopting::Unfinished, |a| *a)
    }

    /// What this daemon run could not confirm of the owner's own file.
    pub(crate) fn own_missed(&self) -> Option<crate::project_context::own_file::Missed> {
        *self.own_missed.lock().unwrap()
    }

    /// Mark what this daemon run could not confirm of the owner's own file;
    /// it stands for the run.
    pub(crate) fn set_own_missed(&self, m: crate::project_context::own_file::Missed) {
        *self.own_missed.lock().unwrap() = Some(m);
    }

    /// The accepted version kept under `key`, read by `read` the first time
    /// it is asked for; a `read` that finds none keeps nothing, and is asked
    /// again.
    pub(crate) fn accepted(
        &self,
        key: &str,
        read: impl FnOnce() -> Option<crate::project_context::own_file::Kept>,
    ) -> Option<Arc<crate::project_context::own_file::Kept>> {
        if let Some(k) = self.accepted.lock().unwrap().get(key) {
            return Some(k.clone());
        }
        let kept = Arc::new(read()?);
        self.accepted
            .lock()
            .unwrap()
            .insert(key.to_string(), kept.clone());
        Some(kept)
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
            .and_then(|f| f.take((MAX_BYTES + MARGIN) as u64).read_to_end(&mut buf))
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

fn reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "not found".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}

/// A file's header names it and its level, so a reader of the block sees
/// which files are every session's, which the persona's, which the owner's
/// own, and which the repository's.
pub(crate) fn header(shown: &str, level: &Level) -> String {
    match level {
        Level::System => format!("# Context file (system): {shown}"),
        Level::Persona(name) => format!("# Context file (persona {name}): {shown}"),
        Level::User => format!("# Context file (user): {shown}"),
        Level::Project => format!("# Context file (project): {shown}"),
    }
}

/// What the reader carries of a file holding `bytes`: its record (digest,
/// size, cut) and the text under its header, as a read of it would give
/// them. The daemon's own write of a context file keeps this
/// (`Core::held_written`), never the file read back.
pub(crate) fn as_read(shown: &str, bytes: &[u8]) -> (ContextFileRef, String) {
    let held = section(shown, bytes[..bytes.len().min(MAX_BYTES + MARGIN)].to_vec());
    (held.file, held.body)
}

/// A file's record and text from up to `MAX_BYTES + MARGIN` bytes of it:
/// past `MAX_BYTES` it is cut, at a character and before any known value.
fn section(shown: &str, buf: Vec<u8>) -> Held {
    let cut = buf.len() > MAX_BYTES;
    let all = String::from_utf8_lossy(&buf);
    let text = if cut {
        // Do not end inside a UTF-8 sequence.
        let mut end = MAX_BYTES;
        if let Err(e) = std::str::from_utf8(&buf[..end]) {
            if e.error_len().is_none() {
                end = e.valid_up_to();
            }
        }
        let plain = String::from_utf8_lossy(&buf[..end]).into_owned();
        // A known value that runs across the cap moves the cut to its start.
        let (known, _) = crate::scrub::safe::head_bytes(&all, plain.len());
        // plain cut: a length to compare with, never kept: the text kept is `known` or `plain`
        let (floor, _) =
            crate::scrub::safe::head_bytes_with(&crate::scrub::Cuts::none(), &all, plain.len());
        if known.len() < floor.len() {
            known.to_string()
        } else {
            plain
        }
    } else {
        all.into_owned()
    };
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
            level: Default::default(),
            public: false,
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
            level: Default::default(),
            public: false,
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
                level: Level::System,
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

    /// A known value that runs across the cap is kept whole or not at all:
    /// the file's section ends before it, and its record counts what is kept.
    #[test]
    fn a_file_cut_at_its_byte_cap_holds_a_value_whole_or_not_at_all() {
        use crate::tests_cut_sites::{whole_or_absent, V};
        let board = crate::secrets::SecretBoard::empty();
        board.hold("cut_test", crate::secrets::Secret::new(V.into()));
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("KEYS.md");
        let text = format!("{}{V}{}", "o".repeat(MAX_BYTES - 10), "o".repeat(100));
        std::fs::write(&p, &text).unwrap();
        let (f, _) = ContextFiles::default().load(&system(&[&p]));
        let f = &f[0];
        assert!(f.file.cut);
        let rest = f.section.replace(&"o".repeat(1_000), "");
        whole_or_absent("the file's section", &rest, V);
        assert_eq!(f.file.bytes, MAX_BYTES as u64 - 10);
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
            level: Level::of_persona(persona),
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
