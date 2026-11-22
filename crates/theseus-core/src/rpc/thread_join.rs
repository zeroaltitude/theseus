//! What the split writer (`thread_home.rs`) and the link writer
//! (`thread_link.rs`) share. Threads were retired on 2026-11-22 (issue 44):
//! `thread.merge` and `thread.unmerge` are gone, and the split and link
//! writers are kept only as fixture writers for other units' tests, until
//! the sweep.

use anyhow::anyhow;

use super::server::RpcFailure;
use super::threads::failure;
use super::Core;
use crate::approval::Answerer;

impl Core {
    /// A session opened for a split that did not happen: retired, so it is
    /// not left looking like a conversation. Best effort, and said.
    pub(super) fn drop_orphan(&self, sid: &str) {
        let now = theseus_protocol::now_unix_ms();
        let done = self.store.update_session(sid, |r| {
            r.retired = Some(theseus_protocol::sessions::SessionRetired {
                reason: theseus_protocol::sessions::RetiredReason::Empty,
                at_ms: now,
            });
            Ok(vec![])
        });
        if let Err(e) = done {
            tracing::warn!(session = %sid, error = %format!("{e:#}"), "a split's unused session was not retired");
        }
    }

    pub(super) fn judge_thread_pair(
        &self,
        who: &Answerer,
        method: &'static str,
        a: &str,
        b: &str,
    ) -> Result<(), RpcFailure> {
        let what = format!("the threads {a} and {b}");
        self.judge_act(
            who,
            super::confirms::Act::Thread {
                method,
                what: &what,
            },
        )
        .map_err(failure)
    }
}

pub(super) fn no_thread(slug: &str) -> anyhow::Error {
    anyhow!("no thread {slug}: `theseus thread list` names them")
}

/// A slug taken, or the live limit reached.
pub(super) fn refuse_taken(
    board: &crate::threads::Board,
    slug: &str,
    max_live: usize,
) -> Result<(), RpcFailure> {
    if board.get(slug).is_some() {
        return Err(RpcFailure::invalid(anyhow!("the thread {slug} exists")));
    }
    if board.open() >= max_live {
        return Err(RpcFailure::invalid(anyhow!(
            "{max_live} threads are open ([threads] max_live): finish one first (`theseus thread done`)"
        )));
    }
    Ok(())
}
