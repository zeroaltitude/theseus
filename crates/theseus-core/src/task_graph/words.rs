//! A change to the owner's task in his own current words, without a card
//! (issue 44; the design's §7, "changing its objective", and the owner's
//! decision 6). The gate the build already has stays the one gate: the
//! objective and acceptance are the owner's layer, every edit is a version
//! under compare-and-set, and a model's change is a proposal that waits for
//! his yes on a card. This module says when the card would not be needed,
//! and keeps the words behind each change.
//!
//! **Asked, recorded, and acting only when the ladder says so.** The
//! conversation model proposes through `task.update`, naming his message
//! (`said`) and the change's kind (`kind`), but the kind is the proposing
//! model's own label ("Also deploy to production" appended under `narrows`
//! widens it). So the first check is an independent typed judgment of this
//! exact message, `task_words.v1`'s, asked at the call's gate and kept for
//! the message ([`judged_change`]; `judged.rs` says when it is asked). The
//! pack starts in shadow: [`decide`] then says what the rule WOULD do, the
//! proposal's row records it, and [`applies`] keeps the card (`shadow`) until
//! an operator promotes the pack. The checks of [`decide`]:
//!
//! - the judgment names exactly this task, this version, and `restricts`, at
//!   the pack's act band (anything else the judge says is `unsure`);
//! - the kind is `narrows` (`widens`, `replaces` and anything else keep the
//!   card);
//! - this is not a task's own session; the call names the whole id; the
//!   version it names is the newest, and his message is not older than it;
//! - nothing the turn admitted came from outside or from another person
//!   (`judged::outside_admitted`), and the session is not flagged steered;
//! - the message carries the owner's mark (`exchange::mark`), in person, and
//!   is of the current exchange of this very session;
//! - his message has not authorised a change already: its one-use receipt
//!   (`task.receipt.<node>`) is not spent, for this task or any other;
//! - the owner is alone here, as the gate's `present` says (a channel, an
//!   agent's process at the terminal, an unreadable mark: not alone);
//! - a narrowing keeps the objective as it was, word for word, and adds his
//!   whole message beside it, never a part the model chose (and acceptance
//!   only gains one line, his whole message): words that are not his cannot
//!   ride an "ok" of his, and a part of his words cannot be taken out of what
//!   he said ("delete the old charts" from "don't delete the old charts").
//!
//! The words are kept by reference, never by text, in a small META record,
//! `task.words.<id>.<version>`; a proposal that named his words and fell to
//! the card says why in its `task.change_proposed` row (`words`), so the cards
//! are counted, and what the rule would have done (`words.would`). No stored
//! record gains a field. A pause is not offered by `task.update`: the
//! owner's own `task.pause` is the durable one.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_memory::gate::Present;
use theseus_store::{kinds, NewRecord};

use super::{TaskRecord, TaskState};
use crate::exchange::{Exchange, OwnerMark, OWNER};
use crate::toolrun::TurnCtx;

/// The words behind a change: `task.words.<id>.<version>`.
pub const WORDS_PREFIX: &str = "task.words.";
/// The shortest message a narrowing adds, in characters.
pub const QUOTE_MIN: usize = 3;

pub fn words_key(task: &str, version: u64) -> String {
    format!("{WORDS_PREFIX}{task}.{version}")
}

/// What a proposed change does to the owner's task, as the model declares it.
/// `pauses` and `resumes` are read, never offered: the tool's schema names
/// the other three, and a call that says either keeps its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Narrows,
    Pauses,
    /// The undo of a pause.
    Resumes,
    Widens,
    Replaces,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Narrows => "narrows",
            Kind::Pauses => "pauses",
            Kind::Resumes => "resumes",
            Kind::Widens => "widens",
            Kind::Replaces => "replaces",
        }
    }
}

/// The words behind one version of a task: by reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Words {
    pub task: String,
    /// The version the change made, and the one it was based on.
    pub version: u64,
    pub from: u64,
    /// His message, and the exchange it was of.
    pub node: String,
    pub turn: String,
    /// The session whose model proposed it.
    pub session: String,
    pub kind: Kind,
    /// It went by the card (his yes), not by his words alone.
    pub carded: bool,
    /// For a pause: the state it paused. Read only: no pause is offered now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<TaskState>,
    pub at_ms: u64,
}

