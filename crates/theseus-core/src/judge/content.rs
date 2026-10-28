//! `content.v2` (theseus-9ymz0; v2 theseus-78qay): every item that enters a session is asked about
//! by itself, and what Jev finds becomes a durable label on the session
//! (`crate::content`). The hook sites queue what they wrote (`ask_content`) and
//! go on: nothing here is on a turn's path. This is `reply_data.rs`'s shape,
//! batched and with four answers: the state is prepared on the blocking pool and
//! the day's budget reserved beside the call, the call is made off the turn's
//! path, and the answers land between turns, in one frame under the session's
//! lock. Every missing, late or refused outcome fails closed: nothing is raised,
//! a `content.unjudged` row says why, and `judged_through` does not pass the
//! item, so a request bound for a model of restricted origin does not take the
//! session for judged.
//!
//! - **Batched.** The results of one loop are queued together and asked in one
//!   decision point: a session's queue is drained by one task at a time, after a
//!   short window (`COALESCE`) in which the loop's other results join, so a turn
//!   with ten tool calls makes one decision point and not ten, and a session's
//!   batches land in transcript order.
//! - **Long items** are split at line boundaries (`content::pieces`), each piece
//!   asked in the same decision point; an item's answer is the union of its
//!   pieces'.
//! - **The two bars** are `Flag::bar`'s: an answer of `yes` raises a flag only
//!   at its bar, and an answer below it is a `content.judged` row and nothing
//!   more. `injection` is not asked of the operator's own words: the builder
//!   leaves the question out (`ItemKind::asks_injection`).
//! - **A turn that must wait** for a verdict (the cn floor, `turn/data_step.rs`)
//!   reads [`JudgeService::content_pending`] and waits on
//!   [`JudgeService::wait_judged`], which the landing wakes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde_json::json;
use theseus_judge::builders::{ContentInput, DataNames, PIECE_CHARS};
use theseus_judge::{
    Answer, Ask, DecisionPoint, Input, Judge, Judgment, Mode, Outcome, Pack, Urgency,
};
use tokio::sync::Notify;

use super::{spend, Beside, JudgeService, ScrubWith};
use crate::content::{self, ContentFlags, Flag, ItemKind, Label};
use crate::fact::content::{AnswerRow, ContentJudged, ContentLabelled, ContentUnjudged};
use crate::node::{Body, Node};

/// The pack: the four content questions about one item.
///
/// v2 since theseus-78qay (the owner, 2026-10-27): a flag describes what an item
/// REVEALS, not what it is ABOUT. v1 stays embedded, as the record of what the
/// judgments before the bump were answered against.
pub const PACK: &str = "content.v2";

/// How long a session's queue waits, after its first item, for the rest of the
/// loop's results to join the batch.
pub const COALESCE: Duration = Duration::from_millis(50);

/// The longest a source line is kept.
const SOURCE_CHARS: usize = 200;

/// One item that entered a session, queued for judgment.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentItem {
    /// The node that carries it: what `judged_through` advances to.
    pub node: String,
    pub kind: ItemKind,
    /// The tool and the resource or address it named, or the place a message
    /// arrived in. Shown to the operator, so never a secret.
    pub source: String,
    pub text: String,
}

impl ContentItem {
    pub fn new(node: &str, kind: ItemKind, source: &str, text: &str) -> Self {
        Self {
            node: node.into(),
            kind,
            source: source.into(),
            text: text.into(),
        }
    }

    /// The item a message node is: its text as written, or `None` for a node
    /// that is no item. For the operator's words, a steer, a task's brief or a
    /// report a turn took.
    ///
    /// Whether it is an item at all is `content::item_text`'s to say and no
    /// one else's (theseus-sh1n3): the set submitted here and the set the
    /// floor blocks on are one set, read from one function.
    pub fn of_message(node: &Node, kind: ItemKind, source: &str) -> Option<Self> {
        if !matches!(node.body, Body::UserMessage { .. }) {
            return None;
        }
        let text = content::item_text(node)?;
        Some(Self::new(&node.id, kind, source, text))
    }

    /// The item a tool result node is, or `None` for one that is not classified:
    /// not a result, an acknowledgement of Theseus's own (`content::judges`), or
    /// a result with no text. `content::item_text` decides, as it does for a
    /// message (theseus-sh1n3).
    pub fn of_result(node: &Node) -> Option<Self> {
        let Body::ToolResult { tool, external, .. } = &node.body else {
            return None;
        };
        let text = content::item_text(node)?;
        let kind = match tool.as_str() {
            "http.fetch" => ItemKind::FetchedPage,
            "web.search" => ItemKind::SearchResult,
            _ => ItemKind::ToolResult,
        };
        let source = match external {
            Some(e) => format!("{tool} {}", e.url),
            None => tool.clone(),
        };
        Some(Self::new(&node.id, kind, &source, text))
    }
}

