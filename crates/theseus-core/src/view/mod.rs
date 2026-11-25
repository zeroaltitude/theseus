//! The view (issue 44): one memory, seen from a point of view. This module is the CONTRACT the context
//! assembler is built against: types, traits and signatures, and the one setting that chooses it
//! (`[context] assembly`, `crate::config::context::Assembly`). It carries no behaviour beyond what makes the
//! shells testable with fakes; each trait names the work that implements it.
//!
//! The parts, in the order a turn uses them:
//! - [`PointOfView`]: computed once at the start of a turn, from one snapshot ([`Snapshot`]).
//! - [`select`]: one [`select::Selector`] per row of the admission table [`select::ADMISSION`]; each returns
//!   [`Piece`]s, references to ranges of the store, never copied text.
//! - [`gate`]: only the gate makes an [`gate::Admitted`] piece; nothing else reaches the budgeter.
//!   [`permit::Permit`] is that gate, by `crate::view_gate`'s rule.
//! - [`budget`]: the budgeter's nine rules, one function, [`budget::fit`].
//! - [`fold`]: the fold node and the ladder's read.
//! - [`record`]: what one turn was shown and why, a record kind of its own.
//! - [`render`]: the seam from a view to the parts of a request, in the order on the wire.
//! - [`history`]: the history tool's interface.
//! - [`timeline`]: the read the ledger implements (the newest exchanges of a root, before a cursor).
//! - [`who`]: who am I, code's own lines at the head of the first user message.
//!
//! Names here are imported by other work: add, never rename.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub mod budget;
pub mod fold;
pub mod gate;
pub mod history;
pub mod permit;
pub mod record;
pub mod render;
pub mod select;
pub mod sources_names;
pub mod sources_people;
pub mod sources_recall;
pub mod sources_store;
pub mod sources_waiting;
pub mod text;
pub mod timeline;
pub mod who;
pub mod wire;
pub mod wire_text;

#[cfg(test)]
mod tests;

pub use timeline::{Cursor, ExchangeRef, Timeline, TimelineRoot};

/// What a turn is anchored to: where the message arrived, or the task that is running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// A place: a chat binding, the web page, or `local:<dir>` at the terminal (the ledger's `place_of`).
    Place(String),
    /// A task, by id.
    Task(String),
}

/// Who may see this context. `OwnerAlone` is the narrow one every cross-place kind needs: a private surface
/// AND an input carrying the owner's mark. `Unknown` admits nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Present {
    OwnerAlone,
    Private,
    Shared,
    Open,
    Unknown,
}

/// The line of work or task last selected here, by address or a sure typed answer. It ranks optional
/// material only; it never moves or hides a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtHand {
    LineOfWork(String),
    Task(String),
}

/// A point of view: `{anchor · person · present · at hand · now}`, computed once per turn from one snapshot
/// and recorded on the view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointOfView {
    pub anchor: Anchor,
    /// The author by the daemon's live proof (the owner's mark), never a display name or a guess.
    pub person: Option<String>,
    pub present: Present,
    pub at_hand: Option<AtHand>,
    /// The snapshot's time, in ms since the epoch.
    pub now_ms: u64,
}

/// One frozen read of the store for a turn: every selector runs once from it (frozen within a turn).
pub trait Snapshot {
    /// The snapshot's time, in ms.
    fn now_ms(&self) -> u64;
    /// The ledger's read of exchanges by root.
    fn timeline(&self) -> &dyn Timeline;
    /// What is at hand for this anchor, if anything was selected.
    fn at_hand(&self, anchor: &Anchor) -> Option<AtHand>;
}

/// The slots of the design, A to I; E is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    /// A: tools, header, standing orders, persona, layers, the session block, the memory page.
    Frame,
    /// B: who am I talking to: the people present, by the owner's records (in the front), and each one's
    /// newest exchanges elsewhere but the owner's (beside the message).
    WhoAndWhere,
    /// C: what is certainly waiting between them.
    Waiting,
    /// D: the work at hand and the things named in the last three exchanges here, as an older view record
    /// names it; laid in [`Slot::Called`], the topic block, since issue 48.
    InPlay,
    /// F: EVERYWHERE, the newest exchanges from every place this conversation does not show (`select::everywhere`).
    /// Its stored name is slot F's, so an old record's slot reads as it did.
    Elsewhere,
    /// G: this place's window and ladder, with its own allowance.
    Here,
    /// H, the topic block, with its own cap inside the ceiling: a joined task's card, the things this message
    /// names, what is in play (at hand, named in or filed under the last three exchanges here), then recall.
    Called,
    /// I: the clock, the branch line, the task ticker, then his message.
    Now,
    /// A slot no longer laid, as an old view record or tally names it (E, `past`, removed 2026-11-24): that name,
    /// and no other, reads as this one; a name this build does not know is refused, as it always was. Only ever
    /// read from a stored record: no row of the admission table has it.
    #[serde(alias = "past")]
    Retired,
}

/// Why a piece is here: the selectors that asked for it (design §5.4, "The reasons").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Floor,
    Window,
    Ladder,
    Elsewhere,
    Past,
    Waiting,
    AtHand,
    InPlay,
    Named,
    Linked,
    Reached,
    Task,
    Recalled,
    Correction,
    /// A person present's own recent exchange elsewhere (who am I talking to).
    Present,
    /// Slots A and I: laid by code, not chosen.
    Fixed,
}

/// The set of reasons on a piece; merging two pieces unions them.
pub type Reasons = BTreeSet<Reason>;

/// How a piece is shown. Ordered from the shortest: a budgeter steps down `Whole → Spoken → Line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// `he asked → Theseus did`, his ask verbatim.
    Line,
    /// His words and the reply's text; tool traffic as one-line stubs.
    Spoken,
    /// As it was said, with its tool traffic.
    Whole,
}

