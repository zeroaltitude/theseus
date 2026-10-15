//! The follower's memory (theseus-fuw5d): the tasks the work board holds and the tab
//! each has in herdr, and the rules for which tasks want a tab and when a tab closes.
//! Pure: no clock of its own (every method takes `now`), no socket, so the tests run
//! it with made-up instants.
//!
//! A task wants a tab while it needs you, works, waits, or failed, from the moment it
//! ends until its tab closes; a conversation only while it needs you; a pinned
//! session always. A cancelled task's tab closes at once. A finished one's closes
//! `grace` after it was seen (its tab or pane focused). A task already finished
//! when the follower first reads the board gets a tab only if it ended within the
//! grace, so an old board does not become a wall of tabs.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use tokio::time::Instant;

use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::Level;

/// How soon after a tab opened the same session's tab may open again: a watch that
/// dies at once (its session unknown) must not be started in a loop.
pub const REOPEN_AFTER: Duration = Duration::from_secs(5);

/// A tab in herdr, and its one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub tab_id: String,
    pub pane_id: String,
}

/// One session the follower tracks.
#[derive(Debug, Clone)]
pub struct Task {
    pub sid: String,
    pub kind: WorkKind,
    pub name: String,
    pub state: WorkState,
    pub level: Level,
    pub spent: f64,
    pub pinned: bool,
    pub tab: Option<Tab>,
    /// When the owner first looked at it after it ended.
    pub seen_at: Option<Instant>,
    pub opened_at: Option<Instant>,
    /// The board's last view of it, for the board tab's lines.
    pub last: Option<WorkView>,
    /// It ended while the follower watched, or recently enough to still be news.
    pub news: bool,
}

impl Task {
    fn ended(&self) -> bool {
        self.state.ended()
    }

    /// Whether a tab should exist for it.
    pub fn wants_tab(&self) -> bool {
        if self.pinned {
            return true;
        }
        match self.kind {
            WorkKind::Conversation => self.state == WorkState::NeedsYou,
            _ => match self.state {
                WorkState::Cancelled | WorkState::Expired | WorkState::Unknown => false,
                s if s.ended() => self.news,
                _ => true,
            },
        }
    }

    /// Whether its tab is due to close at `now`.
    fn close_due(&self, now: Instant, grace: Duration) -> bool {
        if self.pinned {
            return false;
        }
        if self.kind == WorkKind::Conversation {
            return self.state != WorkState::NeedsYou;
        }
        match self.state {
            WorkState::Cancelled | WorkState::Expired => true,
            s if s.ended() => self.seen_at.is_some_and(|t| now >= t + grace),
            _ => false,
        }
    }
}

/// What the follower asks of herdr.
#[derive(Debug, PartialEq, Eq)]
pub enum Act {
    /// Tabs to make, one `layout.apply`.
    Open(Vec<String>),
    /// A tab to close.
    Close { tab_id: String, sid: String },
}

/// The counts on the Space row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Counts {
    pub needs: usize,
    pub working: usize,
    pub done: usize,
    pub spend_cents: u64,
}

#[derive(Debug)]
pub struct Follower {
    pub tasks: BTreeMap<String, Task>,
    positions: HashMap<String, u64>,
    pub grace: Duration,
    /// The tab (by id) the owner is looking at.
    focused: Option<String>,
}

/// What a view changes in the tracked tasks, for the board's log.
pub struct Applied {
    pub changed: Vec<WorkView>,
}

impl Follower {
    pub fn new(grace: Duration) -> Self {
        Self {
            tasks: BTreeMap::new(),
            positions: HashMap::new(),
            grace,
            focused: None,
        }
    }

