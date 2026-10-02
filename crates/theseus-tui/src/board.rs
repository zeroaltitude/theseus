//! The board (design `stage2` §2.4): every session the TUI knows, as
//! `executions.watch`'s snapshot and its events tell it, applied by the
//! position rule; each session's title, from `session.list`; and the questions
//! waiting for the operator. The sidebar's rows and its trees are read from
//! it.

use std::collections::{HashMap, HashSet, VecDeque};

use theseus_protocol::{
    Attention, ConfirmRequest, ConfirmResolved, ExecutionView, ExecutionsWatchResult, Level,
    SessionInfo, SessionKind,
};

/// How many resolved questions the board remembers, so a `confirm.requested`
/// that arrives after its own `confirm.resolved` is not shown again.
const RESOLVED_KEPT: usize = 256;

/// The deepest tree the sidebar draws. Depth is one today (DD7); the view
/// does not assume it, and a loop in the parents cannot hang it.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Default)]
pub struct Board {
    /// Each session's execution, as its latest view (one execution per
    /// session, spec §3.2a).
    views: HashMap<String, ExecutionView>,
    /// Titles, kinds, and models, from `session.list`.
    sessions: HashMap<String, SessionInfo>,
    /// Sessions whose title was asked for, so each is asked once.
    asked: HashSet<String>,
    /// The questions waiting, whole, by correlation id.
    confirms: HashMap<String, ConfirmRequest>,
    resolved: VecDeque<String>,
    /// The order in which sessions were first seen: a task with no creation
    /// time sorts by it.
    seen_order: HashMap<String, u64>,
    next_seen: u64,
    /// Since a connect, until its first snapshot: the sessions an event has
    /// updated on the new connection. That snapshot is the truth for every
    /// other session, even at a lower position (a daemon on another store).
    reconnected: Option<HashSet<String>>,
}

/// One row of the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub session_id: String,
    /// 0 for a top-level session; a task sits one deeper than its parent.
    pub depth: usize,
    /// The last child of its parent (`└`), else `├`.
    pub last: bool,
    pub level: Level,
    /// Done until seen (the client's rule, design §2.2).
    pub done: bool,
    /// What it needs, as the sidebar says it: `confirm proc.run`, `turn 2`.
    pub label: String,
    pub name: String,
    pub spent_usd: f64,
    /// Idle tasks folded under this row.
    pub folded: usize,
}

/// Which sessions the sidebar shows: all of them as a tree, or only one level
/// (herdr's b/w/i/d), as a flat list in queue order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Only {
    #[default]
    All,
    NeedsYou,
    Working,
    Ready,
    Done,
}

impl Only {
    pub fn word(self) -> &'static str {
        match self {
            Only::All => "all",
            Only::NeedsYou => "needs you",
            Only::Working => "working",
            Only::Ready => "ready",
            Only::Done => "done",
        }
    }
}

impl Board {
    /// Apply a view by the position rule (design §2.4): only if its position
    /// is greater than the last one applied for its session's execution.
    /// True if it was applied.
    pub fn apply(&mut self, v: ExecutionView) -> bool {
        if self
            .views
            .get(&v.session_id)
            .is_some_and(|old| v.position <= old.position)
        {
            return false;
        }
        if let Some(touched) = self.reconnected.as_mut() {
            touched.insert(v.session_id.clone());
        }
        self.put(v);
        true
    }

    fn put(&mut self, v: ExecutionView) {
        self.note_seen(&v.session_id);
        // A question the newer view no longer lists was answered or closed,
        // unless it was asked after this view (its event came first).
        self.confirms.retain(|_, c| {
            c.session_id != v.session_id
                || c.requested_at_ms >= v.at_ms
                || v.pending
                    .iter()
                    .any(|p| p.correlation_id == c.correlation_id)
        });
        self.views.insert(v.session_id.clone(), v);
    }

    /// A new connection: its first snapshot replaces what the board held,
    /// but for the sessions an event updates before it arrives.
    pub fn reconnected(&mut self) {
        self.reconnected = Some(HashSet::new());
    }

