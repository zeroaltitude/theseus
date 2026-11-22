//! The view's named things over the store (design §5.4; slot D, what is in play, and slot H's named things;
//! `docs/design/names.md`): the table of scoped names, built once for the turn through the turn's own gate,
//! the message's names and numbered items matched against it, and a matched thing's card (`names::card`):
//! its title, what it is, its current facts and its newest exchanges, as one piece.
//!
//! The gate is asked of every record before it is read or counted ([`ViewAdmits`]): a thing it refuses is no
//! name, no candidate, and makes no alias it shares ambiguous; a card's facts, tasks, guidance and exchanges
//! are each admitted first, so a refused one is not even in a card's "… and N more" line. An exchange is
//! admitted only when its session's CURRENT marks (place, audience, seal) and its own arrival place both
//! pass, and none of its messages was moved into a thread not proven clear (`view_gate::Moved`, the turn's
//! one); its words are read only then. The card itself then goes through the same gate again, as every
//! selector's piece does, with its thing's own marks, and carries the sessions it was read from so the
//! view's record re-checks them (`Space::Card::from`).
//!
//! Whose words name things: the turn's own message, and the opening words of an admitted exchange in play.
//! Words the owner's mark proves his may name any admitted thing; another person's, or words with no mark
//! (an agent's process, an import, a rebuilt exchange), name only what is not private (a thing of a shared
//! place), and numbered items only from his. A tool's result (a fetched page) is never matched. Text quoted
//! inside his own typed message is his message.

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
use crate::exchange::{Exchange, OWNER};
use crate::names::card::{self, Admits, Card, Source};
use crate::names::items::{self, ITEM};
use crate::names::table::{self, How, Table};
use crate::store::Store;
use crate::task_graph::joined::Homes;
use crate::view_gate::Moved;

/// The place the owner's own filing is marked with (the ontology's categories and guidance, a standing
/// record for everywhere): private, and never the anchor, so only the owner alone sees it.
pub const FILING: &str = "theseus:filing";