/// What a session's queue holds and what is still out.
#[derive(Default)]
struct Slot {
    queue: Vec<ContentItem>,
    /// A task is draining this queue.
    running: bool,
    /// Items queued or being judged, not yet landed.
    open: usize,
    /// Turns blocked on this session's verdicts right now
    /// (`JudgeService::settled`). **A landing someone is waiting on does not
    /// defer to a moment between turns**: the courtesy exists to keep the
    /// judge's writes off a turn's path, and a turn that is blocked waiting
    /// for the write is not helped by postponing it until after the turn it
    /// is blocking. Without this a request bound for a model of restricted
    /// origin could never be judged in time — the verdict landed about 7.7 s
    /// in against a 4 s wait, measured on a scratch daemon 2026-10-27, so
    /// every such turn fell back to Claude and no China-origin model was ever
    /// reachable.
    ///
    /// It is read **twice**, not once: a landing that began before any turn
    /// was waiting parks in `Turns::between`, and `awaiting` below ends that
    /// park when a turn joins (theseus-8qgwo). Read once, the park outlived
    /// the wait it was blocking and deadlocked the session's whole queue.
    waiters: usize,
    notify: Arc<Notify>,
    /// Fired when `waiters` goes from none to one: a turn has begun to wait
    /// on this session, so a parked landing must write now. A separate
    /// `Notify` from `notify`, which says "a batch landed": a landing waiting
    /// on that one would be waiting on itself.
    awaiting: Arc<Notify>,
}

/// A label told, as the core takes it: the session and the label.
type Told = Box<dyn Fn(&str, &Label) + Send + Sync>;

/// The service's state for this point.
#[derive(Default)]
pub(crate) struct Point {
    names: OnceLock<DataNames>,
    /// `[content] enabled = false`: nothing is asked.
    off: AtomicBool,
    slots: Mutex<HashMap<String, Slot>>,
    /// What the core does with a label once it is written: tell the operator's
    /// surfaces (`Core::tell_label`). Set by the core as it is built.
    told: std::sync::RwLock<Option<Told>>,
    /// `[content] max_pieces`: the most pieces one item is read in.
    max_pieces: std::sync::atomic::AtomicUsize,
    /// Decision points asked, for a test that counts them.
    #[cfg(test)]
    pub(crate) points: std::sync::atomic::AtomicU64,
    /// Every node a hook site offered for judgment, in order, for the test
    /// that holds the submitted set against the blocked set (theseus-sh1n3).
    #[cfg(test)]
    pub(crate) submitted: Mutex<Vec<String>>,
    /// Of those, the ones the sweep offered rather than a write site. A test
    /// that means to hold the **sites** to their queueing must see this
    /// empty, or it is proving the sweep and saying it proved the sites.
    #[cfg(test)]
    pub(crate) swept: Mutex<Vec<String>>,
}

impl Point {
    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Why an item was not asked: nothing was spent either way.
enum Unasked {
    /// The state or the client could not be built.
    Nothing,
    /// The day's budget refused the reservation.
    Refused,
}

impl Unasked {
    fn why(&self) -> &'static str {
        match self {
            Unasked::Nothing => "unbuilt",
            Unasked::Refused => "refused",
        }
    }
}

/// A batch, ready to send: its asks (each the piece of an item), the
/// reservation beside the call, and where the session's `judged_through`
/// stood when it was prepared.
struct Batch {
    built: Arc<super::Built>,
    /// `(item index, piece number, of)` and the ask, in order.
    asks: Vec<((usize, u32, u32), Ask)>,
    /// The items whose text was past the pieces' bound (`content::split`).
    truncated: Vec<usize>,
    need: theseus_judge::price::Micros,
    beside: Beside,
    base: Option<String>,
}

/// One item's outcome, as the landing writes it.
struct Landing {
    item: ContentItem,
    /// What each question said, and the raising answers' labels.
    answers: Vec<AnswerRow>,
    raised: Vec<Label>,
    pieces: u32,
    truncated: bool,
    cost: Option<u64>,
    /// `None`: judged. Else why not.
    unjudged: Option<&'static str>,
}

impl JudgeService {
    /// The operator's names, which the state carries (`data_names`), and
    /// whether the mechanism is on (`[content] enabled`): set once, as the core
    /// is built.
    pub fn content_from(&self, names: DataNames, enabled: bool, max_pieces: usize) {
        let _ = self.content.names.set(names);
        self.content.off.store(!enabled, Ordering::Relaxed);
        self.content.max_pieces.store(max_pieces, Ordering::Relaxed);
    }

