//! The voice channel's status while Theseus is in a call (theseus-arp9l).
//!
//! - **The status**: `🎧 Theseus · ⚙️ 3 tasks · 🔴 1 needs you`, set with
//!   `PUT /channels/{id}/voice-status` (a route twilight does not type, so
//!   the writer is a plain request). Counts only: everyone in the channel
//!   reads it, so it never holds a title, a path, or a dollar figure.
//! - **The counts** come from one watch of the work board, held in the
//!   core and not over the wire: the board's guarded view (a binding's
//!   own) sees shared places alone, and the call's tasks are the owner's.
//!   The watch is `work.watch`'s own (`Bus::watch_work`, interactive
//!   false), kept as a small map of the open tasks and questions, so a
//!   change costs one map write and a count.
//! - **The pace**: a change waits 2 s so a burst is one write (measured from
//!   the first change since the last write, so a flapping count still
//!   lands), at most one write a minute (the route's bucket is not
//!   published), and the last state is always written. Nothing is written
//!   while the counts are unchanged.
//! - **Cleared** at `/leave`, when the call drops, and at the binding's stop.
//!   A 403 (the bot lacks the permission) is said once, in health, and the
//!   call's writes stop.
//! - **A spoken answer**: `Tally::spoken` is one line from the same map, at
//!   most 200 characters, for the voice turn of a private place only, so the
//!   model can answer "what's going on?" in a sentence with no tool call.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use theseus_core::push::WorkScope;
use theseus_core::Core;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{notify, Message, WorkChanged};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::voice::tasks::Tasks;

/// A burst of changes is one write.
pub(super) const DEBOUNCE: Duration = Duration::from_secs(2);
/// At most one write in this long.
pub(super) const GAP: Duration = Duration::from_secs(60);
/// The spoken line's length, in characters.
pub(super) const SPOKEN_MAX: usize = 200;
/// Discord's limit on a status, in characters.
const STATUS_MAX: usize = 500;
/// How long a clear may take at a leave or a stop.
const CLEAR_WAIT: Duration = Duration::from_secs(3);

/// What is open on the board that the status counts.
enum Open {
    /// A task that works (queued, running, or waiting on its work).
    Task,
    /// A question for the owner: what the call is told, and when it came.
    Question {
        who: String,
        prompt: String,
        at_ms: u64,
    },
}

impl Open {
    fn is_task(&self) -> bool {
        matches!(self, Open::Task)
    }
}

/// The open tasks and questions, by view id. Titles stay here, in memory, for
/// the spoken line; the status reads the counts alone.
#[derive(Default)]
pub(super) struct Tally {
    open: HashMap<String, Open>,
    tasks: u32,
    needs: u32,
}

/// The two counts the status holds.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct Counts {
    pub tasks: u32,
    pub needs: u32,
}

impl Counts {
    /// The status text: `🎧 Theseus`, then what works, then what needs you.
    pub(super) fn status(self) -> String {
        let mut s = String::from("🎧 Theseus");
        if self.tasks > 0 {
            let s2 = if self.tasks == 1 { "task" } else { "tasks" };
            s.push_str(&format!(" · ⚙️ {} {s2}", self.tasks));
        }
        if self.needs > 0 {
            s.push_str(&format!(" · 🔴 {} needs you", self.needs));
        }
        s
    }
}

impl Tally {
    pub(super) fn counts(&self) -> Counts {
        Counts {
            tasks: self.tasks,
            needs: self.needs,
        }
    }

    /// Take a view in or out: one map write and a count moved. True when a
    /// count moved.
    pub(super) fn apply(&mut self, v: &WorkView) -> bool {
        let open = match v.kind {
            WorkKind::Task if v.state.working() => Some(Open::Task),
            WorkKind::Question if !v.state.ended() => v.question.as_ref().map(|q| Open::Question {
                who: q.asked_by.title.clone(),
                prompt: q.prompt.clone(),
                at_ms: q.asked_at_ms,
            }),
            _ => None,
        };
        let before = self.counts();
        let now = open.as_ref().map(Open::is_task);
        let was = match open {
            Some(o) => self.open.insert(v.id.clone(), o),
            None => self.open.remove(&v.id),
        };
        self.count(was.as_ref().map(Open::is_task), false);
        self.count(now, true);
        self.counts() != before
    }

    /// An id left the board.
    pub(super) fn drop_id(&mut self, id: &str) -> bool {
        let before = self.counts();
        let was = self.open.remove(id);
        self.count(was.as_ref().map(Open::is_task), false);
        self.counts() != before
    }

