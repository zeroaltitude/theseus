//! The binding hears the work engine (theseus-2j2ka): one task model for all
//! its places, seeded by `work.watch` from the binding's own connection
//! (`interactive: false`, so the daemon does not count it as a screen a person
//! looks at), kept by `work.changed` under the position rule, and read again
//! after `events.lost`. A child of `runtime`.
//!
//! - **The watch names its roots.** The Discord surface reads no private text,
//!   so `work.watch` takes the conversations it asks for: each place's session.
//!   They change (a place opens a session, `/new`, a rebind), so a change of
//!   routes only says so (`Tasks::rewatch`), and one loop asks again, once for
//!   a burst of them.
//! - **A task goes to its place by its root.** A view's `root` is the
//!   execution at the top of its tree, and that conversation's session is a
//!   place's: `place_of_root`. The place gets a `PlaceMsg::Tasks` and draws
//!   its own board from `Tasks::project`; the counts for the bot's status line
//!   (`Counts`) are the whole model's, one number each.
//! - **Nothing is stored.** The model is a projection of the engine's; what
//!   "since you looked" means is one number a place, the time of its last
//!   Clear, in memory (a restart shows today's done tasks again, once).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::{WorkChanged, WorkWatchParams, WorkWatchResult};
use tokio::sync::{watch, Notify};

use super::{PlaceMsg, Shared};

/// How long an ended task stays on a board as done and not yet seen.
pub(crate) const DONE_SHOWN_MS: u64 = 24 * 3_600_000;

/// The whole model's numbers, for the status line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub needs_you: u32,
    pub running: u32,
    /// The next reminder due today, as minutes into the local day
    /// (theseus-j90w2): a time, never a word of it.
    pub reminder: Option<u16>,
}

/// What one place shows, newest information first within each part.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Projection {
    /// Waiting on the owner, the longest waiting first.
    pub needs: Vec<WorkView>,
    /// Working: queued, running, or waiting on calls; the oldest first.
    pub working: Vec<WorkView>,
    /// Ended since the place's last Clear, the newest first.
    pub done: Vec<WorkView>,
}

impl Projection {
    pub fn is_empty(&self) -> bool {
        self.needs.is_empty() && self.working.is_empty() && self.done.is_empty()
    }
}

#[derive(Default)]
struct Model {
    views: HashMap<String, WorkView>,
    /// When each place last cleared its done tasks, by its session id.
    cleared: HashMap<String, u64>,
}

pub(crate) struct Tasks {
    model: Mutex<Model>,
    again: Notify,
    counts: watch::Sender<Counts>,
    /// Discord refused a V2 message from this bot: the text board takes over.
    v2_refused: AtomicBool,
    /// The status line's task runs.
    pub(super) presence_started: AtomicBool,
    /// A fresh gateway session wants the status sent again.
    resend: Notify,
    /// Each place's task pings are quiet until then (unix ms), by its target.
    quiet: Mutex<HashMap<String, u64>>,
    /// The future (theseus-j90w2): the board's Coming up, Routines and Projects.
    pub(super) future: super::horizon::Future,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            model: Mutex::default(),
            again: Notify::new(),
            counts: watch::channel(Counts::default()).0,
            v2_refused: AtomicBool::new(false),
            presence_started: AtomicBool::new(false),
            resend: Notify::new(),
            quiet: Mutex::default(),
            future: Default::default(),
        }
    }
}

/// Whether a view is one the board and the status line count: a task. The
/// chat's own question is a card in that chat already, and a place with no
/// task has no board for it (task status only).
fn counted(v: &WorkView) -> bool {
    v.kind == WorkKind::Task
}

fn counts_of(m: &Model) -> Counts {
    let mut c = Counts::default();
    for v in m.views.values().filter(|v| counted(v)) {
        if v.state == WorkState::NeedsYou {
            c.needs_you += 1;
        } else if v.state.working() {
            c.running += 1;
        }
    }
    c
}

impl Tasks {
    /// Ask for the watch again, once for a burst of reasons.
    pub fn rewatch(&self) {
        self.again.notify_one();
        self.future.dirty();
    }

