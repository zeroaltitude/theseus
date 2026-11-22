//! Whether a source session may be read by a summary's model. Anything goes (2026-11-22): every
//! session may; no seal, pending seal or veil holds one.

use crate::rpc::Core;

impl Core {
    /// Whether session `sid`'s words are kept from a summary's model. Anything goes (2026-11-22): never.
    #[allow(clippy::unused_self)] // a pass-through until its callers go (the sweep)
    pub(crate) fn ladder_hidden(&self, _sid: &str) -> bool {
        false
    }
}
