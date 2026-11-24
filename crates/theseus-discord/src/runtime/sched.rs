//! Scheduled items as cards (issue 30; `docs/design/board-place.md`): a
//! reminder, a check-in Theseus set for itself, and a routine are each shown
//! the way a task is. A child of `runtime`.
//!
//! - **A card of its own in the task home**, one new message per item, posted
//!   when the item is set and never an edit of a shared message. It is edited
//!   for its own item's life only (snoozed, fired, cancelled, a routine paused
//!   or removed); no ping, no mention. The words are `render::sched`'s.
//! - **A post in the default forum**, as a task has, through the forum worker
//!   (`forum.rs`): the item's `Face` is the post's card, a `scheduled` tag
//!   says what state it is in, and at the end it is tagged and renamed as a
//!   task's is. The forum is private or it is not used (`forum::takes`).
//! - **One worker, one request at a time.** It wakes when the future may have
//!   changed (`Future::dirty`), and reads the pending wakes and routines the
//!   future's snapshot holds, and the states the binding's work model holds
//!   (fired, cancelled). A write that fails is tried again after a wait that
//!   doubles to a minute, and the pass stops at the first failure. A pass
//!   that makes many cards makes the first `BURST` at once and the rest a
//!   `SPACE` apart, so the channel keeps room for a task's report, and no
//!   reply waits for any of it.
//! - **A restart** finds each card in one META list (`discord.sched`: the
//!   item, its card, the digest last sent), makes none twice (a create carries
//!   the item's nonce), and brings a card whose item fired or was cancelled
//!   while the daemon was down to its true state once, after the work model
//!   has been read (`Tasks::loaded`).
//! - **Where it is used**: only where the task home stands and the owner's DM
//!   place draws its board (`Shared::sched_target`). Elsewhere the place's own
//!   board shows its own reminders, as before.
//! - **A press** on a card is `wake.cancel` or `wake.snooze` as the presser
//!   (`Shared::card_press`), or `task.cancel` or `task.reschedule` for a task
//!   set to start later, after the gate every press on a place passes: the
//!   home's people are the owners. A routine's buttons are the board's.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::{now_unix_ms, Attention, Level};
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::application::interaction::Interaction;

mod intent;

use super::Shared;
use crate::courier::{nonce, SendErr};
use crate::render::horizon::CANCEL_PREFIX;
use crate::render::sched::{Face, Kind, Life};
use crate::render::v2;

/// META key of the cards.
const KEY: &str = "discord.sched";

/// A burst of changes is one pass: wait this long after the first.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// With nothing said, the items are looked at again after this long.
const IDLE: Duration = Duration::from_secs(60);

/// A failed write is tried again after this long, doubling to `RETRY_MAX`.
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// A card whose item is over stays in the list this long, for a late press.
const KEPT_MS: u64 = 24 * 3_600_000;

/// How many of the newest cancel rows are read to find who cancelled an item.
const ROWS_READ: usize = 500;

/// The first cards of a pass are made at once; each after them waits this long
/// after the one before. Discord gives a channel five new messages in five
/// seconds and a task's report is one of them: 20 items at once are the first 5
/// now and the other 15 over 30 s, one every 2 s, leaving the channel half its
/// bucket.
const BURST: usize = 5;
const SPACE: Duration = Duration::from_secs(2);

#[cfg(test)]
thread_local! {
    static GAP: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

/// A test shortens the gap for its thread.
#[cfg(test)]
pub(crate) fn space_for_tests(d: Duration) {
    GAP.with(|g| g.set(Some(d)));
}

/// The gap now, read where the worker starts (a test's override is the calling
/// thread's, not the task's).
fn space() -> Duration {
    #[cfg(test)]
    if let Some(d) = GAP.with(std::cell::Cell::get) {
        return d;
    }
    SPACE
}

/// An item that ended this recently, with no card yet, is given one in its
/// final state: the owner must be able to see that it was set.
const LATE_MS: u64 = 3_600_000;

/// One item's card, as the list keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Rec {
    pub id: String,
    /// `reminder`, `check-in`, `routine` or `task`.
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub every: Option<String>,
    pub channel: u64,
    pub guild: String,
    pub card: u64,
    pub created_ms: u64,
    #[serde(default)]
    pub due_ms: Option<u64>,
    #[serde(default)]
    pub snoozed: bool,
    #[serde(default)]
    pub ended_ms: Option<u64>,
    /// Who cancelled it, from a press or the item's own record.
    #[serde(default)]
    pub by: Option<String>,
    /// The digest of the card as last sent.
    #[serde(default)]
    pub digest: u64,
}

/// What the other workers need of the schedule, in `TaskPosts`.
#[derive(Default)]
pub(crate) struct Sched {
    /// Each item's face as last drawn, for the forum post's card.
    faces: Mutex<HashMap<String, Face>>,
    /// Who cancelled a wake or a task by a press, until its card says so.
    cancelled_by: Mutex<HashMap<String, String>>,
}

impl Sched {
    pub(crate) fn face(&self, id: &str) -> Option<Face> {
        self.faces.lock().unwrap().get(id).cloned()
    }

    /// A press cancels `wake`: remember who, for the card the worker brings
    /// to its true state.
    pub(crate) fn will_cancel(&self, wake: &str, who: &str) {
        if let Some(who) = crate::render::sched::who_of(who) {
            self.cancelled_by
                .lock()
                .unwrap()
                .insert(wake.to_string(), who);
        }
    }

