//! A watch's frames (theseus-f2r2b; the horizon report's §5.1): the
//! kernel's side of waiting on the world with no model.
//!
//! A watch is a pending wake with the purpose `Watch` and the run
//! `Probe(WakeProbe)`; the core's `watch.rs` checks it on a timer. The kernel
//! writes only what is news:
//!
//! - **`watch.checked`**: the value changed. One frame: the row, and the
//!   watch's state (META `watch.state.<wake id>`: its first and last values,
//!   the checks made, the last failure). An unchanged check writes nothing,
//!   so the next check's time and the failure count live in memory.
//! - **`watch.met`**: the condition held. One frame: the wake leaves its
//!   execution (a `then: turn` or `then: task` puts back a turn wake that is
//!   due now, whose node reads `👁 watch met: …`), the row, the state, and,
//!   for a notice, the post.
//! - **`watch.expired`**: it gave up at its expiry. The wake leaves; the row,
//!   the state, and a post.
//! - **`watch.failing`**: a third check in a row failed. The row, the state
//!   with the failure, and a post, once; the watch stays, and its checks back
//!   off.
//!
//! Nothing here reads a clock but the kernel's, runs a check, or holds the
//! execution's lock across anything but its frame.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::term::words::cut;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::kernel::{exec_record, Kernel};
use crate::outbox::Post;
use crate::types::*;
use crate::wake_kinds::{Purpose, WakeProbe, WakeRun};

/// The META key a watch's state is kept under (`<prefix><wake id>`).
pub const STATE_PREFIX: &str = "watch.state.";

/// What a watch has seen, as its last frame left it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchState {
    /// The first value it saw: what `changes` compares with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<String>,
    /// The last value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// Checks made, up to the frame that wrote this.
    pub checks: u64,
    /// When the last of them ran.
    pub checked_ms: u64,
    /// Why the last checks failed, once three in a row had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing: Option<String>,
}

/// How a check came to be news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// The value changed.
    Changed,
    /// The condition held; the wake becomes a turn when `then` asks for one.
    Met { turn: bool },
    /// It reached its expiry unmet.
    Expired,
    /// A third failure in a row.
    Failing,
}

impl ProbeVerdict {
    fn kind(&self) -> LedgerKind {
        match self {
            ProbeVerdict::Changed => LedgerKind::WatchChecked,
            ProbeVerdict::Met { .. } => LedgerKind::WatchMet,
            ProbeVerdict::Expired => LedgerKind::WatchExpired,
            ProbeVerdict::Failing => LedgerKind::WatchFailing,
        }
    }
}

/// What the core wants written for one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeNews {
    pub verdict: ProbeVerdict,
    pub state: WatchState,
    /// The message: a met watch's node and post, an expiry's or a failure's
    /// post. Empty for a change.
    pub text: String,
    /// More fields for the verdict's ledger row (a reply's message id and
    /// author); null for none.
    pub detail: serde_json::Value,
    /// A META key and its value, kept in the same frame (a mail watch's
    /// history cursor); an empty value is the key cleared.
    pub keep: Option<(String, String)>,
}

/// What `probe_result` wrote.
#[derive(Debug, Clone)]
pub struct ProbeApplied {
    pub execution: Execution,
    /// The wake as it was before the frame.
    pub wake: PendingWake,
    /// The post staged, for the caller to announce once written.
    pub post: Option<Action>,
}

impl PendingWake {
    /// What a watch looks at, when this is one.
    pub fn probe(&self) -> Option<&WakeProbe> {
        match &self.run {
            WakeRun::Probe(p) if self.purpose == Purpose::Watch => Some(p),
            _ => None,
        }
    }
}

impl Kernel {
    /// Every open watch with its execution, soonest created first: read once
    /// when the daemon begins to check them, never by a tick.
    pub fn open_watches(&self) -> Result<Vec<(Execution, PendingWake)>> {
        let mut out: Vec<_> = self
            .pending_wakes()?
            .into_iter()
            .filter(|(_, w)| w.probe().is_some())
            .collect();
        out.sort_by_key(|(_, w)| (w.set_at_ms, w.id.clone()));
        Ok(out)
    }

    /// A watch's state as its last frame left it.
    pub fn watch_state(&self, wake_id: &str) -> Result<Option<WatchState>> {
        match self
            .store()
            .latest_by_key(kinds::META, &format!("{STATE_PREFIX}{wake_id}"))?
        {
            Some(r) => Ok(Some(r.decode::<WatchState>()?)),
            None => Ok(None),
        }
    }

    /// Write one check's news, in one frame under the execution's lock:
    /// its row, the watch's state, and, as the verdict says, the wake's
    /// leaving or turning into a turn and the post `make` builds. `None` when
    /// the wake is no longer pending (cancelled, or its conversation ended):
    /// nothing is written.
    pub fn probe_result(
        &self,
        execution_id: &str,
        wake_id: &str,
        news: &ProbeNews,
        make: impl FnOnce(&Execution, &PendingWake) -> Result<Option<Post>>,
    ) -> Result<Option<ProbeApplied>> {
        let _w = self.lock(&[execution_id]);
        let Some(mut e) = self.execution(execution_id)? else {
            return Ok(None);
        };
        let Some(i) = e
            .wakes
            .iter()
            .position(|w| w.id == wake_id && w.probe().is_some())
        else {
            return Ok(None);
        };
        let wake = e.wakes[i].clone();
        let now = self.now_ms();
        let s = &news.state;
        let mut row = json!({"execution_id": e.id, "wake_id": wake.id,
            "title": cut(&wake.note, 200), "checks": s.checks, "value": s.last.as_deref().map(|v| cut(v, 200))});
        if let Some(p) = wake.probe() {
            row["check"] = json!(p.kind);
        }
        if !news.text.is_empty() {
            row["text"] = json!(cut(&news.text, 200));
        }
        if let Some(f) = &s.failing {
            row["error"] = json!(cut(f, 200));
        }
        if let Some(d) = news.detail.as_object() {
            for (k, v) in d {
                row[k] = v.clone();
            }
        }
        let mut frame = Vec::new();
        let mut post = None;
        match &news.verdict {
            ProbeVerdict::Changed | ProbeVerdict::Failing => {}
            ProbeVerdict::Met { turn } => {
                if *turn {
                    // The watch becomes a turn of its conversation, due now: its
                    // node reads the message as it is (`wake::fired_text`).
                    let w = &mut e.wakes[i];
                    w.note = news.text.clone();
                    w.due_at_ms = now;
                    w.repeat = None;
                    w.occurrence = 0;
                    w.run = WakeRun::Turn;
                } else {
                    e.wakes.remove(i);
                }
                e.updated_at_ms = now;
                frame.push(exec_record(&e)?);
            }
            ProbeVerdict::Expired => {
                e.wakes.remove(i);
                e.updated_at_ms = now;
                frame.push(exec_record(&e)?);
            }
        }
        frame.push(self.ledger(news.verdict.kind(), Some(&e.session_id), row)?);
        frame.push(NewRecord::json(
            kinds::META,
            Some(&format!("{STATE_PREFIX}{wake_id}")),
            &news.state,
        )?);
        if let Some((key, value)) = &news.keep {
            frame.push(NewRecord::json(kinds::META, Some(key), value)?);
        }
        if let Some(p) = make(&e, &wake)? {
            let (a, recs) = self.outbox_stage(p)?;
            frame.extend(recs);
            post = Some(a);
        }
        self.commit(&frame)?;
        Ok(Some(ProbeApplied {
            execution: e,
            wake,
            post,
        }))
    }
}
