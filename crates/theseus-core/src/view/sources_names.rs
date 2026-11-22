//! The view's named things over the store (design §5.4; slot D, what is in play, and slot H's named things;
//! `docs/design/names.md`): the table of scoped names, built once for the turn through the turn's own gate,
//! the message's names and numbered items matched against it, and a matched thing's card (`names::card`):
//! its title, what it is, its current facts and its newest exchanges, as one piece.
//!
//! The gate is asked of every record before it is read or counted ([`ViewAdmits`]): a thing it refuses is no
//! name, no candidate, and makes no alias it shares ambiguous; a card's facts, tasks, guidance and exchanges
//! are each admitted first, so a refused one is not even in a card's "… and N more" line. The card itself
//! then goes through the same gate again, as every selector's piece does, with its thing's own marks.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use theseus_ontology::Guidance;
use theseus_protocol::tasks::TaskRecord;
use theseus_protocol::{StandingRecord, StandingScope};

use super::gate::Gate;
use super::select::sources::{Names, Ref, Things};
use super::select::Band;
use super::text::tokens;
use super::{
    AtHand, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Reason, Sizes,
    SourceRange, Space,
};
use crate::exchange::Exchange;
use crate::names::card::{self, Admits, Card, Source};
use crate::names::items::ITEM;
use crate::names::table::{self, How, Table};
use crate::store::Store;
use crate::task_graph::joined::Homes;

/// The place the owner's own filing is marked with (the ontology's categories and guidance, a standing
/// record for everywhere): private, and never the anchor, so only the owner alone sees it.
pub const FILING: &str = "theseus:filing";

/// The turn's gate, as the names table and a card ask it: each record is made a piece with its own marks
/// and passed to the gate, and the marks of every thing it admits are kept for that thing's card.
pub struct ViewAdmits<'a> {
    pub gate: &'a dyn Gate,
    pub pov: &'a PointOfView,
    /// A session's marks as the gate reads them (its place, its class, its seal).
    pub homes: &'a dyn Homes,
    /// Every task, for a task's home session.
    pub tasks: Vec<TaskRecord>,
    /// The marks of each thing admitted, by its key.
    admitted: RefCell<BTreeMap<String, Marks>>,
}

impl<'a> ViewAdmits<'a> {
    pub fn new(
        gate: &'a dyn Gate,
        pov: &'a PointOfView,
        homes: &'a dyn Homes,
        tasks: Vec<TaskRecord>,
    ) -> Self {
        Self {
            gate,
            pov,
            homes,
            tasks,
            admitted: RefCell::default(),
        }
    }

    /// Whether the gate admits a record of these marks, of this time.
    fn passes(&self, marks: Marks, at_ms: u64) -> bool {
        let pos = Pos {
            at_ms,
            id: String::new(),
        };
        let piece = Piece {
            range: SourceRange {
                space: Space::Card {
                    thing: String::new(),
                },
                first: pos.clone(),
                last: pos,
            },
            band: Band::Named,
            slot: Band::Named.row().slot,
            reasons: BTreeSet::from([Reason::Named]),
            form: Form::Line,
            marks,
            sizes: Sizes::default(),
            handle: Handle(String::new()),
            unit: None,
        };
        self.gate.admit(self.pov, piece).is_some()
    }

    /// A task's marks: its home session's.
    fn task_marks(&self, t: &TaskRecord) -> Marks {
        self.homes.marks(&crate::task_graph::home(&self.tasks, t))
    }

    /// The marks the gate admitted `thing` with, if it did.
    pub fn marks_of(&self, thing: &str) -> Option<Marks> {
        self.admitted.borrow().get(thing).cloned()
    }
}

/// The owner's own filing, marked private and of no place a turn is anchored to.
fn filing() -> Marks {
    private(FILING)
}

fn private(place: &str) -> Marks {
    Marks {
        place: Some(place.to_string()),
        audience: Audience::Private,
        author: Author::Theseus,
        outside: false,
        sealed: false,
        veiled: false,
    }
}

/// An exchange's marks as the view reads them (`sources_store`): its place, its class, its session's seal.
fn exchange_marks(homes: &dyn Homes, e: &Exchange) -> Marks {
    Marks {
        place: Some(e.place.clone()),
        audience: match e.private {
            true => Audience::Private,
            false => Audience::Shared,
        },
        sealed: homes.marks(&e.session_id).sealed,
        ..filing()
    }
}