    /// Take `executions.watch`'s answer: its questions, then its views by the
    /// position rule. The first after a connect is the truth instead: the
    /// views and questions it lacks go, unless an event on the new connection
    /// brought them.
    pub fn snapshot(&mut self, snap: ExecutionsWatchResult) {
        let Some(touched) = self.reconnected.take() else {
            for c in snap.confirms {
                self.confirm_requested(c);
            }
            for v in snap.executions {
                self.apply(v);
            }
            return;
        };
        let listed: HashSet<String> = snap
            .executions
            .iter()
            .map(|v| v.session_id.clone())
            .collect();
        self.views
            .retain(|sid, _| listed.contains(sid) || touched.contains(sid));
        let asked: HashSet<String> = snap
            .confirms
            .iter()
            .map(|c| c.correlation_id.clone())
            .collect();
        self.confirms
            .retain(|id, c| asked.contains(id) || touched.contains(&c.session_id));
        for c in snap.confirms {
            self.confirm_requested(c);
        }
        for v in snap.executions {
            if touched.contains(&v.session_id) {
                self.apply(v);
            } else {
                self.put(v);
            }
        }
    }

    pub fn confirm_requested(&mut self, c: ConfirmRequest) {
        if self.resolved.contains(&c.correlation_id) {
            return;
        }
        // A view newer than the question that does not list it: answered.
        if let Some(v) = self.views.get(&c.session_id) {
            if v.at_ms > c.requested_at_ms
                && !v
                    .pending
                    .iter()
                    .any(|p| p.correlation_id == c.correlation_id)
            {
                return;
            }
        }
        self.note_seen(&c.session_id);
        self.confirms.insert(c.correlation_id.clone(), c);
    }

    pub fn confirm_resolved(&mut self, r: &ConfirmResolved) {
        self.confirms.remove(&r.correlation_id);
        if self.resolved.len() >= RESOLVED_KEPT {
            self.resolved.pop_front();
        }
        self.resolved.push_back(r.correlation_id.clone());
    }

    /// Titles from `session.list`.
    pub fn titles(&mut self, sessions: Vec<SessionInfo>) {
        for s in sessions {
            self.asked.insert(s.session_id.clone());
            self.sessions.insert(s.session_id.clone(), s);
        }
    }

    /// Sessions with a view and no title yet, not asked for before: they are
    /// marked asked.
    pub fn untitled(&mut self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .views
            .keys()
            .filter(|sid| !self.asked.contains(*sid))
            .cloned()
            .collect();
        ids.sort();
        self.asked.extend(ids.iter().cloned());
        ids
    }

    /// Forget which titles were asked for and not answered (a reconnect asks
    /// again).
    pub fn ask_again(&mut self) {
        self.asked = self.sessions.keys().cloned().collect();
    }

    /// A question a view lists that the board does not hold whole: asked for
    /// with `confirm.list`.
    pub fn missing_questions(&self) -> bool {
        self.views.values().any(|v| {
            v.pending
                .iter()
                .any(|p| !self.confirms.contains_key(&p.correlation_id))
        })
    }

    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// A session's latest view.
    pub fn view(&self, sid: &str) -> Option<&ExecutionView> {
        self.views.get(sid)
    }

    /// A session's title, kind, and model, as `session.list` said.
    pub fn info(&self, sid: &str) -> Option<&SessionInfo> {
        self.sessions.get(sid)
    }