    /// The next reminder due today changed (theseus-j90w2): the status line
    /// says its time.
    pub fn set_reminder(&self, r: Option<u16>) {
        self.counts.send_if_modified(|c| {
            if c.reminder == r {
                return false;
            }
            c.reminder = r;
            true
        });
    }

    /// The numbers now, and a receiver that wakes when they change.
    pub fn counts(&self) -> watch::Receiver<Counts> {
        self.counts.subscribe()
    }

    /// The watch's snapshot: the roots it carries are replaced whole (a view
    /// of theirs that is not in it left the board), a root it no longer
    /// carries loses its views that still looked active, and the position
    /// rule holds for the rest. The sessions of the roots touched.
    pub fn snapshot(&self, r: WorkWatchResult) -> HashSet<String> {
        let mut m = self.model.lock().unwrap();
        let roots: HashSet<&str> = r.views.iter().map(|v| v.root.as_str()).collect();
        let mut touched: HashSet<String> = HashSet::new();
        m.views.retain(|_, v| {
            let gone = if roots.contains(v.root.as_str()) {
                !r.views.iter().any(|n| n.id == v.id)
            } else {
                !v.state.ended()
            };
            if gone {
                touched.insert(v.root.clone());
            }
            !gone
        });
        for v in r.views {
            apply(&mut m, v, &mut touched);
        }
        let sessions = sessions_of(&m, &touched);
        let c = counts_of(&m);
        drop(m);
        self.counts.send_if_modified(|old| {
            let c = Counts {
                reminder: old.reminder,
                ..c
            };
            std::mem::replace(old, c) != c
        });
        sessions
    }

    /// One frame's changes. The sessions of the roots they touch.
    pub fn changed(&self, c: WorkChanged) -> HashSet<String> {
        let mut m = self.model.lock().unwrap();
        let mut touched: HashSet<String> = HashSet::new();
        for id in &c.gone {
            if let Some(v) = m.views.remove(id) {
                touched.insert(v.root);
            }
        }
        for v in c.views {
            apply(&mut m, v, &mut touched);
        }
        let sessions = sessions_of(&m, &touched);
        let n = counts_of(&m);
        drop(m);
        self.counts.send_if_modified(|old| {
            let n = Counts {
                reminder: old.reminder,
                ..n
            };
            std::mem::replace(old, n) != n
        });
        sessions
    }

    /// What the place whose conversation is `session_id` shows.
    pub fn project(&self, session_id: &str) -> Projection {
        let m = self.model.lock().unwrap();
        let Some(root) = m
            .views
            .values()
            .find(|v| {
                v.kind == WorkKind::Conversation && v.session_id.as_deref() == Some(session_id)
            })
            .map(|v| v.id.clone())
        else {
            return Projection::default();
        };
        let cleared = m.cleared.get(session_id).copied().unwrap_or(0);
        project(&m.views, &root, cleared, theseus_protocol::now_unix_ms())
    }

    /// The projects the model holds, the soonest next step first.
    pub fn projects(&self) -> Vec<WorkView> {
        let m = self.model.lock().unwrap();
        let mut v: Vec<WorkView> = m
            .views
            .values()
            .filter(|v| v.kind == WorkKind::Project && !v.state.ended())
            .cloned()
            .collect();
        v.sort_by_key(|v| {
            (
                v.project
                    .as_ref()
                    .and_then(|p| p.next.as_ref())
                    .and_then(|n| n.due_at_ms)
                    .unwrap_or(u64::MAX),
                v.id.clone(),
            )
        });
        v
    }

    /// Every view under the root of the place whose conversation is `session_id`.
    pub fn in_place(&self, session_id: &str) -> Vec<WorkView> {
        let m = self.model.lock().unwrap();
        let Some(root) = root_of(&m, session_id) else {
            return Vec::new();
        };
        m.views
            .values()
            .filter(|v| v.root == root)
            .cloned()
            .collect()
    }