pub fn words_record(w: &Words) -> Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&words_key(&w.task, w.version)), w)
}

/// The words behind `version` of `task`, when any were kept.
pub fn words(store: &crate::store::Store, task: &str, version: u64) -> Result<Option<Words>> {
    store.get_meta::<Words>(&words_key(task, version))
}

/// What an independent judge said one message of the owner's asks of one
/// task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// It narrows the task's objective: less, never more or other.
    Restricts,
    Pauses,
    Resumes,
    Widens,
    Replaces,
    /// The judge was shown less than the whole message or task: its answer
    /// cannot authorise the whole of either.
    Clipped,
    /// The judge could not tell, said anything else (widen, redirect, add
    /// work, a question), or answered under its act band.
    Unsure,
}

/// The typed judgment a change without a card needs: recorded for this exact
/// message, naming exactly one task, the version it read, and the operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgedChange {
    pub node: String,
    pub task: String,
    pub base_version: u64,
    pub operation: Operation,
}

/// The judgment kept for the message `node` (`task.judged.<node>`): the
/// judge's, asked of his message and the one task `addressed_task` names,
/// never the proposing model's own word. None when none was asked, or it
/// cannot be read: the card.
pub fn judged_change(store: &crate::store::Store, node: &str) -> Option<JudgedChange> {
    super::judged::judged(store, node)
        .ok()
        .flatten()
        .map(|j| j.change)
}

/// Why a change falls to the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The call names no message of his, or no kind.
    NoWords,
    /// No independent judgment is recorded for his message.
    NoJudgment,
    /// The judge read a clipped message or task.
    Clipped,
    /// The judgment names another task, another version, or another
    /// operation than restricting this one.
    Misjudged,
    /// Any kind but `narrows`.
    Kind(Kind),
    /// A task's own session.
    TaskSession,
    /// The call names the task by less than its whole id.
    NotOneTask,
    /// The version named is not the newest, or his words are older than it.
    Stale,
    /// Text from outside was read since his last message.
    Outside,
    /// No owner's mark for the message, or one that cannot be read.
    NotMarked,
    /// The mark says it was not the owner in person.
    NotHim,
    /// The message is not of this session's current exchange.
    NotCurrent,
    /// The gate does not find the owner alone here.
    NotAlone,
    /// His message has authorised a change already (its receipt is spent),
    /// or the receipt cannot be read.
    Spent,
    /// Every check holds, and the judge that said so is in shadow: the change
    /// would apply without a card, and keeps it.
    Shadow,
    /// The change does not have its kind's shape: a narrowing that drops or
    /// rewrites any of the objective, or adds words that are not his.
    NotItsKind,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Why::NoWords => "no_words",
            Why::NoJudgment => "no_judgment",
            Why::Clipped => "clipped",
            Why::Misjudged => "misjudged",
            Why::Kind(Kind::Widens) => "widens",
            Why::Kind(Kind::Replaces) => "replaces",
            Why::Kind(_) => "kind",
            Why::TaskSession => "task_session",
            Why::NotOneTask => "not_one_task",
            Why::Stale => "stale",
            Why::Outside => "outside",
            Why::NotMarked => "not_marked",
            Why::NotHim => "not_him",
            Why::NotCurrent => "not_current",
            Why::NotAlone => "not_alone",
            Why::Spent => "spent",
            Why::Shadow => "shadow",
            Why::NotItsKind => "not_its_kind",
        }
    }
}

/// What a `task.update` call asks, as [`decide`] reads it.
#[derive(Debug, Clone, Default)]
pub struct Ask<'a> {
    /// The id as the call names it.
    pub named: &'a str,
    pub version: u64,
    pub said: Option<&'a str>,
    pub kind: Option<Kind>,
    pub objective: Option<&'a str>,
    pub acceptance: Option<&'a [String]>,
}