    /// What to do once a label is written and told in the narrative: the core
    /// hands over `Core::tell_label` (the `session.labelled` event and the one
    /// Discord message), set once as it is built.
    /// Last writer wins, as `content_wait::Handle::set` does: the core stands
    /// `Core::tell_label` here as it is built, and a test may stand its own in
    /// its place. A set-once would silently make the test's a no-op.
    pub fn label_told(&self, told: impl Fn(&str, &Label) + Send + Sync + 'static) {
        *self
            .content
            .told
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(told));
    }

    /// Every node a hook site offered for judgment, whether or not the
    /// mechanism was on to ask about it (theseus-sh1n3).
    #[cfg(test)]
    pub(crate) fn content_submitted(&self) -> Vec<String> {
        self.content
            .submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Of those, the ones the sweep offered rather than a write site.
    #[cfg(test)]
    pub(crate) fn content_swept(&self) -> Vec<String> {
        self.content
            .swept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Ends when a turn begins to wait on this session's verdicts
    /// (theseus-8qgwo). A landing that parked because nothing was waiting
    /// yet selects on this, so the park cannot outlive the reason for it.
    ///
    /// Interest is taken **before** the count is read, so a turn that begins
    /// to wait between the two still ends this rather than being missed — the
    /// same contract `content_wait::Verdicts` states for its own future.
    async fn until_awaited(&self, session: &str) {
        let awaiting = {
            let mut slots = self.content.slots();
            slots
                .entry(session.to_string())
                .or_default()
                .awaiting
                .clone()
        };
        let woken = awaiting.notified();
        tokio::pin!(woken);
        woken.as_mut().enable();
        // After enabling, not before: a turn that began to wait in between is
        // seen here, and one that begins after it ends the enabled future.
        if self.content_awaited(session) {
            return;
        }
        woken.await;
    }

    /// Whether a turn is blocked on this session's verdicts right now.
    pub(crate) fn content_awaited(&self, session: &str) -> bool {
        self.content
            .slots()
            .get(session)
            .is_some_and(|s| s.waiters > 0)
    }

    /// Queue `items` for judgment and return: nothing waits here. The items are
    /// in transcript order, and each answer lands between turns unless a turn
    /// is waiting on them, under the session's lock. Nothing is asked with the mechanism off, the judge off or
    /// the pack off, and an item with no text is never asked (it counts as
    /// judged once the batch it rides in lands).
    pub fn ask_content(&self, session: &str, items: Vec<ContentItem>) {
        if items.is_empty() {
            return;
        }
        // Every node a hook site offered, whether or not the mechanism is on:
        // what a test holds the blocker's set against (theseus-sh1n3).
        #[cfg(test)]
        self.content
            .submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(items.iter().map(|i| i.node.clone()));
        if self.content.off.load(Ordering::Relaxed) {
            return;
        }
        if !self.mode_for(PACK, session).on() || self.pack(PACK).is_none() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let spawn = {
            let mut slots = self.content.slots();
            let slot = slots.entry(session.to_string()).or_default();
            slot.open += items.len();
            slot.queue.extend(items);
            !std::mem::replace(&mut slot.running, true)
        };
        if spawn {
            rt.spawn(drain(self.me.clone(), session.to_string()));
        }
    }

    /// Every item of `session` past its mark that nothing has queued, queued
    /// now (theseus-sh1n3). The floor calls it before it waits, so the set a
    /// restricted-origin request blocks on and the set that is submitted are
    /// **the same set by construction**, not by an audit of the write sites.
    ///
    /// Why it exists at all. A node becomes an item the moment it is written,
    /// and the sites that write one are many — a turn's steps, a tool's
    /// completion, a task's reports, an RPC that appends to a session nobody's
    /// turn owns. Each queues what it wrote. A site that forgets costs the
    /// session every model of restricted origin for the rest of its life, in
    /// silence, and nothing fails until someone runs a cn request and reads
    /// the gate's row. Two such sites shipped in theseus-9ymz0. This makes the
    /// floor's own reading of the transcript the submitter of last resort, so
    /// a site added later that forgets costs a round trip, not a session.
    ///
    /// It does nothing while anything is in flight for the session. **That
    /// stops a second, concurrent ask for one node; it is not a claim that a
    /// node is never asked twice** (the review of 2026-10-27, B3).
    ///
    /// **A mark stuck on a batch's own failure is re-asked here too**
    /// (theseus-8qgwo). B3 had this stand down and leave the re-ask to
    /// `with_worklist`, to avoid paying again for the items judged behind the
    /// stuck one. That was right about the cost and wrong about who would pay
    /// it: `with_worklist` rides a **new batch**, and between a tool's result
    /// and the next model call of the same turn there is no new item to queue,
    /// so no batch is ever made for it to ride. One content call that fails
    /// mid-turn takes the session off every model of restricted origin **for
    /// the rest of that turn, and until another item is queued** — which for
    /// a bench trial is the whole trial, and for a chat session is until the
    /// next thing anyone says.
    ///
    /// `reask_stuck` is the caller's, not read from ambient state: only the
    /// floor knows both "I am about to wait on this session" and "I have not
    /// re-asked yet in this wait", and inferring the first from
    /// `content_awaited` made it depend on `cn_wait` happening to call
    /// `verdicts.settled` two lines earlier — three facts across two modules
    /// with nothing asserting them, and an unbounded repeat, since `cn_move`
    /// runs on every loop after every tool result. The cost B3 was protecting
    /// is still not paid: a stuck mark re-asks the **stuck prefix alone**, the
    /// run of worklist items from the mark, never the judged items behind it.
    ///
    /// At most [`SWEEP_MAX`] a call: the mark moves over the batch's own span
    /// (`land` picks the last of the batch's leading judged run, and `span_to`
    /// builds the span from the mark to it), so the rest follow on the next
    /// read.
    ///
    /// Turns of one session are serialised, so the read of `content_pending`
    /// and the queueing that follows it are not racing another turn of the
    /// same session. If that stops being true this wants the slots lock held
    /// across both.
    /// Returns whether it re-asked a **stuck** mark, so a caller that allows
    /// one such re-ask a wait can spend it on the read where the mark is
    /// actually stuck rather than on the first read of the wait. A batch that
    /// fails does so partway through a wait, so "the first read" and "the
    /// read that finds it stuck" are rarely the same read.
    pub fn ask_unsubmitted(&self, session: &str, reask_stuck: bool) -> bool {
        if self.content.off.load(Ordering::Relaxed) || self.content_pending(session) {
            return false;
        }
        // A build with no such pack can never ask, so it need not read.
        //
        // The review (should-fix 4) asked for the per-session **mode** check
        // here too, since everything below is a record read, a whole
        // transcript and up to sixteen item builds that clone a node's whole
        // text, and with the pack off for a session none of it is ever asked.
        // It is deliberately not here: the stand-in judge the floor's tests
        // use (`content_wait::judging`) replaces the judge entirely and runs
        // with this pack's mode off, so a mode check makes the sweep
        // invisible to the only rig that exercises it — and the work it would
        // save falls on a config where the session cannot reach a model of
        // restricted origin at all.
        if self.pack(PACK).is_none() {
            return false;
        }
        let Ok(Some(rec)) = self
            .store
            .get_session::<crate::session::SessionRecord>(session)
        else {
            return false;
        };
        let Ok(nodes) = self.store.transcript(session) else {
            return false;
        };
        let flags = flags_of(&rec);
        let stuck: Vec<String> = flags.map(|f| f.unjudged.clone()).unwrap_or_default();
        let through = flags.and_then(|f| f.judged_through.clone());
        // A mark the transcript has lost reads as nothing judged, exactly as
        // `content_wait::unjudged` reads it: fail closed, and ask again.
        let from = match through.as_deref() {
            None => 0,
            Some(id) => nodes
                .iter()
                .position(|(_, n)| n.id == id)
                .map_or(0, |at| at + 1),
        };
        let item = |n: &&(u64, crate::stub::Stub)| crate::content_wait::is_item(&n.1);
        let past: Vec<&(u64, crate::stub::Stub)> = nodes[from..].iter().filter(item).collect();
        // Nothing past the mark: the session is judged through, and the floor
        // would not have asked.
        let Some((_, first)) = past.first() else {
            return false;
        };
        // The mark stuck on a batch's own failure. Re-ask the stuck run from
        // the mark and nothing behind it, and only when the caller asked for
        // it: `with_worklist` cannot reach this case (see above).
        let wedged = stuck.contains(&first.id);
        if wedged && !reask_stuck {
            return false;
        }
        let brief = crate::task::brief(&self.store, session)
            .ok()
            .flatten()
            .map(|n| n.id);
        let reach: Vec<&&(u64, crate::stub::Stub)> = match wedged {
            true => past
                .iter()
                .take_while(|(_, n)| stuck.contains(&n.id))
                .collect(),
            false => past.iter().collect(),
        };
        let items: Vec<ContentItem> = reach
            .into_iter()
            .take(SWEEP_MAX)
            .filter_map(|(_, n)| rebuild(n, brief.as_deref()))
            .collect();
        #[cfg(test)]
        self.content
            .swept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(items.iter().map(|i| i.node.clone()));
        let asked = !items.is_empty();
        self.ask_content(session, items);
        wedged && asked
    }

    /// Whether items of `session` are queued or being judged: a request bound
    /// for a model of restricted origin waits for them (`wait_judged`), or goes
    /// to Claude.
    pub fn content_pending(&self, session: &str) -> bool {
        self.content
            .slots()
            .get(session)
            .is_some_and(|s| s.open > 0)
    }

    /// Wait at most `max` for the session's queued items to land; whether none
    /// are pending now. Woken by each landing. A session with nothing queued
    /// returns at once.
    pub async fn wait_judged(&self, session: &str, max: Duration) -> bool {
        let notify = {
            let mut slots = self.content.slots();
            slots.entry(session.to_string()).or_default().notify.clone()
        };
        let waited = async {
            loop {
                let woken = notify.notified();
                tokio::pin!(woken);
                woken.as_mut().enable();
                if !self.content_pending(session) {
                    return;
                }
                woken.await;
            }
        };
        let _ = tokio::time::timeout(max, waited).await;
        !self.content_pending(session)
    }
}

/// The judge standing behind the turn's wait (theseus-9ymz0, the join's seam).
/// `content_wait::Ready` answers at once, so until this is set a turn bound for
/// a model of restricted origin with unjudged items goes to Claude without
/// waiting — safe, but it would make `[content] wait_ms` dead and every cn turn
/// a Claude turn. `Core::build` sets it.
///
/// It honours the trait's contract: the wake-up is enabled **before** the
/// pending check, so a verdict that lands between the caller taking this future
/// and first polling it still ends the future rather than being slept through.
/// The caller bounds the wait; this never times out on its own.
/// One turn's wait on a session's verdicts, counted while it lasts.
struct Waiting {
    me: Weak<JudgeService>,
    session: String,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if let Some(svc) = self.me.upgrade() {
            if let Some(slot) = svc.content.slots().get_mut(&self.session) {
                slot.waiters = slot.waiters.saturating_sub(1);
            }
        }
    }
}