/// Whether `gate` admits a record of these marks, of this time, to `pov`: the record made a piece and asked.
pub fn admitted(gate: &dyn Gate, pov: &PointOfView, marks: Marks, at_ms: u64) -> bool {
    let pos = Pos {
        at_ms,
        id: String::new(),
    };
    let piece = Piece {
        range: SourceRange {
            space: Space::Card {
                thing: String::new(),
                from: Vec::new(),
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
    gate.admit(pov, piece).is_some()
}

/// The turn's gate, as the names table and a card ask it: each record is made a piece with its own marks
/// and passed to the gate, and the marks of every thing it admits are kept for that thing's card.
pub struct ViewAdmits<'a> {
    pub gate: &'a dyn Gate,
    pub pov: &'a PointOfView,
    pub store: &'a Store,
    /// A session's marks as the gate reads them now (its place, its class, its seal).
    pub homes: &'a dyn Homes,
    /// The turn's node seal: an exchange holding a message moved into a thread not proven clear is held.
    pub moved: &'a Moved<'a>,
    /// Every task, for a task's home session.
    pub tasks: Vec<TaskRecord>,
    /// The marks of each thing admitted, by its key.
    admitted: RefCell<BTreeMap<String, Marks>>,
    /// Every session a record admitted was read from (`Sessions::depends`).
    depends: &'a RefCell<BTreeSet<String>>,
    /// The sessions read from since [`Self::reading`] began: one card's provenance.
    reading: RefCell<BTreeSet<String>>,
}

impl<'a> ViewAdmits<'a> {
    pub fn new(
        (gate, pov): (&'a dyn Gate, &'a PointOfView),
        (store, homes, moved): (&'a Store, &'a dyn Homes, &'a Moved<'a>),
        tasks: Vec<TaskRecord>,
        depends: &'a RefCell<BTreeSet<String>>,
    ) -> Self {
        Self {
            gate,
            pov,
            store,
            homes,
            moved,
            tasks,
            admitted: RefCell::default(),
            depends,
            reading: RefCell::default(),
        }
    }

    fn passes(&self, marks: Marks, at_ms: u64) -> bool {
        admitted(self.gate, self.pov, marks, at_ms)
    }

    /// The view depends on `session` (a seal of it before a later call revokes the view).
    fn depend(&self, session: &str) {
        self.depends.borrow_mut().insert(session.to_string());
        self.reading.borrow_mut().insert(session.to_string());
    }

    /// [`Self::passes`] for a record of `session`, which the view then depends on.
    fn passes_of(&self, session: &str, marks: Marks, at_ms: u64) -> bool {
        let ok = self.passes(marks, at_ms);
        if ok {
            self.depend(session);
        }
        ok
    }

    /// Start reading one card: what it is read from is collected apart.
    fn reading(&self) {
        self.reading.borrow_mut().clear();
    }

    /// The sessions read from since [`Self::reading`].
    fn read_from(&self) -> Vec<String> {
        self.reading.borrow().iter().cloned().collect()
    }

    /// A task's home session.
    fn home(&self, t: &TaskRecord) -> String {
        crate::task_graph::home(&self.tasks, t)
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

impl Admits for ViewAdmits<'_> {
    fn fact(&self, r: &StandingRecord) -> bool {
        match &r.scope {
            StandingScope::Session(s) => self.passes_of(s, self.homes.marks(s), r.since_ms),
            // A place's record is held as private to it: shown there, or to the owner alone.
            StandingScope::Place(p) => self.passes(private(p), r.since_ms),
            _ => self.passes(filing(), r.since_ms),
        }
    }

    /// Its session's current marks AND its own arrival place pass, no message of it was moved into a thread
    /// not proven clear, and a place the ledger could not prove (`unknown`) is none.
    fn exchange(&self, e: &Exchange) -> bool {
        if self.moved.holds(e) || e.place.is_empty() || e.place == crate::exchange::rebuild::UNKNOWN
        {
            return false;
        }
        let now = self.homes.marks(&e.session_id);
        let arrived = Marks {
            place: Some(e.place.clone()),
            audience: match e.private {
                true => Audience::Private,
                false => Audience::Shared,
            },
            sealed: now.sealed,
            ..filing()
        };
        if !(self.passes(now, e.start_ms) && self.passes(arrived, e.start_ms)) {
            return false;
        }
        self.depend(&e.session_id);
        for s in self.moved.sessions(e) {
            self.depend(&s);
        }
        true
    }

    /// The opening words' author, as the window labels it (`sources_store::author_of`): another person's or
    /// an unknown author's are kept as outside text inside the card.
    fn by(&self, e: &Exchange) -> Option<&'static str> {
        let node = e.ask.as_deref()?;
        match super::sources_store::author_of(self.store, node) {
            Author::His { .. } => None,
            Author::Other => Some("another person (not the owner)"),
            Author::Theseus | Author::Unknown => {
                Some("author unknown (not the owner's instruction)")
            }
        }
    }

    fn thing(&self, s: Source<'_>) -> bool {
        let (key, session, marks) = match s {
            // A line of work with no session of its own has no place to be read from: no name.
            Source::Line(th) => match &th.session_id {
                Some(sid) => (
                    format!("thread:{}", th.slug),
                    Some(sid.clone()),
                    self.homes.marks(sid),
                ),
                None => return false,
            },
            Source::Task(t) => {
                let home = self.home(t);
                let marks = self.homes.marks(&home);
                (format!("task:{}", t.id), Some(home), marks)
            }
            Source::Topic(c) => (c.id.to_string(), None, filing()),
        };
        let now = self.pov.now_ms;
        let ok = match &session {
            Some(s) => self.passes_of(s, marks.clone(), now),
            None => self.passes(marks.clone(), now),
        };
        if ok {
            self.admitted.borrow_mut().insert(key, marks);
        }
        ok
    }

    fn task(&self, t: &TaskRecord) -> bool {
        let home = self.home(t);
        self.passes_of(&home, self.homes.marks(&home), t.created_at_ms)
    }

    fn guidance(&self, _: &Guidance) -> bool {
        self.passes(filing(), self.pov.now_ms)
    }
}

/// The named things of one turn: its table, its message's numbered items, and the cards read, each once.
pub struct Called<'a> {
    pub core: std::sync::Arc<crate::rpc::Core>,
    pub admits: ViewAdmits<'a>,
    table: Table,
    /// The message's numbered items, scoped as they were touched (`names::items`).
    items: Vec<String>,
    /// Each text read, by its space; the renderer lays it (`wire_text`).
    texts: &'a RefCell<BTreeMap<Space, String>>,
}

impl<'a> Called<'a> {
    /// The table of what the stores hold and `admits` admits, built once for the turn.
    pub fn build(
        core: std::sync::Arc<crate::rpc::Core>,
        admits: ViewAdmits<'a>,
        items: Vec<String>,
        texts: &'a RefCell<BTreeMap<Space, String>>,
    ) -> anyhow::Result<Self> {
        let table = table::build_for(&core, &admits)?;
        Ok(Self {
            core,
            admits,
            table,
            items,
            texts,
        })
    }

    fn store(&self) -> &Store {
        &self.core.store
    }

    /// The things `text` names: a qualified name, or an alias one live admitted thing holds. Candidates
    /// (an alias several hold) name nothing. Words not proven the owner's name nothing private.
    fn found(&self, text: &str, his: bool) -> Vec<String> {
        let public = |k: &String| {
            his || self
                .admits
                .marks_of(k)
                .is_some_and(|m| m.audience != Audience::Private)
        };
        (self.table.find(text).into_iter())
            .filter(|f| matches!(f.how, How::Qualified | How::Unique))
            .flat_map(|f| f.things)
            .filter(public)
            .collect()
    }

    /// `thing`'s card as the gate admits it, read once, with the sessions it was read from: a thing of the
    /// table, or a qualified numbered item (its timeline's admitted exchanges; an item with none has no
    /// card, and an unscoped one is a candidate, never one thing).
    fn read(&self, thing: &str) -> anyhow::Result<Option<(Card, Marks, Vec<String>)>> {
        self.admits.reading();
        if thing.starts_with(ITEM) {
            if !items::is_qualified(thing) {
                return Ok(None);
            }
            let Some((c, newest)) = card::item_card(self.store(), thing, &self.admits)? else {
                return Ok(None);
            };
            let marks = self.admits.homes.marks(&newest.session_id);
            return Ok(Some((c, marks, self.admits.read_from())));
        }
        let Some(marks) = self.admits.marks_of(thing) else {
            return Ok(None);
        };
        let c = card::card(&self.core, &self.table, thing, &self.admits)?;
        Ok(c.map(|c| (c, marks, self.admits.read_from())))
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
    /// The message's names, and its qualified numbered items when the owner's mark proves it his.
    fn named(&self, message: &str) -> anyhow::Result<Vec<String>> {
        let his = self.admits.pov.person.as_deref() == Some(OWNER);
        let mut out = self.found(message, his);
        if his {
            out.extend(
                self.items
                    .iter()
                    .filter(|i| items::is_qualified(i))
                    .cloned(),
            );
        }
        Ok(out)
    }
}

impl Things for Called<'_> {
    /// The things the words that opened an exchange in play name, read only once the exchange is admitted
    /// whole (its moved messages included); words with no person's mark, or a rebuilt exchange's, name
    /// nothing.
    fn named_in(&self, turn_id: &str) -> anyhow::Result<Vec<String>> {
        let Some(ex) = crate::exchange::exchange(self.store(), turn_id)? else {
            return Ok(Vec::new());
        };
        if ex.rebuilt || ex.person.is_none() || !self.admits.exchange(&ex) {
            return Ok(Vec::new());
        }
        let Some(ask) = &ex.ask else {
            return Ok(Vec::new());
        };
        let Some((_, node)) = self.store().get_node(ask)? else {
            return Ok(Vec::new());
        };
        let his = ex.person.as_deref() == Some(OWNER);
        Ok(self.found(&crate::recall::text_of(&node), his))
    }

    fn card(&self, thing: &str) -> anyhow::Result<Vec<Ref>> {
        let Some((c, marks, from)) = self.read(thing)? else {
            return Ok(Vec::new());
        };
        let text = card_text(&c);
        let (whole, line) = (tokens(&text), tokens(&c.title));
        let space = Space::Card {
            thing: thing.to_string(),
            from,
        };
        self.texts.borrow_mut().insert(space.clone(), text);
        let pos = Pos {
            at_ms: self.admits.pov.now_ms,
            id: thing.to_string(),
        };
        Ok(vec![Ref {
            range: SourceRange {
                space,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Opener;
    use crate::view::permit::Permit;
    use crate::view::{Anchor, Present};
    use crate::view_gate::Verified;
    use theseus_memory::gate as rule;

    /// A session's marks as the gate would read them now, fixed by the test.
    struct Now(Marks);

    impl Homes for Now {
        fn marks(&self, _: &str) -> Marks {
            self.0.clone()
        }
    }

    fn dm(place: &str) -> Exchange {
        Exchange {
            turn_id: "turn_1".into(),
            session_id: "ses_1".into(),
            place: place.into(),
            private: true,
            opened_by: Opener::Message,
            person: Some(OWNER.into()),
            start_ms: 1_000,
            end_ms: Some(1_001),
            ask: None,
            reply: None,
            messages: Vec::new(),
            stop: None,
            rebuilt: false,
        }
    }

    /// Whether the owner alone at the terminal is admitted `e` when its session reads `now`, and whether
    /// the view then depends on its session.
    fn admits(now: Marks, e: &Exchange) -> (bool, bool) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let permit = Permit::of(Verified::for_tests(rule::PointOfView {
            anchor: "local".into(),
            place: theseus_memory::recall::Place::Private,
            present: rule::Present::OwnerAlone,
        }));
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: Some(OWNER.into()),
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms: 9_000,
        };
        let moved = Moved::of(&store, None, None);
        let depends = RefCell::default();
        let homes = Now(now);
        let a = ViewAdmits::new(
            (&permit, &pov),
            (&store, &homes, &moved),
            Vec::new(),
            &depends,
        );
        let ok = a.exchange(e);
        drop(a);
        let depended = depends.borrow().contains("ses_1");
        (ok, depended)
    }

    /// KEY: an exchange's words are admitted only when its session's CURRENT place and audience read and
    /// pass, and its own arrival place is a place: an unread place, an unknown audience, or an exchange of
    /// the `unknown` place contributes nothing, and the view does not depend on it.
    #[test]
    fn an_exchange_is_admitted_by_its_sessions_current_marks_and_fails_closed() {
        let clear = private("discord:dm:42");
        assert_eq!(admits(clear.clone(), &dm("discord:dm:42")), (true, true));
        let unplaced = Marks {
            place: None,
            ..clear.clone()
        };
        assert_eq!(admits(unplaced, &dm("discord:dm:42")), (false, false));
        let unknown = Marks {
            audience: Audience::Unknown,
            ..clear.clone()
        };
        assert_eq!(admits(unknown, &dm("discord:dm:42")), (false, false));
        let sealed = Marks {
            sealed: true,
            ..clear
        };
        assert_eq!(admits(sealed, &dm("discord:dm:42")), (false, false));
        let nowhere = crate::exchange::rebuild::UNKNOWN;
        assert_eq!(admits(clear.clone(), &dm(nowhere)), (false, false));
        assert_eq!(admits(clear, &dm("")), (false, false));
    }
}
