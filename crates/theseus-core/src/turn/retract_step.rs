//! A retraction's correction notes, written into the session at its next
//! turn (theseus-2ced). `fact.retract` queues a note for each live session
//! that used the node (`retract::Notes`); the turn that finds one waiting
//! writes it as the harness's own user-role node (as a wake's or a report's
//! is), in a frame of its own that also takes it off the queue, so it is
//! written once. A turn with none waiting reads two atomics and writes nothing.

use super::*;

impl TurnRunner {
    /// Write the notes waiting for this session, once.
    pub(super) fn read_corrections(&self, t: &mut Turn<'_>) -> Result<()> {
        let tc = &t.tc;
        if !self.memory.corrections.waiting(tc.store, tc.session_id) {
            return Ok(());
        }
        let mut nodes = Vec::new();
        let delivered = self
            .memory
            .corrections
            .deliver(tc.store, tc.session_id, |notes| {
                let mut records = Vec::new();
                for n in notes {
                    let node = crate::retract::note_node(tc.session_id, tc.turn_id, n);
                    records.push(node.record()?);
                    nodes.push(node);
                }
                Ok(records)
            })?;
        for n in &nodes {
            tc.node_written(n);
        }
        for (node, note) in nodes.iter().zip(delivered.iter().flatten()) {
            tc.record(&fact::retract::Corrected {
                node_id: &note.node_id,
                session_id: tc.session_id,
                note_id: &node.id,
                text: &note.text,
            });
        }
        Ok(())
    }
}
