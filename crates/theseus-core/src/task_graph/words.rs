//! A change to the owner's task in his own current words, without a card
//! (issue 44; the design's §7, "changing its objective", and the owner's
//! decision 6). The gate the build already has stays the one gate: the
//! objective and acceptance are the owner's layer, every edit is a version
//! under compare-and-set, and a model's change is a proposal that waits for
//! his yes on a card. This module says when the card is not needed, and
//! keeps the words behind each change.
//!
//! The conversation model proposes through `task.update`, naming his message
//! (`said`) and the change's kind (`kind`). It applies without a card only
//! when every check of [`decide`] holds:
//!
//! - the message carries the owner's mark (`exchange::mark`), in person, and
//!   is of the current exchange of this very session;
//! - the owner is alone here, as the gate's `present` says (a channel, an
//!   agent's process at the terminal, an unreadable mark: not alone);
//! - nothing from outside was read since his last message, and this is not
//!   a task's own session;
//! - exactly one task is identified (the call names its whole id), and the
//!   version it names is still the newest, and his message is not older than
//!   that version;
//! - the kind is `narrows` or `pauses` (or `resumes`, the undo of a pause
//!   made so). A narrowing keeps the objective as it was, word for word, and
//!   adds only a verbatim quote of his message (and acceptance only gains
//!   lines quoted so): so words that are not his cannot ride an "ok" of his.
//!   A pause sets the task `suspended`; its undo restores the state before it.
//!
//! `widens`, `replaces`, and anything unsure keep the existing card, with the
//! objective before and after. The words are kept by reference, never by
//! text, in a small META record, `task.words.<id>.<version>`; the proposal
//! that falls to the card says why in its `task.change_proposed` row
//! (`words`), so the cards are counted. No stored record gains a field.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_memory::gate::Present;
use theseus_store::{kinds, NewRecord};

use super::{TaskRecord, TaskState};
use crate::exchange::{Exchange, OwnerMark, OWNER};
use crate::toolrun::TurnCtx;

/// The words behind a change: `task.words.<id>.<version>`.
pub const WORDS_PREFIX: &str = "task.words.";
/// The shortest quote a narrowing adds, in characters.
pub const QUOTE_MIN: usize = 3;

pub fn words_key(task: &str, version: u64) -> String {
    format!("{WORDS_PREFIX}{task}.{version}")
}

/// What a proposed change does to the owner's task, as the model declares it.
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
    /// For a pause: the state it paused, which its undo restores.
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

/// Why a change falls to the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The call names no message of his, or no kind.
    NoWords,
    /// `widens` or `replaces`.
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
    /// The change does not have its kind's shape: a narrowing that drops or
    /// rewrites any of the objective, or adds words that are not his.
    NotItsKind,
    /// A resume with no pause made by his words to undo.
    NoPause,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Why::NoWords => "no_words",
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
            Why::NotItsKind => "not_its_kind",
            Why::NoPause => "no_pause",
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
    pub paused: Option<bool>,
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
}

/// Whitespace runs as one space.
fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `part` is a quote of `of`, word for word, and long enough to say something.
fn quotes(part: &str, of: &str) -> bool {
    let part = squeeze(part);
    part.chars().count() >= QUOTE_MIN && squeeze(of).contains(&part)
}

/// What a narrowing of `old` to `new` adds, when it keeps all of `old`.
fn added<'a>(old: &str, new: &'a str) -> Option<&'a str> {
    let rest = new.strip_prefix(old)?;
    let rest = rest
        .trim_start_matches(|c: char| c.is_whitespace() || ";,:.-—–(".contains(c))
        .trim_end_matches(|c: char| c.is_whitespace() || ".)".contains(c));
    (!rest.is_empty()).then_some(rest)
}

/// A narrowing's shape: the objective kept and his quote added; the
/// acceptance kept, line for line, with only lines he said added.
fn narrows(rec: &TaskRecord, ask: &Ask<'_>, his: &str) -> bool {
    if ask.paused.is_some() || (ask.objective.is_none() && ask.acceptance.is_none()) {
        return false;
    }
    let objective_ok = ask
        .objective
        .is_none_or(|new| added(&rec.objective, new).is_some_and(|quote| quotes(quote, his)));
    let acceptance_ok = ask.acceptance.is_none_or(|new| {
        new.len() > rec.acceptance.len()
            && new.starts_with(&rec.acceptance)
            && new[rec.acceptance.len()..].iter().all(|l| quotes(l, his))
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
    if matches!(kind, Kind::Widens | Kind::Replaces) {
        return Err(Why::Kind(kind));
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
    if mark.at_ms < rec.updated_at_ms {
        return Err(Why::Stale);
    }
    if ev.present(&mark) != Present::OwnerAlone {
        return Err(Why::NotAlone);
    }
    let shaped = match kind {
        Kind::Narrows => match ev.text(said) {
            Ok(Some(his)) => narrows(rec, ask, &his),
            _ => false,
        },
        Kind::Pauses => {
            ask.paused == Some(true)
                && ask.objective.is_none()
                && ask.acceptance.is_none()
                && rec.state != TaskState::Suspended
        }
        Kind::Resumes => {
            if ask.paused != Some(false) || ask.objective.is_some() || ask.acceptance.is_some() {
                false
            } else {
                let paused = ev.words(&rec.id, rec.version).ok().flatten();
                if !paused.is_some_and(|w| w.kind == Kind::Pauses && !w.carded) {
                    return Err(Why::NoPause);
                }
                rec.state == TaskState::Suspended
            }
        }
        Kind::Widens | Kind::Replaces => false,
    };
    match shaped {
        true => Ok(kind),
        false => Err(Why::NotItsKind),
    }
}

/// [`Evidence`] for a turn's call: the store's records, the gate's `present`.
pub struct Turn<'t, 'a>(pub &'t TurnCtx<'a>);

impl Evidence for Turn<'_, '_> {
    fn session(&self) -> &str {
        self.0.session_id
    }

    fn is_task(&self) -> bool {
        self.0.task.is_some()
    }

    fn mark(&self, node: &str) -> Result<Option<OwnerMark>> {
        crate::exchange::mark(self.0.store, node)
    }

    fn exchange(&self) -> Result<Option<Exchange>> {
        crate::exchange::exchange(self.0.store, self.0.turn_id)
    }

    fn present(&self, mark: &OwnerMark) -> Present {
        use crate::view_gate::{present, Input};
        use theseus_memory::recall::Place;
        // Where the message arrived, before any thread routing: its own
        // session's target (none: the terminal or the web page).
        let Ok(target) = self.0.outbox.try_target(&mark.arrived_session) else {
            return Present::Unknown;
        };
        let place = match self.0.class {
            crate::places::PlaceClass::Private => Place::Private,
            crate::places::PlaceClass::Shared => {
                target.clone().map_or(Place::Unknown, Place::Shared)
            }
        };
        present(
            &place,
            target.as_deref(),
            Input::Message(&mark.node),
            self.0.store,
        )
    }

    fn text(&self, node: &str) -> Result<Option<String>> {
        Ok(match self.0.store.get_node(node)? {
            Some((_, n)) if n.session_id == self.0.session_id => match n.body {
                crate::node::Body::UserMessage { text, .. } => Some(text),
                _ => None,
            },
            _ => None,
        })
    }

    fn outside(&self) -> bool {
        let (outside, steered) = crate::config_change::outside_text(self.0);
        outside || steered
    }

    fn words(&self, task: &str, version: u64) -> Result<Option<Words>> {
        words(self.0.store, task, version)
    }
}

#[cfg(test)]
#[path = "tests_words.rs"]
mod tests;
