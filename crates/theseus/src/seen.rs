//! Done until seen (design `stage2` §2.2 and §2.9): which sessions finished
//! since the operator last looked at them. It is the client's, on this
//! machine, never the server's: `$XDG_STATE_HOME/theseus/seen.json`, by
//! default `~/.local/state/theseus/seen.json`. One file per machine, shared by
//! the TUI and the CLI (theseus-yus0): when it is absent, the TUI's older
//! `tui-seen.json` beside it is read, and the new name is written from then on.
//!
//! Every write reads the file again, takes per execution the greatest of each
//! position it holds, and replaces the file atomically, so two clients that
//! write at once lose nothing that matters.
//!
//! A session is done when its level moved from working or needs you to ready
//! or idle, after the TUI last displayed it, and it did not end `cancelled`.
//! The file keeps, for each execution, the position last displayed (the
//! design's field) and the view the TUI last knew of it (its position, level,
//! and turns), so a finish that happened while the TUI was closed is still
//! found: a level that moved, or a turn that ran, between the two runs.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use theseus_protocol::{ExecutionView, Level};

/// The file's format.
const VERSION: u32 = 1;

/// The file's name, and the TUI's older one, read when it is absent.
const FILE_NAME: &str = "seen.json";
const OLD_FILE_NAME: &str = "tui-seen.json";

/// What the TUI knows of one execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// The position of the view last displayed: the session in focus, on a
    /// terminal that had focus.
    pub displayed: u64,
    /// The position at which the TUI last saw it finish; 0 for never.
    pub finished: u64,
    /// The view the TUI last knew: its position, level, and turns.
    pub position: u64,
    pub level: Level,
    pub turns: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    version: u32,
    /// The TUI has read a board: an execution it never knew is new. A file a
    /// CLI made before any TUI ran says false, so a first start still shows
    /// nothing done. A file without the field was the TUI's own: true.
    #[serde(default = "yes")]
    board_read: bool,
    executions: HashMap<String, Mark>,
    /// Executions to drop from the file (the TUI's board no longer holds
    /// them). Never written to disk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    forget: Vec<String>,
}

fn yes() -> bool {
    true
}

/// The file at `path`, or the older one beside it when `path` is absent. A
/// file that does not parse is no file.
fn read_file(path: &Path) -> Option<File> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            std::fs::read_to_string(path.with_file_name(OLD_FILE_NAME)).ok()?
        }
        Err(_) => return None,
    };
    serde_json::from_str::<File>(&text)
        .ok()
        .filter(|f| f.version == VERSION)
}

/// Two marks of one execution, by the greatest position of each: the greater
/// of the displayed and finished positions, and the view (position, level,
/// turns) of the newer.
fn merge_mark(a: Mark, b: Mark) -> Mark {
    let newer = if b.position > a.position { b } else { a };
    Mark {
        displayed: a.displayed.max(b.displayed),
        finished: a.finished.max(b.finished),
        position: newer.position,
        level: newer.level,
        turns: newer.turns,
    }
}

fn merge_into(into: &mut HashMap<String, Mark>, from: impl IntoIterator<Item = (String, Mark)>) {
    for (id, m) in from {
        let merged = into.get(&id).map_or(m, |old| merge_mark(*old, m));
        into.insert(id, merged);
    }
}

/// Hold the machine's seen file against another writer, while `f` reads,
/// merges, and replaces it. The lock is a file beside it; if it can't be
/// taken the write goes on without it (the merge still keeps the greater
/// positions, but a write in the same instant may be lost).
fn locked<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    use std::os::fd::AsRawFd;
    // The first write finds no directory: make it before the lock, or that
    // write goes unlocked.
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_file_name("seen.json.lock"))
        .ok();
    if let Some(l) = &lock {
        // SAFETY: flock on a descriptor this function owns; released on drop.
        unsafe {
            libc::flock(l.as_raw_fd(), libc::LOCK_EX);
        }
    }
    f()
}

