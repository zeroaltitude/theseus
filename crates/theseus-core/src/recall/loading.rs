//! The meaning search after an idle stretch (issue 21 of 1.0.7). The index
//! unloads its embedding model after ten idle minutes and takes about a
//! second to load it, and a recall that met it unloaded answered by words and
//! names alone: 235 of a month's 421 recalls did. Two things close that gap,
//! and neither makes a turn wait on a broken index:
//!
//! - **Load at the first sign of a turn.** A person's message, at its
//!   arrival (`Memory::warm_on_message`, beside Jev's connections), sends the
//!   tender `index.warm`, which starts the load if the model is unloaded. It
//!   runs in a task of its own beside the turn's other early steps, so the
//!   load is under way (or done) when recall asks. A first keystroke sends
//!   the same call earlier (`rpc/typing.rs`). Only a turn that arrives with a
//!   message sends it here, as for Jev: a person's, or a self-improvement
//!   run's objective, whose recall asks for the model anyway. The tender's
//!   idle clock restarts on it, so nothing that does not recall keeps the
//!   model loaded.
//! - **Recall's wait covers a load, up to [`LOAD_BOUND`].** A recall's whole
//!   query carries `wait_ms = LOAD_BOUND`, so the tender holds it for a load
//!   in progress instead of answering at once without vectors. The recall's
//!   own deadline (`[memory] recall_deadline_ms`, 250 ms) is longer than a
//!   loaded model's embedding and shorter than a load, so when it passes with
//!   the whole answer still out, recall asks the tender once whether the
//!   model is loading ([`covered`]) and, only if it is, waits on to
//!   `LOAD_BOUND` from the recall's start. A model already loaded (a slow
//!   embedding), a tender that is down or does not answer, and a model whose
//!   load failed all keep the ordinary deadline: the words alone answer,
//!   exactly as before. Nothing waits past `LOAD_BOUND`.
//!
//! Time to first word: a turn whose model was unloaded may now wait for the
//! load before its model call, up to `LOAD_BOUND`, less the time its other
//! early steps already spent. A turn after a keystroke, or in a warm session,
//! waits no longer than before. The design note is `docs/design/recall-warm.md`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use super::Memory;
use crate::config::MemoryMode;
use crate::tender::IndexTender;

/// The longest a recall waits for the embedding model to load, from the
/// recall's start. A load is 0.6 to 1.1 s on the owner's machine and the
/// embedding after it about 0.1 s, so two seconds covers both with room, and
/// a load that takes longer is given up on, with the words alone answering.
pub const LOAD_BOUND: Duration = Duration::from_secs(2);

/// The longest recall waits for the tender's answer to the question whether
/// the model is loading: a call on a local socket that answers at once, so a
/// tender that does not is treated as down.
const PROBE_BOUND: Duration = Duration::from_millis(100);

/// The tender's embedding model state (`loaded`, `loading`, `unloaded`, ...)
/// after an `index.warm`, or `None` when no tender answered.
pub type WarmFuture = Pin<Box<dyn Future<Output = Option<String>> + Send>>;
/// Who answers `index.warm`: the index tender, or a test's stand-in.
pub type Warm = Arc<dyn Fn() -> WarmFuture + Send + Sync>;

/// The tender's `index.warm`, which also restarts its idle clock.
pub(super) fn warm_of(t: Arc<IndexTender>) -> Warm {
    Arc::new(move || -> WarmFuture {
        let t = t.clone();
        Box::pin(async move { t.warm().await.ok().map(|w| w.model) })
    })
}

/// Whether the tender says its model is loading now, asked once. A recall
/// that finds its whole answer still out at its deadline waits on only when
/// this is true.
pub(super) async fn covered(warm: Option<&Warm>) -> bool {
    let Some(warm) = warm else { return false };
    let asked = tokio::time::timeout(PROBE_BOUND, warm()).await;
    matches!(asked, Ok(Some(m)) if m == "loading")
}

impl Memory {
    /// A person's message arrived: tell the tender to load its model now,
    /// unless one such call is out already. Never awaited by the turn: the
    /// call runs in a task of its own, and a tender that is off, down or slow
    /// costs nothing.
    pub fn warm_on_message(&self) {
        // Recall asks the vector source in `shadow` whatever the arm, and in
        // `live` only when the arm has one: an arm of words alone never needs
        // it.
        let vectors = match self.cfg.mode {
            MemoryMode::Off => false,
            MemoryMode::Shadow => true,
            MemoryMode::Live => self.cfg.arm.sources().contains(&"vector"),
        };
        if !vectors {
            return;
        }
        let Some(warm) = self.warm_ask() else { return };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.warming.swap(true, Ordering::SeqCst) {
            return;
        }
        let (out, telemetry) = (self.warming.clone(), self.telemetry.get().cloned());
        rt.spawn(async move {
            let model = warm().await;
            out.store(false, Ordering::SeqCst);
            if let Some(t) = telemetry {
                t.record_warm("tender_turn", model.as_deref().unwrap_or("down"));
            }
        });
    }

    /// Ask `warm` for the model's state instead of the tender (tests: a
    /// stand-in that loads for a while).
    pub fn set_warm(&self, warm: Warm) {
        *self.warm.write().unwrap_or_else(|e| e.into_inner()) = Some(warm);
    }

    pub(super) fn warm_ask(&self) -> Option<Warm> {
        self.warm.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
