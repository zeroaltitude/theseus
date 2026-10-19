//! What a command left beside the turn's work (theseus-53ujg.1).
//!
//! At a `proc.run` job's launch the entries of a few directories are listed:
//! the job's working directory and the directories of the files this turn's
//! file tools wrote (at most `MAX_DIRS`; one with more than `MAX_ENTRIES`
//! entries is skipped, never read in part). At the job's first result they
//! are listed again, and the entries that are new and no newer than the job's
//! end are the files the command left: one line after the result's header, and
//! a record the turn's end reads (`turn/end_note_step.rs`). In memory only,
//! keyed by correlation id (a listing waits for its result; a restart between
//! the two leaves no line, never a guess) and by execution (the turn's union,
//! dropped at its end). No frame, no row: a result's line is its text.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// The most directories a launch lists.
pub(crate) const MAX_DIRS: usize = 8;
/// A directory with more entries than this is skipped.
pub(crate) const MAX_ENTRIES: usize = 512;
/// The most names a line gives; the rest are counted.
const NAMED: usize = 8;

type Names = BTreeSet<OsString>;

/// What the turn's commands left, and when its last finished job ended.
#[derive(Debug, Default, Clone)]
pub(crate) struct Facts {
    pub left: Vec<PathBuf>,
    pub last_end_ms: Option<u64>,
}

#[derive(Default)]
struct Book {
    /// A job's directories as they were at its launch.
    before: HashMap<String, Vec<(PathBuf, Names)>>,
    /// The line a job's result said, or that it said none.
    said: HashMap<String, Option<String>>,
    /// Each execution's turn.
    turns: HashMap<String, Facts>,
}

static BOOK: LazyLock<Mutex<Book>> = LazyLock::new(Mutex::default);

/// The entries of `dir`, or none when it cannot be read whole.
fn list(dir: &Path) -> Option<Names> {
    let mut names = Names::new();
    for e in std::fs::read_dir(dir).ok()? {
        names.insert(e.ok()?.file_name());
        if names.len() > MAX_ENTRIES {
            return None;
        }
    }
    Some(names)
}

/// The directories a job lists: its own, then those of the turn's written
/// files, each once, at most `MAX_DIRS`.
fn dirs_of(cwd: &Path, written: &[crate::rewind::Written]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![cwd.to_path_buf()];
    for w in written {
        if let Some(d) = w.path.parent().filter(|d| !dirs.iter().any(|x| x == d)) {
            dirs.push(d.to_path_buf());
        }
    }
    dirs.truncate(MAX_DIRS);
    dirs
}

/// List the directories a job at `cwd` will work beside, before it starts.
pub(crate) fn before(execution_id: &str, correlation_id: &str, cwd: &Path) {
    let dirs = dirs_of(cwd, &crate::rewind::written(execution_id));
    let listed: Vec<_> = theseus_store::blocking(|| {
        dirs.into_iter()
            .filter_map(|d| list(&d).map(|n| (d, n)))
            .collect()
    });
    let mut b = BOOK.lock().unwrap();
    // A batch's next step launches under the same id: its own listing, its
    // own line.
    b.said.remove(correlation_id);
    b.before.insert(correlation_id.into(), listed);
}

/// A turn began: its commands' facts gather from here until `forget_turn`.
/// A job that outlives its turn (a batch's later step, a late result) adds to
/// nothing, so no turn's entry outlives it.
pub(crate) fn begin_turn(execution_id: &str) {
    BOOK.lock()
        .unwrap()
        .turns
        .entry(execution_id.into())
        .or_default();
}

/// A job's output was removed: its listing and its line go.
pub(crate) fn forget(correlation_id: &str) {
    let mut b = BOOK.lock().unwrap();
    b.before.remove(correlation_id);
    b.said.remove(correlation_id);
}

/// What the turn of `execution_id` has had its commands leave.
pub(crate) fn facts(execution_id: &str) -> Facts {
    BOOK.lock()
        .unwrap()
        .turns
        .get(execution_id)
        .cloned()
        .unwrap_or_default()
}

/// A test's stand-in for commands that left `left` and ended at `last_end_ms`.
#[cfg(test)]
pub(crate) fn seed(execution_id: &str, left: Vec<PathBuf>, last_end_ms: u64) {
    let mut b = BOOK.lock().unwrap();
    let turn = b.turns.entry(execution_id.into()).or_default();
    turn.left = left;
    turn.last_end_ms = Some(last_end_ms);
}

/// A turn ended: drop what its commands left.
pub(crate) fn forget_turn(execution_id: &str) {
    BOOK.lock().unwrap().turns.remove(execution_id);
}

