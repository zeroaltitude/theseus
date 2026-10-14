//! Threads on the board (theseus-5i8i): `work.watch`'s views and thread groups,
//! applied by the position rule, and the sidebar's Threads block read from
//! them. Each live thread is a group the operator can fold: its header
//! (`theseus_protocol::GroupedThread::header`, the one wording the CLI shares),
//! then its open tasks, questions and wakes. The groups and the counting rules
//! are the protocol's (`group_threads`); this file only keeps them, and says
//! which line is where.

use std::collections::{BTreeMap, HashMap, HashSet};

use theseus_protocol::work::WorkView;
use theseus_protocol::{group_threads, Level, ThreadGroup, WorkChanged, WorkWatchResult};

/// One line of the Threads block.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub level: Level,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A thread's header: its slug, and whether its items are shown.
    Header { slug: String, open: bool },
    /// A seal recommended and not yet answered.
    Seal,
    /// An open task, question or wake, and the session it lives in.
    Item { session: Option<String> },
}

#[derive(Debug, Default)]
pub struct ThreadBoard {
    views: HashMap<String, WorkView>,
    groups: BTreeMap<String, ThreadGroup>,
    folded: HashSet<String>,
    /// The line under the cursor, while the block has the keys.
    pub cursor: usize,
}

impl ThreadBoard {
    /// A new connection: the first snapshot is the truth, whatever positions
    /// the last daemon had.
    pub fn reset(&mut self) {
        self.views.clear();
        self.groups.clear();
    }

    /// `work.watch`'s snapshot. A view applies only if it is newer than the
    /// one held; what the snapshot does not hold and is not newer than it is
    /// gone.
    pub fn snapshot(&mut self, snap: WorkWatchResult) {
        let ids: HashSet<&str> = snap.views.iter().map(|v| v.id.as_str()).collect();
        self.views
            .retain(|id, v| ids.contains(id.as_str()) || v.position > snap.position);
        for v in snap.views {
            self.apply(v);
        }
        for g in snap.threads {
            self.group(g);
        }
    }

    /// One frame's changes.
    pub fn changed(&mut self, c: WorkChanged) {
        for v in c.views {
            self.apply(v);
        }
        for id in c.gone {
            self.views.remove(&id);
        }
        for g in c.threads {
            self.group(g);
        }
    }

    fn apply(&mut self, v: WorkView) {
        if self
            .views
            .get(&v.id)
            .is_none_or(|old| v.position > old.position)
        {
            self.views.insert(v.id.clone(), v);
        }
    }

    fn group(&mut self, g: ThreadGroup) {
        match self.groups.get_mut(&g.slug) {
            Some(held) => held.merge(g),
            None => {
                self.groups.insert(g.slug.clone(), g);
            }
        }
    }

    /// The Threads block's lines at `width`, needs you first.
    pub fn lines(&self, now_ms: u64, width: usize) -> Vec<Line> {
        let mut views: Vec<WorkView> = self.views.values().cloned().collect();
        views.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.clone()));
        let groups: Vec<ThreadGroup> = self.groups.values().cloned().collect();
        let g = group_threads(&views, &groups);
        let mut out = Vec::new();
        for t in &g.threads {
            let slug = t.group.slug.clone();
            let open = !self.folded.contains(&slug);
            let level = match (t.needs_you, t.working) {
                (0, 0) => Level::Idle,
                (0, _) => Level::Working,
                _ => Level::NeedsYou,
            };
            let count = match (t.needs_you, t.working) {
                (0, 0) => String::new(),
                (0, w) => format!("  ◐ {w}"),
                (n, _) => format!("  ● {n}"),
            };
            let room = width.saturating_sub(2 + count.chars().count());
            out.push(Line {
                text: format!(
                    "{} {}{count}",
                    if open { '▾' } else { '▸' },
                    t.header(now_ms, room)
                ),
                level,
                kind: Kind::Header { slug, open },
            });
            if !open {
                continue;
            }
            if let Some(why) = &t.group.seal_pending {
                out.push(Line {
                    text: format!("  ● seal recommended: {why}"),
                    level: Level::NeedsYou,
                    kind: Kind::Seal,
                });
            }
            for v in &t.items {
                let title = if v.title.is_empty() {
                    &v.attention.label
                } else {
                    &v.title
                };
                out.push(Line {
                    text: format!(
                        "  {} {title}  {}",
                        theseus_client::render::glyph(v.attention.level),
                        v.attention.label
                    ),
                    level: v.attention.level,
                    kind: Kind::Item {
                        session: v.session_id.clone(),
                    },
                });
            }
        }
        out
    }

    /// Whether any thread is on the board.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// What waits on the owner in the threads that the executions' own
    /// count lacks: a seal recommended, in a thread that is not paused.
    pub fn seals_pending(&self) -> usize {
        self.groups
            .values()
            .filter(|g| {
                g.seal_pending.is_some()
                    && g.state != theseus_protocol::threads::ThreadState::Paused
            })
            .count()
    }

    /// Fold or unfold the thread under the cursor's line.
    pub fn toggle(&mut self, now_ms: u64, width: usize) {
        let lines = self.lines(now_ms, width);
        if let Some(Kind::Header { slug, .. }) = lines.get(self.cursor).map(|l| &l.kind) {
            if !self.folded.remove(slug) {
                self.folded.insert(slug.clone());
            }
        }
    }

    /// The session of the line under the cursor, when it is an item.
    pub fn session_at(&self, now_ms: u64, width: usize) -> Option<String> {
        match self.lines(now_ms, width).get(self.cursor)?.kind.clone() {
            Kind::Item { session } => session,
            _ => None,
        }
    }

    /// Move the cursor by `by` lines, kept inside the block.
    pub fn step(&mut self, by: i64, now_ms: u64, width: usize) {
        let n = self.lines(now_ms, width).len() as i64;
        self.cursor = (self.cursor as i64 + by).clamp(0, (n - 1).max(0)) as usize;
    }
}
