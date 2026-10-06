//! Durable delivery (theseus-q4v; spec §3.16, and the Discord binding's
//! delivery): what must reach a channel is an outbox post, written by the core
//! when it becomes true, whether or not a binding is connected. A binding only
//! delivers: it sends a target's posts in order, and settles each with the
//! message ids the channel gave.
//!
//! Posts are:
//! - a turn's reply: its loops' text, by node, and its footer;
//! - a confirm card, and the settle that says how its question closed;
//! - a notice the operator must see: a refused answer from a job's process, a
//!   restart onto a changed config, a failed turn, and the binding's own notes.
//!
//! A task's report is one too (DD7). Live progress is not: typing, and the
//! edits of a reply while its turn runs, stay the binding's, best-effort and
//! never replayed.
//!
//! A session's posts go to the place whose record names it (the Discord
//! binding's `discord.session.<place>` meta): `discord:<place>`. A task's go
//! where its parent's went when it started (`task.place.<task session>`),
//! even after that place moves to a new session. And `discord:operator` is
//! wherever approvals go.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{
    Action, ActionState, Completion, Kernel, Outcome, Post, RetryClass, Settled, BUDGET_TOOL,
};
use theseus_protocol::OutboxStatus;
use theseus_store::{kinds, NewRecord, Store as _};

use crate::store::Store;

/// The Discord binding's place records: `discord.session.<place>` names the
/// session a place runs on (M3c).
pub const PLACE_META_PREFIX: &str = "discord.session.";
/// A task's record of where it reports (DD7): `task.place.<task session>`
/// names its parent's target when it started.
pub const TASK_META_PREFIX: &str = "task.place.";
/// Where the reply to a wake or a task's report goes when its session posts
/// nowhere by then (theseus-4lx): `wake.target.<session>` names the target the
/// wake was set from, or the report was sent to, in the frame that took it.
/// It outlives the turn that took it, so the retry of a turn that failed
/// before its reply answers where the first would have.
pub const WAKE_TARGET_PREFIX: &str = "wake.target.";
/// Where approvals go, whichever DM that is.
pub const OPERATOR_TARGET: &str = "discord:operator";
/// The downstream key of a post that creates messages: Discord's nonce.
pub const NONCE_KEY: &str = "discord.nonce";

/// How a card's question closed, as its settle says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    /// `approved`, `declined`, `superseded`, `withdrawn`, `ended`, `stopped`
    /// (W1), or `closed`.
    pub how: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Closed {
    pub fn new(how: &str, by: Option<&str>) -> Self {
        Self {
            how: how.into(),
            by: by.map(str::to_string),
            note: None,
        }
    }

    /// How a question that no longer waits closed, from its action alone:
    /// the level-triggered path, for a close no event said (the end of its
    /// execution, or one before a restart). None while it still waits.
    pub fn of(q: &Action) -> Option<Self> {
        if q.awaits_confirm() {
            return None;
        }
        let r = q.resolution.as_deref().unwrap_or("");
        if let Some(c) = &q.confirm {
            return Some(Self::new("approved", Some(&c.by)));
        }
        if let Some(rest) = r.strip_prefix("approved by ") {
            let by = rest.split_once(':').map_or(rest, |(b, _)| b);
            return Some(Self::new("approved", Some(by)));
        }
        if let Some(rest) = r
            .strip_prefix("declined by ")
            .or_else(|| r.strip_prefix("denied by "))
        {
            let (by, note) = rest.split_once(": ").unwrap_or((rest, ""));
            let how = if note.starts_with("superseded") {
                "superseded"
            } else {
                "declined"
            };
            return Some(Self {
                how: how.into(),
                by: Some(by.into()),
                note: (!note.is_empty()).then(|| note.to_string()),
            });
        }
        // `/stop` declined it (W1): it will not run, and the session goes on.
        if let Some(by) = r.strip_prefix("stopped by ") {
            return Some(Self::new("stopped", Some(by)));
        }
        let how = if r.starts_with("withdrawn") {
            "withdrawn"
        } else if r.starts_with("the execution ended") {
            "ended"
        } else {
            "closed"
        };
        Some(Self {
            how: how.into(),
            by: None,
            note: Some(if r.is_empty() {
                q.state.as_str().to_string()
            } else {
                r.to_string()
            }),
        })
    }
}

