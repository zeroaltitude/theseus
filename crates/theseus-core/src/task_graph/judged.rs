//! The three things a change in the owner's own words needs before it may
//! skip his card (issue 44, decision 6; the review of the first build), each
//! from the judge or from records, never from the acting model:
//!
//! 1. **The typed judgment** (`task_words.v1`): asked at the gate of a
//!    `task.update` that names his message, only when `classify.v4`'s
//!    `addressed_task` names exactly one task for that message and the
//!    message carries the owner's mark, in person. The judge reads his
//!    message alone and the task as it stands; nothing of the proposal. The
//!    answer is kept for that message, `task.judged.<node>`, once.
//! 2. **Evidence that the words are his and current**, computed from the
//!    turn's admitted inputs ([`outside_admitted`]): from the opening of the
//!    current exchange, every node is his marked message, Theseus's own reply
//!    or call, or a result not marked as outside; and no piece the turn's view
//!    admitted came from outside or from another person. Anything else, or
//!    anything unreadable, counts as outside.
//! 3. **A one-use receipt**, `task.receipt.<node>`: one judged message
//!    authorises one change, to one task, once. It is claimed under a lock
//!    keyed by the message, in the frame of the change (or, in shadow, of the
//!    proposal that would have applied), so a replay, a second task or a later
//!    turn finds it spent.
//!
//! **What state this lands in is the ladder's** (m5-judgment §2.7): a new
//! pack starts in shadow. In shadow the judgment is asked and kept, the
//! `task.change_proposed` row says what the rule WOULD have done
//! (`words.would`), and every change still waits on the card. Only an
//! operator's promotion of `task_words.v1` makes it act.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_store::{kinds, NewRecord};

use super::words::{JudgedChange, Kind, Operation};
use crate::judge::JudgeService;
use crate::node::{Body, Origin};
use crate::toolrun::TurnCtx;

/// `task.judged.<node>`: the judgment of one message of his.
pub const JUDGED_PREFIX: &str = "task.judged.";
/// `task.receipt.<node>`: the one change one message of his authorised.
pub const RECEIPT_PREFIX: &str = "task.receipt.";

/// The judgment kept for one message: what it asks of which task, whether
/// the pack acted when it was asked (live or in the canary's arm), and the
/// judgment's own id, the ledger row that holds the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judged {
    pub change: JudgedChange,
    pub acting: bool,
    pub judgment: String,
    /// The option the judge chose, before the band: `restricts`, `other`, ….
    pub answer: String,
    pub at_ms: u64,
}

/// The one change one message authorised, or would have in shadow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub node: String,
    pub task: String,
    pub base_version: u64,
    pub kind: Kind,
    pub turn: String,
    /// It applied without a card (false: shadow, it would have).
    pub acting: bool,
    pub at_ms: u64,
}

pub fn judged(store: &crate::store::Store, node: &str) -> anyhow::Result<Option<Judged>> {
    store.get_meta(&format!("{JUDGED_PREFIX}{node}"))
}

pub fn receipt(store: &crate::store::Store, node: &str) -> anyhow::Result<Option<Receipt>> {
    store.get_meta(&format!("{RECEIPT_PREFIX}{node}"))
}

pub fn receipt_record(r: &Receipt) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&format!("{RECEIPT_PREFIX}{}", r.node)), r)
}

/// The lock that arbitrates one message's receipt across tasks.
pub fn lock_said<'a>(tc: &TurnCtx<'a>, node: &str) -> crate::store::SessionLock<'a> {
    tc.store.lock_task(&format!("said:{node}"))
}

/// The judge's option as an operation: anything but the three is `Unsure`,
/// and so is any answer under the act band.
pub fn operation_of(answer: &theseus_judge::judge::AnswerRecord) -> Operation {
    if answer.band.band != theseus_judge::Band::Act {
        return Operation::Unsure;
    }
    match answer.ran() {
        Some("restricts") => Operation::Restricts,
        Some("pauses") => Operation::Pauses,
        Some("resumes") => Operation::Resumes,
        _ => Operation::Unsure,
    }
}