    /// The view `id` if it is under the place's root: a command never opens
    /// another place's task by naming its id.
    pub fn view_in_place(&self, session_id: &str, id: &str) -> Option<WorkView> {
        let m = self.model.lock().unwrap();
        let root = root_of(&m, session_id)?;
        m.views.get(id).filter(|v| v.root == root).cloned()
    }

    /// The place's task that `text` names: an id or its end, else the one task
    /// whose title holds the words (case aside), if there is exactly one.
    pub fn find_task(&self, session_id: &str, text: &str) -> Option<WorkView> {
        let t = text.trim().trim_start_matches('…').to_lowercase();
        if t.is_empty() {
            return None;
        }
        let tasks: Vec<WorkView> = self
            .in_place(session_id)
            .into_iter()
            .filter(|v| v.kind == WorkKind::Task)
            .collect();
        if let Some(v) = tasks.iter().find(|v| {
            v.id.to_lowercase() == t || (t.len() >= 4 && v.id.to_lowercase().ends_with(&t))
        }) {
            return Some(v.clone());
        }
        let mut by_title = tasks
            .into_iter()
            .filter(|v| v.title.to_lowercase().contains(&t));
        let first = by_title.next()?;
        by_title.next().is_none().then_some(first)
    }

    /// Quiet the place's task pings until `until` (unix ms), or end the quiet.
    pub fn set_quiet(&self, target: &str, until: Option<u64>) {
        let mut q = self.quiet.lock().unwrap();
        match until {
            Some(t) => q.insert(target.to_string(), t),
            None => q.remove(target),
        };
    }

    /// The place's done tasks are seen: the board drops them from now.
    pub fn clear_done(&self, session_id: &str) {
        let mut m = self.model.lock().unwrap();
        m.cleared
            .insert(session_id.to_string(), theseus_protocol::now_unix_ms());
    }

    /// A view held, by its id.
    pub fn view(&self, id: &str) -> Option<WorkView> {
        self.model.lock().unwrap().views.get(id).cloned()
    }

    /// The open question of the task `id`: its own view, which carries the
    /// options, else the task's brief.
    pub fn question_of(&self, id: &str) -> Option<theseus_protocol::work::Question> {
        let m = self.model.lock().unwrap();
        m.views
            .values()
            .filter(|v| {
                v.kind == WorkKind::Question && v.parent.as_deref() == Some(id) && !v.state.ended()
            })
            .find_map(|v| v.question.clone())
            .or_else(|| m.views.get(id).and_then(|v| v.question.clone()))
    }

    /// The sessions the binding should watch, as `work.watch` names them.
    fn roots(shared: &Shared) -> Vec<String> {
        let r = shared.routes.lock().unwrap();
        r.by_session
            .keys()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect()
    }

    /// The status is due again (a fresh gateway session).
    pub fn resend(&self) {
        self.resend.notify_one();
    }

    /// Resolves when `resend` was called.
    pub async fn resent(&self) {
        self.resend.notified().await;
    }

    pub fn v2_refused(&self) -> bool {
        self.v2_refused.load(Ordering::Relaxed)
    }

    pub fn refuse_v2(&self) {
        self.v2_refused.store(true, Ordering::Relaxed);
    }

    /// When the place's task pings are quiet until, if they are.
    pub fn quiet_until(&self, target: &str) -> Option<u64> {
        let now = theseus_protocol::now_unix_ms();
        let mut q = self.quiet.lock().unwrap();
        match q.get(target) {
            Some(&t) if t > now => Some(t),
            Some(_) => {
                q.remove(target);
                None
            }
            None => None,
        }
    }

    /// Quiet an hour, or end the quiet. True when it is quiet now.
    pub fn toggle_quiet(&self, target: &str) -> bool {
        if self.quiet_until(target).is_some() {
            self.quiet.lock().unwrap().remove(target);
            return false;
        }
        let until = theseus_protocol::now_unix_ms() + 3_600_000;
        self.quiet.lock().unwrap().insert(target.to_string(), until);
        true
    }

