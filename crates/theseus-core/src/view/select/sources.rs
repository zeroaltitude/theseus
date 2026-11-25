//! What the selectors read beyond the snapshot's timeline: one small trait per kind of record. Each is a
//! read of the turn's one snapshot, implemented over the store by the work that wires the view into the
//! compiler (the ledger's exchanges, the fold ladder, the touch index, the records of what waits, the name
//! matcher, a task's card, recall). The selectors are pure functions over these and are tested with fakes.
//!
//! A source never decides what is shown: it says what exists, with its marks and its sizes by today's
//! estimate (trim item 13); the gate and the budgeter decide.

use super::super::fold::{FoldNode, Ladder};
use super::super::gate::Gate;
use super::super::{AtHand, Form, Handle, Marks, Piece, PointOfView, Pos, Sizes, SourceRange};

/// One exchange as the selectors see it. Tokens are today's estimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeFacts {
    /// The ledger's place of the exchange.
    pub place: String,
    /// Its marks: the place's audience, who wrote the message that opened it, seal, veil, outside.
    pub marks: Marks,
    /// The person whose marked message opened it; `None` for a wake, a report, a continuation or an
    /// unmarked input.
    pub person: Option<String>,
    /// His words in it.
    pub his: u64,
    /// The reply's text.
    pub reply: u64,
    /// Its tool calls and results, whole.
    pub tools: u64,
    /// Its tool traffic as one-line stubs.
    pub stubs: u64,
    /// Its `did` line; `None` when it was not written (shown as "reply not summarised").
    pub did: Option<u64>,
    pub handle: Handle,
}

/// The ledger's exchanges.
pub trait Exchanges {
    fn facts(&self, turn_id: &str) -> anyhow::Result<Option<ExchangeFacts>>;
}

/// A fold node's sizes: whole (the account and his words), spoken, and a line.
pub trait Folds {
    fn sizes(&self, node: &FoldNode) -> anyhow::Result<Sizes>;
}

/// Places and their `/new` marks.
pub trait Places {
    /// The place's newest `/new` mark: nothing of the place before it is in its window, ladder or play.
    fn new_mark(&self, place: &str) -> anyhow::Result<Option<Pos>>;
    /// Every place the person has spoken in, any order. (The selectors no longer read it: his newest
    /// exchange per place is found by paging his own timeline, so a place where others posted since he spoke
    /// is not missed.)
    fn places_of(&self, person: &str) -> anyhow::Result<Vec<String>>;
}

/// A stored thing a selector may ask for that is not an exchange: a card, a fact, a task's card, a waiting
/// record, a recall hit. `unit` ties a stale fact to its correction (rule 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub range: SourceRange,
    pub marks: Marks,
    pub sizes: Sizes,
    pub handle: Handle,
    /// The form the source offers it in.
    pub form: Form,
    pub unit: Option<String>,
}

/// What is certainly waiting between Theseus and the person, from any place: questions parked for him,
/// tasks waiting on him, a message of his still being answered elsewhere.
pub trait Waiting {
    fn waiting(&self, person: &str) -> anyhow::Result<Vec<Ref>>;
}

/// Things, by the touch index's names (`type:value`).
pub trait Things {
    /// The things named in an exchange, by him or by Theseus, or whose handle was opened in it.
    fn named_in(&self, turn_id: &str) -> anyhow::Result<Vec<String>>;
    /// A thing's card and current facts, with any stale fact sharing a unit with its correction.
    fn card(&self, thing: &str) -> anyhow::Result<Vec<Ref>>;
    /// The card of what is at hand.
    fn at_hand(&self, at: &AtHand) -> anyhow::Result<Vec<Ref>>;
}

/// The name matcher (built by the names work): the things this message names, a qualified name winning,
/// an alias only when unique among live things.
pub trait Names {
    fn named(&self, message: &str) -> anyhow::Result<Vec<String>>;
}

/// A joined task's card (built by the tasks work).
pub trait TaskCards {
    fn joined(&self, pov: &PointOfView, message: &str) -> anyhow::Result<Vec<Ref>>;
}

/// Today's recall, as one more selector's source.
pub trait Recall {
    fn recall(&self, pov: &PointOfView, message: &str) -> anyhow::Result<Vec<Ref>>;
}

/// A held person, as the selectors of slot B see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The id of the record that holds them: one id whatever handle they spoke by.
    pub id: String,
    /// The owner, by identity: the ledger's own spelling, an effective owner handle, or a record one of those
    /// resolves to, whatever handle this exchange was marked by. A filter of what is selected only: it gives
    /// no mark the owner's authority.
    pub owner: bool,
    /// Every handle but a name the ledger may name them by: the record's, and the one asked for.
    pub handles: Vec<String>,
}

/// The owner's records of the people present (slot B, who am I talking to).
pub trait People {
    /// The held person `person` is (as the ledger names whoever opened an exchange): none for a person no
    /// record holds (nothing is a person until the owner accepts it).
    fn held(&self, person: &str) -> anyhow::Result<Option<Held>>;
    /// Whether `person`, held or not, is the owner by identity: the ledger's own spelling, or an effective
    /// owner handle (the place rule's).
    fn owner(&self, person: &str) -> bool;
    /// What is laid in the front about held person `id`: its name, its description and the owner's guidance
    /// for it, in under `most` tokens; the same for every person, whoever speaks.
    fn about(&self, id: &str, most: u64) -> anyhow::Result<Vec<Ref>>;
    /// The one line beside the message that names held person `id` as the turn's speaker.
    fn speaking(&self, id: &str) -> anyhow::Result<Vec<Ref>>;
    /// A recent line's tokens as it is laid beside the message: its headings, both wrappers and the name in
    /// them, after escaping, and the blank line that parts it from the next (today's estimate).
    fn laid(&self, piece: &Piece, pov: &PointOfView) -> anyhow::Result<u64>;
}

/// Every source a turn's selectors read, the gate, and the turn's own text.
#[derive(Clone, Copy)]
pub struct Sources<'a> {
    /// The turn's gate: every finite choice a selector makes (the floor's newest three, the five places,
    /// the newest reply per place, the exchanges in play) is made over candidates it has already admitted,
    /// so a record the gate denies cannot change what is chosen. The same gate as [`super::assemble`]'s.
    pub gate: &'a dyn Gate,
    pub exchanges: &'a dyn Exchanges,
    pub ladder: &'a dyn Ladder,
    pub folds: &'a dyn Folds,
    pub places: &'a dyn Places,
    pub waiting: &'a dyn Waiting,
    pub things: &'a dyn Things,
    pub names: &'a dyn Names,
    pub tasks: &'a dyn TaskCards,
    pub recall: &'a dyn Recall,
    pub people: &'a dyn People,
    /// The message this turn answers.
    pub message: &'a str,
    /// The look-backs' sizes, `[context]`'s: EVERYWHERE's count is `sizes.everywhere`.
    pub sizes: crate::config::context::Sizes,
}
