//! A thread per task in a guild channel (theseus-i3do6; the plan's §1.8, 8:
//! guild tasks get threads, the task forum is offered and off by default).
//!
//! - **One thread a task.** A guild channel's place (`task_threads`, on by
//!   default; a DM has no threads) sees its tasks in the work model
//!   (`Tasks::all_of`) each time they change (`Place::sync_task_threads`). A
//!   task seen working with no thread gets one, once: `POST
//!   /channels/{id}/threads` through the binding's one helper
//!   (`guild_thread_create`, owned by the activity threads, theseus-sjzm),
//!   `type: 11` given and `auto_archive_duration: 1440`, named `⚙️ a1b2c3 ·
//!   <title>` within Discord's 100 characters. The channel gets one quiet line
//!   linking it. The task's session is then a place of its own: a lane (its own
//!   rate bucket), its events routed to it (`TaskThreads::place_of`, read by
//!   `route`), and its posts sent to it (`Outbox::bind_place`: its replies, its
//!   tool lines, its approval cards). When the task ends the thread is renamed
//!   once (`✅` done, `❌` failed or cancelled) and left to archive itself after
//!   a day of quiet; its place and lane go after `GRACE`.
//! - **The class is the parent's.** A task takes its place's class (the place
//!   rule), so the thread is bound with its parent's privacy, guild and
//!   ceiling (`bound_place`): a task in a shared channel never becomes private
//!   by moving into its thread. A task's title is shown only where its own
//!   place is: the work model files a task under its root conversation, and a
//!   place reads only its own.
//! - **What a restart finds.** The open threads are one META list
//!   (`discord.task_threads`, no store format: a key), written at a thread's
//!   creation, at its end and when its place goes. At the start each one's lane
//!   and bound place return before the courier refuses posts for places with no
//!   lane (`revive_task_threads`, from `start_lanes`), and the place itself
//!   starts at the first sync after the work model is read.
//! - **A message in the thread** is an operator message to the task
//!   (`Place::task_message`): `turn.submit` on the task's session, under the
//!   kernel's rule for it (`turn.rs`): a task that waits or is blocked is woken
//!   by the input; one that has ended refuses it (`execution_*`); one that
//!   runs would queue it behind its turn (a task's turn is no person's, so the
//!   steer, which joins a running turn, does not take it), and refuse it when
//!   that turn ends the task: so a running task's message is not sent to it but
//!   at once to the conversation that started the task, quoting the task,
//!   as is a refused one, and the thread says so.
//! - **Pace.** A create and a rename each cost one request; a task's changes
//!   cost none (the sync only reads the model). A failed create backs off
//!   `RETRY` and the task goes on in the channel as before.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_core::places::{BoundPlace, PlaceClass};
use theseus_protocol::work::{WorkState, WorkView};
use theseus_protocol::{PlaceCeiling, SessionRef, TurnSubmitParams, TurnSubmitResult};
use tokio::sync::mpsc;
use twilight_model::channel::thread::AutoArchiveDuration;
use twilight_model::id::marker::ChannelMarker;
use twilight_model::id::Id;

use super::inbound::{Inbound, Pending};
use super::{show, Place, PlaceMsg, Shared};
use crate::courier::LaneMsg;
use crate::render::board_v2;

/// A task thread archives itself after a day of quiet.
const ARCHIVE: AutoArchiveDuration = AutoArchiveDuration::Day;

/// Discord's longest thread name.
const NAME_MAX: usize = 100;

/// How long a finished task's place and lane stay, for the posts still on
/// their way (its report, a card's settle).
pub(crate) const GRACE: Duration = Duration::from_secs(600);

/// How long after a failed create the task is tried again.
const RETRY: Duration = Duration::from_secs(300);

/// A thread older than this and never finished is dropped from the list.
const STALE_MS: u64 = 7 * 24 * 3_600_000;

/// META key of the open task threads.
const OPEN_KEY: &str = "discord.task_threads";

/// A task's thread, as the list keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Made {
    /// The task's work id (`exe_…`) and its session.
    pub task: String,
    pub session: String,
    pub thread: u64,
    /// The channel it is in, and the conversation that started the task.
    pub parent: u64,
    pub parent_session: String,
    /// The thread's name now.
    pub name: String,
    pub guild: Option<u64>,
    /// The class the thread was bound with, until the next sync reads the
    /// parent's again.
    pub private: bool,
    #[serde(default)]
    pub ceiling: Option<PlaceCeiling>,
    pub created_ms: u64,
    /// When it was renamed at the task's end, once.
    #[serde(default)]
    pub ended_ms: Option<u64>,
}