/// An unsettled post, as the index keeps it.
#[derive(Debug, Clone)]
struct Open {
    target: String,
    planned_at_ms: u64,
}

/// A card whose settle is not written yet.
#[derive(Debug, Clone)]
struct Card {
    post: String,
    target: String,
    session_id: String,
    execution_id: String,
}

/// What the outbox keeps in memory: read from the store on first use, then
/// kept by every write. The store is the truth; this is its index.
#[derive(Default)]
struct Index {
    /// Unsettled posts, oldest first: their ids are time-ordered (UUID v7,
    /// monotonic within a process).
    open: BTreeMap<String, Open>,
    /// Session id → where its posts go.
    targets: HashMap<String, String>,
    /// Task session id → where it reports (DD7). Kept apart from `targets`,
    /// which a rebound place prunes.
    tasks: HashMap<String, String>,
    /// Question id → its card, until the card's settle is written.
    cards: HashMap<String, Card>,
    /// Every question that has a card post, settled or not.
    carded: std::collections::HashSet<String>,
    /// By binding (`discord`): delivered, refused for good, the last error.
    counts: BTreeMap<String, Counts>,
}

#[derive(Default, Clone)]
struct Counts {
    sent: u64,
    failed: u64,
    last_error: Option<(u64, String)>,
}

/// The binding a target belongs to: `discord` for `discord:dm:1`.
fn binding_of(target: &str) -> &str {
    target.split_once(':').map_or(target, |(b, _)| b)
}

/// A post's kind: `reply`, `card`, `settle`, `notice`, …
pub fn kind_of(a: &Action) -> &str {
    a.proposal
        .as_ref()
        .and_then(|p| p.args["kind"].as_str())
        .unwrap_or("")
}

/// What a post says: its body.
pub fn body_of(a: &Action) -> &Value {
    static NULL: Value = Value::Null;
    a.proposal.as_ref().map_or(&NULL, |p| &p.args)
}

/// Where a post goes.
pub fn target_of(a: &Action) -> &str {
    a.resource.as_deref().unwrap_or("")
}

pub struct Outbox {
    store: Store,
    kernel: Arc<Kernel>,
    index: Mutex<Option<Index>>,
    /// Bumped by every change a deliverer cares about: a new post, a settle.
    changed: tokio::sync::watch::Sender<u64>,
    /// The posts in flight, and whether the daemon is stopping (theseus-pfv).
    flight: Arc<tokio::sync::watch::Sender<Flight>>,
}

/// The posts this process has sent and not yet settled, and when its stop
/// began (theseus-pfv).
#[derive(Debug, Default)]
struct Flight {
    sending: usize,
    stopping: Option<tokio::time::Instant>,
}

/// One post in flight: taken just before its dispatch, and dropped once it
/// is settled, or its delivery gives up and it waits, dispatched. A clean
/// stop waits for every one, within its grace (`Outbox::settle_in_flight`).
pub struct Sending(Arc<tokio::sync::watch::Sender<Flight>>);

impl Drop for Sending {
    fn drop(&mut self) {
        self.0.send_modify(|f| f.sending -= 1);
    }
}

/// What a stop's wait for the posts in flight found (theseus-pfv).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InFlight {
    /// In flight when the wait began.
    pub waited_for: usize,
    /// Still in flight at its end: each is left dispatched, and the next
    /// start sends it again under the same nonce.
    pub left: usize,
}

impl Outbox {
    pub fn new(store: Store, kernel: Arc<Kernel>) -> Self {
        Self {
            store,
            kernel,
            index: Mutex::new(None),
            changed: tokio::sync::watch::Sender::new(0),
            flight: Arc::new(tokio::sync::watch::Sender::new(Flight::default())),
        }
    }

    /// A post about to be dispatched, in flight until the guard drops; None
    /// once the daemon is stopping, when no post is dispatched and each waits
    /// for the next start (theseus-pfv).
    pub fn sending(&self) -> Option<Sending> {
        let mut took = false;
        self.flight.send_if_modified(|f| {
            took = f.stopping.is_none();
            if took {
                f.sending += 1;
            }
            took
        });
        took.then(|| Sending(self.flight.clone()))
    }

