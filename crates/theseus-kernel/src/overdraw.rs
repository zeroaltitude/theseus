//! A spend limit that notifies (theseus-usei): with `[kernel]
//! spend_limit_mode = "notify"` (`KernelConfig::spend_limit_notify`), a
//! provider call whose reservation passes its execution's limit is still
//! planned, reserved, and recorded, never refused, and the turn that made it
//! tells the session's place once the spend reaches the limit, and once at
//! each multiple after. A task's carve is then what it asked for, up to its
//! parent's whole limit, even past what its parent has left.
//!
//! What still asks: every other reservation (an AWS hands group's), and a
//! limit its opener or a place named (`Budget::pinned`: an MCP client's
//! session, a place's ceiling). A task notifies when its parent does: its
//! carve is pinned, but it is the parent's limit, divided.

use crate::kernel::Kernel;
use crate::types::*;

impl Kernel {
    /// Whether `e`'s spend limit notifies instead of asking: the config says
    /// so, and its limit is the config's, or it is a task whose parent's is.
    pub fn overdraws(&self, e: &Execution) -> bool {
        if !self.config().spend_limit_notify {
            return false;
        }
        match (&e.parent, e.kind) {
            (Some(p), SessionKind::Task) => self
                .execution(p)
                .ok()
                .flatten()
                .is_some_and(|parent| !parent.budget.pinned),
            _ => !e.budget.pinned,
        }
    }

    /// Whether a reservation for `tool` may pass `e`'s limit: a provider
    /// call's, under a limit that notifies.
    pub(crate) fn overdraws_for(&self, e: &Execution, tool: &str) -> bool {
        tool == PROVIDER_TOOL && self.overdraws(e)
    }
}