/// A thread place's mailbox, made before the place so that the task's events,
/// heard from the moment the task is seen, wait in it while the thread is made.
type Mailbox = (
    mpsc::UnboundedSender<PlaceMsg>,
    mpsc::UnboundedReceiver<PlaceMsg>,
);

/// What a place needs to make a thread of its own kind.
#[derive(Clone)]
pub(super) struct Parent {
    pub channel: u64,
    pub key: String,
    pub target: String,
    pub label: String,
    pub session: String,
    pub users: Vec<u64>,
    pub show: show::Show,
}

/// How a task's thread stands: the mark its name carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Running,
    Done,
    Failed,
}

impl Mark {
    fn icon(self) -> &'static str {
        match self {
            Mark::Running => "⚙️",
            Mark::Done => "✅",
            Mark::Failed => "❌",
        }
    }

    pub(crate) fn of(state: WorkState) -> Self {
        match state {
            WorkState::Done => Mark::Done,
            WorkState::Failed | WorkState::Cancelled | WorkState::Expired => Mark::Failed,
            _ => Mark::Running,
        }
    }
}

/// A thread's name: `⚙️ a1b2c3 · Fix the flaky lifecycle test`, within
/// Discord's limit, the mark first.
pub(crate) fn task_name(mark: Mark, task: &str, title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let head = format!("{} {} · ", mark.icon(), board_v2::short(task));
    let room = NAME_MAX.saturating_sub(head.chars().count());
    let title: String = title.chars().take(room).collect();
    let title = if title.is_empty() {
        "task".into()
    } else {
        title
    };
    format!("{head}{title}")
}

/// What one sync does about one task.
#[derive(Debug, PartialEq)]
pub(super) enum Next {
    Nothing,
    Create,
    /// Its thread is in the list but its place is not running (a restart).
    Revive(Made),
    /// The task ended: its one rename.
    Finish(Made, Mark),
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    made: Vec<Made>,
    /// Task ids with a create, a revive or a finish under way.
    busy: HashSet<String>,
    /// Task ids whose create failed, until when.
    failed: HashMap<String, Instant>,
    /// Channels whose `task_threads` the bindings file turned off.
    off: HashSet<u64>,
    /// The running thread places, by the task's session.
    places: HashMap<String, mpsc::UnboundedSender<PlaceMsg>>,
    /// The threads' channel ids.
    threads: HashSet<u64>,
    /// Where each task's thread is, by task, for the board's link: kept
    /// after the thread's place goes (in memory; a restart keeps the open
    /// ones' through the list).
    urls: HashMap<String, String>,
}

/// The binding's task threads (theseus-i3do6), in `Shared`.
#[derive(Default)]
pub(crate) struct TaskThreads {
    inner: Mutex<Inner>,
}

impl TaskThreads {
    /// The list as the store has it, once.
    pub(crate) fn load(&self, store: &theseus_core::store::Store) {
        let mut i = self.inner.lock().unwrap();
        if i.loaded {
            return;
        }
        i.loaded = true;
        let now = theseus_protocol::now_unix_ms();
        let list: Vec<Made> = store.get_meta(OPEN_KEY).ok().flatten().unwrap_or_default();
        i.made = list
            .into_iter()
            .filter(|m| now.saturating_sub(m.created_ms) < STALE_MS)
            .collect();
        let threads: Vec<u64> = i.made.iter().map(|m| m.thread).collect();
        i.threads.extend(threads);
        let urls: Vec<(String, String)> = i
            .made
            .iter()
            .filter_map(|m| {
                let g = m.guild?;
                Some((
                    m.task.clone(),
                    format!("https://discord.com/channels/{g}/{}", m.thread),
                ))
            })
            .collect();
        i.urls.extend(urls);
    }

    /// A channel's `task_threads`, from the bindings file.
    pub(crate) fn configure(&self, channel: u64, on: bool) {
        let mut i = self.inner.lock().unwrap();
        if on {
            i.off.remove(&channel);
        } else {
            i.off.insert(channel);
        }
    }

    pub(super) fn enabled(&self, channel: u64) -> bool {
        !self.inner.lock().unwrap().off.contains(&channel)
    }