    /// The daemon is stopping: from now on no post is dispatched. The first
    /// call marks when the stop began, the start of its grace.
    pub fn stop_sending(&self) {
        self.flight.send_if_modified(|f| {
            let first = f.stopping.is_none();
            if first {
                f.stopping = Some(tokio::time::Instant::now());
            }
            first
        });
    }

    /// Whether the daemon is stopping, so a lane takes no new post.
    pub fn stopping(&self) -> bool {
        self.flight.borrow().stopping.is_some()
    }

    /// A clean stop's wait (theseus-pfv): no post is dispatched any more, and
    /// the posts already sent settle, until `grace` after the stop began. The
    /// rest stay dispatched, as after a crash, and the next start sends them
    /// again under the same nonce.
    pub async fn settle_in_flight(&self, grace: std::time::Duration) -> InFlight {
        self.stop_sending();
        let began = self
            .flight
            .borrow()
            .stopping
            .unwrap_or_else(tokio::time::Instant::now);
        let waited_for = self.flight.borrow().sending;
        if waited_for > 0 {
            let mut rx = self.flight.subscribe();
            let _ = tokio::time::timeout_at(began + grace, rx.wait_for(|f| f.sending == 0)).await;
        }
        InFlight {
            waited_for,
            left: self.flight.borrow().sending,
        }
    }

    /// Run `f` on the index, reading it from the store first if this is the
    /// first use: every post, and the binding's place records.
    ///
    /// The store is read outside the lock, so nothing waits on it for the
    /// read: every change comes after its own write and goes through the
    /// index that is in place, and a read that finishes second is dropped.
    fn with<R>(&self, f: impl FnOnce(&mut Index) -> R) -> Result<R> {
        if let Some(ix) = self.index.lock().unwrap().as_mut() {
            return Ok(f(ix));
        }
        let loaded = self.load()?;
        let mut g = self.index.lock().unwrap();
        Ok(f(g.get_or_insert(loaded)))
    }

    /// Run `f` on the index only when it is read already: health never pays
    /// for reading it (FAST).
    fn peek<R>(&self, f: impl FnOnce(&Index) -> R) -> Option<R> {
        self.index.lock().unwrap().as_ref().map(f)
    }

    /// Read the index now: the daemon does this once it serves, so neither a
    /// first answer nor a first turn waits for it.
    pub fn warm(&self) {
        if let Err(e) = self.with(|_| ()) {
            tracing::warn!(error = %format!("{e:#}"), "outbox unreadable");
        }
    }

    fn load(&self) -> Result<Index> {
        let t0 = std::time::Instant::now();
        let mut ix = Index::default();
        let mut settled_cards = Vec::new();
        let posts = self.kernel.outbox_actions()?;
        for a in &posts {
            let binding = binding_of(target_of(a)).to_string();
            match a.state {
                ActionState::Succeeded => ix.counts.entry(binding).or_default().sent += 1,
                ActionState::Failed => ix.counts.entry(binding).or_default().failed += 1,
                _ => {
                    ix.open.insert(
                        a.correlation_id.clone(),
                        Open {
                            target: target_of(a).to_string(),
                            planned_at_ms: a.planned_at_ms,
                        },
                    );
                }
            }
            let body = body_of(a);
            match kind_of(a) {
                "card" => {
                    if let Some(q) = body["question"].as_str() {
                        ix.carded.insert(q.to_string());
                        ix.cards.insert(
                            q.to_string(),
                            Card {
                                post: a.correlation_id.clone(),
                                target: target_of(a).to_string(),
                                session_id: a.session_id.clone(),
                                execution_id: a.execution_id.clone(),
                            },
                        );
                    }
                }
                "settle" => {
                    if let Some(q) = body["question"].as_str() {
                        settled_cards.push(q.to_string());
                    }
                }
                _ => {}
            }
        }
        for q in settled_cards {
            ix.cards.remove(&q);
        }
        for r in self.store.inner().latest_of_kind(kinds::META)? {
            let key = r.key.as_deref().unwrap_or("");
            if let Some(task) = key.strip_prefix(TASK_META_PREFIX) {
                if let Ok(target) = r.decode::<String>() {
                    ix.tasks.insert(task.to_string(), target);
                }
                continue;
            }
            let Some(place) = key.strip_prefix(PLACE_META_PREFIX) else {
                continue;
            };
            if let Ok(sid) = r.decode::<String>() {
                ix.targets.insert(sid, format!("discord:{place}"));
            }
        }
        tracing::info!(
            posts = posts.len(),
            open = ix.open.len(),
            places = ix.targets.len(),
            ms = t0.elapsed().as_millis() as u64,
            "outbox read"
        );
        Ok(ix)
    }

