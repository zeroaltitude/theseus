//! The threads' board: what the daemon still reads of the thread records an older store holds.
//! Threads were retired on 2026-11-22 (issue 44); nothing writes a thread record, and every
//! `thread.*` method is refused.

use theseus_protocol::error_code;

use super::server::RpcFailure;
use super::Core;
use crate::threads::Board;

impl Core {
    /// Every `thread.*` method: threads were retired on 2026-11-22 (issue 44), so each is refused,
    /// and a client that still sends one is told why.
    pub(super) fn rpc_threads(&self, name: &str) -> Result<serde_json::Value, RpcFailure> {
        Err(RpcFailure::new(
            error_code::REFUSED,
            format!(
                "{name}: threads were retired on 2026-11-22; their stored records are kept, and no \
                 command reads them"
            ),
        ))
    }

    /// The threads in memory (`Board`): built by two META scans the first
    /// time, after serving (`warm_threads`) or at the first reader. Never a
    /// store read on a turn's path once built. A store that cannot be read
    /// gives an empty board, warned of, and is tried again.
    pub fn threads(&self) -> std::sync::Arc<Board> {
        self.runner.threads.all(&self.store).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "the threads cannot be read");
            Default::default()
        })
    }

    /// Read the threads now, off the start path: after serving, on the
    /// blocking pool. A reader that comes sooner reads them itself, once.
    pub fn warm_threads(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.runner.threads.all(&core.store) {
                tracing::warn!(error = %format!("{e:#}"), "the threads cannot be read");
            }
        });
    }
}
