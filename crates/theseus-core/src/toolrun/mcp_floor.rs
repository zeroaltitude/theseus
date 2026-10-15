//! The floor of an MCP client's execution (step 41b), the gate's one read of
//! it, moved out of `toolrun.rs` (theseus-lyxy: that file is at its ceiling).

use theseus_kernel::Kernel;

use super::TurnCtx;
use crate::policy::Posture;

/// The floor of the turn's execution, when an MCP client opened its session
/// (step 41b, `mcp_server::floor_of`). An execution that cannot be read is
/// floored at `approve`: the gate never guesses the looser way.
pub(super) fn mcp_floor(tc: &TurnCtx<'_>) -> Option<Posture> {
    mcp_floor_of(tc.kernel, tc.execution_id)
}

/// An MCP client's floor for the calls of `execution_id` (step 41b): one
/// that cannot be read is the strictest.
pub(crate) fn mcp_floor_of(kernel: &Kernel, execution_id: &str) -> Option<Posture> {
    match kernel.execution(execution_id) {
        Ok(Some(e)) => crate::mcp_server::floor_of(&e.authority),
        Ok(None) => None,
        Err(_) => Some(Posture::Approve),
    }
}
