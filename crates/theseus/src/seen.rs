//! Done until seen (design `stage2` §2.2 and §2.9): which sessions finished
//! since the operator last looked at them. It is the client's, on this
//! machine, never the server's: `$XDG_STATE_HOME/theseus/tui-seen.json`, by
//! default `~/.local/state/theseus/tui-seen.json`.
//!
//! A session is done when its level moved from working or needs you to ready
//! or idle, after the TUI last displayed it, and it did not end `cancelled`.
//! The file keeps, for each execution, the position last displayed (the
//! design's field) and the view the TUI last knew of it (its position, level,
//! and turns), so a finish that happened while the TUI was closed is still
//! found: a level that moved, or a turn that ran, between the two runs.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use theseus_protocol::{ExecutionView, Level};

/// The file's format.
const VERSION: u32 = 1;

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
    executions: HashMap<String, Mark>,
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
        let file = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str::<File>(&text).ok())
            .filter(|f| f.version == VERSION);
        match file {
            Some(f) => Self {
                path,
                marks: f.executions,
                fresh: false,
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

    /// The default path: `$XDG_STATE_HOME/theseus/tui-seen.json`, else
    /// `~/.local/state/theseus/tui-seen.json`.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))
            })?;
        Some(base.join("theseus").join("tui-seen.json"))
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

    /// The board was read for the first time: from now on an execution the
    /// TUI never knew is new, not part of a first start's board.
    pub fn first_board_read(&mut self) {
        self.fresh = false;
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
        serde_json::to_string(&File {
            version: VERSION,
            executions,
        })
        .unwrap_or_default()
    }
}