    /// `channel` is a task's thread.
    pub(crate) fn is_thread(&self, channel: u64) -> bool {
        self.inner.lock().unwrap().threads.contains(&channel)
    }

    /// A place here renders this task session.
    pub(crate) fn has_session(&self, session: &str) -> bool {
        self.inner.lock().unwrap().places.contains_key(session)
    }

    /// The thread place that runs the task session `session`.
    pub(super) fn place_of(&self, session: &str) -> Option<mpsc::UnboundedSender<PlaceMsg>> {
        self.inner.lock().unwrap().places.get(session).cloned()
    }

    /// The thread whose channel is `thread`.
    pub(super) fn made_in(&self, thread: u64) -> Option<Made> {
        let i = self.inner.lock().unwrap();
        i.made.iter().find(|m| m.thread == thread).cloned()
    }

    /// Every thread in the list.
    pub(super) fn all(&self) -> Vec<Made> {
        self.inner.lock().unwrap().made.clone()
    }

    /// What to do about the task `v` seen in the channel `parent`'s place.
    pub(super) fn next(&self, v: &WorkView) -> Next {
        let mut i = self.inner.lock().unwrap();
        if i.busy.contains(&v.id) {
            return Next::Nothing;
        }
        let made = i.made.iter().find(|m| m.task == v.id).cloned();
        match (made, v.state.ended()) {
            (None, false) => {
                if i.failed.get(&v.id).is_some_and(|t| Instant::now() < *t) {
                    return Next::Nothing;
                }
                if v.session_id.is_none() {
                    return Next::Nothing;
                }
                i.busy.insert(v.id.clone());
                Next::Create
            }
            (None, true) => Next::Nothing,
            (Some(m), false) => {
                if i.places.contains_key(&m.session) {
                    return Next::Nothing;
                }
                i.busy.insert(v.id.clone());
                Next::Revive(m)
            }
            (Some(m), true) => {
                if m.ended_ms.is_some() {
                    return Next::Nothing;
                }
                i.busy.insert(v.id.clone());
                Next::Finish(m, Mark::of(v.state))
            }
        }
    }

    fn settle(&self, task: &str) {
        self.inner.lock().unwrap().busy.remove(task);
    }

    /// The session's mailbox goes: its place never started.
    fn not_running(&self, session: &str) {
        self.inner.lock().unwrap().places.remove(session);
    }

    fn failed(&self, task: &str) {
        let mut i = self.inner.lock().unwrap();
        i.busy.remove(task);
        i.failed.insert(task.to_string(), Instant::now() + RETRY);
    }

    /// The link to the task's thread, if it had one.
    pub(super) fn url_of(&self, task: &str) -> Option<String> {
        self.inner.lock().unwrap().urls.get(task).cloned()
    }

    fn add(&self, m: Made) {
        let mut i = self.inner.lock().unwrap();
        if let Some(g) = m.guild {
            if i.urls.len() >= 512 {
                i.urls.clear();
            }
            i.urls.insert(
                m.task.clone(),
                format!("https://discord.com/channels/{g}/{}", m.thread),
            );
        }
        i.threads.insert(m.thread);
        i.made.retain(|x| x.task != m.task);
        i.made.push(m);
    }

    fn update(&self, task: &str, f: impl FnOnce(&mut Made)) {
        let mut i = self.inner.lock().unwrap();
        if let Some(m) = i.made.iter_mut().find(|m| m.task == task) {
            f(m);
        }
    }

    fn drop_made(&self, task: &str) -> Option<Made> {
        let mut i = self.inner.lock().unwrap();
        let at = i.made.iter().position(|m| m.task == task)?;
        let m = i.made.remove(at);
        i.threads.remove(&m.thread);
        i.places.remove(&m.session);
        i.failed.remove(task);
        Some(m)
    }

    fn running(&self, session: &str, tx: mpsc::UnboundedSender<PlaceMsg>) {
        self.inner
            .lock()
            .unwrap()
            .places
            .insert(session.to_string(), tx);
    }

    fn save(&self, store: &theseus_core::store::Store) {
        let list = self.all();
        if let Err(e) = store.put_meta(OPEN_KEY, &list) {
            tracing::warn!(error = %format!("{e:#}"), "the open task threads were not saved");
        }
    }
}

