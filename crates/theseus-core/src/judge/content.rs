//! `content.v3` (theseus-9ymz0; v2 theseus-78qay, v3 theseus-gyjyo.11): every item that enters a session is asked about
//! by itself, and what Jev finds becomes a durable label on the session
//! (`crate::content`). The hook sites queue what they wrote (`ask_content`) and
//! go on: nothing here is on a turn's path. This is `reply_data.rs`'s shape,
//! batched and with four answers: the state is prepared on the blocking pool and
//! the day's budget reserved beside the call, the call is made off the turn's
//! path, and the answers land between turns, in one frame under the session's
//! lock. A missing, late or refused outcome raises nothing and says why in a
//! `content.unjudged` row. An ask that **failed** is asked once more; if that
//! fails too the row is written and the owner is told once, in the session's
//! place (once per session for a run of failures; a refusal or an unbuilt judge
//! is not retried and is told at once), and the session goes on: **an item whose ask failed twice can reach a
//! model of restricted origin unlabelled, with a notice** (the owner's trade,
//! theseus-gyjyo.7). Nothing here proves a session judged; the session carries
//! its labels and a turn goes on those.
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
//!   waits on a counter, the asks open for its session
//!   ([`crate::content_wait::Verdicts::outstanding`]), which a landing brings
//!   down and wakes. The retry happens **inside** the open ask, so the count
//!   stays up across it. Because a turn may be blocked on it, a content call
//!   goes out as [`theseus_judge::Urgency::Awaited`] (theseus-oc9n4): it may
//!   take one of the in-flight permits the client keeps back for the calls a
//!   turn waits on, rather than queueing behind ordinary judge work no one is
//!   waiting for. With the permits saturated and a content call taking an
//!   ordinary one, the floor spent its whole `wait_ms` on the queue and the
//!   turn fell off its model with the verdict landing just after it ended.

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
/// v3 since theseus-gyjyo.11: the state carries who wrote the item, where the
/// session speaks and works, whether the operator lists that tree as open
/// source, and the operator's own paragraph (`content_scene`). v2 (theseus-78qay,
/// the owner, 2026-10-27: a flag describes what an item REVEALS, not what it is
/// ABOUT) and v1 stay embedded, as the record of what the judgments before each
/// bump were answered against.
pub const PACK: &str = "content.v3";

/// How long a session's queue waits, after its first item, for the rest of the
/// loop's results to join the batch.
pub const COALESCE: Duration = Duration::from_millis(50);

/// The longest a source line is kept.
const SOURCE_CHARS: usize = 200;

/// One item that entered a session, queued for judgment.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentItem {
    /// The node that carries it.
    pub node: String,
    pub kind: ItemKind,
    /// The tool and the resource or address it named, or the place a message
    /// arrived in. Shown to the operator, so never a secret.
    pub source: String,
    pub text: String,
    /// When the node entered the session (its `created_at_ms`). In memory
    /// only: a label this item raises is refused if the operator removed that
    /// flag after this time (`ContentFlags::raise_after`).
    pub entered_ms: u64,
}