    /// Apply a snapshot or a change. A view at or below the last position for its id
    /// is old (the position rule). `snapshot` marks a first read: its ended tasks are
    /// news only if recent.
    pub fn apply(
        &mut self,
        views: &[WorkView],
        gone: &[String],
        snapshot: bool,
        now: Instant,
        now_ms: u64,
    ) -> Applied {
        let mut changed = Vec::new();
        for v in views {
            if !matches!(v.kind, WorkKind::Task | WorkKind::Conversation) {
                continue;
            }
            let Some(sid) = v.session_id.clone() else {
                continue;
            };
            let last = self.positions.entry(v.id.clone()).or_default();
            if v.position <= *last && *last > 0 {
                continue;
            }
            *last = v.position;
            let recent = v
                .ended_at_ms
                .is_none_or(|e| now_ms.saturating_sub(e) <= self.grace.as_millis() as u64);
            let focused = self.focused_sid().is_some_and(|f| f == sid);
            let existed = self.tasks.contains_key(&sid);
            let t = self.tasks.entry(sid.clone()).or_insert_with(|| Task {
                sid: sid.clone(),
                kind: v.kind,
                name: String::new(),
                state: v.state,
                level: v.attention.level,
                spent: 0.0,
                pinned: false,
                tab: None,
                seen_at: None,
                opened_at: None,
                last: None,
                news: false,
            });
            let was_ended = existed && t.ended();
            t.kind = v.kind;
            t.state = v.state;
            t.level = v.attention.level;
            if !v.title.is_empty() {
                t.name.clone_from(&v.title);
            }
            if let Some(c) = &v.cost {
                t.spent = c.spent_usd;
            }
            if t.ended() && !was_ended {
                t.news = !snapshot || recent;
                if focused {
                    t.seen_at = Some(now);
                }
            }
            if !t.ended() {
                t.seen_at = None;
                t.news = false;
            }
            t.last = Some(v.clone());
            changed.push(v.clone());
        }
        for id in gone {
            self.positions.remove(id);
        }
        Applied { changed }
    }

    /// Keep a tab for this session whatever its state.
    pub fn pin(&mut self, sid: &str, kind: WorkKind, name: String) {
        let t = self.tasks.entry(sid.to_string()).or_insert_with(|| Task {
            sid: sid.to_string(),
            kind,
            name,
            state: WorkState::Ready,
            level: Level::Ready,
            spent: 0.0,
            pinned: true,
            tab: None,
            seen_at: None,
            opened_at: None,
            last: None,
            news: false,
        });
        t.pinned = true;
    }

    /// A snapshot lists every root that needs you or works with its open subtree: a
    /// task it no longer lists, that was not ended, ended while the follower was away.
    pub fn forget_missing(&mut self, views: &[WorkView]) {
        let listed: HashSet<&str> = views
            .iter()
            .filter_map(|v| v.session_id.as_deref())
            .collect();
        for t in self.tasks.values_mut() {
            if !t.ended() && t.state != WorkState::Ready && !listed.contains(t.sid.as_str()) {
                t.state = WorkState::Done;
                t.level = Level::Idle;
                t.news = true;
            }
        }
    }

    fn focused_sid(&self) -> Option<&str> {
        let f = self.focused.as_deref()?;
        self.tasks
            .values()
            .find(|t| {
                t.tab
                    .as_ref()
                    .is_some_and(|b| b.tab_id == f || b.pane_id == f)
            })
            .map(|t| t.sid.as_str())
    }

