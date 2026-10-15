//! The proposal a confirm binds (moved whole from toolrun.rs, theseus-n5ui2's join: its line limit).

use anyhow::{anyhow, Result};
use theseus_kernel::{Action, Proposal};

use crate::node::{Body, Node};
use crate::store::Store;

/// The proposal a confirm binds and `authorize` re-checks: the action's own,
/// or, for an action planned before actions kept it (theseus-0g4), the one on
/// its tool-call node's gate record. `node` saves the transcript scan when the
/// caller has the node.
pub fn confirm_proposal(store: &Store, a: &Action, node: Option<&Node>) -> Result<Proposal> {
    if let Some(p) = &a.proposal {
        return Ok(p.clone());
    }
    let found;
    let node = match node {
        Some(n) => n,
        None => {
            found = store
                .session_nodes(&a.session_id)?
                .into_iter()
                .map(|(_, n)| n)
                .find(|n| {
                    matches!(&n.body, Body::ToolCall { correlation_id: Some(c), .. }
                        if *c == a.correlation_id)
                });
            found
                .as_ref()
                .ok_or_else(|| anyhow!("no tool call node for {}", a.correlation_id))?
        }
    };
    let Body::ToolCall { gate, .. } = &node.body else {
        return Err(anyhow!("no tool call node for {}", a.correlation_id));
    };
    gate.as_ref()
        .map(|g| g.proposal.clone())
        .ok_or_else(|| anyhow!("no gate record for {}", a.correlation_id))
}
