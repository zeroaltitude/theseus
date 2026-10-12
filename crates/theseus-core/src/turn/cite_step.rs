//! A reply's citations at the turn's end (theseus-ejdk): the handles the
//! reply holds, resolved against the turn's own `Recall` node. A handle the
//! turn was not given is left as text. Each resolved handle is a
//! `memory.cited` row in the turn's next frame (no frame of its own), and its
//! source, when the place may show it, rides in the result and the reply's
//! post for the surfaces to draw. Citations only show: nothing asks or pings.

use theseus_protocol::cite::{handles_in, Citation};

use super::recall_step::defer_row;
use super::{Turn, TurnRunner};
use crate::fact::memory::MemoryCited;
use crate::places::PlaceClass;
use crate::recall::cite::item_for;

impl TurnRunner {
    /// The sources the turn's reply cites, in the order it first cites them.
    pub(super) fn cite_reply(&self, t: &mut Turn<'_>) -> Vec<Citation> {
        if t.recall.count == 0 || t.output.is_empty() {
            return Vec::new();
        }
        let shared = t.tc.class != PlaceClass::Private;
        let items = std::mem::take(&mut t.recall.items);
        let recall_id = t.recall.recall_id.clone();
        let mut cites = Vec::new();
        for handle in handles_in(&t.output) {
            let Some(item) = item_for(&items, &handle) else {
                continue;
            };
            defer_row(
                t,
                &MemoryCited {
                    recall_id: &recall_id,
                    handle: &handle,
                    node_id: &item.node_id,
                    source_session: &item.session_id,
                },
            );
            cites.extend(self.citation(item, shared));
        }
        t.recall.items = items;
        cites
    }
}