/// What [`decide`] reads beyond the call: each a read of a record, or the gate.
pub trait Evidence {
    /// The session the call runs in.
    fn session(&self) -> &str;
    /// A task's own session.
    fn is_task(&self) -> bool;
    /// The owner's mark of `node` (`exchange::mark`).
    fn mark(&self, node: &str) -> Result<Option<OwnerMark>>;
    /// The current exchange of the session (`exchange::exchange` of the turn).
    fn exchange(&self) -> Result<Option<Exchange>>;
    /// The gate's `present` for the message `mark` marks.
    fn present(&self, mark: &OwnerMark) -> Present;
    /// The words of the message `node`.
    fn text(&self, node: &str) -> Result<Option<String>>;
    /// Text from outside was read since his last message.
    fn outside(&self) -> bool;
    /// The words kept for a version of a task.
    fn words(&self, task: &str, version: u64) -> Result<Option<Words>>;
    /// The independent judgment recorded for the message `node`.
    fn judged(&self, node: &str) -> Option<JudgedChange>;
    /// The message `node` has authorised a change already, or its receipt
    /// cannot be read.
    fn spent(&self, node: &str) -> bool;
    /// The judgment of `node` was asked while its pack acted (live, or in the
    /// canary's arm), and the ladder lets the pack act now; false in shadow,
    /// and false once it is rolled back or off.
    fn acting(&self, node: &str) -> bool;
}

/// A text as it is compared: whitespace runs as one space, and the
/// punctuation that joins it to an objective taken off its ends.
fn plain(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim_matches(|c: char| c.is_whitespace() || ";,:.!-—–()\"'".contains(c))
        .to_string()
}

/// `part` is the whole of his message, word for word, and says something.
fn whole(part: &str, his: &str) -> bool {
    let part = plain(part);
    part.chars().count() >= QUOTE_MIN && part == plain(his)
}

/// What a narrowing of `old` to `new` adds, when it keeps all of `old`.
fn added<'a>(old: &str, new: &'a str) -> Option<&'a str> {
    let rest = new.strip_prefix(old)?;
    (!plain(rest).is_empty()).then_some(rest)
}

/// A narrowing's shape: the objective kept and his whole message added; the
/// acceptance kept, line for line, with one line added, his whole message.
fn narrows(rec: &TaskRecord, ask: &Ask<'_>, his: &str) -> bool {
    if ask.objective.is_none() && ask.acceptance.is_none() {
        return false;
    }
    let objective_ok = ask
        .objective
        .is_none_or(|new| added(&rec.objective, new).is_some_and(|more| whole(more, his)));
    let acceptance_ok = ask.acceptance.is_none_or(|new| {
        new.len() == rec.acceptance.len() + 1
            && new.starts_with(&rec.acceptance)
            && whole(&new[rec.acceptance.len()], his)
    });
    objective_ok && acceptance_ok
}

/// Whether `ask` applies to `rec` without a card; why not otherwise. The
/// call is the model's; every fact it rests on is read here, not taken from
/// it.
pub fn decide(ev: &dyn Evidence, rec: &TaskRecord, ask: &Ask<'_>) -> Result<Kind, Why> {
    let (Some(said), Some(kind)) = (ask.said, ask.kind) else {
        return Err(Why::NoWords);
    };
    // The meaning is never the proposing model's word: an independent
    // judgment of this very message must say it restricts this very task.
    let Some(judged) = ev.judged(said) else {
        return Err(Why::NoJudgment);
    };
    if judged.operation == Operation::Clipped {
        return Err(Why::Clipped);
    }
    if kind != Kind::Narrows {
        return Err(Why::Kind(kind));
    }
    if judged.node != said
        || judged.task != rec.id
        || judged.base_version != rec.version
        || judged.operation != Operation::Restricts
    {
        return Err(Why::Misjudged);
    }
    if ev.is_task() {
        return Err(Why::TaskSession);
    }
    if ask.named != rec.id {
        return Err(Why::NotOneTask);
    }
    if ask.version != rec.version {
        return Err(Why::Stale);
    }
    if ev.outside() {
        return Err(Why::Outside);
    }
    let mark = match ev.mark(said) {
        Ok(Some(m)) if m.node == said => m,
        _ => return Err(Why::NotMarked),
    };
    if !mark.in_person || mark.person().as_deref() != Some(OWNER) {
        return Err(Why::NotHim);
    }
    let current = match ev.exchange() {
        Ok(Some(ex)) => {
            ex.session_id == ev.session()
                && mark.ran_session == ev.session()
                && (ex.ask.as_deref() == Some(said) || ex.messages.iter().any(|m| m == said))
        }
        _ => false,
    };
    if !current {
        return Err(Why::NotCurrent);
    }
    if mark.at_ms <= rec.updated_at_ms {
        return Err(Why::Stale);
    }
    if ev.spent(said) {
        return Err(Why::Spent);
    }
    if ev.present(&mark) != Present::OwnerAlone {
        return Err(Why::NotAlone);
    }
    let shaped = match ev.text(said) {
        Ok(Some(his)) => narrows(rec, ask, &his),
        _ => false,
    };
    match shaped {
        true => Ok(kind),
        false => Err(Why::NotItsKind),
    }
}