impl crate::content_wait::Verdicts for JudgeService {
    fn settled(
        &self,
        session: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let (notify, pending, joined) = {
            let mut slots = self.content.slots();
            let slot = slots.entry(session.to_string()).or_default();
            // Counted before the pending check, so a verdict landing between
            // this call and the first poll still writes without deferring.
            slot.waiters += 1;
            (
                slot.notify.clone(),
                slot.open > 0,
                (slot.waiters == 1).then(|| slot.awaiting.clone()),
            )
        };
        // The first waiter tells a landing already parked in `Turns::between`
        // that there is now a turn to write for (theseus-8qgwo). Outside the
        // lock: `notify_waiters` wakes, and nothing it wakes needs the slots.
        if let Some(a) = joined {
            a.notify_waiters();
        }
        // The count comes down however the wait ends — the caller's timeout
        // included — so a dropped future never leaves a session marked awaited.
        let guard = Waiting {
            me: self.me.clone(),
            session: session.to_string(),
        };
        Box::pin(async move {
            let _guard = guard;
            if !pending {
                return;
            }
            notify.notified().await;
        })
    }
}

impl JudgeService {
    /// The blocking half: every item's state (a piece each, a long item cut at
    /// line boundaries), their blobs staged, and one reservation for the batch,
    /// its frame left beside the call.
    fn prepare_content(
        &self,
        pack: Arc<Pack>,
        session: &str,
        items: &[ContentItem],
        today: &str,
    ) -> Result<Batch, Unasked> {
        let scrub = ScrubWith(self.scrubber.clone());
        let names = self.content.names.get().cloned().unwrap_or_default();
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .map_err(|_| Unasked::Nothing)?;
        let batch_id = format!("cbt_{}", uuid::Uuid::now_v7().simple());
        let mut asks = Vec::new();
        let mut truncated = Vec::new();
        let max = self.content.max_pieces.load(Ordering::Relaxed).max(1);
        for (i, item) in items.iter().enumerate() {
            if item.text.trim().is_empty() {
                continue;
            }
            let (pieces, cut) = content::split(&item.text, PIECE_CHARS, max);
            if cut {
                truncated.push(i);
            }
            for (n, of, text) in pieces {
                let input = ContentInput {
                    text,
                    kind: item.kind.as_str().into(),
                    source: item.source.clone(),
                    piece: Some((n, of)),
                    data_names: names.clone(),
                };
                let state = theseus_judge::prepare(&pack, &Input::Content(input), &scrub)
                    .map_err(|_| Unasked::Nothing)?;
                let blob = self.stage_blob(state.state.json.as_bytes());
                let mut context = json!({
                    "session": session, "purpose": "content", "node": item.node,
                    "kind": item.kind, "piece": [n, of], "batch": batch_id,
                    "batch_items": items.len(), "baseline": "none", "blob": blob,
                    "on_path_ms": 0,
                });
                let mode = self.ask_mode(PACK, &mut context);
                asks.push(((i, n, of), Ask::new(pack.clone(), &state, mode, context)));
            }
        }
        let need = if asks.is_empty() {
            0
        } else {
            let all: Vec<Ask> = asks.iter().map(|(_, a)| a.clone()).collect();
            built.judge.inner().reserve_micros(&all).unwrap_or(0)
        };
        let beside = match asks.is_empty() {
            true => Beside {
                records: vec![],
                said: vec![],
            },
            false => self.reserve_beside(today, need).ok_or(Unasked::Refused)?,
        };
        let base = self
            .store
            .get_session::<crate::session::SessionRecord>(session)
            .ok()
            .flatten()
            .and_then(|r| flags_of(&r).and_then(|f| f.judged_through.clone()));
        Ok(Batch {
            built,
            asks,
            truncated,
            need,
            beside,
            base,
        })
    }

