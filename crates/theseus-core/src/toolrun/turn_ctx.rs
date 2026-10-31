//! What a turn hands the tool layer for one call (`TurnCtx`), and where its
//! facts, ledger rows and nodes go. Split from `toolrun.rs`, which
//! re-exports it so every caller keeps its path.

use anyhow::Result;
use serde_json::Value;
use theseus_kernel::{Action, Kernel, TurnGuard};
use theseus_protocol::LedgerKind;

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::narrative::Narrator;
use crate::node::Node;
use crate::store::Store;

/// What a turn hands the tool layer for one call.
pub struct TurnCtx<'a> {
    pub kernel: &'a Kernel,
    pub store: &'a Store,
    pub guard: &'a TurnGuard,
    pub session_id: &'a str,
    pub execution_id: &'a str,
    pub turn_id: &'a str,
    pub loop_index: Option<u32>,
    pub sink: &'a EventSink,
    pub confirm_ttl_ms: u64,
    pub narrator: &'a Narrator,
    /// Where a card for a call that waits goes (theseus-q4v).
    pub outbox: &'a crate::outbox::Outbox,
    /// Posts whose records wait in `store` for the turn's next frame: the
    /// turn indexes them once that is written.
    pub posts: &'a std::sync::Mutex<Vec<Action>>,
    /// What the turn runs against: a task it starts runs on it too (DD7).
    pub target: Option<&'a crate::turn::Target>,
    /// Where this session came from, when it is a task (DD7): its notices
    /// name it, and it starts no tasks.
    pub task: Option<&'a crate::session::TaskOf>,
    /// The class of the place the turn's words go to (the place rule,
    /// theseus-nbsh), fixed once it has taken its wakes and reports: a
    /// shared place's calls are only the public tools (`places::refusal`).
    pub class: crate::places::PlaceClass,
    /// That place's ceiling (step 38a), taken with its class: a floor, and
    /// the tools it offers (`ceiling.rs`).
    pub ceiling: Option<&'static crate::ceiling::Ceiling>,
    /// The bound places, as a glide names them (38b).
    pub places: crate::places::Places<'a>,
    /// The session's directory, as its client sent it (theseus-aab7):
    /// where its calls work (`ToolRuntime::ctx_in`). None: `[tools] cwd`.
    pub dir: Option<&'a str>,
    /// The conversation's pending wakes as the turn last knew them: what its
    /// requests' commitments block shows (theseus-lherl), set by the turn's
    /// read of its wakes and by each wake tool's frame.
    pub commitments: &'a crate::commitments::Pending,
    /// The threads, as `thread.note` writes them (`thread_context`).
    pub threads: &'a crate::threads::Threads,
}

impl TurnCtx<'_> {
    /// Where the turn's words go: its class and its ceiling.
    pub fn place(&self) -> crate::ceiling::PlaceView {
        crate::ceiling::PlaceView {
            class: self.class,
            ceiling: self.ceiling,
            voice: false,
        }
    }
}

impl TurnCtx<'_> {
    /// Where this turn's facts go (`crate::fact`): its session and turn, its
    /// clients, and its next frame.
    pub fn rec(&self) -> crate::fact::Rec<'_> {
        crate::fact::Rec {
            narrator: self.narrator,
            session: Some(self.session_id),
            turn: Some(self.turn_id),
            to: crate::fact::To::Sink(self.sink),
            store: self.store,
        }
    }

    /// Record a fact of this turn's on each of its channels.
    pub fn record<F: crate::fact::Fact>(&self, f: &F) {
        self.rec().record(f);
    }

    /// A ledger row for this turn. It is no state transition, so it rides in
    /// the turn's next frame (theseus-qa0). A row that cannot be encoded is
    /// logged, not fatal.
    pub fn ledger(&self, kind: LedgerKind, data: Value) {
        if let Err(e) = self
            .ledger_record(kind, data)
            .and_then(|r| self.store.defer(r))
        {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    /// A ledger row for this turn, as a record for a frame the caller builds.
    pub fn ledger_record(&self, kind: LedgerKind, data: Value) -> Result<theseus_store::NewRecord> {
        theseus_store::NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &LedgerRow::new(kind, Some(self.session_id), Some(self.turn_id), data),
        )
    }

    /// Tell the session's clients about a node this turn wrote.
    pub fn node_written(&self, node: &Node) {
        self.record(&crate::fact::turn::NodeWritten {
            session_id: self.session_id,
            node,
        });
    }
}