    /// herdr's own view of our tabs, from `pane.list`: each pane with a `kind` and a
    /// `session` token is that session's tab. A pane with a `pinned` token pins its
    /// session. A session the board does not hold (its tab outlived a follower) is
    /// taken as ended and seen, so its tab closes after the grace. A task whose tab
    /// is gone loses it.
    pub fn set_panes(
        &mut self,
        panes: &[PaneRow],
        names: &HashMap<String, (WorkKind, String)>,
        now: Instant,
    ) {
        let mut found: HashSet<&str> = HashSet::new();
        for p in panes {
            let (Some(sid), Some(kind)) = (p.session.as_deref(), p.kind.as_deref()) else {
                continue;
            };
            if !matches!(kind, "task" | "conversation") {
                continue;
            }
            found.insert(sid);
            if p.focused {
                self.focused = Some(p.tab_id.clone());
            }
            let t = self.tasks.entry(sid.to_string()).or_insert_with(|| {
                let (kind, name) = names.get(sid).cloned().unwrap_or_else(|| {
                    let k = if kind == "task" {
                        WorkKind::Task
                    } else {
                        WorkKind::Conversation
                    };
                    (k, sid.to_string())
                });
                let ended = kind == WorkKind::Task;
                Task {
                    sid: sid.to_string(),
                    kind,
                    name,
                    state: if ended {
                        WorkState::Done
                    } else {
                        WorkState::Ready
                    },
                    level: Level::Idle,
                    spent: 0.0,
                    pinned: false,
                    tab: None,
                    seen_at: ended.then_some(now),
                    opened_at: None,
                    last: None,
                    news: false,
                }
            });
            t.pinned |= p.pinned;
            t.tab = Some(Tab {
                tab_id: p.tab_id.clone(),
                pane_id: p.pane_id.clone(),
            });
        }
        for t in self.tasks.values_mut() {
            if !found.contains(t.sid.as_str()) {
                t.tab = None;
            }
        }
    }

    /// The owner's focus moved to a tab or a pane (herdr's id for either): a task that
    /// ended and is looked at is seen.
    pub fn focus(&mut self, id: &str, now: Instant) {
        self.focused = Some(id.to_string());
        for t in self.tasks.values_mut() {
            let here = t
                .tab
                .as_ref()
                .is_some_and(|b| b.tab_id == id || b.pane_id == id);
            if here && t.ended() && t.seen_at.is_none() {
                t.seen_at = Some(now);
            }
        }
    }

    /// A pane exited: its tab is gone from herdr. Whether it is made again is the
    /// plan's: a task that still wants a tab gets one.
    pub fn pane_exited(&mut self, pane_id: &str) -> bool {
        let mut hit = false;
        for t in self.tasks.values_mut() {
            if t.tab.as_ref().is_some_and(|b| b.pane_id == pane_id) {
                t.tab = None;
                hit = true;
            }
        }
        hit
    }

    /// A tab was made.
    pub fn opened(&mut self, sid: &str, tab: Tab, now: Instant) {
        if let Some(t) = self.tasks.get_mut(sid) {
            t.tab = Some(tab);
            t.opened_at = Some(now);
        }
    }

    /// A tab was closed.
    pub fn closed(&mut self, sid: &str) {
        if let Some(t) = self.tasks.get_mut(sid) {
            t.tab = None;
        }
        let drop = self
            .tasks
            .get(sid)
            .is_some_and(|t| !t.pinned && t.ended() && t.tab.is_none() && !t.wants_tab_news());
        if drop {
            self.tasks.remove(sid);
        }
    }

    /// What herdr should do now: open the tabs wanted and missing, close those due.
    pub fn plan(&self, now: Instant) -> Vec<Act> {
        let mut open = Vec::new();
        let mut acts = Vec::new();
        for t in self.tasks.values() {
            match &t.tab {
                None if t.wants_tab() && may_open(t, now) => open.push(t.sid.clone()),
                Some(b) if t.close_due(now, self.grace) => acts.push(Act::Close {
                    tab_id: b.tab_id.clone(),
                    sid: t.sid.clone(),
                }),
                _ => {}
            }
        }
        if !open.is_empty() {
            acts.insert(0, Act::Open(open));
        }
        acts
    }

    /// The next instant the plan changes by the clock alone: a seen task's close, a
    /// reopen held back. None while nothing waits on one.
    pub fn deadline(&self) -> Option<Instant> {
        self.tasks
            .values()
            .filter_map(|t| {
                if t.tab.is_some() && t.ended() && !t.pinned && t.kind == WorkKind::Task {
                    t.seen_at.map(|s| s + self.grace)
                } else if t.tab.is_none() && t.wants_tab() {
                    t.opened_at.map(|o| o + REOPEN_AFTER)
                } else {
                    None
                }
            })
            .min()
    }