    /// One more (or one fewer) of a kind: `Some(true)` is a task.
    fn count(&mut self, kind: Option<bool>, add: bool) {
        let n = match kind {
            Some(true) => &mut self.tasks,
            Some(false) => &mut self.needs,
            None => return,
        };
        *n = if add { *n + 1 } else { n.saturating_sub(1) };
    }

    /// Replace everything: the snapshot after a watch begins.
    fn seed(&mut self, views: &[WorkView]) {
        *self = Self::default();
        for v in views {
            self.apply(v);
        }
    }

    /// One line from the board for the call's voice turn, at most
    /// `SPOKEN_MAX` characters: `3 tasks working; 1 needs you: Beta summary
    /// asks fs.write create projects/beta-summary.txt (40 B)`.
    pub(super) fn spoken(&self) -> String {
        let tasks = match self.tasks {
            0 => "no tasks working".to_string(),
            1 => "1 task working".to_string(),
            n => format!("{n} tasks working"),
        };
        let oldest = self
            .open
            .values()
            .filter_map(|o| match o {
                Open::Question { who, prompt, at_ms } => Some((*at_ms, who, prompt)),
                Open::Task => None,
            })
            .min();
        let line = match (self.needs, oldest) {
            (0, _) | (_, None) => format!("{tasks}; nothing needs you"),
            (1, Some((_, who, prompt))) => {
                format!("{tasks}; 1 needs you: {}", asks(who, prompt))
            }
            (n, Some((_, who, prompt))) => {
                format!("{tasks}; {n} need you, the oldest: {}", asks(who, prompt))
            }
        };
        clip(&line, SPOKEN_MAX)
    }
}

/// `Beta summary asks fs.write …`, on one line.
fn asks(who: &str, prompt: &str) -> String {
    let who = who.trim();
    let who = if who.is_empty() { "A task" } else { who };
    format!("{who} asks {}", prompt.trim())
}

/// `s` on one line, cut to `max` characters with an ellipsis.
fn clip(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// When the next write is due, and what it holds. Pure: the clock is the
/// caller's.
#[derive(Default)]
pub(super) struct Pacer {
    wanted: Option<String>,
    written: Option<String>,
    /// When `wanted` first differed from `written`.
    pending_since: Option<Instant>,
    last_write: Option<Instant>,
}

impl Pacer {
    /// The status the counts now call for.
    pub(super) fn want(&mut self, text: String, now: Instant) {
        if self.wanted.as_deref() == Some(&text) {
            return;
        }
        self.wanted = Some(text);
        self.pending_since = match self.wanted == self.written {
            true => None,
            false => self.pending_since.or(Some(now)),
        };
    }

    /// When to write next: after the debounce and the minute's gap. None when
    /// there is nothing to write.
    pub(super) fn due(&self) -> Option<Instant> {
        let since = self.pending_since?;
        let after_gap = self.last_write.map(|t| t + GAP);
        Some(match after_gap {
            Some(g) => (since + DEBOUNCE).max(g),
            None => since + DEBOUNCE,
        })
    }

    /// The text to write now.
    pub(super) fn take(&mut self) -> Option<String> {
        self.wanted.clone()
    }

    /// A write was tried at `now`: `ok` when Discord took it.
    pub(super) fn tried(&mut self, ok: bool, now: Instant) {
        self.last_write = Some(now);
        if ok {
            self.written = self.wanted.clone();
            self.pending_since = None;
        } else {
            // Kept pending, a gap on: the next try comes after the minute.
            self.pending_since = Some(now);
        }
    }
}

/// Why a write did not land.
#[derive(Debug)]
pub(super) enum PutError {
    /// 403: the bot may not set a voice channel's status.
    Forbidden,
    /// Anything else: a rate limit, Discord down, the network.
    Failed(String),
}

pub(super) type PutFuture<'a> = Pin<Box<dyn Future<Output = Result<(), PutError>> + Send + 'a>>;

/// Where the status goes.
pub(super) trait Put: Send + Sync {
    /// Set `channel`'s status to `status`; an empty one clears it.
    fn put<'a>(&'a self, channel: u64, status: &'a str) -> PutFuture<'a>;
}

/// Discord's REST, or a stand-in's (`[discord] rest_proxy`).
pub(super) struct Rest {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Rest {
    /// None until the bot's token resolves (it has, by a join).
    pub(super) fn of(core: &Core) -> Option<Self> {
        use theseus_core::secrets::SecretState;
        let cfg = &core.cfg.discord;
        let token = match core.secrets.subscribe().borrow().get(&cfg.token_secret) {
            Some(SecretState::Ready(t)) => t.expose().to_string(),
            _ => return None,
        };
        let base = match cfg.rest_proxy.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => format!("http://{p}/api/v10"),
            None => "https://discord.com/api/v10".to_string(),
        };
        Self::new(base, token)
    }

    /// A writer to `base` (`…/api/v10`) as the bot `token`.
    pub(super) fn new(base: String, token: String) -> Option<Self> {
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .ok()?,
            base,
            token,
        })
    }
}

