//! What slot H may lay of the turn's recall (issue 44: memory intact on the view): the items today's recall
//! found, admitted as a transcript turn's `Recall` note admits them before each compile
//! (`view_gate::readmit_sources`: the place rule under the turn's presence, and each source's node seal),
//! then read for the view (`view::sources_recall`): the note's own rendering of each, and its source
//! session's marks as the gate reads them now. The view's gate then asks those marks as for any piece, so
//! the view lays no more than the note carried, and never in company what it did not.

use std::sync::Mutex;

use theseus_memory::gate::{Seal, Veil};
use theseus_memory::recall::Place;

use super::super::TurnRunner;
use crate::node::{Body, Node, RecalledRef};
use crate::recall::render::{self, Sources};
use crate::view::sources_recall::Recalled;
use crate::view::{Audience, Author, Marks};

impl TurnRunner {
    /// Of `items`, those a turn of `asker` may be shown now, and each as slot H reads it.
    pub(super) fn view_recall(
        &self,
        asker: &str,
        items: Vec<RecalledRef>,
    ) -> (Vec<RecalledRef>, Vec<Recalled>) {
        let cache = Mutex::default();
        let mut sources = Sources::new();
        for r in &items {
            if let Some(n) = render::source(&self.store, r, &cache) {
                sources.insert(r.node_id.clone(), n);
            }
        }
        crate::view_gate::readmit_sources(self, asker, &mut sources);
        let (mut kept, mut laid) = (Vec::new(), Vec::new());
        for r in items {
            let Some(n) = sources.get(&r.node_id) else {
                continue;
            };
            laid.push(Recalled {
                text: render::item(&r, Some(n)),
                marks: self.recall_marks(asker, n),
                at_ms: n.created_at_ms,
                item: r.clone(),
            });
            kept.push(r);
        }
        (kept, laid)
    }

    /// Whether an item the frozen view lays is held out now (a seal, a move): the view is reassembled.
    pub(super) fn view_recall_revoked(&self, asker: &str, laid: &[RecalledRef]) -> bool {
        !laid.is_empty() && self.view_recall(asker, laid.to_vec()).0.len() != laid.len()
    }

    /// A recalled source's marks: its session's place, class and seal as the gate reads them for `asker`,
    /// and who wrote it as far as it is proven. Theseus's reply is his own side; a tool's result is outside
    /// text; a message is the owner's only by his mark, another person's by the chat's proof, else of
    /// unknown author (imported text, anything said before the ledger: decision 4).
    fn recall_marks(&self, asker: &str, n: &Node) -> Marks {
        let m = crate::view_gate::session_marks(self, Some(asker), &n.session_id, Veil::Clear);
        let (author, outside) = match &n.body {
            Body::AssistantMessage { .. } => (Author::Theseus, false),
            Body::ToolResult { .. } => (Author::Unknown, true),
            Body::UserMessage { .. } => (crate::view::sources_store::author_of(&self.store, &n.id), false),
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
            sealed: m.seal != Seal::Clear,
            veiled: false,
        }
    }
}