    /// The views of `kind` under the task `id`, newest first.
    pub fn children_of(&self, id: &str, kind: WorkKind) -> Vec<WorkView> {
        let m = self.model.lock().unwrap();
        let mut v: Vec<WorkView> = m
            .views
            .values()
            .filter(|v| v.kind == kind && v.parent.as_deref() == Some(id))
            .cloned()
            .collect();
        v.sort_by_key(|v| std::cmp::Reverse(v.started_at_ms));
        v
    }
}

/// The execution at the top of the conversation `session_id`.
fn root_of(m: &Model, session_id: &str) -> Option<String> {
    m.views
        .values()
        .find(|v| v.kind == WorkKind::Conversation && v.session_id.as_deref() == Some(session_id))
        .map(|v| v.id.clone())
}

/// The position rule: a view older than the one held is dropped.
fn apply(m: &mut Model, v: WorkView, touched: &mut HashSet<String>) {
    if m.views
        .get(&v.id)
        .is_some_and(|old| old.position > v.position)
    {
        return;
    }
    touched.insert(v.root.clone());
    m.views.insert(v.id.clone(), v);
}

/// The sessions of the conversations whose execution is one of `roots`.
fn sessions_of(m: &Model, roots: &HashSet<String>) -> HashSet<String> {
    m.views
        .values()
        .filter(|v| v.kind == WorkKind::Conversation && roots.contains(&v.id))
        .filter_map(|v| v.session_id.clone())
        .collect()
}

/// One place's three parts, from the views under `root`.
fn project(
    views: &HashMap<String, WorkView>,
    root: &str,
    cleared_ms: u64,
    now_ms: u64,
) -> Projection {
    let mut p = Projection::default();
    for v in views.values().filter(|v| v.root == root && counted(v)) {
        if v.state == WorkState::NeedsYou {
            p.needs.push(v.clone());
        } else if v.state.working() {
            p.working.push(v.clone());
        } else if v.state.ended() {
            let at = v.ended_at_ms.unwrap_or(v.state_since_ms);
            if at > cleared_ms && now_ms.saturating_sub(at) < DONE_SHOWN_MS {
                p.done.push(v.clone());
            }
        }
    }
    p.needs.sort_by_key(|v| {
        (
            v.question
                .as_ref()
                .map_or(v.state_since_ms, |q| q.asked_at_ms),
            v.id.clone(),
        )
    });
    p.working.sort_by_key(|v| (v.started_at_ms, v.id.clone()));
    p.done.sort_by_key(|v| {
        (
            std::cmp::Reverse(v.ended_at_ms.unwrap_or(v.state_since_ms)),
            v.id.clone(),
        )
    });
    p
}