    fn bump(&self) {
        self.changed.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Woken by every new post and settle.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changed.subscribe()
    }

    // ------------------------------------------------------------ places

    /// Where a session's posts go, if anywhere: its place's, or, for a task,
    /// where it reports (DD7).
    pub fn target(&self, session_id: &str) -> Option<String> {
        match self.try_target(session_id) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "outbox unreadable: a post has nowhere to go");
                None
            }
        }
    }

    /// `target`, with the error when the outbox cannot be read.
    pub fn try_target(&self, session_id: &str) -> Result<Option<String>> {
        self.with(|ix| {
            ix.targets
                .get(session_id)
                .or_else(|| ix.tasks.get(session_id))
                .cloned()
        })
    }

    /// The session a place runs on (`dm:<user>`, `channel:<id>`), from its
    /// record.
    pub fn place_session(&self, place: &str) -> Result<Option<String>> {
        self.store
            .get_meta::<String>(&format!("{PLACE_META_PREFIX}{place}"))
    }

    /// A place now runs on `session_id`: its record, and where that session's
    /// posts go. The session it ran on before posts nothing more there; the
    /// posts it already wrote still go.
    pub fn bind_place(&self, place: &str, session_id: &str) -> Result<()> {
        self.store
            .put_meta(&format!("{PLACE_META_PREFIX}{place}"), &session_id)?;
        let target = format!("discord:{place}");
        self.with(|ix| {
            ix.targets.retain(|_, t| *t != target);
            ix.targets.insert(session_id.to_string(), target);
        })
    }

    /// A task's record of where it reports, for the frame that opens it
    /// (DD7); `task_bound` once that frame is written.
    pub fn task_record(&self, task_session: &str, target: &str) -> Result<NewRecord> {
        NewRecord::json(
            kinds::META,
            Some(&format!("{TASK_META_PREFIX}{task_session}")),
            &target,
        )
    }

    /// The record of where the reply to what a turn took (a due wake, or a
    /// task's report) goes when `session_id` posts nowhere, for the frame
    /// that takes it (theseus-4lx). A later take replaces it.
    pub fn wake_target_record(&self, session_id: &str, target: &str) -> Result<NewRecord> {
        NewRecord::json(
            kinds::META,
            Some(&format!("{WAKE_TARGET_PREFIX}{session_id}")),
            &target,
        )
    }

    /// What `wake_target_record` last wrote for `session_id`.
    pub fn wake_target(&self, session_id: &str) -> Option<String> {
        self.try_wake_target(session_id).ok().flatten()
    }

    /// `wake_target`, with the error when the store cannot be read.
    pub fn try_wake_target(&self, session_id: &str) -> Result<Option<String>> {
        self.store
            .get_meta::<String>(&format!("{WAKE_TARGET_PREFIX}{session_id}"))
    }

    /// A task's record is written: its posts go to `target` from now on.
    pub fn task_bound(&self, task_session: &str, target: &str) {
        let r = self.with(|ix| {
            ix.tasks
                .insert(task_session.to_string(), target.to_string())
        });
        if let Err(e) = r {
            tracing::warn!(error = %format!("{e:#}"), "outbox index unreadable; the task's record is in the store");
        }
    }

    // ------------------------------------------------------------ writing

    /// A post's records, for the caller's frame; `posted` once they are written.
    pub fn stage(
        &self,
        session_id: &str,
        execution_id: &str,
        target: &str,
        body: Value,
    ) -> Result<(Action, Vec<NewRecord>)> {
        let retry_class = if body["kind"] == "settle" {
            // It only edits: the same edit twice leaves the same message.
            RetryClass::SafeToRepeat
        } else {
            RetryClass::IdempotentWithKey {
                key: NONCE_KEY.into(),
            }
        };
        self.kernel.outbox_stage(Post {
            session_id: session_id.into(),
            execution_id: execution_id.into(),
            target: target.into(),
            body,
            retry_class,
        })
    }

    /// A staged post is written: index it, and wake whoever delivers.
    pub fn posted(&self, a: &Action) {
        let r = self.with(|ix| {
            ix.open.insert(
                a.correlation_id.clone(),
                Open {
                    target: target_of(a).to_string(),
                    planned_at_ms: a.planned_at_ms,
                },
            );
            let body = body_of(a);
            if let Some(q) = body["question"].as_str() {
                match kind_of(a) {
                    "card" => {
                        ix.carded.insert(q.to_string());
                        ix.cards.insert(
                            q.to_string(),
                            Card {
                                post: a.correlation_id.clone(),
                                target: target_of(a).to_string(),
                                session_id: a.session_id.clone(),
                                execution_id: a.execution_id.clone(),
                            },
                        );
                    }
                    "settle" => {
                        ix.cards.remove(q);
                    }
                    _ => {}
                }
            }
        });
        if let Err(e) = r {
            tracing::warn!(error = %format!("{e:#}"), "outbox index unreadable; the post is in the store");
        }
        self.bump();
    }

    /// Write a post in a frame of its own.
    pub fn post(
        &self,
        session_id: &str,
        execution_id: &str,
        target: &str,
        body: Value,
    ) -> Result<Action> {
        let (a, frame) = self.stage(session_id, execution_id, target, body)?;
        self.store.append(&frame)?;
        self.posted(&a);
        Ok(a)
    }

    /// A session's post, to its place, when it has one.
    pub fn post_for(
        &self,
        session_id: &str,
        execution_id: &str,
        body: Value,
    ) -> Result<Option<Action>> {
        match self.target(session_id) {
            Some(t) => self.post(session_id, execution_id, &t, body).map(Some),
            None => Ok(None),
        }
    }

    /// A notice the operator must see, where approvals go; when no DM takes
    /// approvals, the binding falls back to `fallback`, a session's place.
    pub fn to_operator(&self, session_id: Option<&str>, mut body: Value) -> Result<Action> {
        if let Some(t) = session_id.and_then(|s| self.target(s)) {
            body["fallback"] = json!(t);
        }
        self.post(session_id.unwrap_or(""), "", OPERATOR_TARGET, body)
    }

    /// `to_operator`'s post staged for the caller's frame (Jev's notices
    /// write their row beside it); `posted` once the frame is written. The
    /// fallback rides as `to_operator`'s does: a kind that must never reach
    /// a shared place is one whose courier reads none (`jev_notice`).
    pub fn stage_to_operator(
        &self,
        session_id: Option<&str>,
        mut body: Value,
    ) -> Result<(Action, Vec<NewRecord>)> {
        if let Some(t) = session_id.and_then(|s| self.target(s)) {
            body["fallback"] = json!(t);
        }
        self.stage(session_id.unwrap_or(""), "", OPERATOR_TARGET, body)
    }

    /// The question a card asks closed: write the card's settle, once. Nothing
    /// when the question has no card (its session posts nowhere, or its card
    /// was posted before this outbox existed).
    pub fn closed(&self, question: &str, how: Closed) -> Result<Option<Action>> {
        let Some(card) = self.with(|ix| ix.cards.get(question).cloned())? else {
            return Ok(None);
        };
        let body =
            json!({"kind": "settle", "question": question, "card": card.post, "closed": how});
        self.post(&card.session_id, &card.execution_id, &card.target, body)
            .map(Some)
    }

    /// Every card whose question no longer waits gets its settle: a close no
    /// event said, like the end of its execution, or one that came while the
    /// daemon was down. The binding runs this on connecting, and the
    /// heartbeat on every beat. Returns how many settles it wrote.
    pub fn reconcile_cards(&self) -> Result<usize> {
        let cards: Vec<(String, Card)> = self.with(|ix| {
            ix.cards
                .iter()
                .map(|(q, c)| (q.clone(), c.clone()))
                .collect()
        })?;
        let mut n = 0;
        for (q, _) in cards {
            let how = match self.kernel.action(&q)? {
                Some(a) => match Closed::of(&a) {
                    Some(how) => how,
                    None => continue,
                },
                None => Closed {
                    how: "closed".into(),
                    by: None,
                    note: Some("its question is no longer in the store".into()),
                },
            };
            if self.closed(&q, how)?.is_some() {
                n += 1;
            }
        }
        Ok(n)
    }

    // ------------------------------------------------------------ delivering

    /// The targets with posts waiting, each once.
    pub fn open_targets(&self) -> Vec<String> {
        self.with(|ix| {
            let mut t: Vec<String> = ix.open.values().map(|o| o.target.clone()).collect();
            t.sort();
            t.dedup();
            t
        })
        .unwrap_or_default()
    }

    /// A target's unsettled posts, oldest first, as the store has them.
    pub fn open_for(&self, target: &str) -> Vec<Action> {
        let ids = self.open_for_ids(target);
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            match self.kernel.outbox_action(&id) {
                Ok(Some(a)) if !a.state.is_settled() => out.push(a),
                Ok(Some(_)) => {
                    self.with(|ix| ix.open.remove(&id)).ok();
                }
                // Staged and not written yet: the next wake has it.
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), post = %id, "outbox post unreadable");
                    break;
                }
            }
        }
        out
    }

    /// Refuse each unsettled post of `binding` for a place it does not bind
    /// (theseus-l3m): such a post would wait forever, health would count it
    /// pending, and its oldest-pending age would grow without end, hiding a
    /// real delivery problem. Each is settled as refused, "not bound here any
    /// more (<place>)", ledgered as the settle and counted in health's
    /// refused; nothing is sent. The operator's posts follow wherever
    /// approvals go, so they are never refused here. Returns how many.
    pub fn refuse_unbound(&self, binding: &str, bound: impl Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for target in self.open_targets() {
            if binding_of(&target) != binding || target == OPERATOR_TARGET || bound(&target) {
                continue;
            }
            let place = target.split_once(':').map_or(target.as_str(), |(_, p)| p);
            let why = format!("not bound here any more ({place})");
            for a in self.open_for(&target) {
                let detail = json!({"error": why, "unbound": place});
                match self.settle(&a.correlation_id, Outcome::Failed, None, detail, binding) {
                    Ok(_) => n += 1,
                    Err(e) => tracing::warn!(
                        error = %format!("{e:#}"),
                        post = %a.correlation_id,
                        "a post for a place no longer bound was not settled"
                    ),
                }
            }
        }
        n
    }

    /// `dispatched`, before the post's first call.
    pub fn dispatch(&self, correlation_id: &str) -> Result<Action> {
        self.kernel.outbox_dispatch(correlation_id)
    }

    /// Settle a post with what the channel answered: `detail` holds its
    /// messages. A failure is a refusal no retry can change.
    pub fn settle(
        &self,
        correlation_id: &str,
        outcome: Outcome,
        external_op_id: Option<String>,
        detail: Value,
        producer: &str,
    ) -> Result<Action> {
        let now = theseus_protocol::now_unix_ms();
        let s = self.kernel.outbox_settle(&Completion {
            correlation_id: correlation_id.into(),
            outcome,
            result_ref: None,
            external_op_id,
            started_at_ms: now,
            finished_at_ms: now,
            producer: producer.into(),
            signature: None,
            cost_micros: None,
            detail: Some(detail),
        })?;
        let a = match s {
            Settled::Now(a) => {
                let error = (a.state == ActionState::Failed).then(|| {
                    a.detail
                        .as_ref()
                        .and_then(|d| d["error"].as_str())
                        .unwrap_or("refused")
                        .to_string()
                });
                self.with(|ix| {
                    ix.open.remove(&a.correlation_id);
                    let c = ix
                        .counts
                        .entry(binding_of(target_of(&a)).to_string())
                        .or_default();
                    match &error {
                        None => c.sent += 1,
                        Some(e) => {
                            c.failed += 1;
                            c.last_error = Some((now, e.clone()));
                        }
                    }
                })?;
                a
            }
            Settled::Already(a) => {
                self.with(|ix| {
                    ix.open.remove(&a.correlation_id);
                })?;
                a
            }
        };
        self.bump();
        Ok(a)
    }

    /// A delivery went wrong and will be tried again: health says so.
    pub fn error(&self, binding: &str, what: impl Into<String>) {
        let what = what.into();
        let _ = self.with(|ix| {
            ix.counts.entry(binding.to_string()).or_default().last_error =
                Some((theseus_protocol::now_unix_ms(), what));
        });
    }

    /// A binding's outbox for health and the Observatory; zeros until the
    /// index is read (`warm`, just after serving).
    pub fn status(&self, binding: &str) -> OutboxStatus {
        self.peek(|ix| {
            let open: Vec<&Open> = ix
                .open
                .values()
                .filter(|o| binding_of(&o.target) == binding)
                .collect();
            let c = ix.counts.get(binding).cloned().unwrap_or_default();
            OutboxStatus {
                pending: open.len() as u64,
                sent: c.sent,
                failed: c.failed,
                oldest_pending_ms: open.iter().map(|o| o.planned_at_ms).min().unwrap_or(0),
                last_error: c.last_error.as_ref().map(|(_, e)| e.clone()),
                last_error_ms: c.last_error.as_ref().map_or(0, |(t, _)| *t),
            }
        })
        .unwrap_or_default()
    }

    /// Whether a question's card is an outbox post: a press on it is settled
    /// by the card's settle, not by the press. A card from before the outbox
    /// is not.
    pub fn has_card(&self, question: &str) -> bool {
        self.with(|ix| ix.carded.contains(question))
            .unwrap_or(false)
    }

    /// The next post a target waits on, oldest first; None when none waits,
    /// or the next is not written yet.
    pub fn next_for(&self, target: &str) -> Option<Action> {
        self.open_for_ids(target)
            .first()
            .and_then(|id| match self.kernel.outbox_action(id) {
                Ok(Some(a)) if !a.state.is_settled() => Some(a),
                Ok(Some(a)) => {
                    let _ = self.with(|ix| ix.open.remove(&a.correlation_id));
                    self.next_for(target)
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), post = %id, "outbox post unreadable");
                    None
                }
            })
    }

    fn open_for_ids(&self, target: &str) -> Vec<String> {
        self.with(|ix| {
            ix.open
                .iter()
                .filter(|(_, o)| o.target == target)
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default()
    }

    /// The text of a reply's loops, by node: `[[loop, node id], …]` in the
    /// body becomes `[(loop, text)]`. A node that is gone is skipped.
    pub fn reply_texts(&self, body: &Value) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        for l in body["loops"].as_array().into_iter().flatten() {
            let (Some(i), Some(id)) = (l[0].as_u64(), l[1].as_str()) else {
                continue;
            };
            if let Some(text) = self.said(id) {
                out.push((i as u32, text));
            }
        }
        out
    }

    /// What an assistant node said, by its id: its text, when it has any.
    /// A task's report names its last message so (DD7).
    pub fn said(&self, node_id: &str) -> Option<String> {
        let node = self
            .store
            .inner()
            .latest_by_key(kinds::NODE, node_id)
            .ok()
            .flatten()
            .and_then(|r| r.decode::<crate::node::Node>().ok())?;
        match node.body {
            crate::node::Body::AssistantMessage { blocks, .. } => {
                Some(crate::provider::text_of(&blocks)).filter(|t| !t.is_empty())
            }
            _ => None,
        }
    }
}

