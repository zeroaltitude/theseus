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
