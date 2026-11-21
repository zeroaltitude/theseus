//! Whether a source session may be read by a summary's model: only when it
//! is PROVEN neither sealed nor veiled. Anything else is held out and its
//! exchanges are folded by code: a threads board that cannot be read, a
//! session record that is missing or does not read, a thread named by a
//! session's provenance that the board does not hold, a seal or a proposed
//! seal on that thread or on the thread it was merged into, and an imported
//! conversation (whose labels the ladder does not read).
//!
//! The rule is the gate's, one rule for every reader (`view_gate::seal_of`,
//! and its durable provenance `view_gate::thread_of`): the ladder asks it
//! with no asker, so no thread's own exception applies.

use theseus_memory::gate::Seal;

use crate::rpc::Core;

impl Core {
    /// Whether session `sid`'s words are kept from a summary's model: true
    /// unless it is proven neither sealed nor veiled (above).
    pub(crate) fn ladder_hidden(&self, sid: &str) -> bool {
        if crate::import::is_imported(sid) {
            return true;
        }
        crate::view_gate::seal_of(&self.runner, None, sid) != Seal::Clear
    }
}