/// The loop that keeps the watch: asks at the start, and again whenever the
/// places' sessions change. It ends with the daemon's stop.
pub(super) fn watch_loop(shared: &Arc<Shared>) {
    super::horizon::start(shared);
    let s = shared.clone();
    tokio::spawn(async move {
        let stopped = s.core.outbox.stopped();
        tokio::pin!(stopped);
        loop {
            ask(&s).await;
            tokio::select! {
                () = s.tasks.again.notified() => {}
                () = &mut stopped => return,
            }
            // A burst of binds is one ask.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
}

async fn ask(shared: &Arc<Shared>) {
    let roots = Tasks::roots(shared);
    if roots.is_empty() {
        return;
    }
    let p = WorkWatchParams {
        limit: Some(20),
        roots: Some(roots),
        interactive: Some(false),
    };
    match shared
        .rpc
        .call::<_, WorkWatchResult>(theseus_protocol::method::WORK_WATCH, p)
        .await
    {
        Ok(r) => {
            shared.tasks.snapshot(r);
            // Every place draws again: a board pinned before a restart is
            // brought up to date, or says all clear.
            let r = shared.routes.lock().unwrap();
            for tx in r.by_session.values() {
                let _ = tx.send(PlaceMsg::Tasks);
            }
        }
        Err(e) => shared.board.error("work.watch", None, e),
    }
}

/// The engine's events the model takes (`route`): a frame's changes, and a
/// lost stream, which is read again. True when the event is the model's.
pub(super) fn heard(shared: &Shared, e: &theseus_protocol::Event) -> bool {
    use theseus_protocol::Event;
    match e {
        Event::WorkChanged(c) => {
            let sessions = shared.tasks.changed(c.clone());
            notify_places(shared, sessions);
            shared.tasks.future.dirty();
            true
        }
        Event::WorkNotice(_) => true,
        // A reminder, a routine's or a watch's wake was told: the future moved.
        Event::WakeNoticed(_) => {
            shared.tasks.future.dirty();
            false
        }
        Event::EventsLost(_) => {
            shared.tasks.rewatch();
            true
        }
        _ => false,
    }
}

/// Tell the places whose tasks changed.
fn notify_places(shared: &Shared, sessions: HashSet<String>) {
    let r = shared.routes.lock().unwrap();
    for s in sessions {
        if let Some(tx) = r.by_session.get(&s) {
            let _ = tx.send(PlaceMsg::Tasks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::work::Rollup;
    use theseus_protocol::{Attention, Level};

    pub(crate) fn view(
        id: &str,
        kind: WorkKind,
        root: &str,
        state: WorkState,
        position: u64,
    ) -> WorkView {
        WorkView {
            id: id.into(),
            kind,
            position,
            at_ms: position,
            parent: None,
            root: root.into(),
            session_id: Some(format!("ses_{id}")),
            title: format!("title of {id}"),
            place: None,
            state,
            previous: None,
            attention: Attention {
                level: Level::Working,
                label: String::new(),
                since_ms: position,
            },
            started_at_ms: position,
            state_since_ms: position,
            ended_at_ms: state.ended().then_some(position),
            doing: None,
            progress: None,
            question: None,
            cost: None,
            eta: None,
            below: Some(Rollup::default()),
            why: None,
            report: None,
            woke: None,
            flagged: false,
            dismissed: false,
            retry_of: None,
            forecast: None,
            thread: None,
            purpose: None,
            next_at_ms: None,
            series: None,
            paused: false,
            routine: None,
            watch: None,
            project: None,
        }
    }

    fn changed_of(views: Vec<WorkView>, gone: &[&str]) -> WorkChanged {
        WorkChanged {
            position: views.iter().map(|v| v.position).max().unwrap_or(0),
            at_ms: 0,
            views,
            gone: gone.iter().map(|s| s.to_string()).collect(),
            threads: vec![],
        }
    }

    #[test]
    fn a_task_goes_to_the_place_of_its_roots_conversation() {
        let t = Tasks::default();
        let sessions = t.changed(changed_of(
            vec![
                view(
                    "exe_chat",
                    WorkKind::Conversation,
                    "exe_chat",
                    WorkState::Ready,
                    1,
                ),
                view("exe_a", WorkKind::Task, "exe_chat", WorkState::Running, 2),
                view(
                    "exe_other",
                    WorkKind::Conversation,
                    "exe_other",
                    WorkState::Ready,
                    1,
                ),
                view("exe_b", WorkKind::Task, "exe_other", WorkState::Running, 2),
            ],
            &[],
        ));
        assert_eq!(sessions.len(), 2);
        let mine = t.project("ses_exe_chat");
        assert_eq!(mine.working.len(), 1);
        assert_eq!(mine.working[0].id, "exe_a");
        assert_eq!(t.project("ses_exe_other").working[0].id, "exe_b");
        assert!(t.project("ses_nobody").is_empty());
    }

    #[test]
    fn the_chats_own_question_is_no_task_and_makes_no_board() {
        let t = Tasks::default();
        let mut chat = view(
            "exe_chat",
            WorkKind::Conversation,
            "exe_chat",
            WorkState::NeedsYou,
            2,
        );
        chat.attention.level = Level::NeedsYou;
        t.changed(changed_of(vec![chat], &[]));
        assert!(t.project("ses_exe_chat").is_empty());
        assert_eq!(*t.counts().borrow(), Counts::default());
    }

    #[test]
    fn an_older_position_never_replaces_a_newer_view() {
        let t = Tasks::default();
        t.changed(changed_of(
            vec![
                view(
                    "exe_chat",
                    WorkKind::Conversation,
                    "exe_chat",
                    WorkState::Ready,
                    1,
                ),
                view("exe_a", WorkKind::Task, "exe_chat", WorkState::Done, 9),
            ],
            &[],
        ));
        // A late frame says it still ran.
        t.changed(changed_of(
            vec![view(
                "exe_a",
                WorkKind::Task,
                "exe_chat",
                WorkState::Running,
                5,
            )],
            &[],
        ));
        assert_eq!(t.view("exe_a").unwrap().state, WorkState::Done);
    }

    #[test]
    fn counts_are_the_whole_models_and_a_gone_view_leaves_them() {
        let t = Tasks::default();
        let mut rx = t.counts();
        let mut ask = view("exe_a", WorkKind::Task, "exe_chat", WorkState::NeedsYou, 3);
        ask.attention.level = Level::NeedsYou;
        t.changed(changed_of(
            vec![
                view(
                    "exe_chat",
                    WorkKind::Conversation,
                    "exe_chat",
                    WorkState::Ready,
                    1,
                ),
                ask,
                view("exe_b", WorkKind::Task, "exe_chat", WorkState::Running, 2),
                view("exe_c", WorkKind::Task, "exe_chat", WorkState::Waiting, 2),
                // A job is not a task.
                view("act_j", WorkKind::Job, "exe_chat", WorkState::Running, 2),
            ],
            &[],
        ));
        assert_eq!(
            *rx.borrow_and_update(),
            Counts {
                needs_you: 1,
                running: 2,
                reminder: None
            }
        );
        t.changed(changed_of(vec![], &["exe_b", "exe_c"]));
        assert_eq!(
            *rx.borrow_and_update(),
            Counts {
                needs_you: 1,
                running: 0,
                reminder: None
            }
        );
    }

    #[test]
    fn a_snapshot_replaces_its_roots_and_drops_the_active_views_of_roots_it_lost() {
        let t = Tasks::default();
        t.changed(changed_of(
            vec![
                view(
                    "exe_chat",
                    WorkKind::Conversation,
                    "exe_chat",
                    WorkState::Ready,
                    1,
                ),
                view("exe_a", WorkKind::Task, "exe_chat", WorkState::Running, 2),
                view(
                    "exe_old",
                    WorkKind::Conversation,
                    "exe_old",
                    WorkState::Ready,
                    1,
                ),
                view("exe_x", WorkKind::Task, "exe_old", WorkState::Running, 2),
            ],
            &[],
        ));
        // While the binding was away: a ended, exe_old went cold and ended.
        t.snapshot(WorkWatchResult {
            position: 10,
            views: vec![
                view(
                    "exe_chat",
                    WorkKind::Conversation,
                    "exe_chat",
                    WorkState::Ready,
                    8,
                ),
                view("exe_a", WorkKind::Task, "exe_chat", WorkState::Done, 9),
            ],
            total: 2,
            threads: vec![],
        });
        assert_eq!(t.view("exe_a").unwrap().state, WorkState::Done);
        assert!(
            t.view("exe_x").is_none(),
            "an active view of a lost root is stale"
        );
    }

    #[test]
    fn done_tasks_show_until_the_place_clears_them() {
        let t = Tasks::default();
        let now = theseus_protocol::now_unix_ms();
        let mut done = view(
            "exe_a",
            WorkKind::Task,
            "exe_chat",
            WorkState::Done,
            now - 1000,
        );
        done.ended_at_ms = Some(now - 1000);
        let mut old = view(
            "exe_b",
            WorkKind::Task,
            "exe_chat",
            WorkState::Done,
            now - 2 * DONE_SHOWN_MS,
        );
        old.ended_at_ms = Some(now - 2 * DONE_SHOWN_MS);
        t.changed(changed_of(
            vec![
                view(
                    "exe_chat",
                    WorkKind::Conversation,
                    "exe_chat",
                    WorkState::Ready,
                    1,
                ),
                done,
                old,
            ],
            &[],
        ));
        assert_eq!(
            t.project("ses_exe_chat").done.len(),
            1,
            "a day-old one is not shown"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        t.clear_done("ses_exe_chat");
        assert!(t.project("ses_exe_chat").done.is_empty());
    }
}
