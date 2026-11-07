//! One lock for every owner-card proposal write (theseus-kxb8d).
//!
//! A proposal is a META record `card.proposal.<id>` written again whole (the
//! latest wins, `state.rs`), so every writer is a read-modify-write: it reads
//! the proposals, changes one, and appends it. Four writers do that — the
//! owner's answer (`card_settle`), expiry (`card_expire`), the DM post that
//! stamps `posted_ms` (`card_dm_write`), and the two passes that add
//! questions (`run.rs`, `live.rs`) — and before this row none of them
//! excluded the others:
//!
//! - `pass.writing()` ([`crate::memory_pass::turns::Writing`]) counts writers,
//!   it does not exclude them. Its job is to keep a frame out of a running
//!   turn, and `turns.rs` says so: "Writers are counted". Two card writers
//!   each take one happily and append in either order.
//! - `run::RUNNING` is a `try_lock` on a process-global static that only keeps
//!   the nightly run and the live pass off each other. The answer path and the
//!   DM path take nothing at all.
//!
//! So in a millisecond window the second appender wrote back a record it had
//! read before the first appender changed it, and the first write was
//! reverted: an answer back to Open (and posted), or `posted_ms` lost, so the
//! day gave one extra post.
//!
//! [`Core::card_frame`] is the whole fix, and it is the shape
//! [`crate::standing::Board::write`] already uses for the records' board: the
//! proposals are read **again under the lock**, the frame is built from that
//! read, and it is appended before the lock is released. The lock is a plain
//! `std::sync::Mutex` taken inside [`theseus_store::blocking`] and **never
//! held across an `.await`**, which is what keeps it from deadlocking against
//! the pass handshake: that handshake can wait up to
//! [`crate::memory_pass::BUSY_BOUND`] (600 s), and a lock that spanned it
//! would stall the owner's next answer for ten minutes.
//!
//! Every writer therefore takes the two in one order — `pass.writing()`
//! first, `card_writes` second — and a builder that needs the fresh
//! proposals gets them as the closure's argument rather than reading the
//! store again itself.

use anyhow::Result;
use theseus_protocol::CardProposal;
use theseus_store::NewRecord;

use super::state;
use crate::rpc::Core;

impl Core {
    /// Build one card frame from the proposals as they are **now** and append
    /// it, with no other card writer in between (theseus-kxb8d).
    ///
    /// `build` is given every proposal, read under the lock, and returns the
    /// records to append and whatever the caller wants back. An empty frame
    /// appends nothing. `build` must not call another card writer: the lock is
    /// not reentrant, as a kernel transition must use the locked inner form
    /// rather than call a second public transition.
    pub(crate) async fn card_frame<T>(
        &self,
        build: impl FnOnce(&[CardProposal]) -> Result<(Vec<NewRecord>, T)>,
    ) -> Result<T> {
        let w = self.runner.pass.writing().await;
        let out = theseus_store::blocking(|| {
            let _g = self
                .card_writes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let all = state::proposals(&self.store)?;
            let (frame, out) = build(&all)?;
            if !frame.is_empty() {
                self.store.append(&frame)?;
            }
            Ok(out)
        });
        drop(w);
        out
    }
}
