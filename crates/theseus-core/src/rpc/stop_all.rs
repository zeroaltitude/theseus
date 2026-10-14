//! `execution.stop` with `tasks` (theseus-cdcwr): stop a conversation, then cancel every task under it.
//!
//! `/stop` keeps its meaning (the conversation's own work; its tasks run on). This is the other verb: it stops the
//! conversation exactly as `/stop` does, then cancels the tree of tasks below it, depth first, so a task's own tasks
//! and jobs are gone before it is. Each cancel is `task.cancel`'s (`Core::cancel_execution_judged`), authored by the
//! caller, in a frame of its own. A wake the conversation set stays: it is a commitment, not running work (a stop
//! never touches the conversation's wakes). A task asleep until its start time is work the conversation started, so
//! it is cancelled with the rest. The walk reads the kernel's task index, one lookup per task, never a scan.

use std::collections::HashSet;

use anyhow::Result;

use super::Core;
use crate::fact;

/// Rounds of the walk at most: a running task may open a task between the walk and its own cancel, and the next
/// round takes it. A task cancelled opens no more.
const ROUNDS: usize = 4;

impl Core {
    /// `/stop` for `id`, then the cancel of every task under it. The result is `stop_execution`'s, with `cancelled`
    /// naming the tasks this call cancelled (execution ids, deepest first) and `tasks_running` what is left.
    pub async fn stop_execution_all(
        &self,
        id: &str,
        by: &str,
    ) -> Result<theseus_protocol::ExecutionStopResult> {
        let mut result = self.stop_execution(id, by).await?;
        let mut cancelled: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for _ in 0..ROUNDS {
            // Children before the task they belong to; one read of the task index per task.
            let live: Vec<String> = self
                .kernel
                .task_tree(id)?
                .into_iter()
                .filter(|t| !t.state.is_terminal() && seen.insert(t.id.clone()))
                .map(|t| t.id)
                .collect();
            if live.is_empty() {
                break;
            }
            for task in live {
                self.cancel_execution_judged(&task, by).await?;
                cancelled.push(task);
            }
        }
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
        result.tasks_running = self
            .kernel
            .tasks(Some(id))?
            .iter()
            .filter(|t| !t.state.is_terminal())
            .count() as u32;
        self.session_rec(&e.session_id)
            .record(&fact::driver::ExecutionStoppedAll {
                execution: &e,
                by,
                cancelled: &cancelled,
            });
        result.cancelled = cancelled;
        Ok(result)
    }
}