    /// The press did not cancel it.
    pub(crate) fn not_cancelled(&self, wake: &str) {
        self.cancelled_by.lock().unwrap().remove(wake);
    }

    fn who_cancelled(&self, wake: &str) -> Option<String> {
        self.cancelled_by.lock().unwrap().get(wake).cloned()
    }
}

fn digest(c: &[Value]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(c).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// A wake that is a reminder or a check-in: a watch's and a routine's own
/// wakes are not (`horizon::is_wake_of_conversation`).
fn is_item(v: &WorkView) -> bool {
    v.kind == WorkKind::Wake
        && v.id.starts_with("wak_")
        && !matches!(v.purpose.as_deref(), Some("watch" | "routine"))
}

impl Shared {
    /// A test says the bot's own id is, or is not, known yet.
    #[cfg(test)]
    pub(crate) fn set_bot_id_for_tests(&self, id: u64) {
        self.bot_id.store(id, std::sync::atomic::Ordering::Relaxed);
    }

    /// Where the cards go: the task home's channel, guild and name, while it
    /// stands and the owner's DM place draws into it (and the bot can send V2).
    pub(crate) fn sched_target(&self) -> Option<(u64, String, String)> {
        if !self.v2_cards() || self.owner_dm_place().is_none() {
            return None;
        }
        self.task_posts.home.target()
    }

    /// The name of the conversation a task was set in: its root's session.
    fn task_from(&self, v: &WorkView, names: &HashMap<String, String>) -> Option<String> {
        let root = self.tasks.view(&v.root)?;
        names.get(root.session_id.as_deref()?).cloned()
    }

    /// The items now: the pending wakes and the routines the future's
    /// snapshot holds, and the wakes the work model holds in any state (a
    /// fired one is `Done`, a cancelled one `Cancelled`). None before the
    /// snapshot was first read.
    fn schedule_now(&self, recs: &[Rec]) -> Option<Vec<Face>> {
        let snap = self.tasks.future.snapshot();
        if snap.at_ms == 0 {
            return None;
        }
        let names = self.conversation_names();
        let sched = &self.task_posts.sched;
        let mut out: Vec<Face> = Vec::new();
        let item_line = |l: &theseus_protocol::work::horizon::AgendaLine| {
            l.kind == WorkKind::Wake
                && l.id.starts_with("wak_")
                && !matches!(l.purpose.as_deref(), Some("watch" | "routine"))
        };
        // What the agenda says is pending is pending, whatever the model's
        // view of the id says: a fired reminder set again under its own id is
        // the agenda's line, not the `Done` a stale view may still hold. Only
        // a line read AFTER the view ended counts: one read before it fired
        // is the old truth, not a set-again.
        let pending: std::collections::HashSet<&str> = snap
            .lines
            .iter()
            .filter(|l| item_line(l))
            .map(|l| l.id.as_str())
            .collect();
        for v in self.tasks.scheduled().iter().filter(|v| is_item(v)) {
            let ended_at = v.ended_at_ms.unwrap_or(v.state_since_ms);
            if v.state.ended() && pending.contains(v.id.as_str()) && snap.at_ms >= ended_at {
                continue;
            }
            let rec = recs.iter().find(|r| r.id == v.id);
            out.push(wake_face(v, rec, &names, sched));
        }
        // A wake the snapshot holds and the model does not (yet, or not as
        // pending): its line.
        let mut lines = snap.lines.clone();
        lines.sort_by_key(|l| l.at_ms);
        for l in lines {
            let known = out.iter().any(|f| f.id == l.id);
            if known || !item_line(&l) {
                continue;
            }
            let rec = recs.iter().find(|r| r.id == l.id);
            let moved = rec.is_some_and(|r| {
                r.snoozed
                    || (l.occurrence.is_none()
                        && r.due_ms.is_some_and(|was| was.abs_diff(l.at_ms) > 1_000))
            });
            out.push(Face {
                id: l.id.clone(),
                kind: kind_of(l.purpose.as_deref()),
                title: l.title.clone(),
                due_ms: Some(l.at_ms),
                life: Life::Scheduled { snoozed: moved },
                from: l
                    .session_id
                    .as_ref()
                    .and_then(|s| names.get(s))
                    .cloned()
                    .or_else(|| rec.and_then(|r| r.from.clone())),
                every: None,
                paused: false,
                routine: None,
            });
        }
        // A task set to start later is a scheduled item too: one card of its
        // own, and no second post (the task has its own). One that began or
        // was cancelled has its card brought to that.
        for v in self.tasks.waiting_to_start() {
            let rec = recs.iter().find(|r| r.id == v.id);
            out.push(task_face(&v, rec, self.task_from(&v, &names), sched));
        }
        for r in recs
            .iter()
            .filter(|r| r.kind == "task" && r.ended_ms.is_none())
        {
            if out.iter().any(|f| f.id == r.id) {
                continue;
            }
            if let Some(v) = self.tasks.view(&r.id) {
                out.push(task_face(&v, Some(r), self.task_from(&v, &names), sched));
            }
        }
        for r in &snap.routines {
            out.push(Face {
                id: r.id.clone(),
                kind: Kind::Routine,
                title: r.title.clone(),
                due_ms: r.next_ms,
                life: Life::Scheduled { snoozed: false },
                from: None,
                every: Some(r.series.clone()),
                paused: r.paused,
                routine: Some((r.clone(), snap.records.get(&r.id).copied())),
            });
        }
        self.complete_notes(&mut out, recs)?;
        Some(out)
    }

    fn complete_notes(&self, out: &mut [Face], recs: &[Rec]) -> Option<()> {
        // The work view's title is already shortened. Recover the complete
        // reminder text before the exit masks it and the card clips it. Keep
        // that raw text in the existing card record for a later ended edit.
        let notes: HashMap<_, _> = self
            .core
            .kernel
            .pending_wakes()
            .ok()?
            .into_iter()
            .map(|(_, w)| (w.id, w.note))
            .collect();
        for f in out {
            if matches!(f.kind, Kind::Reminder | Kind::CheckIn) {
                if let Some(note) = notes
                    .get(&f.id)
                    .or_else(|| recs.iter().find(|r| r.id == f.id).map(|r| &r.title))
                {
                    f.title.clone_from(note);
                }
            }
        }
        Some(())
    }
}

fn kind_of(purpose: Option<&str>) -> Kind {
    match purpose {
        Some("remind") => Kind::Reminder,
        _ => Kind::CheckIn,
    }
}

/// A wake view's face. `snoozed` once its time moved (a snooze sets it an hour
/// from now, which may be earlier than it was; a series moves on by itself and
/// is not snoozed).
fn wake_face(
    v: &WorkView,
    rec: Option<&Rec>,
    names: &HashMap<String, String>,
    sched: &Sched,
) -> Face {
    let every = v
        .series
        .as_ref()
        .and_then(|s| s.every.clone().or_else(|| s.cron.clone()));
    let due = v.next_at_ms;
    let ended = v.ended_at_ms.unwrap_or(v.state_since_ms);
    let life = match v.state {
        WorkState::Done => Life::Fired(ended),
        WorkState::Cancelled | WorkState::Expired => Life::Cancelled(
            ended,
            sched
                .who_cancelled(&v.id)
                .or_else(|| rec.and_then(|r| r.by.clone())),
        ),
        _ => Life::Scheduled {
            snoozed: rec.is_some_and(|r| {
                r.snoozed
                    || (every.is_none()
                        && matches!((r.due_ms, due), (Some(was), Some(now)) if now.abs_diff(was) > 1_000))
            }),
        },
    };
    Face {
        id: v.id.clone(),
        kind: kind_of(v.purpose.as_deref()),
        title: v.title.clone(),
        due_ms: due.or(rec.and_then(|r| r.due_ms)),
        life,
        from: v
            .session_id
            .as_ref()
            .and_then(|s| names.get(s))
            .cloned()
            .or_else(|| rec.and_then(|r| r.from.clone())),
        every,
        paused: v.paused,
        routine: None,
    }
}

/// A task's face: scheduled while it waits for its start, else how its start
/// went (it began, or it was cancelled before it).
fn task_face(v: &WorkView, rec: Option<&Rec>, from: Option<String>, sched: &Sched) -> Face {
    let due = v.next_at_ms.or(rec.and_then(|r| r.due_ms));
    let ended = v.ended_at_ms.unwrap_or(v.state_since_ms);
    let life = if crate::render::task_view::waits_to_start(v) {
        Life::Scheduled {
            snoozed: rec.is_some_and(|r| {
                r.snoozed
                    || matches!((r.due_ms, v.next_at_ms), (Some(was), Some(now)) if now.abs_diff(was) > 1_000)
            }),
        }
    } else if matches!(v.state, WorkState::Cancelled | WorkState::Expired) {
        Life::Cancelled(
            ended,
            sched
                .who_cancelled(&v.id)
                .or_else(|| rec.and_then(|r| r.by.clone())),
        )
    } else {
        Life::Started(due.map_or_else(now_unix_ms, |d| d.min(now_unix_ms())))
    };
    Face {
        id: v.id.clone(),
        kind: Kind::Task,
        title: v.title.clone(),
        due_ms: due,
        life,
        from: from.or_else(|| rec.and_then(|r| r.from.clone())),
        every: None,
        paused: v.paused,
        routine: None,
    }
}

/// The work view the forum worker takes a face for: a wake or a routine, its
/// state the item's life.
fn view_of(f: &Face, created_ms: u64, now_ms: u64) -> WorkView {
    WorkView {
        moved_to: None,
        forum_url: None,
        id: f.id.clone(),
        kind: if f.kind == Kind::Routine {
            WorkKind::Routine
        } else {
            WorkKind::Wake
        },
        position: now_ms,
        at_ms: now_ms,
        parent: None,
        root: f.id.clone(),
        session_id: None,
        title: f.title.clone(),
        place: None,
        state: match f.life {
            Life::Scheduled { .. } => WorkState::Waiting,
            Life::Fired(_) => WorkState::Done,
            Life::Started(_) => WorkState::Running,
            Life::Cancelled(..) | Life::Removed(_) => WorkState::Cancelled,
        },
        previous: None,
        attention: Attention {
            level: Level::Working,
            label: String::new(),
            since_ms: now_ms,
        },
        started_at_ms: created_ms,
        state_since_ms: created_ms,
        ended_at_ms: match f.life {
            Life::Scheduled { .. } | Life::Started(_) => None,
            Life::Fired(at) | Life::Cancelled(at, _) | Life::Removed(at) => Some(at),
        },
        doing: None,
        progress: None,
        question: None,
        cost: None,
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        expect: None,
        flagged: false,
        dismissed: false,
        read: false,
        retry_of: None,
        forecast: None,
        purpose: None,
        next_at_ms: f.due_ms,
        series: None,
        paused: f.paused,
        routine: None,
        watch: None,
        project: None,
        data: None,
    }
}

pub(crate) struct Worker {
    shared: Arc<Shared>,
    recs: Vec<Rec>,
    /// Cards about to be made, or made and not yet recorded (`intent.rs`).
    intents: Vec<intent::Intent>,
    retry: Duration,
    loaded: bool,
    /// Cards made in this pass, and the gap kept between those after the first
    /// `BURST`.
    made: usize,
    space: Duration,
    /// The last failure was a read for an intent (the pass goes on with the other
    /// items, and makes nothing for that one).
    read_failed: bool,
}

/// Start the worker, once. It ends with the daemon's stop.
pub(super) fn start(shared: &Arc<Shared>) {
    let s = shared.clone();
    tokio::spawn(async move {
        let stopped = s.core.outbox.stopped();
        tokio::pin!(stopped);
        let mut w = Worker {
            shared: s.clone(),
            recs: Vec::new(),
            intents: Vec::new(),
            retry: Duration::ZERO,
            loaded: false,
            made: 0,
            space: space(),
            read_failed: false,
        };
        let mut wait = IDLE;
        loop {
            tokio::select! {
                () = s.tasks.future.sched.notified() => {}
                () = tokio::time::sleep(wait) => {}
                () = &mut stopped => return,
            }
            tokio::time::sleep(DEBOUNCE).await;
            wait = if w.sync().await { IDLE } else { w.retry };
        }
    });
}

/// Where a card goes, as `sync` found it.
#[derive(Clone)]
struct Home {
    channel: u64,
    guild: String,
    label: String,
}

impl Worker {
    /// A worker that has read nothing, as a restarted process has (a test's
    /// second pass over a list the first one wrote).
    #[cfg(test)]
    pub(crate) fn fresh(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            recs: Vec::new(),
            intents: Vec::new(),
            retry: Duration::ZERO,
            loaded: false,
            made: 0,
            space: space(),
            read_failed: false,
        }
    }

    fn load(&mut self) {
        if !self.loaded {
            self.loaded = true;
            self.recs = self
                .shared
                .core
                .store
                .get_meta::<Vec<Rec>>(KEY)
                .ok()
                .flatten()
                .unwrap_or_default();
            let held = self
                .shared
                .core
                .store
                .get_meta::<Vec<intent::Intent>>(intent::KEY)
                .ok()
                .flatten()
                .unwrap_or_default();
            self.intents = intent::live(held.clone(), &self.recs);
            if self.intents.len() != held.len() {
                self.save_intents();
            }
        }
    }

    fn save(&self) {
        if let Err(e) = self.shared.core.store.put_meta(KEY, &self.recs) {
            tracing::warn!(error = %format!("{e:#}"), "the schedule cards were not saved");
        }
    }

    /// A write failed: wait before the next try, doubling.
    fn failed(&mut self, what: &str, e: impl std::fmt::Display) {
        self.shared.board.error(what, None, e);
        self.retry = if self.retry.is_zero() {
            RETRY_FIRST
        } else {
            (self.retry * 2).min(RETRY_MAX)
        };
    }

    /// Bring the cards up to date, one request at a time, and stop at the
    /// first that fails. True when nothing failed.
    pub(crate) async fn sync(&mut self) -> bool {
        self.load();
        let s = self.shared.clone();
        let Some((channel, guild, label)) = s.sched_target() else {
            return true;
        };
        let home = Home {
            channel,
            guild,
            label,
        };
        self.retry = Duration::ZERO;
        self.made = 0;
        let now = now_unix_ms();
        let Some(mut faces) = s.schedule_now(&self.recs) else {
            return true;
        };
        // An item that is over has no card to find, and its intent goes: by
        // the item's end, never by age (a card does not archive).
        if s.tasks.loaded() {
            let held = self.intents.len();
            self.intents = intent::standing(std::mem::take(&mut self.intents), &faces);
            if self.intents.len() != held {
                self.save_intents();
            }
        }
        let mut shown: Vec<Face> = Vec::new();
        let mut ok = true;
        for f in &mut faces {
            if !self.name_canceller(f).await {
                ok = false;
                break;
            }
        }
        // A write that fails stops the pass; a READ for an item's intent that
        // fails leaves that item alone (nothing is made for it blind) and the
        // pass goes on with the rest, which need no read.
        let mut stopped = !ok;
        for f in &faces {
            if stopped {
                break;
            }
            if !self.card_for(f, &home, now).await {
                ok = false;
                if std::mem::take(&mut self.read_failed) {
                    continue;
                }
                stopped = true;
                break;
            }
            shown.push(f.clone());
        }
        // Only once the work model has been read: before it, an item that is
        // not in it is an item it has not told of (a task set for later, a
        // reminder over 30 days out, the Home conversation's), not one gone.
        if !stopped && s.tasks.loaded() {
            // An item that is nowhere any more: fired or cancelled while the
            // daemon was down, or a routine removed.
            let gone: Vec<Rec> = self
                .recs
                .iter()
                .filter(|r| r.ended_ms.is_none() && !faces.iter().any(|f| f.id == r.id))
                .cloned()
                .collect();
            for r in gone {
                let mut f = gone_face(&r, now, &s.task_posts.sched);
                if !self.name_canceller(&mut f).await {
                    ok = false;
                    break;
                }
                if !self.card_for(&f, &home, now).await {
                    ok = false;
                    if std::mem::take(&mut self.read_failed) {
                        continue;
                    }
                    break;
                }
                shown.push(f);
            }
        }
        // An entry stays while its item stands, whenever it ended before.
        let standing: std::collections::HashSet<&str> = faces
            .iter()
            .filter(|f| !f.is_over())
            .map(|f| f.id.as_str())
            .collect();
        self.recs
            .retain(|r| kept(r, now, standing.contains(r.id.as_str())));
        self.save();
        self.tell_the_forum(&shown, &home, now);
        ok
    }

    /// A card that is about to say `Cancelled` with no name: the item's own
    /// record says who cancelled it (the author of the cancel call, from the
    /// terminal or elsewhere), and the card says so. False when the record
    /// could not be read: the pass waits and looks again, so a card is never
    /// left without a name it could have had.
    async fn name_canceller(&mut self, f: &mut Face) -> bool {
        let Life::Cancelled(at, None) = &f.life else {
            return true;
        };
        let at = *at;
        let at_its_end = self
            .recs
            .iter()
            .find(|r| r.id == f.id)
            .is_some_and(|r| r.ended_ms.is_some() || r.by.is_some());
        if at_its_end {
            return true;
        }
        match self.cancelled_by(&f.id, f.kind).await {
            Ok(by) => {
                f.life = Life::Cancelled(at, by);
                true
            }
            Err(e) => {
                self.failed("schedule card", e);
                false
            }
        }
    }

    /// The label the newest `wake.cancelled` row of the wake `id` (or
    /// `execution.cancelled` row of the task) holds as its author.
    async fn cancelled_by(&self, id: &str, kind: Kind) -> Result<Option<String>, String> {
        let (row, key) = if kind == Kind::Task {
            ("execution.cancelled", "execution_id")
        } else {
            ("wake.cancelled", "wake_id")
        };
        let rows = self
            .shared
            .rpc
            .call::<_, theseus_protocol::LedgerTailResult>(
                theseus_protocol::method::LEDGER_TAIL,
                theseus_protocol::LedgerTailParams {
                    n: Some(ROWS_READ),
                    kind: Some(row.into()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.message)?
            .rows;
        Ok(rows
            .iter()
            .rev()
            .find(|r| r.data[key].as_str() == Some(id))
            .and_then(|r| r.data["by"].as_str())
            .and_then(crate::render::sched::who_of))
    }

    /// One item's card: posted if it has none, edited if what it says
    /// changed. False when the write failed.
    async fn card_for(&mut self, f: &Face, home: &Home, now: u64) -> bool {
        let card =
            crate::render::sched::card(&crate::render::leaving::face(&self.shared.core.exits, f));
        let dig = digest(&card);
        let Some(i) = self.recs.iter().position(|r| r.id == f.id) else {
            let late = match f.life {
                Life::Fired(t) | Life::Started(t) | Life::Cancelled(t, _) | Life::Removed(t) => {
                    now < t + LATE_MS
                }
                Life::Scheduled { .. } => true,
            };
            return !late || self.post(f, &card, dig, home, now, None).await;
        };
        let rec = self.recs[i].clone();
        if rec.digest == dig && rec.ended_ms.is_some() == f.is_over() {
            return true;
        }
        match self.edit(&rec, &card).await {
            Ok(()) => {
                let r = &mut self.recs[i];
                r.digest = dig;
                r.due_ms = f.due_ms.or(r.due_ms);
                r.snoozed = r.snoozed || matches!(f.life, Life::Scheduled { snoozed: true });
                r.title = f.title.clone();
                r.from = f.from.clone();
                if let Life::Cancelled(_, by) = &f.life {
                    r.by = by.clone();
                    self.shared.task_posts.sched.not_cancelled(&f.id);
                }
                // An item that stands again (a fired reminder set again under
                // its id) is no ended entry: no edit every pass, and the list
                // keeps it.
                if !f.is_over() {
                    r.ended_ms = None;
                } else if r.ended_ms.is_none() {
                    r.ended_ms = Some(now);
                }
                self.log(&home.label, rec.card, f, "edited");
                true
            }
            Err(e) if e.gone => {
                // Its card was deleted by hand. An item that stands gets a new
                // one, under a nonce of its own; one that has ended is left
                // without (the owner deleted it): its entry stays, at its end,
                // until its day is over.
                if f.is_over() {
                    let r = &mut self.recs[i];
                    r.digest = dig;
                    r.ended_ms.get_or_insert(now);
                    return true;
                }
                self.recs.remove(i);
                self.post(f, &card, dig, home, now, Some(rec.card)).await
            }
            Err(e) => {
                self.failed("schedule card", &e.message);
                false
            }
        }
    }

    /// The item's card, made. `after` is the message of a card of the item
    /// that was deleted, which keeps the new nonce apart from the old one.
    async fn post(
        &mut self,
        f: &Face,
        card: &[Value],
        dig: u64,
        home: &Home,
        now: u64,
        after: Option<u64>,
    ) -> bool {
        // A catch-up (a first start, many items set at once) leaves the task
        // home's channel room for a task's report: after the first few, one
        // card in each `SPACE`.
        if self.made >= BURST {
            tokio::time::sleep(self.space).await;
        }
        self.made += 1;
        // A card a stop cut off before it was recorded is found, not made again.
        match self.settle_intent(f, home, now).await {
            Ok(true) => return self.edit_found(f, card, dig, home, now).await,
            Ok(false) => {}
            Err(e) => {
                // A read that failed: no card is made for this item blind, and
                // the others do not wait for it.
                self.read_failed = true;
                self.failed("schedule card", format!("{e:#}"));
                return false;
            }
        }
        self.intend(&f.id, home.channel);
        match self.create(home.channel, &f.id, card, after).await {
            Ok(message) => {
                self.recs.push(Rec {
                    id: f.id.clone(),
                    kind: f.kind.noun().to_string(),
                    title: f.title.clone(),
                    from: f.from.clone(),
                    every: f.every.clone(),
                    channel: home.channel,
                    guild: home.guild.clone(),
                    card: message,
                    created_ms: now,
                    due_ms: f.due_ms,
                    snoozed: false,
                    ended_ms: f.is_over().then_some(now),
                    by: match &f.life {
                        Life::Cancelled(_, by) => by.clone(),
                        _ => None,
                    },
                    digest: dig,
                });
                self.save();
                self.settle(&f.id);
                self.log(&home.label, message, f, "posted");
                true
            }
            Err(e) => {
                // A create that certainly made nothing leaves no intent; one
                // that may have gone keeps it, and is settled before the next.
                if !e.away && !e.unsure {
                    self.settle(&f.id);
                }
                self.failed("schedule card", &e.message);
                false
            }
        }
    }

    /// The card found again after a stop is edited to the item's truth now.
    async fn edit_found(
        &mut self,
        f: &Face,
        card: &[Value],
        dig: u64,
        home: &Home,
        now: u64,
    ) -> bool {
        let Some(i) = self.recs.iter().position(|r| r.id == f.id) else {
            return true;
        };
        let rec = self.recs[i].clone();
        match self.edit(&rec, card).await {
            Ok(()) => {
                let r = &mut self.recs[i];
                r.digest = dig;
                r.ended_ms = f.is_over().then_some(now);
                self.log(&home.label, rec.card, f, "found");
                true
            }
            Err(e) => {
                self.failed("schedule card", &e.message);
                false
            }
        }
    }

    /// A new message in the task home, silent, under the item's own nonce so
    /// that a second send of it is the first.
    async fn create(
        &self,
        channel: u64,
        id: &str,
        card: &[Value],
        after: Option<u64>,
    ) -> Result<u64, SendErr> {
        v2::validate(card).map_err(SendErr::refused)?;
        let mut body = v2::body(card, true);
        let key = match after {
            Some(old) => format!("sched:{id}:{old}"),
            None => format!("sched:{id}"),
        };
        body["nonce"] = json!(nonce(&key));
        body["enforce_nonce"] = json!(true);
        body["allowed_mentions"] = json!({"parse": []});
        let req = Request::builder(&HttpRoute::CreateMessage {
            channel_id: channel,
        })
        .json(&{
            crate::render::leaving::json(&self.shared.core.exits, &mut body);
            body
        })
        .build()
        .map_err(|e| SendErr::of(&e))?;
        let made: Value = self
            .shared
            .http
            .request::<Value>(req)
            .await
            .map_err(|e| SendErr::of(&e))?
            .model()
            .await
            .map_err(|e| SendErr::refused(e.to_string()))?;
        made["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| SendErr::refused("the new card has no id"))
    }

    async fn edit(&self, rec: &Rec, card: &[Value]) -> Result<(), SendErr> {
        let mut body = json!({"flags": v2::IS_COMPONENTS_V2, "components": card,
                          "allowed_mentions": {"parse": []}});
        let req = Request::builder(&HttpRoute::UpdateMessage {
            channel_id: rec.channel,
            message_id: rec.card,
        })
        .json(&{
            crate::render::leaving::json(&self.shared.core.exits, &mut body);
            body
        })
        .build()
        .map_err(|e| SendErr::of(&e))?;
        self.shared
            .http
            .request::<Value>(req)
            .await
            .map(|_| ())
            .map_err(|e| SendErr::of(&e))
    }

    /// One info line a card write: where, which message, what kind, and the
    /// counts. Never a title.
    fn log(&self, label: &str, message: u64, f: &Face, what: &str) {
        let open = self.recs.iter().filter(|r| r.ended_ms.is_none()).count();
        tracing::info!(
            place = %label,
            message,
            kind = f.kind.noun(),
            what,
            open,
            cards = self.recs.len(),
            "the schedule card"
        );
    }

    /// The forum's posts follow the cards: a view for each item that has one,
    /// with the link to its card.
    fn tell_the_forum(&self, shown: &[Face], home: &Home, now: u64) {
        let s = &self.shared;
        let mut views = Vec::new();
        let mut links = HashMap::new();
        {
            let mut faces = s.task_posts.sched.faces.lock().unwrap();
            faces.clear();
            for f in shown {
                // A task has its own post; only the other items are made one.
                let Some(r) = self
                    .recs
                    .iter()
                    .find(|r| r.id == f.id)
                    .filter(|_| f.kind != Kind::Task)
                else {
                    continue;
                };
                faces.insert(f.id.clone(), f.clone());
                views.push(view_of(f, r.created_ms, now));
                links.insert(
                    f.id.clone(),
                    format!(
                        "https://discord.com/channels/{}/{}/{}",
                        home.guild, home.channel, r.card
                    ),
                );
            }
        }
        s.task_posts.tell_scheduled(views, links, &home.guild);
    }
}

/// Whether the list keeps an entry: while its item stands, whenever it ended
/// before (no second card, no orphan with live buttons), else a day after its
/// end.
fn kept(r: &Rec, now: u64, standing: bool) -> bool {
    standing || r.ended_ms.is_none_or(|at| now < at + KEPT_MS)
}

/// The face of an item that is nowhere any more: a wake that was due fired,
/// one that was not due was cancelled, a routine was removed.
fn gone_face(r: &Rec, now: u64, sched: &Sched) -> Face {
    let kind = match r.kind.as_str() {
        "routine" => Kind::Routine,
        "check-in" => Kind::CheckIn,
        "task" => Kind::Task,
        _ => Kind::Reminder,
    };
    let life = match (kind, r.due_ms) {
        (Kind::Routine, _) => Life::Removed(now),
        (Kind::Task, Some(due)) if due <= now + 5_000 => Life::Started(due),
        (_, Some(due)) if due <= now + 5_000 => Life::Fired(due),
        _ => Life::Cancelled(now, sched.who_cancelled(&r.id).or_else(|| r.by.clone())),
    };
    Face {
        id: r.id.clone(),
        kind,
        title: r.title.clone(),
        due_ms: r.due_ms,
        life,
        from: r.from.clone(),
        every: r.every.clone(),
        paused: false,
        routine: None,
    }
}

/// Who a card says cancelled it, for a press: the presser's display name when
/// Discord gives them one, else their user name (`who`, `discord:<name>`). The
/// cancel call's own author, and so its record, is `who`: a user name and no
/// id, so a cancel from elsewhere can only be named by that.
fn presser_name(i: &Interaction, who: &str) -> String {
    display_of(i.author().and_then(|u| u.global_name.as_deref()), who)
}

fn display_of(global_name: Option<&str>, who: &str) -> String {
    global_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| who.to_string(), str::to_string)
}

impl Shared {
    /// A press of a card's Cancel or Snooze: the call of `/cancel` or
    /// `/snooze` as the presser, told to the presser alone. The press has
    /// passed the gate of every press on a place (the home's people are the
    /// owners). A card whose item is over says so, and the worker brings the
    /// card to how it ended.
    pub(super) async fn card_press(&self, i: &Interaction, custom_id: &str, who: &str) {
        // Before the work model has been read the board knows of nothing: an item
        // it does not hold is not yet an item gone. Nothing is written.
        if !self.tasks.loaded() {
            return self
                .reply_ephemeral(
                    i,
                    "Theseus is still starting: try again in a moment.".into(),
                )
                .await;
        }
        let cancel = custom_id.starts_with(CANCEL_PREFIX);
        let wake = custom_id
            .strip_prefix(CANCEL_PREFIX)
            .or_else(|| custom_id.strip_prefix(crate::render::horizon::SNOOZE_PREFIX))
            .unwrap_or_default();
        if !wake.starts_with("wak_") {
            return self.task_card_press(i, cancel, wake, who).await;
        }
        // Held: the model has the wake, or the agenda lists it pending. A card
        // whose item neither does (long fired, or gone) is nothing to act on:
        // `wake.snooze` would set a long-fired reminder again from its record.
        let held = self.tasks.scheduled().iter().any(|v| v.id == wake)
            || self
                .tasks
                .future
                .snapshot()
                .lines
                .iter()
                .any(|l| l.id == wake);
        if !held {
            self.tasks.future.dirty();
            return self
                .reply_ephemeral(
                    i,
                    "That reminder is no longer on the board: nothing was changed.".into(),
                )
                .await;
        }
        let pending = self
            .tasks
            .future
            .snapshot()
            .lines
            .iter()
            .any(|l| l.id == wake);
        let over = !pending
            && self
                .tasks
                .scheduled()
                .iter()
                .any(|v| v.id == wake && v.state.ended());
        if over {
            self.tasks.future.dirty();
            return self
                .reply_ephemeral(
                    i,
                    "That one is over already: its card shows how it ended.".into(),
                )
                .await;
        }
        if cancel {
            self.task_posts
                .sched
                .will_cancel(wake, &presser_name(i, who));
        }
        if !self.reminder_pick(i, custom_id, who).await && cancel {
            self.task_posts.sched.not_cancelled(wake);
        }
    }
}

impl Shared {
    /// The press of the card of a task set to start later: `task.cancel` or
    /// `task.reschedule` an hour on, as the presser. A task that has started
    /// or ended says so, and its card is brought to that.
    async fn task_card_press(&self, i: &Interaction, cancel: bool, task: &str, who: &str) {
        let waits = self
            .tasks
            .view(task)
            .is_some_and(|v| crate::render::task_view::waits_to_start(&v));
        if !waits {
            self.tasks.future.dirty();
            return self
                .reply_ephemeral(
                    i,
                    "That task has started or ended already: its card shows how.".into(),
                )
                .await;
        }
        if cancel {
            self.task_posts
                .sched
                .will_cancel(task, &presser_name(i, who));
        }
        if !self.task_pick(i, cancel, task, who).await && cancel {
            self.task_posts.sched.not_cancelled(task);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: &str, due: Option<u64>) -> Rec {
        Rec {
            id: "wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd".into(),
            kind: kind.into(),
            title: "stretch".into(),
            from: Some("#pier".into()),
            every: None,
            channel: 1,
            guild: "2".into(),
            card: 3,
            created_ms: 1_000,
            due_ms: due,
            snoozed: false,
            ended_ms: None,
            by: None,
            digest: 0,
        }
    }

    #[test]
    fn an_item_gone_while_the_daemon_was_down_is_fired_when_it_was_due_else_cancelled() {
        let sched = Sched::default();
        let now = 1_790_000_000_000;
        let fired = gone_face(&rec("reminder", Some(now - 60_000)), now, &sched);
        assert_eq!(fired.life, Life::Fired(now - 60_000));
        let cancelled = gone_face(&rec("reminder", Some(now + 3_600_000)), now, &sched);
        assert_eq!(cancelled.life, Life::Cancelled(now, None));
        let routine = gone_face(&rec("routine", Some(now + 3_600_000)), now, &sched);
        assert_eq!(routine.life, Life::Removed(now));
        assert_eq!(routine.kind, Kind::Routine);
    }

    #[test]
    fn a_cancel_by_a_press_names_the_presser_on_the_card() {
        let sched = Sched::default();
        sched.will_cancel("wak_x", "discord:ana");
        let f = gone_face(
            &Rec {
                id: "wak_x".into(),
                ..rec("reminder", Some(u64::MAX / 2))
            },
            1_000,
            &sched,
        );
        assert_eq!(f.life, Life::Cancelled(1_000, Some("ana".into())));
        sched.not_cancelled("wak_x");
        assert_eq!(sched.who_cancelled("wak_x"), None);
    }

    #[test]
    fn only_a_reminder_or_a_check_in_is_an_item_not_a_watchs_or_a_routines_wake() {
        let mut v = view_of(
            &Face {
                id: "wak_1".into(),
                kind: Kind::Reminder,
                title: "t".into(),
                due_ms: Some(5),
                life: Life::Scheduled { snoozed: false },
                from: None,
                every: None,
                paused: false,
                routine: None,
            },
            1,
            2,
        );
        assert!(is_item(&v));
        v.purpose = Some("watch".into());
        assert!(!is_item(&v));
        v.purpose = Some("routine".into());
        assert!(!is_item(&v));
        v.purpose = Some("remind".into());
        v.id = "ses_1".into();
        assert!(!is_item(&v), "only a wake's id");
    }

    #[test]
    fn a_time_that_moved_either_way_is_a_snooze_and_a_series_moving_on_is_not() {
        let names = HashMap::new();
        let sched = Sched::default();
        let mut v = view_of(
            &Face {
                id: "wak_1".into(),
                kind: Kind::Reminder,
                title: "t".into(),
                due_ms: Some(10_000_000),
                life: Life::Scheduled { snoozed: false },
                from: None,
                every: None,
                paused: false,
                routine: None,
            },
            1,
            2,
        );
        let was = Rec {
            due_ms: Some(10_000_000),
            ..rec("reminder", Some(10_000_000))
        };
        assert_eq!(
            wake_face(&v, Some(&was), &names, &sched).life,
            Life::Scheduled { snoozed: false }
        );
        for moved in [4_000_000, 20_000_000] {
            v.next_at_ms = Some(moved);
            assert_eq!(
                wake_face(&v, Some(&was), &names, &sched).life,
                Life::Scheduled { snoozed: true },
                "moved to {moved}"
            );
        }
        v.series = Some(theseus_protocol::work::horizon::Series {
            every: Some("1d".into()),
            ..Default::default()
        });
        assert_eq!(
            wake_face(&v, Some(&was), &names, &sched).life,
            Life::Scheduled { snoozed: false },
            "a series moves on by itself"
        );
    }

    #[test]
    fn a_press_is_named_by_its_display_name_else_its_user_name() {
        assert_eq!(display_of(Some("Ana B"), "discord:ana"), "Ana B");
        assert_eq!(display_of(Some("  "), "discord:ana"), "discord:ana");
        assert_eq!(display_of(None, "discord:ana"), "discord:ana");
        let sched = Sched::default();
        sched.will_cancel("wak_x", &display_of(None, "discord:ana"));
        assert_eq!(sched.who_cancelled("wak_x").as_deref(), Some("ana"));
        sched.will_cancel("wak_y", &display_of(Some("Ana B"), "discord:ana"));
        assert_eq!(sched.who_cancelled("wak_y").as_deref(), Some("Ana B"));
    }

    #[test]
    fn the_list_keeps_an_entry_whose_item_stands_whenever_it_ended_before() {
        let now = 1_790_000_000_000;
        let mut r = rec("reminder", Some(now + 3_600_000));
        r.ended_ms = Some(now - 2 * KEPT_MS);
        assert!(!kept(&r, now, false), "ended long ago and gone");
        assert!(kept(&r, now, true), "set again: its card is its card");
        r.ended_ms = Some(now - KEPT_MS / 2);
        assert!(kept(&r, now, false), "ended today: kept for a late press");
        r.ended_ms = None;
        assert!(kept(&r, now, false));
    }
}