    /// The batch's write, one frame: each item's rows and each label raised,
    /// then `judged_through` moved through the last item judged in transcript
    /// order, all under the session's lock. The move is compare-and-set: a
    /// stored `judged_through` that is no longer the one the batch began
    /// against is left alone (a label is added all the same: it is never wrong
    /// to add one).
    fn land_content(
        &self,
        session: &str,
        landings: &[Landing],
        through: Option<&str>,
        base: &Option<String>,
    ) {
        // The labels this write really added (`raise` is false for one already
        // on the session), as `(landing, label)`.
        let mut fresh: Vec<(usize, &Label)> = Vec::new();
        let written = self.store.update_session(session, |rec| {
            let mut flags = take_flags(rec);
            let mut rows = Vec::new();
            for (at, l) in landings.iter().enumerate() {
                let it = &l.item;
                match l.unjudged {
                    Some(why) => rows.push(crate::fact::row(
                        &ContentUnjudged {
                            node: &it.node,
                            kind: it.kind,
                            source: &it.source,
                            why,
                        },
                        Some(session),
                        None,
                    )?),
                    None if l.pieces > 0 => rows.push(crate::fact::row(
                        &ContentJudged {
                            node: &it.node,
                            kind: it.kind,
                            source: &it.source,
                            pieces: l.pieces,
                            truncated: l.truncated,
                            answers: l.answers.clone(),
                            cost_micros: l.cost,
                        },
                        Some(session),
                        None,
                    )?),
                    None => {}
                }
                for label in &l.raised {
                    if flags.raise(label.clone()) {
                        fresh.push((at, label));
                        rows.push(crate::fact::row(
                            &ContentLabelled {
                                flag: label.flag,
                                node: &it.node,
                                kind: it.kind,
                                source: &it.source,
                                confidence: label.confidence,
                                band: label.band,
                            },
                            Some(session),
                            None,
                        )?);
                    }
                }
            }
            self.move_mark(&mut flags, session, landings, through, base);
            put_flags(rec, flags);
            Ok(rows)
        });
        match &written {
            Ok(Some(_)) => {}
            Ok(None) => {
                tracing::debug!(
                    session,
                    "judge: a session's content was not written; it is gone"
                );
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: a session's content labels were not written");
            }
        }
        if !matches!(written, Ok(Some(_))) {
            return;
        }
        // What was raised is told after it is written, in the session's own words,
        // and to the operator's surfaces.
        for (at, label) in fresh {
            let l = &landings[at];
            self.announce(
                Some(session),
                None,
                &ContentLabelled {
                    flag: label.flag,
                    node: &l.item.node,
                    kind: l.item.kind,
                    source: &l.item.source,
                    confidence: label.confidence,
                    band: label.band,
                },
            );
            // And told to the operator: `Core::tell_label` (`rpc/content.rs`)
            // publishes `session.labelled` and posts the one approved Discord
            // message with its Remove button. The service holds no core, so the
            // core hands it that call once as it is built (`Core::build`).
            if let Some(told) = self
                .content
                .told
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
            {
                told(session, label);
            }
        }
    }
}