/// A budget question's card says what a reset does; a tool call's, what runs.
pub fn is_budget(q: &Action) -> bool {
    q.tool == BUDGET_TOOL
}

impl crate::Core {
    /// The end of every clean stop, once the serving loop has ended
    /// (theseus-bv5, theseus-pfv): the posts already sent settle, until
    /// `[server] stop_grace_ms` after the stop began, and then the index is
    /// checkpointed after them, so the next start replays nothing. With no
    /// post in flight there is no wait, and the checkpoint costs nothing
    /// when the stop's own is still the newest. The web UI's held rows are
    /// written first (theseus-sqpx), in one frame, only when some wait.
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    pub async fn finish_stop(&self) -> InFlight {
        // Each terminal's program ends with the daemon (theseus-n88g.4).
        self.close_terminals().await;
        self.flush_web_rows();
        let t0 = std::time::Instant::now();
        let grace = std::time::Duration::from_millis(self.cfg.server.stop_grace_ms);
        let posts = self.outbox.settle_in_flight(grace).await;
        if posts.waited_for > 0 {
            let waited_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if posts.left == 0 {
                tracing::info!(
                    posts = posts.waited_for,
                    waited_ms,
                    "stopping: the posts in flight settled"
                );
            } else {
                tracing::warn!(
                    posts = posts.waited_for,
                    left = posts.left,
                    waited_ms,
                    "stopping: posts still in flight at the grace's end stay dispatched; the next start sends them again under the same nonce"
                );
            }
        }
        crate::startup::stop_phase("posts settled");
        self.flush_judgments();
        // Nothing written on its own time after serving lands after this
        // checkpoint, where the next start would replay it (theseus-81kk).
        self.close_late_rows();
        // Made durable by redb's close, as the stop's own is (theseus-02k).
        if let Err(e) = self.store.inner().checkpoint_for_close() {
            tracing::warn!(error = %format!("{e:#}"), "stopping: the last checkpoint failed; the next start replays the tail");
        }
        crate::startup::stop_phase("last checkpoint");
        // The board's thread ends now, not when the core drops: the
        // runtime's drop waits for it (theseus-hanu).
        self.push.stop();
        posts
    }

