//! The index tender's facts (roadmap row 51): what its supervisor
//! (`crate::tender`) records of the tender it runs. Each is an
//! `index.tender` row and nothing else: no notification, no sentence, and no
//! span, since the supervisor runs outside every turn. Its hook writes each
//! row off the runtime's workers (`Core::build`'s `index_ledger`).

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// A tender started: the first, or the next after its backoff.
pub struct TenderStarted<'a> {
    pub pid: u32,
    pub restarts: u64,
    pub binary: &'a Path,
}

impl Fact for TenderStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::IndexTender);

    fn row(&self) -> Value {
        json!({"event": "started", "pid": self.pid, "restarts": self.restarts,
            "binary": self.binary.display().to_string()})
    }
}

/// The tender this daemon's last image started, taken over after a restart
/// in place.
pub struct TenderAdopted {
    pub pid: u32,
}

impl Fact for TenderAdopted {
    const KIND: Option<LedgerKind> = Some(LedgerKind::IndexTender);

    fn row(&self) -> Value {
        json!({"event": "adopted", "pid": self.pid})
    }
}

/// A kept tender's `[index]` settings changed: it is sent SIGTERM, and the
/// next starts with the new ones.
pub struct TenderSettingsChanged {
    pub pid: u32,
}

impl Fact for TenderSettingsChanged {
    const KIND: Option<LedgerKind> = Some(LedgerKind::IndexTender);

    fn row(&self) -> Value {
        json!({"event": "settings_changed", "pid": self.pid})
    }
}

/// The tender ended (`how`: its exit, its signal, `held`, or `gone`), after
/// running for `ran`; the next starts after `backoff`.
pub struct TenderExited<'a> {
    pub pid: u32,
    pub how: &'a str,
    pub ran: Duration,
    pub backoff: Duration,
}

impl Fact for TenderExited<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::IndexTender);

    fn row(&self) -> Value {
        json!({"event": "exited", "pid": self.pid, "how": self.how,
            "ran_ms": self.ran.as_millis() as u64, "backoff_ms": self.backoff.as_millis() as u64})
    }
}

/// A start that failed (`why`); the next try is after `backoff`.
pub struct TenderStartFailed<'a> {
    pub why: &'a str,
    pub backoff: Duration,
}

impl Fact for TenderStartFailed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::IndexTender);

    fn row(&self) -> Value {
        json!({"event": "failed", "why": self.why, "backoff_ms": self.backoff.as_millis() as u64})
    }
}

/// An index rebuild's rows (theseus-jn17m): `index.rebuild`, one per event
/// and nothing else (no sentence, no notification). The work board reads
/// them as the daemon's own job, `Rebuilding the index`: `requested` starts
/// it, `progress` carries the tender's backfill (WAL bytes done of the
/// total), `finished` and `failed` end it. `run` is the request's time, so
/// two rebuilds are two jobs.
pub struct Rebuild<'a> {
    pub run: u64,
    pub event: &'a str,
    pub by: Option<&'a str>,
    pub done_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub why: Option<&'a str>,
}

impl Fact for Rebuild<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::IndexRebuild);

    fn row(&self) -> Value {
        let mut row = json!({"event": self.event, "run": self.run});
        if let Some(b) = self.by {
            row["by"] = json!(b);
        }
        if let (Some(d), Some(t)) = (self.done_bytes, self.total_bytes) {
            row["done_bytes"] = json!(d);
            row["total_bytes"] = json!(t);
        }
        if let Some(w) = self.why {
            row["why"] = json!(w);
        }
        row
    }
}