impl ContentItem {
    pub fn new(node: &str, kind: ItemKind, source: &str, text: &str, entered_ms: u64) -> Self {
        Self {
            node: node.into(),
            kind,
            source: source.into(),
            text: text.into(),
            entered_ms,
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
        Some(Self::new(&node.id, kind, source, text, node.created_at_ms))
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
        Some(Self::new(&node.id, kind, &source, text, node.created_at_ms))
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
    notify: Arc<Notify>,
    /// The owner has been told this session's asks are failing, and nothing
    /// has been judged since: a Jev that stays down is told once, not once a
    /// tool call.
    told_unjudged: bool,
}

/// A label told, as the core takes it: the session and the label.
type Told = Box<dyn Fn(&str, &Label) + Send + Sync>;

/// A session's place and directory, as the core reads them.
type SceneOf = Box<dyn Fn(&str) -> crate::content_scene::Scene + Send + Sync>;

/// Failed asks told, as the core takes it: the session and the notice's words.
type UnjudgedTold = Box<dyn Fn(&str, &str) + Send + Sync>;

/// The service's state for this point.
#[derive(Default)]
pub(crate) struct Point {
    names: OnceLock<DataNames>,
    /// The operator's private names, made once: the deterministic source
    /// (`content::deterministic::from_names`) reads it at every item's entry.
    matcher: OnceLock<theseus_judge::names::Names>,
    /// The operator's config as the pack reads it (`[privacy] open_source_paths`,
    /// `[content_based_security] context`), made once as the core is built.
    around: OnceLock<crate::content_scene::Around>,
    /// Where a session speaks and works, read from the core as an item is
    /// prepared (`Core::content_scene`); set once as the core is built.
    scene: std::sync::RwLock<Option<SceneOf>>,
    /// `[content_based_security] session_flags = false`: nothing is asked.
    off: AtomicBool,
    slots: Mutex<HashMap<String, Slot>>,
    /// What the core does with a label once it is written: tell the operator's
    /// surfaces (`Core::tell_label`). Set by the core as it is built.
    told: std::sync::RwLock<Option<Told>>,
    /// What the core does when asks failed twice: tell the owner, in the
    /// session's place (`Core::tell_unjudged`).
    unjudged_told: std::sync::RwLock<Option<UnjudgedTold>>,
    /// `[content_based_security] max_pieces`: the most pieces one item is read in.
    max_pieces: std::sync::atomic::AtomicUsize,
    /// Decision points asked, for a test that counts them.
    #[cfg(test)]
    pub(crate) points: std::sync::atomic::AtomicU64,
    /// Every node a hook site offered for judgment, in order, for the test
    /// that holds the submitted set against the blocked set (theseus-sh1n3).
    #[cfg(test)]
    pub(crate) submitted: Mutex<Vec<String>>,
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

/// A batch, ready to send: its asks (each the piece of an item) and the
/// reservation beside the call.
struct Batch {
    built: Arc<super::Built>,
    /// `(item index, piece number, of)` and the ask, in order.
    asks: Vec<((usize, u32, u32), Ask)>,
    /// The items whose text was past the pieces' bound (`content::split`).
    truncated: Vec<usize>,
    need: theseus_judge::price::Micros,
    beside: Beside,
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
    /// whether the mechanism is on (`[content_based_security] session_flags`): set once, as the core
    /// is built.
    pub fn content_from(&self, names: DataNames, enabled: bool, max_pieces: usize) {
        let _ = self
            .content
            .matcher
            .set(theseus_judge::names::Names::new(&names.private));
        let _ = self.content.names.set(names);
        self.content.off.store(!enabled, Ordering::Relaxed);
        self.content.max_pieces.store(max_pieces, Ordering::Relaxed);
    }

    /// The operator's config as the pack reads it, set once as the core is
    /// built.
    pub fn content_around(&self, around: crate::content_scene::Around) {
        let _ = self.content.around.set(around);
    }

    /// How a session's place and directory are read: the core hands over
    /// `Core::content_scene`, set once as it is built. Without it an item is
    /// asked with no place and no directory, which a test may want.
    pub fn content_scene(
        &self,
        scene: impl Fn(&str) -> crate::content_scene::Scene + Send + Sync + 'static,
    ) {
        *self
            .content
            .scene
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(scene));
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

    /// What the core does when asks failed twice: it hands over
    /// `Core::tell_unjudged`, set once as it is built.
    pub fn unjudged_told(&self, told: impl Fn(&str, &str) + Send + Sync + 'static) {
        *self
            .content
            .unjudged_told
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(told));
    }

    /// Queue `items` for judgment and return: nothing waits here. The items are
    /// in transcript order, and each answer lands, under the session's lock,
    /// as its batch is asked. Nothing is asked with the mechanism off, the
    /// judge off or the pack off, and an item with no text is never asked.
    pub fn ask_content(&self, session: &str, items: Vec<ContentItem>) {
        if items.is_empty() {
            return;
        }
        // Every node a hook site offered, whether or not the mechanism is on:
        // what a test holds the sites' offering against (theseus-sh1n3).
        #[cfg(test)]
        self.content
            .submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(items.iter().map(|i| i.node.clone()));
        if self.content.off.load(Ordering::Relaxed) {
            return;
        }
        self.raise_names(session, &items);
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
}

impl crate::content_wait::Verdicts for JudgeService {
    /// The session's open asks: the items queued or being judged, not yet
    /// landed (`Slot::open`). This is the counter a turn's wait reads.
    fn outstanding(&self, session: &str) -> usize {
        self.content.slots().get(session).map_or(0, |s| s.open)
    }

    /// The next landing, or none: the wait's own slice bounds a wake-up this
    /// misses. `notified()` takes its interest on the **first poll**, so a
    /// landing between this call and that poll is not seen until the next
    /// slice (`content_wait::SLICE`) at worst, never slept through.
    fn settled(
        &self,
        session: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let (notify, pending) = {
            let mut slots = self.content.slots();
            let slot = slots.entry(session.to_string()).or_default();
            (slot.notify.clone(), slot.open > 0)
        };
        Box::pin(async move {
            if !pending {
                return;
            }
            notify.notified().await;
        })
    }
}

impl JudgeService {
    /// The paths the calls behind some tool results named, by result node: a
    /// result's `tool_use_id` finds its call in the session's transcript, and
    /// the call's input names the path (`content_scene::call_paths`). A node the
    /// transcript lacks, or whose call named none, is absent. One read of the
    /// transcript, so a caller reads it once for a batch.
    pub(crate) fn call_paths_of(
        &self,
        session: &str,
        nodes: &[&str],
    ) -> HashMap<String, Vec<String>> {
        let mut out = HashMap::new();
        if nodes.is_empty() {
            return out;
        }
        let Ok(transcript) = self.store.transcript(session) else {
            return out;
        };
        let mut wanted: HashMap<&str, &str> = HashMap::new();
        for (_, n) in transcript.iter() {
            if let Body::ToolResult { tool_use_id, .. } = &n.body {
                if nodes.contains(&n.id.as_str()) {
                    wanted.insert(tool_use_id.as_str(), n.id.as_str());
                }
            }
        }
        for (_, n) in transcript.iter() {
            if let Body::ToolCall {
                tool_use_id, input, ..
            } = &n.body
            {
                if let Some(node) = wanted.get(tool_use_id.as_str()) {
                    let paths = crate::content_scene::call_paths(input);
                    if !paths.is_empty() {
                        out.insert((*node).to_string(), paths);
                    }
                }
            }
        }
        out
    }

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
        let around = self.content.around.get().cloned().unwrap_or_default();
        let scene = self
            .content
            .scene
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|f| f(session))
            .unwrap_or(crate::content_scene::Scene {
                place: None,
                dir: None,
            });
        // The paths the calls behind the batch's tool results named, which the
        // `source` line does not carry: one read of the transcript, and only when
        // the operator lists a tree as open source.
        let called = if around.lists_any() {
            let nodes: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == ItemKind::ToolResult)
                .map(|i| i.node.as_str())
                .collect();
            self.call_paths_of(session, &nodes)
        } else {
            HashMap::new()
        };
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
            // The paths the item names: its source line's and its call's.
            let mut paths = crate::content_scene::paths_in(&item.source);
            paths.extend(called.get(&item.node).cloned().unwrap_or_default());
            for (n, of, text) in pieces {
                let mut input = ContentInput {
                    text,
                    kind: item.kind.as_str().into(),
                    source: item.source.clone(),
                    piece: Some((n, of)),
                    data_names: names.clone(),
                    ..ContentInput::default()
                };
                around.apply(&mut input, item.kind, &scene, &paths);
                let state = theseus_judge::prepare(&pack, &Input::Content(input), &scrub)
                    .map_err(|_| Unasked::Nothing)?;
                let blob = self.stage_blob(state.state.json.as_bytes());
                let mut context = json!({
                    "session": session, "purpose": "content", "node": item.node,
                    "kind": item.kind, "piece": [n, of], "batch": batch_id,
                    "batch_items": items.len(), "baseline": "none", "blob": blob,
                    "on_path_ms": 0, "dynamic": state.dynamic.recorded(&pack),
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
        Ok(Batch {
            built,
            asks,
            truncated,
            need,
            beside,
        })
    }

    /// The batch's write, one frame: each item's rows and each label raised,
    /// all under the session's lock. A label is added all the same whatever
    /// else the session's record has moved to meanwhile: it is never wrong to
    /// add one, and a stored label or removal always wins (`raise_after`).
    fn land_content(&self, session: &str, landings: &[Landing]) {
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
                    if flags.raise_after(label.clone(), it.entered_ms) {
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
        // Whether the session, as just written, carries a label that keeps a
        // model of restricted origin off it: the notice says so either way.
        let kept_off = matches!(&written, Ok(Some(rec))
            if rec.data.as_deref().and_then(ContentFlags::disqualifies_cn).is_some());
        // What was raised is told after it is written, in the session's own words,
        // and to the operator's surfaces.
        for (_, label) in fresh {
            self.tell_label(session, label);
        }
        self.tell_unjudged(session, landings, kept_off);
    }

    /// Tell the owner, once, that asks failed: after the retry, so the notice
    /// says what is true (nothing was judged, and nothing will be asked again).
    /// Told once per session for a run of failures, not once per item or per
    /// batch: a Jev that stays down is one message in each session, and the next
    /// batch of that session that is judged re-arms it. A refusal (the day's
    /// budget) or an unbuilt judge is not retried and is told at once. A `shadow` outcome is the pack's mode, not a failure, and
    /// says nothing. `kept_off` is whether the session already carries a label
    /// that keeps models of restricted origin off it, which the notice says.
    fn tell_unjudged(&self, session: &str, landings: &[Landing], kept_off: bool) {
        let failed: Vec<&Landing> = landings
            .iter()
            .filter(|l| matches!(l.unjudged, Some("failed" | "unbuilt" | "refused")))
            .collect();
        let judged = landings
            .iter()
            .any(|l| l.unjudged.is_none() && l.pieces > 0);
        let tell = {
            let mut slots = self.content.slots();
            let slot = slots.entry(session.to_string()).or_default();
            if judged {
                slot.told_unjudged = false;
            }
            let tell = !failed.is_empty() && !slot.told_unjudged;
            slot.told_unjudged |= tell;
            tell
        };
        if !tell {
            return;
        }
        let text = unjudged_notice(&failed, kept_off);
        if let Some(told) = self
            .content
            .unjudged_told
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            told(session, &text);
        }
    }
}

/// The notice a failed ask tells the owner: what was not checked, why, and
/// what that means for the session: the item raised no label, so it adds no
/// reason to keep a model of restricted origin off; whether the session is kept
/// off one is up to the labels it already carries (`kept_off`) and to
/// `cn_models = "never"`.
fn unjudged_notice(failed: &[&Landing], kept_off: bool) -> String {
    let n = failed.len();
    let first = failed[0];
    let why = match first.unjudged {
        Some("refused") => "the day's judge budget is spent",
        Some("unbuilt") => "the judge could not be started",
        _ => "the check failed twice",
    };
    let what = match n {
        1 => format!(
            "{} (`{}`)",
            first.item.kind.as_str().replace('_', " "),
            first.item.source
        ),
        n => format!(
            "{n} items, the first {} (`{}`)",
            first.item.kind.as_str().replace('_', " "),
            first.item.source
        ),
    };
    let then = match kept_off {
        true => {
            "Nothing was labelled for it; this session's other labels still keep China-origin \
             models off."
        }
        false => {
            "Nothing was labelled, so China-origin models are still allowed for this session, \
             unless `cn_models = \"never\"` keeps them off."
        }
    };
    format!(
        "⚑ **unchecked** — {what} could not be checked for secrets, confidential matter or \
         injection ({why}). {then}"
    )
}

impl JudgeService {
    /// The deterministic source for names (theseus-gyjyo.4): an item that holds
    /// one of `[privacy] sensitive_names` raises `confidential` at its entry,
    /// before and whether or not the pack asks, and with no model call. A flag
    /// is on a session once, so at most one label comes of a call: the first
    /// that the session's removals do not refuse. An item the operator's Remove
    /// came after raises nothing (`ContentFlags::raise_after`), however often
    /// it is offered.
    fn raise_names(&self, session: &str, items: &[ContentItem]) {
        let Some(names) = self.content.matcher.get().filter(|n| !n.is_empty()) else {
            return;
        };
        let now = theseus_protocol::now_unix_ms();
        let hits: Vec<(Label, u64)> = items
            .iter()
            .filter_map(|i| {
                let label =
                    content::deterministic::from_names(names, &i.node, i.kind, &i.text, now)?;
                Some((label, i.entered_ms))
            })
            .collect();
        if !hits.is_empty() {
            self.land_deterministic(session, hits);
        }
    }

    /// Write labels no model raised, each with the time its item entered the
    /// session, in one frame with their `content.labelled` rows, then tell the
    /// operator as a pack's label is told. A flag the session already carries
    /// keeps its first label and writes nothing, and one the operator removed
    /// after the item entered is not raised by it (`ContentFlags::raise_after`);
    /// a removed one is raised again only by an item that enters after the
    /// removal, as a pack's is. The home of every `content::deterministic`
    /// source, and so the one place `[content_based_security] session_flags =
    /// false` turns them all off. The session's lock is taken here: do not call
    /// it from inside a write of the same session's record.
    pub fn land_deterministic(&self, session: &str, labels: Vec<(Label, u64)>) {
        if self.content.off.load(Ordering::Relaxed) {
            return;
        }
        let mut fresh: Vec<&Label> = Vec::new();
        let written = theseus_store::blocking(|| {
            self.store.update_session(session, |rec| {
                let mut flags = take_flags(rec);
                let mut rows = Vec::new();
                for (label, entered_ms) in &labels {
                    if flags.raise_after(label.clone(), *entered_ms) {
                        fresh.push(label);
                        rows.push(crate::fact::row(&labelled(label), Some(session), None)?);
                    }
                }
                put_flags(rec, flags);
                Ok(rows)
            })
        });
        match written {
            Ok(Some(_)) => {}
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: a deterministic label was not written");
                return;
            }
        }
        for label in fresh {
            self.tell_label(session, label);
        }
    }

    /// A label written, told: in the session's own words and to the operator's
    /// surfaces (`Core::tell_label`, which the core hands the service once as
    /// it is built).
    fn tell_label(&self, session: &str, label: &Label) {
        self.announce(Some(session), None, &labelled(label));
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

/// The row a label written says.
fn labelled(l: &Label) -> ContentLabelled<'_> {
    ContentLabelled {
        flag: l.flag,
        node: l.node.as_deref().unwrap_or_default(),
        kind: l.kind,
        source: &l.source,
        confidence: l.confidence,
        band: l.band,
    }
}

/// The record's content flags, taken out to be changed.
fn take_flags(rec: &mut crate::session::SessionRecord) -> ContentFlags {
    rec.data.take().map(|b| *b).unwrap_or_default()
}

/// Put them back; a record with nothing on it keeps no field at all.
fn put_flags(rec: &mut crate::session::SessionRecord, flags: ContentFlags) {
    rec.data = (flags != ContentFlags::default()).then(|| Box::new(flags));
}

/// A session's queue, drained one batch at a time until it is empty. The
/// service is held only around the blocking halves, never across the call.
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
        let _open = Open {
            me: me.clone(),
            session: session.clone(),
            n: batch.len(),
        };
        judge_batch(&me, &session, batch).await;
    }
}

/// A batch's share of the session's open count, brought down when the batch
/// is done **however it ends**: a task that panics mid-batch must not leave
/// the count up for good (every cn turn of that session would wait its whole
/// `wait_ms` for the rest of the daemon's life), nor the queue stuck behind a
/// `running` that nothing will clear.
struct Open {
    me: Weak<JudgeService>,
    session: String,
    n: usize,
}

impl Drop for Open {
    fn drop(&mut self) {
        let Some(svc) = self.me.upgrade() else { return };
        let notify = {
            let mut slots = svc.content.slots();
            let slot = slots.entry(self.session.clone()).or_default();
            slot.open = slot.open.saturating_sub(self.n);
            if std::thread::panicking() {
                slot.open = slot.open.saturating_sub(slot.queue.len());
                slot.queue.clear();
                slot.running = false;
            }
            slot.notify.clone()
        };
        notify.notify_waiters();
    }
}

/// How long an ask that failed waits before it is asked once more: a moment,
/// for a transient failure to clear.
const REASK_PAUSE: Duration = Duration::from_millis(200);

/// One batch: asked in one decision point, then **the items whose ask failed
/// are asked once more**, then landed. The retry is inside the batch, so the
/// session's open count (`Slot::open`) stays up across it and a turn waiting
/// on the session waits for the second answer. An item that fails the second
/// time is rowed `failed` and the owner told (`JudgeService::tell_unjudged`);
/// nothing asks again. A refusal (`refused`, the day's budget) and a state that
/// could not be built (`unbuilt`) are not asks that failed and are not retried:
/// the same call would say the same.
async fn judge_batch(me: &Weak<JudgeService>, session: &str, items: Vec<ContentItem>) {
    let Some(mut landings) = ask_once(me, session, items).await else {
        return;
    };
    let failed: Vec<usize> = landings
        .iter()
        .enumerate()
        .filter(|(_, l)| l.unjudged == Some("failed"))
        .map(|(at, _)| at)
        .collect();
    if !failed.is_empty() {
        tokio::time::sleep(REASK_PAUSE).await;
        let again = failed.iter().map(|&at| landings[at].item.clone()).collect();
        if let Some(second) = ask_once(me, session, again).await {
            for (&at, mut now) in failed.iter().zip(second) {
                // A label a piece of the first attempt did raise is not lost
                // to a second attempt that failed on that item.
                for l in &landings[at].raised {
                    if !now.raised.iter().any(|r| r.flag == l.flag) {
                        now.raised.push(l.clone());
                    }
                }
                landings[at] = now;
            }
        }
    }
    land(me, session, landings).await;
}

/// One attempt: `items` prepared, asked in one decision point, and read into
/// their landings. `None` when the service is gone or the blocking pool lost
/// the preparation.
async fn ask_once(
    me: &Weak<JudgeService>,
    session: &str,
    items: Vec<ContentItem>,
) -> Option<Vec<Landing>> {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let svc = me.upgrade()?;
    let pack = svc.pack(PACK)?;
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
            return Some(
                items
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
                    .collect(),
            );
        }
        Err(_) => return None,
    };
    let Batch {
        built,
        asks,
        truncated,
        need,
        beside,
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
                // A turn may be blocked on this answer right now: the cn
                // floor waits on the session's open count and then goes on
                // the labels it has (theseus-oc9n4). So this call may take
                // one of the permits kept back for the calls a turn waits
                // on, rather than queueing behind ordinary judge work no
                // one is waiting for.
                urgency: Urgency::awaited(total),
            })
            .await;
        settle(me, &today, need, &judgments);
        if let Some(w) = wrote {
            let _ = w.await;
        }
    }
    let svc = me.upgrade()?;
    Some(landings_of(&svc, items, &asks, &truncated, &judgments))
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

