//! The selectors' sources over the store (`super::select::sources`), for a turn in a place set to `view`:
//! an exchange's facts from the ledger's record and its nodes, a fold node's sizes, a place's `/new` mark
//! and the places a person has spoken in, and recall (`super::sources_recall`). What waits (`super::sources_waiting`), the things
//! named and the name matcher (`super::sources_names`) and a task's card (`crate::task_graph::joined`) have
//! sources of their own; their impls here, and facts about the person, answer nothing.

use std::collections::BTreeSet;

use super::fold::FoldNode;
use super::select::sources::{
    ExchangeFacts, Exchanges, Folds, Names, People, Places, Recall, Ref, TaskCards, Things, Waiting,
};
use super::text::tokens;
use super::{AtHand, Audience, Author, Handle, Marks, PointOfView, Pos, Sizes};
use crate::exchange::{self, Opener, Root, OWNER};
use crate::ladder::did;
use crate::node::Body;
use crate::store::Store;

/// The most of a person's timeline [`Places::places_of`] reads.
const PLACES_READ: usize = 500;
/// A tool call's stub, in tokens: one line naming the call.
const STUB_TOKENS: u64 = 30;

/// Each session an assembly reads, read once and shared by the selectors, the text and the window; an
/// exchange's facts once each.
#[derive(Default)]
pub struct Sessions {
    read: std::cell::RefCell<
        std::collections::HashMap<String, std::rc::Rc<crate::store::Transcript>>,
    >,
    facts: std::cell::RefCell<std::collections::HashMap<String, Option<ExchangeFacts>>>,
    /// The text of each piece code reads whole for the turn, by its space: a named thing's card
    /// (`super::sources_names`), a waiting record's line (`super::sources_waiting`), the facts about the
    /// person (`super::sources_people`). The renderer lays what the selectors read, once.
    pub texts: std::cell::RefCell<std::collections::BTreeMap<super::Space, String>>,
    /// The sessions those texts were read from, as the gate admitted them: a seal of one before a later
    /// call of the turn revokes the view (`turn::view_step`).
    pub depends: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// How many times the store was read for a session.
    pub reads: std::cell::Cell<u32>,
    /// A fork's cut (`fork::cut`): no node an ancestor wrote past its point is read here, and an exchange's
    /// reach against the point is asked of it. Empty for a session that is no fork.
    pub cut: crate::fork::cut::Cut,
}

impl Sessions {
    /// The sessions an assembly of a fork reads, cut at its point.
    pub fn with_cut(cut: crate::fork::cut::Cut) -> Self {
        Self {
            cut,
            ..Self::default()
        }
    }

    /// `session`'s nodes, read from the store the first time.
    pub fn of(
        &self,
        store: &Store,
        session: &str,
    ) -> anyhow::Result<std::rc::Rc<crate::store::Transcript>> {
        if let Some(t) = self.read.borrow().get(session) {
            return Ok(t.clone());
        }
        let t = std::rc::Rc::new(store.transcript(session)?);
        self.reads.set(self.reads.get() + 1);
        self.read
            .borrow_mut()
            .insert(session.to_string(), t.clone());
        Ok(t)
    }

    /// The nodes of exchange `turn` of `session`, in order.
    pub fn exchange(
        &self,
        store: &Store,
        session: &str,
        turn: &str,
    ) -> anyhow::Result<Vec<(u64, crate::stub::Stub)>> {
        Ok((self.of(store, session)?.iter())
            .filter(|(p, n)| {
                n.turn_id.as_deref() == Some(turn) && !self.cut.past(&n.session_id, *p)
            })
            .cloned()
            .collect())
    }

    /// Where exchange `ex` stands against a fork's cut: inside for a session that is no fork.
    pub fn reach(
        &self,
        store: &Store,
        ex: &exchange::Exchange,
    ) -> anyhow::Result<crate::fork::cut::Reach> {
        if self.cut.is_empty() {
            return Ok(crate::fork::cut::Reach::Inside);
        }
        self.cut.reach(store, ex)
    }

    /// Whether exchange `turn`'s `did` line may be laid: not when a fork's point falls in it or before it.
    pub fn summarised(&self, store: &Store, turn: &str) -> anyhow::Result<bool> {
        if self.cut.is_empty() {
            return Ok(true);
        }
        Ok(match exchange::exchange(store, turn)? {
            Some(ex) => self.cut.proven_inside(store, &ex)?,
            None => true,
        })
    }
}

/// The sources over `store`. `mark` is the turn's place and its `/new` mark, and `recalled` the items
/// today's recall found for the turn's message and admitted (`turn::view_step::recall`), both read by the
/// turn.
pub struct StoreSources<'a> {
    pub store: &'a Store,
    pub mark: Option<(&'a str, Pos)>,
    pub recalled: Vec<super::sources_recall::Recalled>,
    pub sessions: &'a Sessions,
    /// The asking turn: its own exchange, open while it runs, is its tail and never a piece of the view (it
    /// is not laid in the window, so it is not charged to the budget either).
    pub own: Option<&'a str>,
}

/// Who wrote a person's message, by its node's mark: the owner's mark is his; a chat author's proof is
/// another person's; no mark, or a mark that names no person (an agent's process), is unknown.
pub fn author_of(store: &Store, node: &str) -> Author {
    match exchange::mark(store, node)
        .ok()
        .flatten()
        .and_then(|m| m.person())
    {
        Some(p) if p == OWNER => Author::His {
            may_authorise: false,
        },
        Some(_) => Author::Other,
        None => Author::Unknown,
    }
}