/// The `[new files: …]` line of a job's result, `finished_ms` its
/// completion's end. The first call lists again and says; every later call
/// says the same. No completion, or no listing from the launch: no line.
pub(crate) fn line(execution_id: &str, correlation_id: &str, finished_ms: Option<u64>) -> String {
    let said = |b: &Book| b.said.get(correlation_id).cloned();
    if let Some(s) = said(&BOOK.lock().unwrap()) {
        return s.unwrap_or_default();
    }
    let Some(finished) = finished_ms else {
        return String::new();
    };
    let Some(was) = BOOK.lock().unwrap().before.remove(correlation_id) else {
        return String::new();
    };
    let found = theseus_store::blocking(|| new_since(&was, finished));
    let said_now = (!found.is_empty()).then(|| render(&found));
    let mut b = BOOK.lock().unwrap();
    // Two builds of one result at once: the first's line stands.
    if let Some(s) = said(&b) {
        return s.unwrap_or_default();
    }
    if let Some(turn) = b.turns.get_mut(execution_id) {
        turn.last_end_ms = turn.last_end_ms.max(Some(finished));
        for f in &found {
            if !turn.left.contains(&f.path) {
                turn.left.push(f.path.clone());
            }
        }
    }
    b.said.insert(correlation_id.into(), said_now.clone());
    said_now.unwrap_or_default()
}

struct Found {
    path: PathBuf,
    dir: bool,
    bytes: u64,
    mtime_ms: u64,
}

/// The entries now present that `was` lacks, no newer than `finished`, and
/// not ignored by the work tree's `.gitignore`, in the order they were made.
fn new_since(was: &[(PathBuf, Names)], finished: u64) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for (dir, before) in was {
        let Some(now) = list(dir) else { continue };
        let ignore = Ignore::of(dir);
        for name in now.difference(before) {
            let path = dir.join(name);
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            if mtime_ms > finished || ignore.skips(&path, meta.is_dir()) {
                continue;
            }
            if found.iter().any(|f| f.path == path) {
                continue;
            }
            found.push(Found {
                dir: meta.is_dir(),
                bytes: meta.len(),
                path,
                mtime_ms,
            });
        }
    }
    found.sort_by(|a, b| (a.mtime_ms, &a.path).cmp(&(b.mtime_ms, &b.path)));
    found
}

/// `412 KB`: whole kilobytes, then megabytes.
fn sized(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{} KB", n.div_ceil(1024)),
        _ => format!("{} MB", n.div_ceil(1_048_576)),
    }
}

fn render(found: &[Found]) -> String {
    let named: Vec<String> = found
        .iter()
        .take(NAMED)
        .map(|f| match f.dir {
            true => format!("{}/", f.path.display()),
            false => format!("{} ({})", f.path.display(), sized(f.bytes)),
        })
        .collect();
    let more = match found.len().saturating_sub(NAMED) {
        0 => String::new(),
        n => format!(" and {n} more"),
    };
    format!("[new files: {}{more}]\n", named.join(", "))
}

/// The `.gitignore`s and exclude file of the work tree holding a directory.
struct Ignore(Option<(ignore::gitignore::Gitignore, PathBuf)>);

impl Ignore {
    /// None outside a git work tree.
    fn of(dir: &Path) -> Self {
        let Some(top) = dir.ancestors().find(|a| a.join(".git").exists()) else {
            return Self(None);
        };
        let mut b = ignore::gitignore::GitignoreBuilder::new(top);
        // The tree's own, top down, then the directory's ancestors'.
        b.add(top.join(".git/info/exclude"));
        let mut chain: Vec<&Path> = dir.ancestors().take_while(|a| a.starts_with(top)).collect();
        chain.reverse();
        for d in chain {
            b.add(d.join(".gitignore"));
        }
        Self(b.build().ok().map(|g| (g, top.to_path_buf())))
    }