    /// The judge's settled judgments, written before the stop's last
    /// checkpoint (theseus-ych4): the sink writes only between turns, so a
    /// busy daemon can hold a backlog, and a restart must read every one.
    fn flush_judgments(&self) {
        let t0 = std::time::Instant::now();
        let judge = &self.runner.judge;
        let written = theseus_store::blocking(|| judge.flush_sink());
        if written > 0 {
            tracing::info!(
                judgments = written,
                ms = t0.elapsed().as_secs_f64() * 1000.0,
                "stopping: the judge's settled judgments are written"
            );
        }
        crate::startup::stop_phase("judgments written");
    }

    /// After a restart onto the vault's changed config note (theseus-2fo), one
    /// line where approvals go, which reaches Discord whenever the binding is
    /// back.
    pub fn post_restart_notice(&self) {
        let Some(r) = self.config_gate.restarted() else {
            return;
        };
        let body = json!({"kind": "restarted", "at_unix_ms": r.at_unix_ms, "tables": r.tables});
        if let Err(e) = self.outbox.to_operator(None, body) {
            tracing::warn!(error = %format!("{e:#}"), "the restart's notice was not written");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn outbox(dir: &tempfile::TempDir) -> Arc<Outbox> {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let kernel = Arc::new(Kernel::new(
            store.shared(),
            Arc::new(theseus_kernel::RealClock),
            Default::default(),
        ));
        Arc::new(Outbox::new(store, kernel))
    }

    /// A clean stop's wait (theseus-pfv): once the stop begins no post is
    /// dispatched; a post already sent is waited for until it settles; with
    /// none in flight there is no wait.
    #[tokio::test]
    async fn a_stop_waits_for_the_posts_already_sent_and_dispatches_no_more() {
        let dir = tempfile::tempdir().unwrap();
        let o = outbox(&dir);
        let sending = o.sending().expect("not stopping yet");
        o.stop_sending();
        assert!(o.stopping());
        assert!(
            o.sending().is_none(),
            "a stopping daemon dispatches nothing"
        );
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            drop(sending);
        });
        let t0 = Instant::now();
        let r = o.settle_in_flight(Duration::from_secs(10)).await;
        assert_eq!(
            r,
            InFlight {
                waited_for: 1,
                left: 0
            }
        );
        let waited = t0.elapsed();
        assert!(waited >= Duration::from_millis(20), "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");

        let other = tempfile::tempdir().unwrap();
        let idle = outbox(&other);
        let t0 = Instant::now();
        let r = idle.settle_in_flight(Duration::from_secs(10)).await;
        assert_eq!(
            r,
            InFlight {
                waited_for: 0,
                left: 0
            }
        );
        assert!(t0.elapsed() < Duration::from_secs(1));
        assert!(idle.sending().is_none());
    }

    /// The grace counts from the stop's start, not from the wait's: a post
    /// that never settles is left, and the stop takes no longer for it.
    #[tokio::test]
    async fn a_post_that_never_settles_is_left_when_the_grace_since_the_stop_ends() {
        let dir = tempfile::tempdir().unwrap();
        let o = outbox(&dir);
        let _sending = o.sending().unwrap();
        o.stop_sending();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let t0 = Instant::now();
        let r = o.settle_in_flight(Duration::from_millis(400)).await;
        assert_eq!(
            r,
            InFlight {
                waited_for: 1,
                left: 1
            }
        );
        let waited = t0.elapsed();
        assert!(
            waited < Duration::from_millis(350),
            "the grace ran from the wait, not the stop: {waited:?}"
        );
    }
}