/// Replace `path` with `bytes`, whole or not at all: write a temporary file
/// in the same directory, then rename it over. `rename` is a parameter so a
/// test can make it fail.
fn write_atomic(
    path: &Path,
    bytes: &[u8],
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("seen"),
        std::process::id()
    ));
    let done = std::fs::write(&tmp, bytes).and_then(|()| rename(&tmp, path));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// Read the file, change it with `change`, and write it back atomically,
/// under the lock. Returns the file as written.
fn update(
    path: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
    change: impl FnOnce(&mut File),
) -> io::Result<File> {
    locked(path, || {
        let mut file = read_file(path).unwrap_or(File {
            version: VERSION,
            board_read: false,
            executions: HashMap::new(),
            forget: Vec::new(),
        });
        change(&mut file);
        let text = serde_json::to_string(&file).map_err(io::Error::other)?;
        write_atomic(path, text.as_bytes(), rename)?;
        Ok(file)
    })
}

/// Write the TUI's `text` (from [`Seen::text`]) into the file, merged with
/// what is there. Returns the executions as written, for the TUI to take up
/// what another client recorded.
pub fn write_merged(path: &Path, text: &str) -> io::Result<HashMap<String, Mark>> {
    let mine: File = serde_json::from_str(text).map_err(io::Error::other)?;
    let file = update(path, &|a, b| std::fs::rename(a, b), |cur| {
        cur.board_read |= mine.board_read;
        for id in &mine.forget {
            cur.executions.remove(id);
        }
        merge_into(&mut cur.executions, mine.executions);
    })?;
    Ok(file.executions)
}