impl Shared {
    /// The class a thread of the parent's task is bound with: the parent's
    /// privacy, guild and ceiling, read now.
    fn bound_place(&self, p: &Parent, thread: u64, name: &str) -> BoundPlace {
        let rule = &self.core.runner.place_rule;
        let class = rule.class(&self.core.cfg, Some(&p.target));
        let bits = self.place_bits.get(&p.key);
        BoundPlace {
            target: format!("discord:channel:{thread}"),
            name: name.to_string(),
            private: class == PlaceClass::Private,
            guild: bits.guild,
            ceiling: bits.ceiling,
            persona: rule.persona_of(&p.target),
        }
    }

    /// The thread's lane, its bound place and where its task's posts go, from
    /// what the list keeps: the part of a thread that must stand before
    /// anything is posted to it (`stored` says what class to bind).
    fn task_lane(self: &Arc<Self>, m: &Made, label: &str, bound: BoundPlace) {
        let target = format!("discord:channel:{}", m.thread);
        if self.lane(&target).is_none() {
            let parent_target = format!("discord:channel:{}", m.parent);
            self.pings.set(&target, self.pings.of(&parent_target));
            self.start_lane(target, "channel", label.to_string(), Some(m.thread), None);
        }
        self.core.runner.place_rule.bind_more(bound);
        // Where the session's posts go, in memory: the record that says so
        // (`Outbox::bind_place`) is written once, when the thread is made.
        if let Err(e) = self
            .core
            .outbox
            .place_bound(&format!("channel:{}", m.thread), &m.session)
        {
            self.board
                .error("task thread", Some(&m.session), format!("{e:#}"));
        }
    }

    /// Lanes and bound places for the threads the last run left open, before
    /// the start refuses the posts of places it has no lane for. The place
    /// itself starts at the first sync (`Next::Revive`); a thread already
    /// finished leaves after its grace.
    pub(super) fn revive_task_threads(self: &Arc<Self>) {
        self.task_threads.load(&self.core.store);
        for m in self.task_threads.all() {
            let bound = BoundPlace {
                target: format!("discord:channel:{}", m.thread),
                name: m.name.clone(),
                private: m.private,
                guild: m.guild.map(|g| g.to_string()),
                ceiling: m.ceiling.clone(),
                persona: None,
            };
            self.task_lane(&m, &m.name, bound);
            if m.ended_ms.is_some() {
                self.retire_later(m.task.clone());
            }
        }
    }

    /// The thread place of the task, running: the actor that renders the
    /// task's session in its thread and takes the thread's messages.
    fn start_task_place(
        self: &Arc<Self>,
        m: &Made,
        p: &Parent,
        (tx, rx): Mailbox,
    ) -> anyhow::Result<()> {
        let key = format!("channel:{}", m.thread);
        let target = format!("discord:{key}");
        let bound = self.bound_place(p, m.thread, &m.name);
        self.task_lane(m, &m.name, bound);
        let lane = self
            .lane(&target)
            .ok_or_else(|| anyhow::anyhow!("no lane for {target}"))?;
        {
            let mut r = self.routes.lock().unwrap();
            r.by_channel.insert(m.thread, tx.clone());
            r.users.insert(m.thread, p.users.clone());
        }
        let place = Place {
            shared: self.clone(),
            key,
            target,
            kind: "channel",
            label: format!("{} {}", p.label, board_v2::short(&m.task)),
            channel: Some(Id::<ChannelMarker>::new(m.thread)),
            users: p.users.clone(),
            mention_only: false,
            session_id: m.session.clone(),
            renderer: self.renderer(),
            view: show::View::new(p.show),
            lane,
            inflight: false,
            queued: Vec::new(),
            saw_failure: false,
            stopped_turn: None,
            stopping: false,
            last_activity_ms: 0,
            fresh: false,
            tx,
        };
        place.report();
        tokio::spawn(place.run(rx));
        Ok(())
    }