impl Put for Rest {
    fn put<'a>(&'a self, channel: u64, status: &'a str) -> PutFuture<'a> {
        Box::pin(async move {
            let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bot {}", self.token))
                .map_err(|_| PutError::Failed("the token is not a header".into()))?;
            auth.set_sensitive(true);
            let r = self
                .http
                .put(format!("{}/channels/{channel}/voice-status", self.base))
                .header(reqwest::header::AUTHORIZATION, auth)
                .json(&serde_json::json!({ "status": status }))
                .send()
                .await
                .map_err(|e| PutError::Failed(e.without_url().to_string()))?;
            match r.status().as_u16() {
                200..=299 => Ok(()),
                403 => Err(PutError::Forbidden),
                n => Err(PutError::Failed(format!("Discord answered {n}"))),
            }
        })
    }
}

/// The running status of one call.
pub(super) struct Handle {
    /// The open tasks and questions, for the spoken line.
    pub(super) tally: Arc<Mutex<Tally>>,
    /// The call's status is written, and the board's line is told.
    pub(super) board: bool,
    /// What the call says of the tasks, and waits to be answered on.
    pub(super) tasks: Option<Arc<Mutex<Tasks>>>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Handle {
    /// Watch the board and keep `channel`'s status for the call, when there
    /// is a `put`; `tasks` hears the same watch. `told` is called once, with
    /// a sentence, when Discord refuses the permission.
    pub(super) fn start(
        core: Arc<Core>,
        put: Option<Arc<dyn Put>>,
        tasks: Option<Arc<Mutex<Tasks>>>,
        channel: u64,
        serial: u64,
        told: impl Fn(String) + Send + 'static,
    ) -> Self {
        let tally = Arc::new(Mutex::new(Tally::default()));
        let (stop, stopped) = oneshot::channel();
        let board = put.is_some();
        let watching = Watching {
            core,
            put,
            channel,
            conn: format!("discord-voice-status-{serial}"),
            tally: Arc::clone(&tally),
            tasks: tasks.clone(),
        };
        let task = tokio::spawn(run(watching, stopped, told));
        Self {
            tally,
            board,
            tasks,
            stop: Some(stop),
            task,
        }
    }

    /// End the status: cleared (unless Discord refused it), then done. Waits
    /// for the clear, a few seconds at most.
    pub(super) async fn finish(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(CLEAR_WAIT * 2, &mut self.task).await;
        self.task.abort();
    }
}

/// What one call's watch of the board feeds.
struct Watching {
    core: Arc<Core>,
    /// Where the status goes; none when only tasks are spoken.
    put: Option<Arc<dyn Put>>,
    channel: u64,
    conn: String,
    tally: Arc<Mutex<Tally>>,
    tasks: Option<Arc<Mutex<Tasks>>>,
}

/// The counts in, the writes out, until the call ends or the daemon stops.
async fn run(w: Watching, mut stop: oneshot::Receiver<()>, told: impl Fn(String)) {
    let Watching {
        core,
        put,
        channel,
        conn,
        tally,
        tasks,
    } = w;
    let (tx, mut feed) = mpsc::unbounded_channel::<Message>();
    let watched = watch(&core, &conn, tx, (&*tally, tasks.as_deref())).await;
    let mut pacer = Pacer::default();
    let mut refused = false;
    if watched && put.is_some() {
        pacer.want(tally.lock().unwrap().counts().status(), Instant::now());
    }
    loop {
        let due = pacer.due();
        tokio::select! {
            m = feed.recv(), if watched => {
                let Some(m) = m else { break };
                if changed(&m, (&*tally, tasks.as_deref()), &core) && put.is_some() {
                    let text = tally.lock().unwrap().counts().status();
                    pacer.want(text, Instant::now());
                }
            }
            () = async { tokio::time::sleep_until(due.unwrap()).await }, if due.is_some() && !refused => {
                let (Some(text), Some(put)) = (pacer.take(), &put) else { continue };
                let text = clip(&text, STATUS_MAX);
                match put.put(channel, &text).await {
                    Ok(()) => pacer.tried(true, Instant::now()),
                    Err(PutError::Forbidden) => {
                        refused = true;
                        told(
                            "voice status: Discord refused it (403): the bot needs the Set Voice \
                             Channel Status permission in the voice channel; not retried this call"
                                .to_string(),
                        );
                    }
                    Err(PutError::Failed(why)) => {
                        tracing::debug!(why, "discord: voice status not set");
                        pacer.tried(false, Instant::now());
                    }
                }
            }
            _ = &mut stop => break,
            () = core.outbox.stopped() => break,
        }
    }
    core.bus.unwatch_work(&conn);
    // Only a status that was set is cleared: nothing to undo otherwise.
    if let (false, true, Some(put)) = (refused, pacer.written.is_some(), &put) {
        let _ = tokio::time::timeout(CLEAR_WAIT, put.put(channel, "")).await;
    }
}

/// Begin the watch and seed the map: the subscription first, so no change
/// falls between it and the snapshot. False when the board will not seed.
async fn watch(
    core: &Arc<Core>,
    conn: &str,
    tx: mpsc::UnboundedSender<Message>,
    (tally, tasks): (&Mutex<Tally>, Option<&Mutex<Tasks>>),
) -> bool {
    if let Err(e) = core.push.ensure(core).await {
        tracing::warn!(error = %e, "discord: voice status cannot read the work board");
        return false;
    }
    let scope = WorkScope::default();
    core.bus.watch_work(conn, tx, scope.clone(), false);
    // Every root that works or needs you, with its open subtree.
    let (_, views, _) = core.push.work_snapshot(0, &scope);
    tally.lock().unwrap().seed(&views);
    if let Some(tasks) = tasks {
        tasks.lock().unwrap().seed(&views);
    }
    true
}

/// A `work.changed` applied to the map, and heard by the tasks: true when a
/// count moved.
fn changed(
    m: &Message,
    (tally, tasks): (&Mutex<Tally>, Option<&Mutex<Tasks>>),
    core: &Core,
) -> bool {
    let Message::Notification(n) = m else {
        return false;
    };
    if n.method != notify::WORK_CHANGED {
        return false;
    }
    let Ok(c) = serde_json::from_value::<WorkChanged>(n.params.clone()) else {
        return false;
    };
    let mut t = tally.lock().unwrap();
    let mut moved = false;
    for v in &c.views {
        moved |= t.apply(v);
        if let Some(tasks) = tasks {
            tasks.lock().unwrap().see(core, v);
        }
    }
    for id in &c.gone {
        moved |= t.drop_id(id);
    }
    moved
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::json;

    /// A task's or a question's view, with only what the status reads.
    pub(in crate::runtime) fn view(
        id: &str,
        kind: &str,
        state: &str,
        title: &str,
        ask: Option<&str>,
    ) -> WorkView {
        let mut v = json!({
            "id": id, "kind": kind, "position": 1, "at_ms": 1, "root": "exe_root",
            "title": title, "state": state,
            "attention": {"level": "working", "label": "", "since_ms": 1},
            "started_at_ms": 1, "state_since_ms": 1,
        });
        if let Some(prompt) = ask {
            v["question"] = json!({
                "id": id, "kind": "approval",
                "asked_by": {"id": "exe_t", "kind": "task", "title": title},
                "prompt": prompt, "options": [], "floor": false, "may_answer": true,
                "asked_at_ms": 5,
            });
        }
        serde_json::from_value(v).expect("a work view")
    }

    #[test]
    fn the_status_reads_counts_and_nothing_else() {
        let c = |tasks, needs| Counts { tasks, needs }.status();
        assert_eq!(c(0, 0), "🎧 Theseus");
        assert_eq!(c(1, 0), "🎧 Theseus · ⚙️ 1 task");
        assert_eq!(c(2, 0), "🎧 Theseus · ⚙️ 2 tasks");
        assert_eq!(c(3, 1), "🎧 Theseus · ⚙️ 3 tasks · 🔴 1 needs you");
        assert_eq!(c(0, 2), "🎧 Theseus · 🔴 2 needs you");
        // A title or a path on the board never reaches the status.
        let mut t = Tally::default();
        t.apply(&view("exe_a", "task", "running", "Beta summary", None));
        t.apply(&view(
            "act_q",
            "question",
            "needs_you",
            "Beta summary",
            Some("fs.write create projects/beta-summary.txt (40 B)"),
        ));
        let text = t.counts().status();
        assert_eq!(text, "🎧 Theseus · ⚙️ 1 task · 🔴 1 needs you");
        assert!(
            !text.contains("Beta") && !text.contains("beta-summary"),
            "{text}"
        );
    }

    #[test]
    fn the_tally_follows_the_boards_changes_and_counts_only_open_work() {
        let mut t = Tally::default();
        assert!(t.apply(&view("exe_a", "task", "running", "A", None)));
        assert!(
            !t.apply(&view("exe_a", "task", "waiting", "A", None)),
            "still a task: no move"
        );
        assert!(t.apply(&view("exe_b", "task", "queued", "B", None)));
        assert_eq!(t.counts(), Counts { tasks: 2, needs: 0 });
        // A conversation, a step and a job are not tasks.
        for kind in ["conversation", "step", "job", "wake"] {
            assert!(!t.apply(&view("x", kind, "running", "X", None)), "{kind}");
        }
        assert!(t.apply(&view(
            "act_q",
            "question",
            "needs_you",
            "A",
            Some("fs.write x")
        )));
        assert_eq!(t.counts(), Counts { tasks: 2, needs: 1 });
        // An answered question and an ended task leave.
        assert!(t.apply(&view("act_q", "question", "done", "A", Some("fs.write x"))));
        assert!(t.apply(&view("exe_a", "task", "done", "A", None)));
        assert_eq!(t.counts(), Counts { tasks: 1, needs: 0 });
        assert!(t.drop_id("exe_b"));
        assert!(!t.drop_id("exe_b"));
        assert_eq!(t.counts(), Counts::default());
    }

    #[test]
    fn the_spoken_line_is_one_short_line_from_the_board() {
        let mut t = Tally::default();
        assert_eq!(t.spoken(), "no tasks working; nothing needs you");
        for i in 0..3 {
            t.apply(&view(&format!("exe_{i}"), "task", "running", "T", None));
        }
        assert_eq!(t.spoken(), "3 tasks working; nothing needs you");
        t.apply(&view(
            "act_q",
            "question",
            "needs_you",
            "Beta summary",
            Some("fs.write create projects/beta-summary.txt (40 B)"),
        ));
        assert_eq!(
            t.spoken(),
            "3 tasks working; 1 needs you: Beta summary asks fs.write create projects/beta-summary.txt (40 B)"
        );
        // Never past the cap, one line, whatever the words.
        t.apply(&view(
            "act_r",
            "question",
            "needs_you",
            "Gamma\nreport",
            Some(&"x".repeat(900)),
        ));
        let line = t.spoken();
        assert!(
            line.chars().count() <= SPOKEN_MAX,
            "{}",
            line.chars().count()
        );
        assert!(!line.contains('\n'));
        assert!(
            line.starts_with("3 tasks working; 2 need you, the oldest: "),
            "{line}"
        );
    }

    fn at(base: Instant, s: u64) -> Instant {
        base + Duration::from_secs(s)
    }

    #[test]
    fn the_pacer_debounces_waits_a_minute_and_writes_the_last_state() {
        let base = Instant::now();
        let mut p = Pacer::default();
        assert_eq!(p.due(), None, "nothing wanted, nothing due");
        // The first state waits the debounce, not the minute.
        p.want("A".into(), base);
        assert_eq!(p.due(), Some(at(base, 2)));
        // A change inside the debounce is the same write, holding the last.
        p.want("B".into(), at(base, 1));
        assert_eq!(p.due(), Some(at(base, 2)), "measured from the first change");
        assert_eq!(p.take().as_deref(), Some("B"));
        p.tried(true, at(base, 2));
        assert_eq!(p.due(), None, "written: nothing due");
        // The same state again moves nothing.
        p.want("B".into(), at(base, 10));
        assert_eq!(p.due(), None);
        // Two changes inside the minute: one write, at the minute, of the last.
        p.want("C".into(), at(base, 10));
        p.want("D".into(), at(base, 30));
        assert_eq!(p.due(), Some(at(base, 62)));
        assert_eq!(p.take().as_deref(), Some("D"));
        p.tried(true, at(base, 62));
        // A change back to what is written is no write.
        p.want("E".into(), at(base, 70));
        p.want("D".into(), at(base, 71));
        assert_eq!(p.due(), None);
        // After a quiet minute a change waits the debounce alone.
        p.want("F".into(), at(base, 200));
        assert_eq!(p.due(), Some(at(base, 202)));
        // A failed try waits a minute and keeps the state pending.
        p.tried(false, at(base, 202));
        assert_eq!(p.due(), Some(at(base, 262)));
    }
}