impl JudgeService {
    /// What this batch did to the session's mark.
    ///
    /// What it could not judge is recorded durably, as a worklist for a re-ask
    /// and so the operator can see it; that record gates nothing.
    ///
    /// The mark itself moves only over a transcript span whose every node this
    /// batch judged (`ContentFlags::judged_run`). The span comes from the
    /// **transcript**, not from the batch, which is what makes this hold across
    /// a restart: the judging process forgets which item it gave up on, and the
    /// transcript remembers for it. The compare-and-set against `base` still
    /// guards a mark that moved while the batch was in flight.
    fn move_mark(
        &self,
        flags: &mut ContentFlags,
        session: &str,
        landings: &[Landing],
        through: Option<&str>,
        base: &Option<String>,
    ) {
        for l in landings.iter().filter(|l| l.unjudged.is_some()) {
            flags.could_not_judge(&l.item.node);
        }
        let Some(n) = through else { return };
        if !(flags.judged_through == *base || flags.judged_through.as_deref() == Some(n)) {
            return;
        }
        let Some(span) = span_to(&self.store, session, flags.judged_through.as_deref(), n) else {
            return;
        };
        let judged: Vec<String> = landings
            .iter()
            .filter(|l| l.unjudged.is_none())
            .map(|l| l.item.node.clone())
            .collect();
        flags.judged_run(&span, &judged, n);
    }
}

/// The most nodes of the worklist one batch takes back: a Jev that fails every
/// call keeps each failure on the list, and none of them may make a batch
/// without bound.
const REASK_MAX: usize = 16;

/// The most items one sweep takes (`JudgeService::ask_unsubmitted`): a session
/// that has run a long way on Claude and is then asked for a model of
/// restricted origin has every item of it to judge, and none of them may make
/// one unbounded batch. The mark moves over each batch's own span, so the next
/// read takes the next lot.
const SWEEP_MAX: usize = 16;

impl JudgeService {
    /// `items`, preceded by the nodes of the session's `unjudged` worklist
    /// (`ContentFlags::unjudged`): what an earlier batch could not judge is asked
    /// again in this one, so a covering batch moves the mark
    /// (`ContentFlags::judged_run`) and the session becomes eligible for a model
    /// of restricted origin again, which a single failure otherwise forfeits
    /// for good.
    ///
    /// A node is read back from the transcript by id and its item rebuilt. A node
    /// the transcript no longer holds, or that is no longer an item, is dropped
    /// from the list rather than retried for ever. The list is read only when a
    /// batch is queued, never on a timer: a re-ask that fails again leaves its
    /// node where it is and starts nothing.
    fn with_worklist(&self, session: &str, items: Vec<ContentItem>) -> Vec<ContentItem> {
        let Ok(Some(rec)) = self
            .store
            .get_session::<crate::session::SessionRecord>(session)
        else {
            return items;
        };
        let worklist: Vec<String> = match flags_of(&rec) {
            Some(f) if !f.unjudged.is_empty() => f.unjudged.clone(),
            _ => return items,
        };
        let Ok(nodes) = self.store.transcript(session) else {
            return items;
        };
        let brief = crate::task::brief(&self.store, session)
            .ok()
            .flatten()
            .map(|n| n.id);
        let (mut again, mut gone) = (Vec::new(), Vec::new());
        for id in &worklist {
            if items.iter().any(|i| i.node == *id) {
                continue;
            }
            let rebuilt = nodes
                .iter()
                .any(|(_, n)| n.id == *id)
                .then(|| self.store.get_node(id).ok().flatten())
                .flatten()
                .and_then(|(_, n)| rebuild(&n, brief.as_deref()));
            match rebuilt {
                Some(item) if again.len() < REASK_MAX => again.push(item),
                Some(_) => {}
                None => gone.push(id.clone()),
            }
        }
        if !gone.is_empty() {
            let dropped = self.store.update_session(session, |rec| {
                let mut flags = take_flags(rec);
                flags.unjudged.retain(|n| !gone.contains(n));
                put_flags(rec, flags);
                Ok(vec![])
            });
            if let Err(e) = dropped {
                tracing::warn!(error = %format!("{e:#}"), "judge: the unjudged list was not trimmed");
            }
        }
        again.extend(items);
        again
    }
}

/// The item a node of the transcript is, as it was queued. A tool result and a
/// task's brief come back as they were. Any other message comes back as a
/// person's: its kind is not stored, and asking more of a message than was
/// asked is the safe direction. `None` for a node that is no longer an item.
fn rebuild(node: &Node, brief: Option<&str>) -> Option<ContentItem> {
    match &node.body {
        Body::ToolResult { .. } => ContentItem::of_result(node),
        Body::UserMessage { .. } if brief == Some(node.id.as_str()) => {
            ContentItem::of_message(node, ItemKind::TaskBrief, "task brief")
        }
        Body::UserMessage { .. } => {
            ContentItem::of_message(node, ItemKind::PersonMessage, "message")
        }
        _ => None,
    }
}