    /// The Space row's counts.
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        let mut spend = 0.0;
        for t in self.tasks.values() {
            spend += t.spent;
            match t.level {
                Level::NeedsYou => c.needs += 1,
                Level::Working => c.working += 1,
                _ if t.ended() && t.news && t.seen_at.is_none() => c.done += 1,
                _ => {}
            }
        }
        c.spend_cents = (spend * 100.0).round() as u64;
        c
    }
}

/// Whether a tab may be opened for it yet.
fn may_open(t: &Task, now: Instant) -> bool {
    t.opened_at.is_none_or(|o| now >= o + REOPEN_AFTER)
}

impl Task {
    /// An ended task that is still news keeps its entry.
    fn wants_tab_news(&self) -> bool {
        self.news && self.seen_at.is_none()
    }
}

/// A pane as `pane.list` gives it, the fields the follower reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    pub pane_id: String,
    pub tab_id: String,
    pub session: Option<String>,
    pub kind: Option<String>,
    pub pinned: bool,
    pub focused: bool,
}

impl PaneRow {
    pub fn from_value(p: &serde_json::Value) -> Option<Self> {
        let s = |v: &serde_json::Value| v.as_str().map(str::to_string);
        Some(Self {
            pane_id: s(&p["pane_id"])?,
            tab_id: s(&p["tab_id"]).unwrap_or_default(),
            session: s(&p["tokens"]["session"]),
            kind: s(&p["tokens"]["kind"]),
            pinned: p["tokens"]["pinned"].is_string(),
            focused: p["focused"].as_bool().unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(id: &str, sid: &str, kind: &str, state: &str, level: &str, pos: u64) -> WorkView {
        serde_json::from_value(json!({
            "id": id, "kind": kind, "position": pos, "at_ms": pos, "root": id,
            "session_id": sid, "title": format!("title of {sid}"), "state": state,
            "attention": {"level": level, "label": state, "since_ms": 1},
            "started_at_ms": 1, "state_since_ms": 1
        }))
        .unwrap()
    }

    fn tab(n: u32) -> Tab {
        Tab {
            tab_id: format!("w1:t{n}"),
            pane_id: format!("w1:p{n}"),
        }
    }

    const G: Duration = Duration::from_secs(600);

    /// A working task wants a tab, a conversation does only while it needs you.
    #[test]
    fn working_tasks_and_asking_conversations_want_tabs() {
        let now = Instant::now();
        let mut f = Follower::new(G);
        f.apply(
            &[
                view("exe_1", "ses_t1", "task", "running", "working", 1),
                view("exe_2", "ses_c1", "conversation", "running", "working", 2),
                view(
                    "exe_3",
                    "ses_c2",
                    "conversation",
                    "needs_you",
                    "needs_you",
                    3,
                ),
            ],
            &[],
            true,
            now,
            0,
        );
        assert_eq!(
            f.plan(now),
            [Act::Open(vec!["ses_c2".into(), "ses_t1".into()])]
        );
    }

    /// A finished task closes `grace` after it was seen, and not before; with nothing
    /// to wait for there is no deadline.
    #[test]
    fn a_finished_task_closes_a_grace_after_it_was_seen() {
        let t0 = Instant::now();
        let mut f = Follower::new(G);
        f.apply(
            &[view("exe_1", "ses_t1", "task", "running", "working", 1)],
            &[],
            false,
            t0,
            0,
        );
        f.opened("ses_t1", tab(1), t0);
        assert_eq!(f.deadline(), None);
        f.apply(
            &[view("exe_1", "ses_t1", "task", "done", "idle", 2)],
            &[],
            false,
            t0,
            0,
        );
        assert!(f.plan(t0 + G * 3).is_empty(), "unseen: it stays");
        assert_eq!(f.deadline(), None);
        f.focus("w1:t1", t0 + Duration::from_secs(60));
        let due = t0 + Duration::from_secs(60) + G;
        assert_eq!(f.deadline(), Some(due));
        assert!(f.plan(due - Duration::from_secs(1)).is_empty());
        assert_eq!(
            f.plan(due),
            [Act::Close {
                tab_id: "w1:t1".into(),
                sid: "ses_t1".into()
            }]
        );
    }

    /// A cancelled task's tab closes at once, seen or not.
    #[test]
    fn a_cancelled_task_closes_at_once() {
        let now = Instant::now();
        let mut f = Follower::new(G);
        f.apply(
            &[view("exe_1", "ses_t1", "task", "running", "working", 1)],
            &[],
            false,
            now,
            0,
        );
        f.opened("ses_t1", tab(1), now);
        f.apply(
            &[view("exe_1", "ses_t1", "task", "cancelled", "idle", 2)],
            &[],
            false,
            now,
            0,
        );
        assert_eq!(f.plan(now).len(), 1);
    }

    /// Old views change nothing; a task finished long before the first read gets no tab.
    #[test]
    fn old_views_and_old_endings_are_not_news() {
        let now = Instant::now();
        let mut f = Follower::new(G);
        let mut old = view("exe_1", "ses_t1", "task", "done", "idle", 5);
        old.ended_at_ms = Some(1_000);
        f.apply(&[old], &[], true, now, 100_000_000);
        assert!(f.plan(now).is_empty());
        let mut f = Follower::new(G);
        f.apply(
            &[view("exe_1", "ses_t1", "task", "running", "working", 5)],
            &[],
            true,
            now,
            0,
        );
        f.apply(
            &[view("exe_1", "ses_t1", "task", "done", "idle", 4)],
            &[],
            false,
            now,
            0,
        );
        assert_eq!(f.tasks["ses_t1"].state, WorkState::Running, "position rule");
    }

    /// A pane that exited makes its tab again if the task still wants one, but not
    /// inside the reopen hold.
    #[test]
    fn an_exited_pane_is_rebuilt_after_the_hold() {
        let t0 = Instant::now();
        let mut f = Follower::new(G);
        f.apply(
            &[view("exe_1", "ses_t1", "task", "running", "working", 1)],
            &[],
            false,
            t0,
            0,
        );
        f.opened("ses_t1", tab(1), t0);
        assert!(f.pane_exited("w1:p1"));
        assert!(f.plan(t0 + Duration::from_secs(1)).is_empty());
        assert_eq!(f.deadline(), Some(t0 + REOPEN_AFTER));
        assert_eq!(
            f.plan(t0 + REOPEN_AFTER),
            [Act::Open(vec!["ses_t1".into()])]
        );
    }

    /// The Space row's counts.
    #[test]
    fn counts_for_the_space_row() {
        let now = Instant::now();
        let mut f = Follower::new(G);
        let mut w = view("exe_1", "ses_t1", "task", "running", "working", 1);
        w.cost = Some(theseus_protocol::work::Cost {
            spent_usd: 1.234,
            limit_usd: 5.0,
            turn_usd: None,
        });
        f.apply(
            &[
                w,
                view("exe_2", "ses_t2", "task", "needs_you", "needs_you", 1),
                view("exe_3", "ses_t3", "task", "running", "working", 1),
            ],
            &[],
            false,
            now,
            0,
        );
        f.apply(
            &[view("exe_3", "ses_t3", "task", "done", "idle", 2)],
            &[],
            false,
            now,
            0,
        );
        assert_eq!(
            f.counts(),
            Counts {
                needs: 1,
                working: 1,
                done: 1,
                spend_cents: 123
            }
        );
    }
}