    /// The task's thread, made now: created, listed, started as a place and
    /// linked from the channel.
    async fn make_task_thread(self: &Arc<Self>, p: Parent, v: WorkView) {
        let Some(session) = v.session_id.clone() else {
            self.task_threads.settle(&v.id);
            return;
        };
        let name = task_name(Mark::of(v.state), &v.id, &v.title);
        // The task's events are heard from now: they wait in the place's
        // mailbox while the thread is made (live progress is best effort, but
        // a task's first lines are the ones worth keeping).
        let mailbox = mpsc::unbounded_channel();
        self.task_threads.running(&session, mailbox.0.clone());
        self.watch(&session).await;
        match self
            .guild_thread_create(p.channel, &name, "", ARCHIVE)
            .await
        {
            Ok((thread, guild)) => {
                let bound = self.bound_place(&p, thread, &name);
                let m = Made {
                    task: v.id.clone(),
                    session,
                    thread,
                    parent: p.channel,
                    parent_session: p.session.clone(),
                    name,
                    guild,
                    private: bound.private,
                    ceiling: bound.ceiling,
                    created_ms: theseus_protocol::now_unix_ms(),
                    ended_ms: None,
                };
                self.task_threads.add(m.clone());
                self.task_threads.save(&self.core.store);
                if let Err(e) = self
                    .core
                    .outbox
                    .bind_place(&format!("channel:{thread}"), &m.session)
                {
                    self.board
                        .error("task thread", Some(&m.session), format!("{e:#}"));
                }
                if let Err(e) = self.start_task_place(&m, &p, mailbox) {
                    self.board
                        .error("task thread", Some(&m.session), format!("{e:#}"));
                }
                // One quiet line in the channel, linking it.
                let line = format!("🧵 <#{thread}> · {}", clip(&v.title));
                self.say_pointer(p.channel, line, &p.session);
                self.task_threads.settle(&v.id);
                // The task may have ended while the thread was made.
                self.tell_channel(p.channel, PlaceMsg::Tasks);
            }
            Err(e) => {
                self.board
                    .error("task thread", Some(&session), format!("{e:#}"));
                self.task_threads.not_running(&session);
                self.task_threads.failed(&v.id);
            }
        }
    }

    /// A thread whose place stopped (a restart): run it again.
    async fn revive_task_thread(self: &Arc<Self>, p: Parent, m: Made) {
        let mailbox = mpsc::unbounded_channel();
        self.task_threads.running(&m.session, mailbox.0.clone());
        self.watch(&m.session).await;
        if let Err(e) = self.start_task_place(&m, &p, mailbox) {
            self.board
                .error("task thread", Some(&m.session), format!("{e:#}"));
        }
        self.task_threads.settle(&m.task);
    }

    /// The task ended: the thread's one rename, and its place goes after the
    /// grace. The thread archives itself after its day of quiet.
    async fn finish_task_thread(self: &Arc<Self>, m: Made, mark: Mark, title: String) {
        let name = task_name(mark, &m.task, &title);
        if name != m.name {
            if let Err(e) = self.guild_thread_set(m.thread, None, Some(&name)).await {
                self.board
                    .error("task thread", Some(&m.session), format!("{e:#}"));
            }
        }
        self.task_threads.update(&m.task, |x| {
            x.name.clone_from(&name);
            x.ended_ms = Some(theseus_protocol::now_unix_ms());
        });
        self.task_threads.save(&self.core.store);
        self.task_threads.settle(&m.task);
        self.retire_later(m.task);
    }

    /// The place and the lane of a finished task, after `GRACE`.
    fn retire_later(self: &Arc<Self>, task: String) {
        let s = self.clone();
        tokio::spawn(async move {
            let stopped = s.core.outbox.stopped();
            tokio::pin!(stopped);
            tokio::select! {
                () = tokio::time::sleep(GRACE) => {}
                () = &mut stopped => return,
            }
            s.retire_task_thread(&task).await;
        });
    }

    /// The thread's place, routes, lane and listing go.
    pub(super) async fn retire_task_thread(self: &Arc<Self>, task: &str) {
        let Some(m) = self.task_threads.drop_made(task) else {
            return;
        };
        self.task_threads.save(&self.core.store);
        let target = format!("discord:channel:{}", m.thread);
        let place = {
            let mut r = self.routes.lock().unwrap();
            r.users.remove(&m.thread);
            r.mention_only.remove(&m.thread);
            r.by_channel.remove(&m.thread)
        };
        if let Some(tx) = place {
            let _ = tx.send(PlaceMsg::Unbind);
        }
        {
            let lanes = self.lanes.lock().unwrap();
            if let Some(lane) = lanes.get(&target) {
                self.retired.lock().unwrap().insert(target.clone());
                let _ = lane.send(LaneMsg::Wake);
            }
        }
        self.core.outbox.place_unbound(&m.session);
        let _ = self
            .rpc
            .call::<_, serde_json::Value>(
                theseus_protocol::method::SESSION_UNWATCH,
                SessionRef {
                    session_id: m.session,
                },
            )
            .await;
    }