    fn skips(&self, path: &Path, is_dir: bool) -> bool {
        self.0.as_ref().is_some_and(|(g, top)| {
            path.starts_with(top) && g.matched_path_or_any_parents(path, is_dir).is_ignore()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_ms() -> u64 {
        theseus_protocol::now_unix_ms()
    }

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// Make `name` the `ago`th-from-last thing made, whatever the clock's grain.
    fn aged(dir: &Path, name: &str, ms_ago: u64) {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(now_ms() - ms_ago);
        let f = std::fs::File::options()
            .read(true)
            .open(dir.join(name))
            .unwrap();
        f.set_modified(at).unwrap();
    }

    /// One command's line: list, let `make` run, list again.
    fn said(d: &Path, id: &str, make: impl FnOnce()) -> String {
        before("exec_b", id, d);
        make();
        line("exec_b", id, Some(now_ms() + 1_000))
    }

    #[test]
    fn a_commands_result_names_its_new_files() {
        let d = dir();
        std::fs::write(d.path().join("old.txt"), "x").unwrap();
        let p = d.path().to_path_buf();
        let id = "corr_two_files";
        let l = said(&p, id, || {
            std::fs::write(p.join("main"), vec![0u8; 412 * 1024]).unwrap();
            std::fs::write(p.join("cmain"), vec![0u8; 16 * 1024]).unwrap();
            std::fs::create_dir(p.join("out")).unwrap();
            aged(&p, "main", 3_000);
            aged(&p, "cmain", 2_000);
            aged(&p, "out", 1_000);
        });
        let at = |n: &str| format!("{}/{n}", p.display());
        assert_eq!(
            l,
            format!(
                "[new files: {} (412 KB), {} (16 KB), {}/]\n",
                at("main"),
                at("cmain"),
                at("out")
            )
        );
        // A second build of the same result says the same.
        assert_eq!(line("exec_b", id, Some(now_ms() + 1_000)), l);
        // One that created none says nothing.
        assert_eq!(said(&p, "corr_none", || {}), "");
        forget(id);
    }

    #[test]
    fn a_line_names_eight_and_counts_the_rest() {
        let d = dir();
        let p = d.path().to_path_buf();
        let l = said(&p, "corr_many", || {
            for i in 0..11 {
                std::fs::write(p.join(format!("f{i:02}")), "x").unwrap();
            }
        });
        assert!(l.ends_with(" and 3 more]\n"), "{l}");
        assert_eq!(l.matches(" (1 B)").count(), 8);
    }

    #[test]
    fn gitignored_outputs_are_not_named() {
        let d = dir();
        let p = d.path().to_path_buf();
        std::fs::create_dir(p.join(".git")).unwrap();
        std::fs::write(p.join(".gitignore"), "/main\n*.o\n").unwrap();
        let l = said(&p, "corr_ignored", || {
            for n in ["main", "x.o", "kept"] {
                std::fs::write(p.join(n), "x").unwrap();
            }
        });
        assert_eq!(l, format!("[new files: {}/kept (1 B)]\n", p.display()));
    }

    #[test]
    fn a_big_directory_is_skipped_not_read_in_part() {
        let d = dir();
        let p = d.path().to_path_buf();
        for i in 0..=MAX_ENTRIES {
            std::fs::write(p.join(format!("e{i}")), "").unwrap();
        }
        assert_eq!(std::fs::read_dir(&p).unwrap().count(), MAX_ENTRIES + 1);
        assert_eq!(
            said(&p, "corr_big", || std::fs::write(p.join("new"), "x")
                .unwrap()),
            ""
        );
        // The same directory, one entry fewer, is read.
        for gone in ["e0", "e1", "new"] {
            std::fs::remove_file(p.join(gone)).unwrap();
        }
        let l = said(&p, "corr_edge", || {
            std::fs::write(p.join("new"), "x").unwrap()
        });
        assert!(l.contains("/new (1 B)"), "{l}");
    }

    #[test]
    fn no_completion_or_no_listing_says_nothing() {
        let d = dir();
        before("exec_b", "corr_nocompletion", d.path());
        assert_eq!(line("exec_b", "corr_nocompletion", None), "");
        // A restart in between: a result with no launch listing.
        assert_eq!(line("exec_b", "corr_unlaunched", Some(now_ms())), "");
    }

    #[test]
    fn a_job_outliving_its_turn_adds_to_no_turn() {
        let d = dir();
        let p = d.path().to_path_buf();
        // The turn is over (or never began): its command still gets its line.
        before("exec_gone", "corr_gone", &p);
        std::fs::write(p.join("out"), "x").unwrap();
        aged(&p, "out", 1_000);
        assert!(line("exec_gone", "corr_gone", Some(now_ms())).starts_with("[new files: "));
        assert!(!BOOK.lock().unwrap().turns.contains_key("exec_gone"));
        // A live turn gathers it, and its end drops it.
        begin_turn("exec_live");
        before("exec_live", "corr_live", &p);
        std::fs::write(p.join("out2"), "x").unwrap();
        aged(&p, "out2", 1_000);
        line("exec_live", "corr_live", Some(now_ms()));
        assert_eq!(facts("exec_live").left, vec![p.join("out2")]);
        forget_turn("exec_live");
        assert!(!BOOK.lock().unwrap().turns.contains_key("exec_live"));
        forget("corr_gone");
        forget("corr_live");
    }

    #[test]
    fn a_file_newer_than_the_commands_end_is_not_its() {
        let d = dir();
        let p = d.path().to_path_buf();
        before("exec_b", "corr_late", &p);
        std::fs::write(p.join("later"), "x").unwrap();
        assert_eq!(
            line("exec_b", "corr_late", Some(now_ms().saturating_sub(60_000))),
            ""
        );
    }
}