    /// The questions the board holds whole for a session, by correlation id.
    #[cfg(test)]
    pub fn confirm_ids(&self, sid: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .confirms
            .values()
            .filter(|c| c.session_id == sid)
            .map(|c| c.correlation_id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// A session's name in the sidebar: its label or title, else its kind
    /// and the end of its id.
    pub fn name(&self, sid: &str) -> String {
        let info = self.sessions.get(sid);
        let named = info.and_then(|s| {
            s.label
                .as_deref()
                .filter(|l| !l.trim().is_empty())
                .or(s.title.as_deref())
                .filter(|t| !t.trim().is_empty())
        });
        if let Some(n) = named {
            return one_line(n);
        }
        let kind = self
            .views
            .get(sid)
            .map(|v| v.kind)
            .or(info.map(|s| s.kind))
            .unwrap_or(SessionKind::Conversation);
        match kind {
            SessionKind::Task => format!("task {}", short(sid)),
            SessionKind::Conversation => format!("ses {}", short(sid)),
        }
    }

    /// The sidebar: with `Only::All` and no text, every session as a tree (a
    /// task under its parent, oldest first, folded when idle), each tree in
    /// queue order by its most urgent session (the rollup); otherwise the
    /// sessions that match, as a flat list in queue order.
    pub fn rows(&self, only: Only, text: &str, done: &dyn Fn(&ExecutionView) -> bool) -> Vec<Row> {
        let text = text.trim().to_lowercase();
        if only != Only::All || !text.is_empty() {
            let mut flat: Vec<&ExecutionView> = self
                .views
                .values()
                .filter(|v| {
                    let d = done(v);
                    let level_ok = match only {
                        Only::All => true,
                        Only::NeedsYou => v.attention.level == Level::NeedsYou,
                        Only::Working => !d && v.attention.level == Level::Working,
                        Only::Ready => !d && v.attention.level == Level::Ready,
                        Only::Done => d,
                    };
                    level_ok && (text.is_empty() || self.matches(v, &text))
                })
                .collect();
            flat.sort_by(|a, b| queue_order(a, done(a), b, done(b)));
            return flat
                .into_iter()
                .map(|v| self.row(v, 0, true, done(v), 0))
                .collect();
        }
        let mut children: HashMap<&str, Vec<&ExecutionView>> = HashMap::new();
        let mut roots = Vec::new();
        for v in self.views.values() {
            match v.parent_session_id.as_deref() {
                Some(p) if self.views.contains_key(p) && p != v.session_id => {
                    children.entry(p).or_default().push(v)
                }
                _ => roots.push(v),
            }
        }
        for kids in children.values_mut() {
            kids.sort_by_key(|v| (self.created(&v.session_id), v.session_id.clone()));
        }
        roots.sort_by(|a, b| {
            let (ua, ub) = (urgent(a, &children, done, 0), urgent(b, &children, done, 0));
            queue_order(ua, done(ua), ub, done(ub)).then_with(|| a.session_id.cmp(&b.session_id))
        });
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        for root in roots {
            self.walk(root, 0, true, &children, done, &mut visited, &mut out);
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn walk<'a>(
        &'a self,
        v: &'a ExecutionView,
        depth: usize,
        last: bool,
        children: &HashMap<&str, Vec<&'a ExecutionView>>,
        done: &dyn Fn(&ExecutionView) -> bool,
        visited: &mut HashSet<&'a str>,
        out: &mut Vec<Row>,
    ) {
        if !visited.insert(v.session_id.as_str()) || depth > MAX_DEPTH {
            return;
        }
        let kids = children
            .get(v.session_id.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        // An idle task folds: nothing more will happen in it, and it was seen.
        let (shown, folded): (Vec<&ExecutionView>, Vec<&ExecutionView>) = kids
            .iter()
            .copied()
            .partition(|k| !(k.attention.level == Level::Idle && !done(k)));
        out.push(self.row(v, depth, last, done(v), folded.len()));
        let n = shown.len();
        for (i, k) in shown.into_iter().enumerate() {
            self.walk(k, depth + 1, i + 1 == n, children, done, visited, out);
        }
    }

    fn row(&self, v: &ExecutionView, depth: usize, last: bool, done: bool, folded: usize) -> Row {
        Row {
            session_id: v.session_id.clone(),
            depth,
            last,
            level: v.attention.level,
            done,
            label: if done {
                "done".to_string()
            } else {
                short_label(&v.attention)
            },
            name: self.name(&v.session_id),
            spent_usd: v.spent_usd,
            folded,
        }
    }

    fn matches(&self, v: &ExecutionView, text: &str) -> bool {
        self.name(&v.session_id).to_lowercase().contains(text)
            || v.session_id.to_lowercase().contains(text)
            || v.attention.label.to_lowercase().contains(text)
    }

    /// When a session was created, as `session.list` said; else the order in
    /// which the board first saw it.
    fn created(&self, sid: &str) -> u64 {
        self.sessions
            .get(sid)
            .map(|s| s.created_at_unix_ms)
            .filter(|ms| *ms > 0)
            .unwrap_or_else(|| self.seen_order.get(sid).copied().unwrap_or(u64::MAX))
    }

    fn note_seen(&mut self, sid: &str) {
        if !self.seen_order.contains_key(sid) {
            self.seen_order.insert(sid.to_string(), self.next_seen);
            self.next_seen += 1;
        }
    }

    /// How many sessions need you, and how many are done until seen.
    pub fn counts(&self, done: &dyn Fn(&ExecutionView) -> bool) -> (usize, usize) {
        let need = self
            .views
            .values()
            .filter(|v| v.attention.level == Level::NeedsYou)
            .count();
        let finished = self
            .views
            .values()
            .filter(|v| v.attention.level != Level::NeedsYou && done(v))
            .count();
        (need, finished)
    }
}

/// The most urgent session in a tree, by queue order: the tree's place among
/// the others (design §2.2: the order is for queues and rollups).
fn urgent<'a>(
    v: &'a ExecutionView,
    children: &HashMap<&str, Vec<&'a ExecutionView>>,
    done: &dyn Fn(&ExecutionView) -> bool,
    depth: usize,
) -> &'a ExecutionView {
    let mut best = v;
    if depth < MAX_DEPTH {
        for k in children.get(v.session_id.as_str()).into_iter().flatten() {
            let u = urgent(k, children, done, depth + 1);
            if queue_order(u, done(u), best, done(best)).is_lt() {
                best = u;
            }
        }
    }
    best
}

/// A view's place in the queue: needs you, then done, then working, ready,
/// and idle (design §2.2). Within needs you, the longest waiting comes first;
/// within the rest, the most recently changed. Then by session, so the order
/// is total.
pub fn queue_order(
    a: &ExecutionView,
    a_done: bool,
    b: &ExecutionView,
    b_done: bool,
) -> std::cmp::Ordering {
    let rank = |v: &ExecutionView, d: bool| {
        if v.attention.level == Level::NeedsYou {
            0
        } else if d {
            1
        } else {
            v.attention.level.rank()
        }
    };
    let (ra, rb) = (rank(a, a_done), rank(b, b_done));
    ra.cmp(&rb)
        .then_with(|| {
            if ra == 0 {
                a.attention.since_ms.cmp(&b.attention.since_ms)
            } else {
                b.at_ms.cmp(&a.at_ms)
            }
        })
        .then_with(|| a.session_id.cmp(&b.session_id))
}

/// An attention label as the sidebar says it: a question by its tool alone
/// (`confirm proc.run`), a block or a failure by its word; the card says the
/// rest.
pub fn short_label(a: &Attention) -> String {
    let l = a.label.as_str();
    if let Some(rest) = l.strip_prefix("confirm ") {
        let tool = rest.split([':', ' ']).next().unwrap_or(rest);
        return format!("confirm {tool}");
    }
    for word in ["budget", "blocked", "failed"] {
        if l.starts_with(&format!("{word}: ")) {
            return word.to_string();
        }
    }
    one_line(l)
}

/// The end of an id, as people name it (`/cancel a1b2c3`).
pub fn short(id: &str) -> String {
    let n = id.chars().count();
    id.chars().skip(n.saturating_sub(6)).collect()
}

/// The first line of a text, trimmed.
pub fn one_line(s: &str) -> String {
    s.lines().next().unwrap_or_default().trim().to_string()
}