/// Every item node of `session`'s transcript strictly after `through`, up to
/// and including `node`: the span `ContentFlags::judged_run` is held to. `None`
/// when the transcript cannot be read, when it does not hold `node`, or when it
/// does not hold `through` — each of which leaves the mark where it is, which
/// fails closed, exactly as `content_wait::unjudged` reads a mark the
/// transcript has lost.
///
/// Only items count. A node that is not an item (an assistant message, a tool
/// call) was never going to be judged, so including it would stop the mark for
/// good.
fn span_to(
    store: &crate::store::Store,
    session: &str,
    through: Option<&str>,
    node: &str,
) -> Option<Vec<String>> {
    let nodes = store.transcript(session).ok()?;
    let at = nodes.iter().position(|(_, n)| n.id == node)?;
    let from = match through {
        None => 0,
        Some(id) => nodes.iter().position(|(_, n)| n.id == id)? + 1,
    };
    if from > at {
        return None;
    }
    Some(
        nodes[from..=at]
            .iter()
            .filter(|(_, n)| crate::content_wait::is_item(n))
            .map(|(_, n)| n.id.clone())
            .collect(),
    )
}

/// The record's content flags, taken out to be changed.
fn take_flags(rec: &mut crate::session::SessionRecord) -> ContentFlags {
    rec.data.take().map(|b| *b).unwrap_or_default()
}

/// Put them back; a record with nothing on it keeps no field at all.
fn put_flags(rec: &mut crate::session::SessionRecord, flags: ContentFlags) {
    rec.data = (flags != ContentFlags::default()).then(|| Box::new(flags));
}

fn flags_of(rec: &crate::session::SessionRecord) -> Option<&ContentFlags> {
    rec.data.as_deref()
}

/// A session's queue, drained one batch at a time until it is empty. The
/// service is held only around the blocking halves, never across the call or
/// the wait for a quiet moment.
async fn drain(me: Weak<JudgeService>, session: String) {
    loop {
        tokio::time::sleep(COALESCE).await;
        let Some(svc) = me.upgrade() else { return };
        let batch = {
            let mut slots = svc.content.slots();
            let Some(slot) = slots.get_mut(&session) else {
                return;
            };
            if slot.queue.is_empty() {
                slot.running = false;
                return;
            }
            std::mem::take(&mut slot.queue)
        };
        drop(svc);
        let n = batch.len();
        judge_batch(&me, &session, batch).await;
        let Some(svc) = me.upgrade() else { return };
        let notify = {
            let mut slots = svc.content.slots();
            let slot = slots.entry(session.clone()).or_default();
            slot.open = slot.open.saturating_sub(n);
            slot.notify.clone()
        };
        notify.notify_waiters();
    }
}

/// One batch: asked in one decision point, then landed.
async fn judge_batch(me: &Weak<JudgeService>, session: &str, items: Vec<ContentItem>) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let Some(pack) = svc.pack(PACK) else { return };
    let total = Duration::from_secs(svc.cfg.total_secs);
    // What an earlier batch could not judge rides this one (theseus-9ymz0, the
    // review's B4): the mark cannot pass it until it is judged.
    let items = {
        let (svc, sid) = (svc.clone(), session.to_string());
        let held = items.clone();
        tokio::task::spawn_blocking(move || svc.with_worklist(&sid, items))
            .await
            .unwrap_or(held)
    };
    let (day, queued, sid) = (today.clone(), items.clone(), session.to_string());
    let prepared = tokio::task::spawn_blocking({
        let svc = svc.clone();
        move || svc.prepare_content(pack, &sid, &queued, &day)
    })
    .await;
    drop(svc);
    let batch = match prepared {
        Ok(Ok(b)) => b,
        Ok(Err(why)) => {
            let landings = items
                .into_iter()
                .map(|item| Landing {
                    item,
                    answers: vec![],
                    raised: vec![],
                    pieces: 0,
                    truncated: false,
                    cost: None,
                    unjudged: Some(why.why()),
                })
                .collect();
            return land(me, session, landings, &None).await;
        }
        Err(_) => return,
    };
    let Batch {
        built,
        asks,
        truncated,
        need,
        beside,
        base,
    } = batch;
    let mut judgments: Vec<Judgment> = Vec::new();
    if !asks.is_empty() {
        let wrote = beside.spawn(me);
        #[cfg(test)]
        if let Some(svc) = me.upgrade() {
            svc.content.points.fetch_add(1, Ordering::Relaxed);
        }
        judgments = built
            .judge
            .judge(DecisionPoint {
                asks: asks.iter().map(|(_, a)| a.clone()).collect(),
                urgency: Urgency::live(total),
            })
            .await;
        settle(me, &today, need, &judgments);
        if let Some(w) = wrote {
            let _ = w.await;
        }
    }
    let landings = {
        let Some(svc) = me.upgrade() else { return };
        landings_of(&svc, items, &asks, &truncated, &judgments)
    };
    land(me, session, landings, &base).await;
}

/// Each judgment settles its share of the point's reservation, the last taking
/// what is left; a call whose usage is unknown is booked at its share,
/// conservatively.
fn settle(
    me: &Weak<JudgeService>,
    today: &str,
    need: theseus_judge::price::Micros,
    js: &[Judgment],
) {
    let Some(svc) = me.upgrade() else { return };
    let mut left = need;
    let n = js.len();
    for (k, j) in js.iter().enumerate() {
        let reserved = match k + 1 == n {
            true => left,
            false => j.reserve_micros.unwrap_or(0).min(left),
        };
        left -= reserved;
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { reserved } else { 0 });
        svc.budget.settle(today, reserved, spent, called, failed);
    }
}

