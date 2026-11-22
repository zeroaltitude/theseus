//! What slot H lays of the turn's recall (issue 44: memory intact on the view): the items today's recall
//! found, each whose source still reads, read for the view (`view::sources_recall`): the note's own
//! rendering of each, and its source session's place and who wrote it. Anything goes (2026-11-22): no
//! presence, seal or move holds an item out.

use std::collections::BTreeSet;
use std::sync::Mutex;

use theseus_memory::gate::Veil;
use theseus_memory::recall::Place;

use super::super::TurnRunner;
use crate::node::{Body, Node, RecalledRef};
use crate::recall::render::{self, Sources};
use crate::view::sources_recall::Recalled;
use crate::view::{Audience, Author, Marks};

impl TurnRunner {
    /// Of `items`, those whose source still reads, and each as slot H reads it.
    pub(super) fn view_recall(
        &self,
        asker: &str,
        items: Vec<RecalledRef>,
    ) -> (Vec<RecalledRef>, Vec<Recalled>) {
        let cache = Mutex::default();
        let mut sources = Sources::new();
        // A fork lays nothing its ancestors said after the point each was forked from it (`fork::window`).
        let after = crate::fork::window::after_point(&self.store, asker).unwrap_or_default();
        let items: Vec<RecalledRef> = (items.into_iter())
            .filter(|r| !after.holds_node(&r.node_id))
            .collect();
        for r in &items {
            if let Some(n) = render::source(&self.store, r, &cache) {
                sources.insert(r.node_id.clone(), n);
            }
        }
        let (mut kept, mut laid) = (Vec::new(), Vec::new());
        for r in items {
            let Some(n) = sources.get(&r.node_id) else {
                continue;
            };
            laid.push(Recalled {
                text: render::item(&r, Some(n)),
                marks: self.recall_marks(asker, n),
                at_ms: n.created_at_ms,
                turn: n.turn_id.clone(),
                item: r.clone(),
            });
            kept.push(r);
        }
        (kept, laid)
    }

    /// What a view turn's recall counts as already in front of the model (issue 44): the turn's own nodes
    /// (its tail), and what the window's floor lays with its words on every turn ([`Self::view_floor`]),
    /// by node. A `Recall` note's sources exclude nothing (slot H is laid afresh each turn), nor does a node
    /// the floor lays only as a receipt, nor an older node of the session no window holds.
    pub(in crate::turn) fn view_in_context(&self, t: &super::super::Turn<'_>) -> BTreeSet<String> {
        let (store, turn) = (t.tc.store, t.tc.turn_id);
        let mut out = self.view_floor(t).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"),
                "recall: the view's floor cannot be read; only the turn's own nodes count as in context");
            BTreeSet::new()
        });
        let Ok(nodes) = store.transcript(t.tc.session_id) else {
            return out;
        };
        let moves = crate::moved::read(&nodes);
        out.extend(
            (nodes.iter())
                .filter(|(_, n)| {
                    n.kind != crate::stub::Kind::Recall && !moves.hidden.contains(&n.id)
                })
                .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
                .map(|(_, n)| n.id.clone()),
        );
        out
    }

    /// The nodes the window's floor lays with their words, read as the view's assembly reads them: the
    /// same point of view, mark, fork, ladder and facts, the floor chosen over what the gate
    /// admits (`select::Here`), each exchange counted by the form the floor asks for it
    /// (`sources_recall::carries`: spoken). The floor is never cut, so
    /// what it lays here the assembled view lays too.
    fn view_floor(&self, t: &super::super::Turn<'_>) -> anyhow::Result<BTreeSet<String>> {
        use crate::view::select::{Band, Here, Selector};
        let (store, sid, turn) = (t.tc.store, t.tc.session_id, t.tc.turn_id);
        let snap = super::StoreSnapshot {
            now_ms: self.kernel.now_ms(),
            timeline: super::StoreTimeline(store),
            at_hand: None,
        };
        let verified = super::Verified::of_turn(self, sid, t.exchange.read_place());
        let pov = super::verified_pov(&verified, t.exchange.person().0, snap.now_ms);
        let super::Anchor::Place(place) = pov.anchor.clone() else {
            return Ok(BTreeSet::new());
        };
        let since = crate::ladder::new_mark(store, &place)?;
        let (fork, permit) = crate::fork::window::Fork::permit(store, sid, turn, verified);
        let read = fork.sessions();
        let sources = super::StoreSources {
            store,
            mark: since.map(|m| (place.as_str(), m)),
            recalled: Vec::new(),
            sessions: &read,
            own: Some(turn),
        };
        let store_ladder = fork.ladder(store);
        let ladder = super::OnceLadder::new(&store_ladder);
        let moving = self.moving(store, (&permit, &pov), &[], &read)?;
        let cards = super::StoreCards {
            store,
            kernel: &self.kernel,
            names: &sources,
            homes: self,
            judged: None,
        };
        let src = super::sources_of(&permit, (&sources, &moving), &ladder, &cards, "");
        let mut out = BTreeSet::new();
        for p in Here::new(Band::Floor, src).select(&pov, &snap)? {
            let id = &p.range.first.id;
            let Some(ex) = crate::exchange::exchange(store, id)? else {
                continue;
            };
            let nodes = read.exchange(store, &ex.session_id, id)?;
            out.extend(
                (nodes.iter())
                    .filter(|(_, n)| crate::view::sources_recall::carries(p.form, n.kind))
                    .map(|(_, n)| n.id.clone()),
            );
        }
        Ok(out)
    }

    /// A recalled source's marks: its session's place and class as the gate reads them for `asker`, and
    /// who wrote it as far as it is proven. Theseus's reply is his own side; a tool's result is outside
    /// text; a message is the owner's only by his mark, another person's by the chat's proof, else of
    /// unknown author (imported text, anything said before the ledger: decision 4).
    fn recall_marks(&self, asker: &str, n: &Node) -> Marks {
        let m = crate::view_gate::session_marks(self, Some(asker), &n.session_id, Veil::Clear);
        let (author, outside) = match &n.body {
            Body::AssistantMessage { .. } => (Author::Theseus, false),
            Body::ToolResult { .. } => (Author::Unknown, true),
            // A known outside import is outside text, wrapped as any other.
            Body::Imported {
                integrity: crate::import::Integrity::Outside,
                ..
            } => (Author::Unknown, true),
            Body::UserMessage { .. } => (
                crate::view::sources_store::author_of(&self.store, &n.id),
                false,
            ),
            _ => (Author::Unknown, false),
        };
        Marks {
            place: m.place,
            audience: match m.class {
                Place::Private => Audience::Private,
                Place::Shared(_) => Audience::Shared,
                Place::Unknown => Audience::Unknown,
            },
            author,
            outside,
            sealed: false,
            veiled: false,
        }
    }
}
