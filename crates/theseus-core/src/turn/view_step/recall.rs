//! What slot H lays of the turn's recall (issue 44: memory intact on the view): the items today's recall
//! found, each whose source still reads, read for the view (`view::sources_recall`): the note's own
//! rendering of each, and its source session's place and who wrote it. Anything goes (2026-11-22): no
//! presence, seal or move holds an item out. A note whose exchange the history tool opens for the asker
//! (`history_tool::page::Lens::opens`) ends with that exchange's handle, so the whole of it is one `open` away
//! (issue 44, step 5); no other note names one.

use std::collections::BTreeSet;
use std::sync::Mutex;

use theseus_memory::gate::Veil;
use theseus_memory::recall::Place;

use super::super::TurnRunner;
use crate::history_tool::page::Lens;
use crate::node::{Body, Node, RecalledRef};
use crate::recall::render::{self, Sources};
use crate::view::history::HISTORY_TOOL;
use crate::view::sources_recall::Recalled;
use crate::view::{Audience, Author, Marks};

impl TurnRunner {
    /// Of `items`, those whose source still reads, and each as slot H reads it.
    pub(crate) fn view_recall(
        &self,
        asker: &str,
        items: Vec<RecalledRef>,
    ) -> anyhow::Result<(Vec<RecalledRef>, Vec<Recalled>)> {
        let cache = Mutex::default();
        let mut sources = Sources::new();
        // A fork lays nothing an ancestor said after the point it was forked from it (`fork::cut`): one keyed
        // read of each item's node for its position. A read that fails fails the turn.
        let cut = crate::fork::cut::Cut::of(&self.store, asker)?;
        let mut kept_items = Vec::with_capacity(items.len());
        for r in items {
            let past = match cut.is_empty() {
                true => false,
                false => (self.store.get_node(&r.node_id)?)
                    .is_some_and(|(p, n)| cut.past(&n.session_id, p)),
            };
            if !past {
                kept_items.push(r);
            }
        }
        let items = kept_items;
        for r in &items {
            if let Some(n) = render::source(&self.store, r, &cache) {
                sources.insert(r.node_id.clone(), n);
            }
        }
        // What the asker's `open` reads through; a lens that cannot be read names no handle.
        let lens = match items.is_empty() {
            true => None,
            false => (crate::history_tool::open_lens(self, asker))
                .inspect_err(|e| tracing::warn!(error = %e, "recall: no note names its exchange"))
                .ok(),
        };
        let (mut kept, mut laid) = (Vec::new(), Vec::new());
        for r in items {
            let Some(n) = sources.get(&r.node_id) else {
                continue;
            };
            let mut text = render::item(&r, Some(n));
            if let Some(turn) =
                (n.turn_id.as_deref()).filter(|t| lens.as_ref().is_some_and(|l| opens(l, t)))
            {
                text.push_str(&format!(
                    "\n    open: {turn} ({})",
                    theseus_tools::wire_name(HISTORY_TOOL)
                ));
            }
            laid.push(Recalled {
                text,
                marks: self.recall_marks(asker, n),
                at_ms: n.created_at_ms,
                turn: n.turn_id.clone(),
                item: r.clone(),
            });
            kept.push(r);
        }
        Ok((kept, laid))
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
        out.extend(
            (nodes.iter())
                .filter(|(_, n)| n.kind != crate::stub::Kind::Recall)
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
        let (fork, permit) = crate::fork::window::Fork::permit(store, sid, turn, verified)?;
        let read = fork.sessions();
        let sources = super::StoreSources {
            store,
            mark: since.map(|m| (place.as_str(), m)),
            recalled: Vec::new(),
            sessions: &read,
            own: Some(turn),
        };
        let store_ladder = fork.ladder(store);
        let resolved = super::ResolvedLadder::new(&store_ladder, store);
        let ladder = super::OnceLadder::new(&resolved);
        let wrong = self.memory.labeled(store)?;
        let said = crate::copies::since::Since::new(store, &wrong);
        let moving = self.moving(store, (&permit, &pov), &[], (&read, &said))?;
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

/// Whether the history tool's `open` gives `turn`'s exchange through `lens` (`history_tool::open`): its record
/// read, and the lens opening it. A read that fails gives none.
fn opens(lens: &Lens<'_>, turn: &str) -> bool {
    match crate::exchange::exchange(lens.store, turn) {
        Ok(Some(ex)) => lens.opens(&ex).unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use theseus_protocol::SessionKind;

    use crate::session::SessionRecord;

    /// Recall's own read of a fork's cut fails plainly when the fork's point cannot be read (issue 44, round
    /// 2): it never answers as though the session were no fork.
    #[tokio::test]
    async fn a_forks_recall_fails_when_its_point_cannot_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
        let model: Arc<dyn crate::provider::Provider> =
            Arc::new(crate::provider::FakeProvider::default());
        let core = crate::Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
        let source = SessionRecord::new(SessionKind::Conversation, None);
        core.store.put_session(&source.session_id, &source).unwrap();
        let mut rec = SessionRecord::new(SessionKind::Conversation, None);
        rec.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
            session: source.session_id,
            node: "nod_gone".into(),
            position: 1,
            compilation: None,
            at_ms: 0,
        }));
        core.store.put_session(&rec.session_id, &rec).unwrap();
        let err = core
            .runner
            .view_recall(&rec.session_id, Vec::new())
            .unwrap_err();
        assert!(format!("{err:#}").contains("cannot be read"), "{err:#}");
    }
}