fn json_tokens(v: &serde_json::Value) -> u64 {
    tokens(&v.to_string())
}

impl Exchanges for StoreSources<'_> {
    fn facts(&self, turn_id: &str) -> anyhow::Result<Option<ExchangeFacts>> {
        if self.own == Some(turn_id) {
            return Ok(None);
        }
        if let Some(f) = self.sessions.facts.borrow().get(turn_id) {
            return Ok(f.clone());
        }
        let f = self.read_facts(turn_id)?;
        (self.sessions.facts.borrow_mut()).insert(turn_id.to_string(), f.clone());
        Ok(f)
    }
}

impl StoreSources<'_> {
    fn read_facts(&self, turn_id: &str) -> anyhow::Result<Option<ExchangeFacts>> {
        let Some(ex) = exchange::exchange(self.store, turn_id)? else {
            return Ok(None);
        };
        // A fork's cut, at the seam every selector reads (`fork::cut`): an exchange an ancestor began after
        // its point is no candidate; one the point falls in is never summarised.
        let reach = self.sessions.reach(self.store, &ex)?;
        if reach == crate::fork::cut::Reach::Beyond {
            return Ok(None);
        }
        let (mut his, mut reply, mut tools, mut stubs, mut outside) = (0, 0, 0, 0, false);
        for (_, n) in self
            .sessions
            .exchange(self.store, &ex.session_id, turn_id)?
        {
            match &n.body {
                Body::UserMessage { text, .. } => his += tokens(text),
                Body::AssistantMessage { blocks, .. } => {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => reply += tokens(b["text"].as_str().unwrap_or_default()),
                            Some("tool_use") => {
                                tools += json_tokens(&b["input"]);
                                stubs += STUB_TOKENS;
                            }
                            _ => {}
                        }
                    }
                }
                Body::ToolResult {
                    content, external, ..
                } => {
                    tools += tokens(content);
                    outside |= external.is_some();
                }
                _ => {}
            }
        }
        let author = match (ex.opened_by, ex.person.as_deref()) {
            (Opener::Message | Opener::Prompt, Some(OWNER)) => Author::His {
                may_authorise: false,
            },
            (Opener::Message | Opener::Prompt, Some(_)) => Author::Other,
            (Opener::Message | Opener::Prompt | Opener::Unknown, None) => Author::Unknown,
            _ => Author::Theseus,
        };
        // Its `did` line summarises the whole exchange: laid only on proof that all of it is before a fork's
        // point (`fork::cut::Cut::proven_inside`).
        let summarised = reach == crate::fork::cut::Reach::Inside
            && self.sessions.cut.proven_inside(self.store, &ex)?;
        Ok(Some(ExchangeFacts {
            marks: Marks {
                place: Some(ex.place.clone()),
                audience: match ex.private {
                    true => Audience::Private,
                    false => Audience::Shared,
                },
                author,
                outside,
                // Anything goes (2026-11-22): no seal, move or veil marks an exchange.
                sealed: false,
                veiled: false,
            },
            place: ex.place,
            person: ex.person,
            his,
            reply,
            tools,
            stubs,
            did: match summarised {
                true => did::did(self.store, turn_id)?.map(|d| tokens(&d.line)),
                false => None,
            },
            handle: Handle(turn_id.to_string()),
        }))
    }
}

impl Folds for StoreSources<'_> {
    fn sizes(&self, node: &FoldNode) -> anyhow::Result<Sizes> {
        let mut words = 0;
        for id in &node.his_words {
            let said = self.store.get_node(id)?.and_then(|(_, n)| did::words(&n));
            words += said.map_or(0, |w| tokens(&w));
        }
        let whole = tokens(&node.title) + tokens(&node.account) + words;
        Ok(Sizes {
            whole,
            spoken: whole,
            line: tokens(&node.title) + tokens(&node.account),
        })
    }
}

impl Places for StoreSources<'_> {
    fn new_mark(&self, place: &str) -> anyhow::Result<Option<Pos>> {
        Ok(self
            .mark
            .as_ref()
            .filter(|(p, _)| *p == place)
            .map(|(_, m)| m.clone()))
    }

    fn places_of(&self, person: &str) -> anyhow::Result<Vec<String>> {
        let mut places = BTreeSet::new();
        for t in exchange::newest(self.store, Root::Person(person), PLACES_READ)? {
            if let Some(ex) = exchange::exchange(self.store, &t.exchange)? {
                places.insert(ex.place);
            }
        }
        Ok(places.into_iter().collect())
    }
}

impl Waiting for StoreSources<'_> {
    fn waiting(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Things for StoreSources<'_> {
    fn named_in(&self, _: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn card(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
    fn at_hand(&self, _: &AtHand) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Names for StoreSources<'_> {
    fn named(&self, _: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

impl TaskCards for StoreSources<'_> {
    fn joined(&self, _: &PointOfView, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Recall for StoreSources<'_> {
    /// Every item today's recall found and admitted, laid as its `Recall` note renders it, once each
    /// (`super::sources_recall`): what recall found, not the exchange it came from.
    fn recall(&self, _: &PointOfView, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(super::sources_recall::refs(&self.recalled, self.sessions))
    }
}

impl People for StoreSources<'_> {
    fn about(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}