    /// A message for the place of the channel `channel`, if one runs.
    fn tell_channel(&self, channel: u64, m: PlaceMsg) {
        let tx = self
            .routes
            .lock()
            .unwrap()
            .by_channel
            .get(&channel)
            .cloned();
        if let Some(tx) = tx {
            let _ = tx.send(m);
        }
    }
}

/// A title for a line of the channel: one line, short.
fn clip(title: &str) -> String {
    let one = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = one.chars().take(120).collect();
    if short.is_empty() {
        "task".into()
    } else {
        short
    }
}

impl Place {
    /// The parts of this place a thread of its task is made from, if it is a
    /// guild channel that wants task threads (and no thread itself).
    fn parent_of_tasks(&self) -> Option<Parent> {
        let channel = self.channel?.get();
        let s = &self.shared;
        let wants = self.kind == "channel"
            && s.task_threads.enabled(channel)
            && !s.task_threads.is_thread(channel)
            && s.threads.slug_of(channel).is_none()
            && !self.session_id.is_empty();
        wants.then(|| Parent {
            channel,
            key: self.key.clone(),
            target: self.target.clone(),
            label: self.label.clone(),
            session: self.session_id.clone(),
            users: self.users.clone(),
            show: self.view.show(),
        })
    }

    /// This place's tasks, as threads: one made for each task working, one
    /// rename for each that ended, a thread whose place stopped run again.
    pub(super) fn sync_task_threads(&self) {
        let Some(p) = self.parent_of_tasks() else {
            return;
        };
        let s = &self.shared;
        s.task_threads.load(&s.core.store);
        for v in s.tasks.all_of(&self.session_id) {
            let s = s.clone();
            let p = p.clone();
            match s.task_threads.next(&v) {
                Next::Nothing => {}
                Next::Create => {
                    tokio::spawn(async move { s.make_task_thread(p, v).await });
                }
                Next::Revive(m) => {
                    tokio::spawn(async move { s.revive_task_thread(p, m).await });
                }
                Next::Finish(m, mark) => {
                    tokio::spawn(async move { s.finish_task_thread(m, mark, v.title).await });
                }
            }
        }
    }