/// How many executions finished since the operator last looked at them, as the
/// file holds it: a finish after the last display, whose last known level is
/// ready or idle. The file is the TUI's account of the finishes (it is the one
/// that watches), so this is as new as the TUI's last save; with no TUI, 0.
/// For `theseus status`'s ◆, which asks no daemon.
pub fn done_count(path: &Path) -> u32 {
    let Some(file) = read_file(path) else {
        return 0;
    };
    let n = file
        .executions
        .values()
        .filter(|m| matches!(m.level, Level::Ready | Level::Idle) && m.finished > m.displayed)
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Each execution's position last displayed, as the file holds it: for
/// `theseus status`'s ✗, which counts a failure only until a client has shown
/// it (theseus-lweh's review). No file, no marks.
pub fn displayed(path: &Path) -> HashMap<String, u64> {
    read_file(path).map_or_else(HashMap::new, |f| {
        f.executions
            .into_iter()
            .map(|(id, m)| (id, m.displayed))
            .collect()
    })
}

/// Record that the operator was shown `views`: each execution is displayed at
/// its view's position. Another client's write is kept by the merge.
pub fn record(path: &Path, views: &[ExecutionView]) -> io::Result<()> {
    record_with(path, views, &|a, b| std::fs::rename(a, b))
}

fn record_with(
    path: &Path,
    views: &[ExecutionView],
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    update(path, rename, |cur| {
        let marks = views.iter().map(|v| {
            (
                v.execution_id.clone(),
                Mark {
                    displayed: v.position,
                    finished: 0,
                    position: v.position,
                    level: v.attention.level,
                    turns: v.turns,
                },
            )
        });
        merge_into(&mut cur.executions, marks);
    })
    .map(|_| ())
}

#[derive(Debug)]
pub struct Seen {
    /// Where the file is; none keeps it in memory (a test, or no home).
    path: Option<PathBuf>,
    marks: HashMap<String, Mark>,
    /// No file was read: the first board the TUI reads counts as seen, so a
    /// first start shows nothing done.
    fresh: bool,
    /// Changed since the file was last written.
    pub dirty: bool,
}

/// A first start's seen state, kept in memory only.
impl Default for Seen {
    fn default() -> Self {
        Self::open(None)
    }
}

impl Seen {
    /// The seen file at `path`, read if it is there. A file that does not
    /// parse starts fresh, as no file does.
    pub fn open(path: Option<PathBuf>) -> Self {
        let file = path.as_deref().and_then(read_file);
        match file {
            Some(f) => Self {
                path,
                marks: f.executions,
                fresh: !f.board_read,
                dirty: false,
            },
            None => Self {
                path,
                marks: HashMap::new(),
                fresh: true,
                dirty: false,
            },
        }
    }

    /// The default path: `$XDG_STATE_HOME/theseus/seen.json`, else
    /// `~/.local/state/theseus/seen.json`.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))
            })?;
        Some(base.join("theseus").join(FILE_NAME))
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// Done until seen: it finished after it was last displayed, and is
    /// still ready or idle, not cancelled.
    pub fn done(&self, v: &ExecutionView) -> bool {
        if !matches!(v.attention.level, Level::Ready | Level::Idle) || v.state == "cancelled" {
            return false;
        }
        self.marks
            .get(&v.execution_id)
            .is_some_and(|m| m.finished > m.displayed)
    }

    /// A view the board applied. `displayed`: the session is in focus on a
    /// terminal that has focus, so the operator sees this view. Returns true
    /// if it finished here.
    pub fn applied(&mut self, v: &ExecutionView, displayed: bool) -> bool {
        let level = v.attention.level;
        let fresh = self.fresh;
        let mark = self.marks.entry(v.execution_id.clone()).or_insert_with(|| {
            if fresh {
                // The first board of a first start: seen as it is.
                Mark {
                    displayed: v.position,
                    finished: 0,
                    position: v.position,
                    level,
                    turns: v.turns,
                }
            } else {
                // An execution the TUI never knew: created since it last looked.
                Mark {
                    displayed: 0,
                    finished: 0,
                    position: 0,
                    level: Level::Working,
                    turns: 0,
                }
            }
        });
        let at_rest = matches!(level, Level::Ready | Level::Idle);
        let was_busy = matches!(mark.level, Level::Working | Level::NeedsYou);
        let mut finished = false;
        if v.position > mark.position {
            if v.state == "cancelled" {
                mark.finished = 0;
            } else if at_rest && (was_busy || v.turns > mark.turns) && v.turns > 0 {
                mark.finished = v.position;
                finished = true;
            }
            mark.position = v.position;
            mark.level = level;
            mark.turns = v.turns;
        }
        if displayed && mark.displayed < v.position {
            mark.displayed = v.position;
        }
        self.dirty = true;
        finished
    }

    /// The position of the execution's view last displayed; 0 for none.
    pub fn displayed(&self, execution_id: &str) -> u64 {
        self.marks.get(execution_id).map_or(0, |m| m.displayed)
    }

    /// The board was read for the first time: from now on an execution the
    /// TUI never knew is new, not part of a first start's board.
    pub fn first_board_read(&mut self) {
        self.fresh = false;
        self.dirty = true;
    }

    /// What another client recorded, as the file holds it after a write: the
    /// greater positions are taken up.
    pub fn adopt(&mut self, marks: HashMap<String, Mark>) {
        merge_into(&mut self.marks, marks);
    }

    /// The operator looks at this execution now: whatever finished is seen.
    pub fn display(&mut self, v: &ExecutionView) {
        if let Some(m) = self.marks.get_mut(&v.execution_id) {
            if m.displayed < v.position.max(m.finished) {
                m.displayed = v.position.max(m.finished);
                self.dirty = true;
            }
        }
    }

    /// The file's text, keeping only executions the board still holds, so it
    /// does not grow forever.
    pub fn text(&self, keep: &dyn Fn(&str) -> bool) -> String {
        let executions: HashMap<String, Mark> = self
            .marks
            .iter()
            .filter(|(id, _)| keep(id))
            .map(|(id, m)| (id.clone(), *m))
            .collect();
        let forget = self.marks.keys().filter(|id| !keep(id)).cloned().collect();
        serde_json::to_string(&File {
            version: VERSION,
            board_read: !self.fresh,
            executions,
            forget,
        })
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
