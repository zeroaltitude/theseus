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
//!
//! **The hard multiple** (theseus-jse4): a limit that notifies still stops a
//! stuck loop. At `KernelConfig::spend_hard_multiple` times it (`[kernel]
//! spend_limit_hard_multiple`, 10 by default; 0 never) a provider call whose
//! reservation would take the spend, with what calls in flight hold, past
//! that multiple is refused with `KernelError::HardStop`, nothing written, as
//! a reservation over a limit that asks is: money is a gate, so the spend
//! never passes it. The core ends the turn and says why.

use crate::kernel::{Kernel, KernelError};
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

    /// Refuse a reservation of `need` for `tool` that a limit that notifies
    /// lets past `e`'s limit, when it would pass the hard multiple.
    pub fn hard_stop(&self, e: &Execution, tool: &str, need: Micros) -> Result<(), KernelError> {
        let multiple = self.config().spend_hard_multiple;
        let b = &e.budget;
        if multiple == 0 || b.limit_micros == 0 || !self.overdraws_for(e, tool) {
            return Ok(());
        }
        let cap = b.limit_micros.saturating_mul(multiple);
        let after = b
            .spent_micros
            .saturating_add(b.reserved_micros)
            .saturating_add(need);
        if after <= cap {
            return Ok(());
        }
        Err(KernelError::HardStop(Box::new(HardStop {
            multiple,
            limit: b.limit_micros,
            spent: b.spent_micros,
            held: b.reserved_micros,
            needed: need,
        })))
    }
}

/// `[kernel] spend_limit_hard_multiple`'s default: 10 times the limit.
pub fn default_hard_multiple() -> u64 {
    10
}

/// A provider call the hard multiple refused (theseus-jse4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardStop {
    pub multiple: u64,
    pub limit: Micros,
    pub spent: Micros,
    /// What its other calls in flight hold.
    pub held: Micros,
    pub needed: Micros,
}

impl HardStop {
    /// The hard stop's amount: the multiple of the limit.
    pub fn cap(&self) -> Micros {
        self.limit.saturating_mul(self.multiple)
    }
}

impl std::fmt::Display for HardStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the session's spend reached its hard stop, {} times its {} limit ({}): {} spent, {} \
             held by calls in flight, and this call needs {}, so the call was not made and \
             the turn ends; below the stop the limit only notifies, and a higher `[kernel] \
             spend_limit_usd` or `spend_limit_hard_multiple` lets the session go on from the \
             daemon's next start",
            self.multiple,
            usd(self.limit),
            usd(self.cap()),
            usd(self.spent),
            usd(self.held),
            usd(self.needed),
        )
    }
}
