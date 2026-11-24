//! The three things a change in the owner's own words needs before it may
//! skip his card (issue 44, decision 6; the review of the first build), each
//! from the judge or from records, never from the acting model:
//!
//! 1. **The typed judgment** (`task_words.v1`): asked at the gate of a
//!    `task.update` that names his message, only when the classify pack's
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
    let Some((j, clipped)) = judge
        .judge_task_words(tc.session_id, node, (&task.id, base), words)
        .await
    else {
        return;
    };
    let answer = j.answer(crate::judge::task_words::CHANGE);
    // The judge read less than the message or the task: its answer cannot
    // authorise the whole of either.
    let operation = match (j.actionable(), answer) {
        _ if clipped => Operation::Clipped,
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
/// the owner in person (evidence 2). The model's transcript is read whole and
/// strictly ([`strict_transcript`]: a fork's inherited prefix included, and
/// any record that cannot be read fails the read). In all of it:
///
/// - no result is marked as outside;
/// - every person's message carries the owner's in-person mark;
/// - no message was relayed from a task's report, or written by an agent, a
///   tool, an MCP prompt or an import;
/// - every source a recall put in front of the model reads, and is his marked
///   message, Theseus's own reply or call, or a plain result;
/// - nothing else of unknown source (an imported text, a synthesis, a task's
///   arrangement) is there.
///
/// A summary's sources are this transcript's own earlier nodes, read above.
/// From the current exchange's ask, a note the harness relayed (a wake, a
/// clock line) counts as outside too. Every view the turn was built from is
/// read completely ([`outside_viewed`]). Anything that cannot be read counts
/// as outside.
pub fn outside_admitted(
    store: &crate::store::Store,
    session: &str,
    turn: &str,
    ex: Option<&crate::exchange::Exchange>,
) -> bool {
    let Some(ask) = ex.and_then(|e| e.ask.as_deref()) else {
        return true;
    };
    let Ok(nodes) = strict_transcript(store, session) else {
        return true;
    };
    let Some(from) = nodes.iter().position(|n| n.id == ask) else {
        return true;
    };
    let his = |id: &str| {
        matches!(crate::exchange::mark(store, id),
            Ok(Some(m)) if m.in_person && m.person().as_deref() == Some(crate::exchange::OWNER))
    };
    let inherited = nodes.iter().any(|n| match &n.body {
        Body::UserMessage { .. } => match n.origin {
            Origin::Operator => !his(&n.id),
            Origin::Harness => n.author.as_deref().is_some_and(|a| a.starts_with("task:")),
            _ => true,
        },
        Body::ToolResult { .. } => crate::external::is_outside(n),
        Body::AssistantMessage { .. } | Body::ToolCall { .. } | Body::Summary { .. } => false,
        Body::Recall { items, .. } => items.iter().any(|i| !own_source(store, &i.node_id, &his)),
        _ => true,
    });
    let outside_node = inherited
        || nodes[from..].iter().any(|n| match &n.body {
            Body::UserMessage { .. } => n.origin != Origin::Operator || !his(&n.id),
            Body::AssistantMessage { .. } | Body::ToolCall { .. } | Body::ToolResult { .. } => {
                false
            }
            _ => true,
        });
    outside_node || outside_viewed(store, turn, &nodes)
}

/// A source a recall showed: it reads, and it is his marked message,
/// Theseus's own reply or call, or a result not marked as outside.
fn own_source(store: &crate::store::Store, node: &str, his: &dyn Fn(&str) -> bool) -> bool {
    match store.get_node(node) {
        Ok(Some((_, n))) => match &n.body {
            Body::UserMessage { .. } => n.origin == Origin::Operator && his(&n.id),
            Body::AssistantMessage { .. } | Body::ToolCall { .. } => true,
            Body::ToolResult { .. } => !crate::external::is_outside(&n),
            _ => false,
        },
        _ => false,
    }
}

/// A session's transcript as the model is shown it (`Store::transcript`: a
/// fork's inherited prefix, then its own nodes), read strictly: a record the
/// index lists that cannot be read, or a node that does not decode, fails
/// the read, where the transcript's own read skips the one and shows the
/// other as a harness note.
pub fn strict_transcript(
    store: &crate::store::Store,
    session: &str,
) -> anyhow::Result<Vec<crate::node::Node>> {
    use theseus_store::kinds::NODE;
    let mut chain = Vec::new();
    let mut through = u64::MAX;
    let mut at = crate::fork::origin_of(store, session)?;
    while let Some(o) = at {
        anyhow::ensure!(chain.len() < 64, "a chain of forks too deep to read");
        through = through.min(o.position);
        at = crate::fork::origin_of(store, &o.session)?;
        chain.push((o.session, through));
    }
    chain.reverse();
    chain.push((session.to_string(), u64::MAX));
    let mut nodes = Vec::new();
    for (s, through) in chain {
        for r in store.inner().scan_scope_strict(&s, 0, usize::MAX)? {
            if r.kind == NODE && r.position <= through {
                nodes.push(store.node_cache().decode(&r.payload)?);
            }
        }
    }
    Ok(nodes)
}

/// `task.carried.<turn>`: the turn's requests carry a block its transcript
/// and its view do not show, whose source is not proven here (the place
/// digest, the place's last words).
pub const CARRIED_PREFIX: &str = "task.carried.";

/// Latch, for `turn`, that its requests carry such a block: written at once,
/// when the turn takes the block, before any request carries it. False when
/// it could not be written; the caller then attaches nothing, so a block is
/// never carried without its latch.
pub fn latch(store: &crate::store::Store, turn: &str) -> bool {
    let key = format!("{CARRIED_PREFIX}{turn}");
    let written =
        NewRecord::json(kinds::META, Some(&key), &true).and_then(|r| store.append(&[r]).map(drop));
    if let Err(e) = &written {
        tracing::warn!(turn, error = %format!("{e:#}"), "a carried block's latch was not written; the block is left out");
    }
    written.is_ok()
}

/// [`latch`], once for the turn: true when it is latched already (or the
/// latch cannot be read, which the checks read as carried too).
pub fn latch_once(store: &crate::store::Store, turn: &str) -> bool {
    carried_in(store, turn) || latch(store, turn)
}

/// Whether `turn` carried such a block, read from its latch: what the turn
/// actually attached, not the thread board as it stands now (a thread retired
/// and opened again mid-turn leaves the board, and the block was attached
/// all the same). A latch that cannot be read counts as carried.
pub fn carried_in(store: &crate::store::Store, turn: &str) -> bool {
    !matches!(
        store.get_meta::<bool>(&format!("{CARRIED_PREFIX}{turn}")),
        Ok(None)
    )
}

/// [`carried_in`] for the call `tc` runs: read at the gate and again under
/// the task's lock (both build [`super::words::Records::of`]).
pub fn carried(tc: &TurnCtx<'_>) -> bool {
    carried_in(tc.store, tc.turn_id)
}

/// The turn's `thread.routed` row gave it the place's last words, as the
/// ledger tells it; an index that cannot answer cannot prove it did not.
/// The authorization checks read the turn's latch ([`carried_in`]).
pub fn tailed(store: &crate::store::Store, session: &str, turn: &str) -> bool {
    use theseus_protocol::LedgerKind;
    use theseus_store::pages::ledger_kind_session;
    use theseus_store::{Page, Store as _};
    let page = Page {
        kind: theseus_store::kinds::LEDGER,
        tags: vec![ledger_kind_session(
            LedgerKind::ThreadRouted.as_str(),
            session,
        )],
        limit: 64,
        ..Page::default()
    };
    // An index still being built cannot prove there was no tail.
    let Ok(Some(got)) = store.inner().page(&page) else {
        return true;
    };
    got.records
        .iter()
        .any(|r| match r.decode::<crate::ledger::LedgerRow>() {
            Ok(row) => row.turn_id.as_deref() == Some(turn) && !row.data["tail"].is_null(),
            Err(_) => true,
        })
}

/// A piece of a view the turn was built from that came from outside or from
/// another person, read completely: every view the turn's index names, and
/// the view of every compilation of the turn's own replies that was
/// assembled as a view, must read; a missing or corrupt one counts as
/// outside. A turn of a place not set to `view` has no view, and its inputs
/// are its transcript.
fn outside_viewed(store: &crate::store::Store, turn: &str, nodes: &[crate::node::Node]) -> bool {
    use crate::view::record::{read, ViewOfTurn, SEP, VIEW_TURN_PREFIX};
    use crate::view::Author;
    let prefix = format!("{VIEW_TURN_PREFIX}{turn}{SEP}");
    let Ok(index) =
        store
            .inner()
            .first_with_prefix_strict(theseus_store::kinds::META, &prefix, usize::MAX)
    else {
        return true;
    };
    let mut wanted: Vec<String> = Vec::new();
    for r in index {
        match r.decode::<ViewOfTurn>() {
            Ok(o) => wanted.push(o.compilation),
            Err(_) => return true,
        }
    }
    let replies = nodes.iter().filter(|n| n.turn_id.as_deref() == Some(turn));
    for n in replies {
        let Body::AssistantMessage {
            compilation_id: Some(c),
            ..
        } = &n.body
        else {
            continue;
        };
        if wanted.contains(c) {
            continue;
        }
        match store.get_compilation(c) {
            Ok(Some(comp)) if comp.strategy == crate::compiler::view::STRATEGY => {
                wanted.push(c.clone())
            }
            Ok(Some(_)) => {}
            _ => return true,
        }
    }
    wanted.iter().any(|c| match read(store, c) {
        Ok(Some(v)) => v
            .shown
            .iter()
            .any(|s| s.piece.marks.outside || s.piece.marks.author == Author::Other),
        _ => true,
    })
}

/// Whether the ladder lets `task_words.v1` act for `session` NOW: its rows
/// read from the store at this moment and folded, so a rollback or an `off`
/// written since the judgment was asked takes the act back. A row that
/// cannot be read is no permission.
pub fn acts_now(store: &crate::store::Store, session: &str) -> bool {
    use crate::judge::ladder::{fold, given_of, scope};
    use crate::judge::task_words::PACK;
    // Strictly: a rollback row that cannot be read is no permission, never
    // a row skipped.
    let Ok(records) = store.inner().scan_scope_strict(&scope(PACK), 0, usize::MAX) else {
        return false;
    };
    let mut rows = Vec::new();
    for r in records {
        let Ok(row) = r.decode::<crate::ledger::LedgerRow>() else {
            return false;
        };
        if row.kind != theseus_protocol::LedgerKind::PackMode.as_str() {
            continue;
        }
        let Ok(mut m) = serde_json::from_value::<theseus_protocol::packs::PackModeRow>(row.data)
        else {
            return false;
        };
        if m.pack == PACK {
            m.position = r.position;
            m.at_unix_ms = row.at_unix_ms;
            rows.push(m);
        }
    }
    let s = fold(
        crate::config::PackMode::Shadow,
        &rows,
        theseus_protocol::now_unix_ms(),
    );
    given_of(s.rung.acts_as(), s.share, PACK, session).judge_mode() != theseus_judge::Mode::Shadow
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