/// Whether `ask` applies now without a card: [`decide`], and the judgment
/// asked while its pack acted. In shadow every change that [`decide`] passes
/// keeps its card as `shadow`, and its row says it would have applied.
pub fn applies(ev: &dyn Evidence, rec: &TaskRecord, ask: &Ask<'_>) -> Result<Kind, Why> {
    let kind = decide(ev, rec, ask)?;
    match ask.said.is_some_and(|s| ev.acting(s)) {
        true => Ok(kind),
        false => Err(Why::Shadow),
    }
}

/// [`Evidence`] for a turn's call: the store's records, the gate's `present`.
pub struct Records<'a> {
    pub store: &'a crate::store::Store,
    pub outbox: &'a crate::outbox::Outbox,
    pub session: &'a str,
    pub turn: &'a str,
    /// The class of the place the turn's words go to.
    pub class: crate::places::PlaceClass,
    pub task: bool,
    /// Something the turn admitted came from outside or from another person,
    /// or the session is flagged steered (`judged::outside_admitted`).
    pub outside: bool,
}

impl<'a> Records<'a> {
    /// The evidence of the call `tc` runs.
    pub fn of(tc: &'a TurnCtx<'a>) -> Self {
        let ex = crate::exchange::exchange(tc.store, tc.turn_id)
            .ok()
            .flatten();
        let outside =
            super::judged::outside_admitted(tc.store, tc.session_id, tc.turn_id, ex.as_ref());
        let steered = super::judged::steered(tc.store, tc.session_id);
        Self {
            store: tc.store,
            outbox: tc.outbox,
            session: tc.session_id,
            turn: tc.turn_id,
            class: tc.class,
            task: tc.task.is_some(),
            outside: outside || steered,
        }
    }
}

impl Evidence for Records<'_> {
    fn session(&self) -> &str {
        self.session
    }

    fn is_task(&self) -> bool {
        self.task
    }

    fn mark(&self, node: &str) -> Result<Option<OwnerMark>> {
        crate::exchange::mark(self.store, node)
    }

    fn exchange(&self) -> Result<Option<Exchange>> {
        crate::exchange::exchange(self.store, self.turn)
    }

    fn present(&self, mark: &OwnerMark) -> Present {
        use crate::view_gate::{present, Input};
        use theseus_memory::recall::Place;
        // Where the message arrived, before any thread routing: its own
        // session's target (none: the terminal or the web page).
        let Ok(target) = self.outbox.try_target(&mark.arrived_session) else {
            return Present::Unknown;
        };
        let place = match self.class {
            crate::places::PlaceClass::Private => Place::Private,
            crate::places::PlaceClass::Shared => {
                target.clone().map_or(Place::Unknown, Place::Shared)
            }
        };
        present(
            &place,
            target.as_deref(),
            Input::Message(&mark.node),
            self.store,
        )
    }

    fn text(&self, node: &str) -> Result<Option<String>> {
        Ok(match self.store.get_node(node)? {
            Some((_, n)) if n.session_id == self.session => match n.body {
                crate::node::Body::UserMessage { text, .. } => Some(text),
                _ => None,
            },
            _ => None,
        })
    }

    fn outside(&self) -> bool {
        self.outside
    }

    fn words(&self, task: &str, version: u64) -> Result<Option<Words>> {
        words(self.store, task, version)
    }

    fn judged(&self, node: &str) -> Option<JudgedChange> {
        judged_change(self.store, node)
    }

    fn spent(&self, node: &str) -> bool {
        !matches!(super::judged::receipt(self.store, node), Ok(None))
    }

    /// Asked while the pack acted, AND the ladder lets it act now.
    fn acting(&self, node: &str) -> bool {
        matches!(super::judged::judged(self.store, node), Ok(Some(j)) if j.acting)
            && super::judged::acts_now(self.store, self.session)
    }
}

#[cfg(test)]
#[path = "tests_words.rs"]
mod tests;
