//! A deferred task's start (theseus-rcgph): its first turn writes one node
//! before it reads anything else, `Deferred task, set <when set> for <due>;
//! started <now>`, with how late it ran when the daemon was down at the due
//! time (`task_start::header`). The node and the mark that it was written (the
//! record's `started_ms`) share one frame, so a retried first turn writes it
//! once. Every other turn pays one comparison.

use super::*;
use crate::task_start::{self, Deferred};

/// The node of a deferred task's first turn; how many nodes it wrote.
pub(super) fn start_node(t: &Turn<'_>) -> Result<u32> {
    let tc = &t.tc;
    if tc.task.is_none() || tc.guard.turn != 1 {
        return Ok(0);
    }
    let Some(mut d) = tc
        .store
        .get_meta::<Deferred>(&task_start::key(tc.session_id))?
    else {
        return Ok(0);
    };
    if d.started_ms.is_some() {
        return Ok(0);
    }
    let now = tc.kernel.now_ms();
    let started = tc.kernel.process_started_ms();
    let while_down = started > 0 && d.due_at_ms < started;
    d.started_ms = Some(now);
    let node = Node::relayed(
        tc.session_id,
        Some(tc.turn_id),
        crate::node::Origin::Harness,
        "harness:deferred",
        &task_start::header(&d, now, while_down),
    );
    tc.store
        .append(&[node.record()?, task_start::record(tc.session_id, &d)?])?;
    tc.node_written(&node);
    Ok(1)
}