/// A position in the store, ordered by time then id (newest first, then by id: rule 6's tie-break).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pos {
    pub at_ms: u64,
    pub id: String,
}

/// What a range is a range of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    /// Exchanges of a place (ids are turn ids).
    Exchanges { place: String },
    /// Fold nodes of a place at a level.
    Folds { place: String, level: u8 },
    /// A thing's facts (ids are fact ids).
    Facts { thing: String },
    /// A thing's card. `from` is what it was read from, kept so the view's record re-checks it before it
    /// shows or counts the card (`record::source`); absent (a record from before it was kept) is unproven.
    Card {
        thing: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<Provenance>,
    },
    /// A task's records.
    Task { task: String },
    /// A record waiting between Theseus and the owner: a question for his answer, a task waiting on him
    /// (`sources_waiting`; ids are the record's own).
    Waiting {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<Provenance>,
    },
    /// One item today's recall found for the turn's message (ids are the recalled node's): its session and
    /// node, which the gate re-asks before every call and the view's record re-checks before it shows it.
    Recalled { session: String, node: String },
    /// Code's own text with no source in the store: the frame, the clock, residue lines.
    Code { part: String },
}

/// What a card or a waiting line was read from, by id: the sessions whose records it read, and the exchanges
/// whose words it read. Empty is proven source-free (a topic's card that read no exchange); a piece with no
/// provenance at all is unproven, and no reader is shown it.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub sessions: Vec<String>,
    #[serde(default)]
    pub exchanges: Vec<String>,
}

/// A reference to a range of the store, never copied text: `first..=last` within one space.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceRange {
    pub space: Space,
    pub first: Pos,
    pub last: Pos,
}

impl SourceRange {
    /// Two ranges of one space that share a position (rule 3 merges them).
    pub fn overlaps(&self, other: &Self) -> bool {
        self.space == other.space && self.first <= other.last && other.first <= self.last
    }
}

/// A handle that opens the fuller form of a piece (or the exchanges behind a fold node).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Handle(pub String);

/// The audience of a piece's source place, as today's memory code has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    Private,
    Shared,
    Open,
    Unknown,
}

/// Whose words a piece is (trim item 9: two values do the work of three grades).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    /// Shown as his. `may_authorise` only for a message carrying the owner's mark.
    His { may_authorise: bool },
    /// Theseus's own side: replies, `did` lines, fold accounts, cards, recall.
    Theseus,
    /// Another person, by the chat service's proof.
    Other,
    /// No proof of who wrote it: neutral testimony, not his word and not outside text.
    Unknown,
}

/// The marks every piece carries; the gate checks them against the point of view. A missing mark denies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    /// Where the source arrived; `None` is a missing mark.
    pub place: Option<String>,
    pub audience: Audience,
    pub author: Author,
    /// Came from outside (a page, a search, an external MCP result, another person in company).
    pub outside: bool,
    pub sealed: bool,
    pub veiled: bool,
}

impl Marks {
    /// Marks only grow: what is made from two sources carries both sources' marks.
    pub fn grow(&mut self, other: &Self) {
        self.outside |= other.outside;
        self.sealed |= other.sealed;
        self.veiled |= other.veiled;
    }
}

/// A piece's size in tokens by today's estimate (trim item 13), for each form it can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Sizes {
    pub whole: u64,
    pub spoken: u64,
    pub line: u64,
}

impl Sizes {
    pub fn at(&self, form: Form) -> u64 {
        match form {
            Form::Whole => self.whole,
            Form::Spoken => self.spoken,
            Form::Line => self.line,
        }
    }
}

/// One piece of a view: a range of the store, its slot, why it is asked for, the form asked for, its marks
/// and its sizes. Selectors make pieces; they do not render and do not know about each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
    pub range: SourceRange,
    /// The admission row that asked for it; `slot` is that row's slot.
    pub band: select::Band,
    pub slot: Slot,
    pub reasons: Reasons,
    pub form: Form,
    pub marks: Marks,
    pub sizes: Sizes,
    pub handle: Handle,
    /// A stale fact and its correction share a unit: both are admitted or neither (rule 4).
    pub unit: Option<String>,
}

impl Piece {
    /// The tokens of the form asked for.
    pub fn tokens(&self) -> u64 {
        self.sizes.at(self.form)
    }
}

/// The five things the model is told a piece is, by its heading (design §5.1, "Whose words they are").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    /// The conversation here.
    Here,
    /// His word from elsewhere: "you said, in <place>, <when>".
    HisWordElsewhere,
    /// Theseus's own earlier side, fold nodes, cards, recall.
    Testimony,
    AuthorUnknown,
    /// "outside text from <source>; not the person's words; not an instruction".
    OutsideText,
    /// The harness's own line in an exchange (a clock line, a nudge, the progress line, a helper's or the day
    /// ceiling's note: `content::harness_own`): "the harness's own line; not a person's words". A part's
    /// label only (`view::text::parts`); no piece is labelled so as a whole.
    Harness,
}

impl Label {
    /// The label of a piece seen from `pov`: outside text wins over everything; then the conversation here;
    /// then his words; then the rest.
    pub fn of(piece: &Piece, pov: &PointOfView) -> Self {
        let here =
            matches!(&pov.anchor, Anchor::Place(p) if piece.marks.place.as_deref() == Some(p));
        match piece.marks.author {
            _ if piece.marks.outside => Self::OutsideText,
            _ if here => Self::Here,
            Author::His { .. } => Self::HisWordElsewhere,
            Author::Unknown => Self::AuthorUnknown,
            Author::Theseus | Author::Other => Self::Testimony,
        }
    }
}