/// A `task.update` that names his message, at its gate: ask `task_words.v1`
/// of that message, once, when `addressed_task` names exactly one task and
/// the message is his, in person; keep the answer. Anything missing asks
/// nothing, and nothing asked is the card.
pub async fn ask(judge: &JudgeService, tc: &TurnCtx<'_>, tool: &str, input: &Value) {
    if tool != super::tools::UPDATE || tc.task.is_some() {
        return;
    }
    let (Some(node), Some(_)) = (input.get("said").and_then(Value::as_str), input.get("kind"))
    else {
        return;
    };
    if !matches!(judged(tc.store, node), Ok(None)) {
        return;
    }
    let Some((message, task)) = asked_of(judge, tc, node) else {
        return;
    };
    let base = task.version;
    let words = theseus_judge::builders::TaskWordsInput {
        message,
        title: task.title.clone(),
        objective: task.objective.clone(),
        acceptance: task.acceptance.clone(),
        state: task.state.as_str().to_string(),
    };
    let Some(j) = judge
        .judge_task_words(tc.session_id, node, (&task.id, base), words)
        .await
    else {
        return;
    };
    let answer = j.answer(crate::judge::task_words::CHANGE);
    let operation = match (j.actionable(), answer) {
        (true, Some(a)) => operation_of(a),
        _ => Operation::Unsure,
    };
    let kept = Judged {
        change: JudgedChange {
            node: node.into(),
            task: task.id.clone(),
            base_version: base,
            operation,
        },
        acting: matches!(
            j.mode,
            theseus_judge::Mode::Live | theseus_judge::Mode::Canary
        ),
        judgment: j.id.clone(),
        answer: answer
            .and_then(|a| a.ran())
            .unwrap_or("unanswered")
            .to_string(),
        at_ms: theseus_protocol::now_unix_ms(),
    };
    let key = format!("{JUDGED_PREFIX}{node}");
    let written = NewRecord::json(kinds::META, Some(&key), &kept)
        .and_then(|r| tc.store.append(&[r]).map(drop));
    if let Err(e) = written {
        tracing::warn!(error = %format!("{e:#}"), "a task words judgment was not kept; the change keeps its card");
    }
}

/// His message and the one task the judge says it addresses, when the
/// message is his, in person, in this session; none otherwise.
fn asked_of(
    judge: &JudgeService,
    tc: &TurnCtx<'_>,
    node: &str,
) -> Option<(String, super::TaskRecord)> {
    let mark = crate::exchange::mark(tc.store, node).ok()??;
    if !mark.in_person || mark.person().as_deref() != Some(crate::exchange::OWNER) {
        return None;
    }
    let message = match tc.store.get_node(node).ok()?? {
        (_, n) if n.session_id == tc.session_id => match n.body {
            Body::UserMessage { text, .. } => text,
            _ => return None,
        },
        _ => return None,
    };
    let id = judge.addressed_task(tc.session_id, node)?;
    let task = super::get(tc.store, &id).ok()??;
    task.is_owners().then_some((message, task))
}

/// Whether anything the turn admitted came from outside, or from anyone but
/// the owner in person (evidence 2). No result the session ever read is
/// marked as outside (a transcript shows them all). From the current
/// exchange's ask, every node is his marked message, Theseus's own reply or
/// call, or a plain result; a note the harness relayed there (a task's
/// report, a clock line) counts as outside, since its source is not proven
/// here. No piece of the turn's view came from outside or from another
/// person. Anything that cannot be read counts as outside.
pub fn outside_admitted(
    store: &crate::store::Store,
    session: &str,
    turn: &str,
    ex: Option<&crate::exchange::Exchange>,
) -> bool {
    let Some(ask) = ex.and_then(|e| e.ask.as_deref()) else {
        return true;
    };
    let Ok(nodes) = store.session_nodes(session) else {
        return true;
    };
    let Some(from) = nodes.iter().position(|(_, n)| n.id == ask) else {
        return true;
    };
    let his = |id: &str| {
        matches!(crate::exchange::mark(store, id),
            Ok(Some(m)) if m.in_person && m.person().as_deref() == Some(crate::exchange::OWNER))
    };
    let read = nodes.iter().any(|(_, n)| crate::external::is_outside(n));
    let outside_node = read
        || nodes[from..].iter().any(|(_, n)| match &n.body {
            Body::UserMessage { .. } => n.origin != Origin::Operator || !his(&n.id),
            Body::AssistantMessage { .. } | Body::ToolCall { .. } | Body::ToolResult { .. } => {
                false
            }
            _ => true,
        });
    outside_node || outside_viewed(store, turn)
}

/// A piece the turn's view admitted that came from outside or from another
/// person; an unreadable record counts. A turn of a place not set to
/// `view` has no view record, and its inputs are its nodes.
fn outside_viewed(store: &crate::store::Store, turn: &str) -> bool {
    use crate::view::Author;
    match crate::view::record::of_turn(store, turn) {
        Ok(views) => views
            .iter()
            .flat_map(|v| &v.shown)
            .any(|s| s.piece.marks.outside || s.piece.marks.author == Author::Other),
        Err(_) => true,
    }
}

/// Theseus's own check flagged what the session read as steering it; a
/// session record that cannot be read counts as flagged.
pub fn steered(store: &crate::store::Store, session: &str) -> bool {
    match store.get_session::<crate::session::SessionRecord>(session) {
        Ok(Some(s)) => s.data.as_ref().is_some_and(|d| d.frozen()),
        Ok(None) => false,
        Err(_) => true,
    }
}

#[cfg(test)]
#[path = "tests_judged.rs"]
mod tests;