impl Admits for ViewAdmits<'_> {
    fn fact(&self, r: &StandingRecord) -> bool {
        let marks = match &r.scope {
            StandingScope::Session(s) => self.homes.marks(s),
            // A place's record is held as private to it: shown there, or to the owner alone.
            StandingScope::Place(p) => private(p),
            _ => filing(),
        };
        self.passes(marks, r.since_ms)
    }

    fn exchange(&self, e: &Exchange) -> bool {
        self.passes(exchange_marks(self.homes, e), e.start_ms)
    }

    fn thing(&self, s: Source<'_>) -> bool {
        let (key, marks) = match s {
            // A line of work with no session of its own has no place to be read from: no name.
            Source::Line(th) => match &th.session_id {
                Some(sid) => (format!("thread:{}", th.slug), self.homes.marks(sid)),
                None => return false,
            },
            Source::Task(t) => (format!("task:{}", t.id), self.task_marks(t)),
            Source::Topic(c) => (c.id.to_string(), filing()),
        };
        let ok = self.passes(marks.clone(), self.pov.now_ms);
        if ok {
            self.admitted.borrow_mut().insert(key, marks);
        }
        ok
    }

    fn task(&self, t: &TaskRecord) -> bool {
        self.passes(self.task_marks(t), t.created_at_ms)
    }

    fn guidance(&self, _: &Guidance) -> bool {
        self.passes(filing(), self.pov.now_ms)
    }
}

/// The named things of one turn: its table, its message's numbered items, and the cards read, each once.
pub struct Called<'a> {
    pub core: &'a crate::rpc::Core,
    pub admits: ViewAdmits<'a>,
    table: Table,
    /// The message's numbered items, scoped as they were touched (`names::items`).
    items: Vec<String>,
    /// Each card read, by thing; its text is what the renderer lays (`wire_text`).
    cards: &'a RefCell<BTreeMap<String, Option<String>>>,
}

impl<'a> Called<'a> {
    /// The table of what the stores hold and `admits` admits, built once for the turn.
    pub fn build(
        core: &'a crate::rpc::Core,
        admits: ViewAdmits<'a>,
        items: Vec<String>,
        cards: &'a RefCell<BTreeMap<String, Option<String>>>,
    ) -> anyhow::Result<Self> {
        let table = table::build_for(core, &admits)?;
        Ok(Self {
            core,
            admits,
            table,
            items,
            cards,
        })
    }

    fn store(&self) -> &Store {
        &self.core.store
    }

    /// The things `text` names: a qualified name, or an alias one live admitted thing holds. Candidates
    /// (an alias several hold) name nothing.
    fn found(&self, text: &str) -> Vec<String> {
        (self.table.find(text).into_iter())
            .filter(|f| matches!(f.how, How::Qualified | How::Unique))
            .flat_map(|f| f.things)
            .collect()
    }

    /// `thing`'s card as the gate admits it, read once: a thing of the table, or a numbered item (its
    /// timeline's admitted exchanges; an item with none has no card).
    fn read(&self, thing: &str) -> anyhow::Result<Option<(Card, Marks)>> {
        if thing.starts_with(ITEM) {
            let Some((c, newest)) = card::item_card(self.store(), thing, &self.admits)? else {
                return Ok(None);
            };
            return Ok(Some((c, exchange_marks(self.admits.homes, &newest))));
        }
        let Some(marks) = self.admits.marks_of(thing) else {
            return Ok(None);
        };
        Ok(card::card(self.core, &self.table, thing, &self.admits)?.map(|c| (c, marks)))
    }
}

/// A card's text as the renderer lays it: its title, then its lines.
pub fn card_text(c: &Card) -> String {
    std::iter::once(c.title.clone())
        .chain(c.lines.iter().map(|l| format!("- {l}")))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Names for Called<'_> {
    /// The message's names and numbered items. Only the turn's own message is matched with its items.
    fn named(&self, message: &str) -> anyhow::Result<Vec<String>> {
        let mut out = self.found(message);
        out.extend(self.items.iter().cloned());
        Ok(out)
    }
}

impl Things for Called<'_> {
    /// The things the person's words that opened the exchange name (an exchange the selector has already
    /// admitted).
    fn named_in(&self, turn_id: &str) -> anyhow::Result<Vec<String>> {
        let Some(ex) = crate::exchange::exchange(self.store(), turn_id)? else {
            return Ok(Vec::new());
        };
        let Some(ask) = ex.ask else {
            return Ok(Vec::new());
        };
        let Some((_, node)) = self.store().get_node(&ask)? else {
            return Ok(Vec::new());
        };
        Ok(self.found(&crate::recall::text_of(&node)))
    }

    fn card(&self, thing: &str) -> anyhow::Result<Vec<Ref>> {
        let Some((c, marks)) = self.read(thing)? else {
            self.cards.borrow_mut().insert(thing.to_string(), None);
            return Ok(Vec::new());
        };
        let text = card_text(&c);
        let (whole, line) = (tokens(&text), tokens(&c.title));
        self.cards
            .borrow_mut()
            .insert(thing.to_string(), Some(text));
        let pos = Pos {
            at_ms: self.admits.pov.now_ms,
            id: thing.to_string(),
        };
        Ok(vec![Ref {
            range: SourceRange {
                space: Space::Card {
                    thing: thing.to_string(),
                },
                first: pos.clone(),
                last: pos,
            },
            marks,
            sizes: Sizes {
                whole,
                spoken: whole,
                line,
            },
            handle: Handle(thing.to_string()),
            form: Form::Whole,
            unit: None,
        }])
    }

    fn at_hand(&self, _: &AtHand) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}