/// The write: each item's rows and the labels it raised, in one frame under
/// the session's lock, **now**. It is not deferred to a moment between turns:
/// with no mark to move, nothing here races a turn's own write-back, which
/// merges a label in and never over one (`ContentFlags::merge`).
async fn land(me: &Weak<JudgeService>, session: &str, landings: Vec<Landing>) {
    let Some(svc) = me.upgrade() else { return };
    let sid = session.to_string();
    let _ = tokio::task::spawn_blocking(move || svc.land_content(&sid, &landings)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A batch that panics does not leave the count up** (theseus-gyjyo.7, the
    /// review of R5a, the counter's one leak). The share a batch holds of the
    /// session's open count is brought down by a guard that runs however the batch
    /// ends; when it ends in a panic it also takes the queue behind it off the
    /// count and clears `running`, so the session's later items are asked about by
    /// a fresh drain rather than waited on by every cn turn for the rest of the
    /// daemon's life.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_batch_that_panics_does_not_leave_the_count_up() {
        let jev = theseus_judge::fake::FakeJev::start().unwrap();
        let r = crate::tests_judge::rig_with(crate::tests_judge::texts(1), Some(&jev), |_| {});
        let rec =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        let sid = rec.session_id;
        let svc = r.core.runner.judge.clone();
        {
            // One batch of one in flight, two items queued behind it, a drain running.
            let mut slots = svc.content.slots();
            let slot = slots.entry(sid.clone()).or_default();
            slot.open = 3;
            slot.running = true;
            slot.queue = vec![
                ContentItem::new(
                    "n2",
                    crate::content::ItemKind::ToolResult,
                    "fs.read x",
                    "two",
                    1,
                ),
                ContentItem::new(
                    "n3",
                    crate::content::ItemKind::ToolResult,
                    "fs.read x",
                    "three",
                    1,
                ),
            ];
        }
        assert_eq!(r.core.runner.verdicts.outstanding(&sid), 3);
        let (me, session) = (svc.me.clone(), sid.clone());
        let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _open = Open { me, session, n: 1 };
            panic!("the batch died");
        }));
        assert!(died.is_err());
        assert_eq!(
            r.core.runner.verdicts.outstanding(&sid),
            0,
            "the count came down"
        );
        let slots = svc.content.slots();
        let slot = slots.get(&sid).unwrap();
        assert!(
            slot.queue.is_empty() && !slot.running,
            "and the queue is not stuck"
        );
        drop(slots);
        // And an ordinary end takes only its own share.
        {
            let mut slots = svc.content.slots();
            let slot = slots.entry(sid.clone()).or_default();
            slot.open = 3;
            slot.running = true;
        }
        drop(Open {
            me: svc.me.clone(),
            session: sid.clone(),
            n: 1,
        });
        assert_eq!(
            r.core.runner.verdicts.outstanding(&sid),
            2,
            "its share only"
        );
    }
}