    /// A message in the task's thread: an operator message to the task. The
    /// kernel's rule (see the module doc) decides whether the task takes it;
    /// if not it goes to the conversation that started the task, quoting it.
    pub(super) fn task_message(&mut self, m: Inbound) {
        self.last_activity_ms = theseus_protocol::now_unix_ms();
        self.shared.core.binding_ledger(
            theseus_protocol::LedgerKind::DiscordMessageIn,
            Some(&self.session_id),
            json!({"place": self.label, "author": m.author, "chars": m.text.chars().count(),
                   "message_id": m.message.to_string(), "task_thread": true}),
        );
        let _ = self.lane.send(LaneMsg::Author(m.author_id));
        let Some(made) = self.shared.task_threads.made_in(m.channel.get()) else {
            return;
        };
        // A task whose turn is running takes no input now: the kernel would
        // queue it behind the turn, and a task's turn that ends the task
        // refuses what waited ("complete, not runnable"), long after it was
        // said. So the conversation that started the task hears it at once.
        let running = self.shared.tasks.view(&made.task).is_some_and(|v| {
            matches!(
                v.state,
                theseus_protocol::work::WorkState::Running
                    | theseus_protocol::work::WorkState::Queued
            )
        });
        let (rpc, shared, thread) = (
            self.shared.rpc.clone(),
            self.shared.clone(),
            m.channel.get(),
        );
        let (author, text, message) = (format!("discord:{}", m.author), m.text.clone(), m.message);
        let pending: Vec<Pending> = m.files.into_iter().collect();
        tokio::spawn(async move {
            let mut attachments = Vec::new();
            for p in pending {
                attachments.extend(p.wait().await);
            }
            let why = if running {
                "the task is running".to_string()
            } else {
                let to_task = TurnSubmitParams {
                    session_id: Some(made.session.clone()),
                    input: text.clone(),
                    author: Some(author.clone()),
                    attachments: attachments.clone(),
                    reply_to: Some(message.to_string()),
                    ..Default::default()
                };
                let r = rpc
                    .call::<_, TurnSubmitResult>(theseus_protocol::method::TURN_SUBMIT, to_task)
                    .await;
                let Err(e) = r else { return };
                let class = e.data.get("class").and_then(serde_json::Value::as_str);
                let ended = e.code == theseus_protocol::error_code::NOT_FOUND
                    || class.is_some_and(|c| c.starts_with("execution_"));
                if !ended {
                    // The task took it and the turn failed: the thread says so.
                    shared.say_pointer(thread, format!("-# ⚠️ {}", e.message), &made.session);
                    return;
                }
                "the task is over".to_string()
            };
            // The conversation that started the task takes the message, with
            // the task named.
            let said = format!(
                "[Written in the thread of the task “{}” (…{}), and {why}] {text}",
                made.name,
                board_v2::short(&made.task),
            );
            let to_parent = TurnSubmitParams {
                session_id: Some(made.parent_session.clone()),
                input: said,
                author: Some(author),
                attachments,
                ..Default::default()
            };
            let sent = rpc
                .call::<_, TurnSubmitResult>(theseus_protocol::method::TURN_SUBMIT, to_parent)
                .await;
            let line = match sent {
                Ok(_) => format!("-# ↪ {why}: sent to the conversation that started it"),
                Err(e) => format!("-# ⚠️ nothing took it: {}", e.message),
            };
            shared.say_pointer(thread, line, &made.session);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tests::core_with;
    use crate::runtime::voice_status::tests::view;

    fn made(task: &str, thread: u64) -> Made {
        Made {
            task: task.into(),
            session: format!("ses_{task}"),
            thread,
            parent: 223_456_789_012_345_678,
            parent_session: "ses_parent".into(),
            name: task_name(Mark::Running, task, "Chart the reef"),
            guild: Some(100_000_000_000_000_001),
            private: true,
            ceiling: None,
            created_ms: theseus_protocol::now_unix_ms(),
            ended_ms: None,
        }
    }

    fn task(id: &str, state: &str) -> WorkView {
        let mut v = view(id, "task", state, "Chart the reef", None);
        v.session_id = Some(format!("ses_{id}"));
        v
    }

    /// What a restart finds: the open threads come back from the store, a
    /// task seen working with its thread listed gets its place started again
    /// (never a second thread), one that ended meanwhile gets its one rename,
    /// and a task over with no thread gets none.
    #[tokio::test]
    async fn the_open_threads_come_back_from_the_store_and_no_second_thread_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_with(
            dir.path(),
            theseus_core::secrets::SecretBoard::empty(),
            |_| {},
        );
        let thread = 900_000_000_000_500_007;
        core.store
            .put_meta(OPEN_KEY, &vec![made("exe_reef", thread)])
            .unwrap();
        let shared = crate::runtime::shared_for_tests(&core);
        shared.revive_task_threads();
        let t = &shared.task_threads;
        assert_eq!(t.all().first().map(|m| m.thread), Some(thread));
        assert!(t
            .url_of("exe_reef")
            .is_some_and(|u| u.ends_with(&thread.to_string())));
        assert!(t.is_thread(thread));
        // Its lane stands and its task's posts go there, bound as private,
        // before anything is posted.
        let target = format!("discord:channel:{thread}");
        assert!(shared.lane(&target).is_some(), "the thread has a lane");
        assert_eq!(
            core.outbox.target("ses_exe_reef").as_deref(),
            Some(target.as_str())
        );
        assert_eq!(
            core.runner.place_rule.class(&core.cfg, Some(&target)),
            PlaceClass::Private,
            "the class it was bound with"
        );
        // Working, with no place yet: the place is started again.
        assert!(
            matches!(t.next(&task("exe_reef", "running")), Next::Revive(m) if m.thread == thread)
        );
        assert_eq!(t.next(&task("exe_reef", "running")), Next::Nothing, "once");
        t.settle("exe_reef");
        // Over meanwhile: the one rename, with its mark; then nothing.
        assert!(matches!(
            t.next(&task("exe_reef", "failed")),
            Next::Finish(_, Mark::Failed)
        ));
        t.update("exe_reef", |m| m.ended_ms = Some(1));
        t.settle("exe_reef");
        assert_eq!(t.next(&task("exe_reef", "failed")), Next::Nothing);
        // A new task gets a thread, an old one over with none gets none.
        assert_eq!(t.next(&task("exe_new", "running")), Next::Create);
        assert_eq!(t.next(&task("exe_old", "done")), Next::Nothing);
        // A failed create waits before it is tried again.
        t.failed("exe_new");
        assert_eq!(t.next(&task("exe_new", "running")), Next::Nothing);
    }
}
