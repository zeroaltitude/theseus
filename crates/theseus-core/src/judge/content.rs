//! `content.v1` (theseus-9ymz0): every item that enters a session is asked about
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
pub const PACK: &str = "content.v1";

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
    /// with none. For the operator's words, a steer, a task's brief or a report
    /// a turn took.
    pub fn of_message(node: &Node, kind: ItemKind, source: &str) -> Option<Self> {
        let Body::UserMessage { text, .. } = &node.body else {
            return None;
        };
        (!text.trim().is_empty()).then(|| Self::new(&node.id, kind, source, text))
    }

    /// The item a tool result node is, or `None` for one that is not classified:
    /// not a result, an acknowledgement of Theseus's own (`content::judges`), or
    /// a result with no text.
    pub fn of_result(node: &Node) -> Option<Self> {
        let Body::ToolResult {
            tool,
            content,
            external,
            ..
        } = &node.body
        else {
            return None;
        };
        if !content::judges(tool) || content.trim().is_empty() {
            return None;
        }
        let kind = match tool.as_str() {
            "http.fetch" => ItemKind::FetchedPage,
            "web.search" => ItemKind::SearchResult,
            _ => ItemKind::ToolResult,
        };
        let source = match external {
            Some(e) => format!("{tool} {}", e.url),
            None => tool.clone(),
        };
        Some(Self::new(&node.id, kind, &source, content))
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
    /// An item of this session could not be judged since this process began:
    /// `judged_through` never passes it, so the session stays off a model of
    /// restricted origin until a restart re-reads it.
    gap: bool,
    notify: Arc<Notify>,
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

    /// Queue `items` for judgment and return: nothing waits here. The items are
    /// in transcript order, and each answer lands between turns, under the
    /// session's lock. Nothing is asked with the mechanism off, the judge off or
    /// the pack off, and an item with no text is never asked (it counts as
    /// judged once the batch it rides in lands).
    pub fn ask_content(&self, session: &str, items: Vec<ContentItem>) {
        if items.is_empty() || self.content.off.load(Ordering::Relaxed) {
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
impl crate::content_wait::Verdicts for JudgeService {
    fn settled(
        &self,
        session: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let notify = {
            let mut slots = self.content.slots();
            slots.entry(session.to_string()).or_default().notify.clone()
        };
        let pending = self.content_pending(session);
        Box::pin(async move {
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
/// item that could not be judged, in this batch or an earlier one of the
/// session (`Slot::gap`).
async fn land(
    me: &Weak<JudgeService>,
    session: &str,
    landings: Vec<Landing>,
    base: &Option<String>,
) {
    let (turns, timing) = match me.upgrade() {
        Some(s) => (s.between.get().cloned(), s.sink_timing()),
        None => return,
    };
    let _writing = match turns {
        Some(t) => Some(t.between(tokio::time::Instant::now(), &timing).await),
        None => None,
    };
    let Some(svc) = me.upgrade() else { return };
    let run = landings.iter().take_while(|l| l.unjudged.is_none()).count();
    let broke = run < landings.len();
    let through = {
        let mut slots = svc.content.slots();
        let slot = slots.entry(session.to_string()).or_default();
        let through = match slot.gap || run == 0 {
            true => None,
            false => landings.get(run - 1).map(|l| l.item.node.clone()),
        };
        slot.gap |= broke;
        through
    };
    let sid = session.to_string();
    let base = base.clone();
    let _ = tokio::task::spawn_blocking(move || {
        svc.land_content(&sid, &landings, through.as_deref(), &base)
    })
    .await;
}