/// Each item's outcome from its pieces' judgments: the union of their answers,
/// and a label for each flag whose answer was `yes` at or above its bar.
fn landings_of(
    svc: &JudgeService,
    items: Vec<ContentItem>,
    asks: &[((usize, u32, u32), Ask)],
    truncated: &[usize],
    judgments: &[Judgment],
) -> Vec<Landing> {
    let now = theseus_protocol::now_unix_ms();
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let mine: Vec<&Judgment> = asks
                .iter()
                .zip(judgments)
                .filter(|(((idx, _, _), _), _)| *idx == i)
                .map(|(_, j)| j)
                .collect();
            let source = clip(&svc.scrubber.scrub(&item.source).0, SOURCE_CHARS);
            let mut l = Landing {
                pieces: mine.len() as u32,
                item,
                answers: vec![],
                raised: vec![],
                truncated: truncated.contains(&i),
                cost: None,
                unjudged: None,
            };
            l.item.source = source;
            for j in &mine {
                l.cost = Some(l.cost.unwrap_or(0) + j.cost_micros.unwrap_or(0));
                if !j.actionable() {
                    l.unjudged = Some("failed");
                    continue;
                }
                if j.mode == Mode::Shadow {
                    l.unjudged = l.unjudged.or(Some("shadow"));
                }
                for flag in Flag::ALL {
                    let Some(a) = j.answer(flag.as_str()) else {
                        continue;
                    };
                    let Answer::Choice {
                        choice, confidence, ..
                    } = &a.answer
                    else {
                        continue;
                    };
                    let yes = choice == "yes";
                    let raises = yes && flag.raised_by(a.band.band) && j.mode != Mode::Shadow;
                    let row = AnswerRow {
                        question: flag.as_str(),
                        choice: choice.clone(),
                        confidence: *confidence,
                        band: a.band.band,
                    };
                    // A question asked of several pieces keeps the one that raised.
                    match l.answers.iter().position(|r| r.question == flag.as_str()) {
                        Some(at) if raises && !l.raised.iter().any(|r| r.flag == flag) => {
                            l.answers[at] = row;
                        }
                        Some(_) => {}
                        None => l.answers.push(row),
                    }
                    if raises && !l.raised.iter().any(|r| r.flag == flag) {
                        l.raised.push(Label {
                            flag,
                            node: Some(l.item.node.clone()),
                            kind: l.item.kind,
                            source: l.item.source.clone(),
                            confidence: *confidence,
                            band: a.band.band,
                            at_ms: now,
                            judgment: j.id.clone(),
                        });
                    }
                }
            }
            if mine.is_empty() {
                l.pieces = 0;
            }
            l
        })
        .collect()
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

/// The write between turns. `judged_through` moves through the last item of the
/// contiguous run of judged items from the batch's start, and never past an
/// item that could not be judged, in this batch or an earlier one: the span
/// from the mark, read from the transcript, must be covered whole
/// (`ContentFlags::judged_run`). An earlier batch's failure is re-asked in the
/// next batch (`JudgeService::with_worklist`), so the mark moves again.
async fn land(
    me: &Weak<JudgeService>,
    session: &str,
    landings: Vec<Landing>,
    base: &Option<String>,
) {
    let Some(svc) = me.upgrade() else { return };
    let (turns, timing, awaited) = (
        svc.between.get().cloned(),
        svc.sink_timing(),
        svc.content_awaited(session),
    );
    // A turn is blocked on this session's verdict: write it now — and **a turn
    // that begins to wait while this is parked ends the park** (theseus-8qgwo).
    //
    // `awaited` read once was a deadlock, and it is what failed 25 of cell 8's
    // 89 `c8c-deepseek-high-k1` trials. This runs on the drain task's own path
    // (`drain` -> `judge_batch` -> here), so parking stops the **whole
    // session's queue**: no later batch is taken, `slot.open` never comes down
    // and `notify_waiters` never fires. A parallel tool-call group writes its
    // results across the loop; the first batch takes whatever was queued
    // within `COALESCE`, Jev answers it while the turn is still running the
    // rest of the group, so no turn is waiting yet and this parks behind the
    // running turn — holding the group's later results in the queue. The turn
    // then reaches the floor, waits its whole `[content] wait_ms` on a notify
    // that cannot fire, and is refused. The landing waits for the turn to end
    // and the turn waits for the landing. Every one of the 25 shows the same
    // 30.007-30.014 s between its last tool result and the `data_gate` row:
    // one uninterrupted `wait_ms`, with nothing having failed at all.
    //
    // A single-call loop cannot reach it — the one result is written, the
    // floor begins waiting within milliseconds, and the landing finds
    // `awaited` already true. That is why none of the cell's 1,181 single-call
    // loops failed and all 25 failures were on groups of 2, 3 or 11.
    let _writing = match turns.filter(|_| !awaited) {
        Some(t) => {
            let park = t.between(tokio::time::Instant::now(), &timing);
            tokio::pin!(park);
            tokio::select! {
                w = &mut park => Some(w),
                () = svc.until_awaited(session) => None,
            }
        }
        None => None,
    };
    let Some(svc) = me.upgrade() else { return };
    // The mark may reach the last item of the run judged from the batch's
    // start. Whether it may pass what an earlier batch could not judge is not
    // decided here: `move_mark` holds it to the transcript's span.
    let run = landings.iter().take_while(|l| l.unjudged.is_none()).count();
    let through = run
        .checked_sub(1)
        .and_then(|last| landings.get(last))
        .map(|l| l.item.node.clone());
    let sid = session.to_string();
    let base = base.clone();
    let _ = tokio::task::spawn_blocking(move || {
        svc.land_content(&sid, &landings, through.as_deref(), &base)
    })
    .await;
}
