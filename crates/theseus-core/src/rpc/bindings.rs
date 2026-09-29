//! Channel bindings (M3c): each binding's latest status, which health shows,
//! and how many are still starting, which a continuation turn waits on so the
//! binding watches that turn from its first event.

use std::collections::BTreeMap;
use std::sync::RwLock;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::BindingStatus;
use tokio::sync::watch;

use super::Core;
use crate::ledger::LedgerRow;

pub struct BindingBoard {
    status: RwLock<BTreeMap<String, BindingStatus>>,
    /// Bindings expected and not yet started.
    starting: watch::Sender<u32>,
}

impl Default for BindingBoard {
    fn default() -> Self {
        Self {
            status: RwLock::default(),
            starting: watch::Sender::new(0),
        }
    }
}

impl BindingBoard {
    /// A binding is about to start: continuations wait for it (see `wait`).
    pub fn expect(&self) {
        self.starting.send_modify(|n| *n += 1);
    }

    /// A binding is watching its sessions (or gave up): continuations may run.
    pub fn started(&self) {
        self.starting.send_modify(|n| *n = n.saturating_sub(1));
    }

    /// Wait until every expected binding has started, at most `max`. True when
    /// they all did; false on timeout (the caller goes ahead anyway).
    pub async fn wait(&self, max: Duration) -> bool {
        let mut starting = self.starting.subscribe();
        let done = tokio::time::timeout(max, starting.wait_for(|n| *n == 0)).await;
        matches!(done, Ok(Ok(_)))
    }

    /// A binding reports its state; health shows the latest report.
    pub fn set(&self, status: BindingStatus) {
        self.status
            .write()
            .unwrap()
            .insert(status.kind.clone(), status);
    }

    /// Every binding's latest report, by kind.
    pub fn all(&self) -> Vec<BindingStatus> {
        self.status.read().unwrap().values().cloned().collect()
    }
}

impl Core {
    /// The Discord binding checked who can view a guild channel listed in
    /// `[approval]` (theseus-sgh). An answer from there is judged against the
    /// latest check; a change of verdict is ledgered and narrated.
    pub fn approval_checked(&self, channel: u64, checked: crate::approval::Checked) {
        let (trusted, detail) = (checked.trusted, checked.detail.clone());
        if !self.approval.report(channel, checked) {
            return;
        }
        tracing::info!(channel, trusted, detail = %detail, "approval: Discord channel checked");
        self.binding_ledger(
            "approval.channel_checked",
            None,
            serde_json::json!({"channel": format!("discord:{channel}"), "trusted": trusted, "detail": detail}),
        );
        crate::narrative::narrate!(
            self.narrator,
            Approval,
            None,
            None,
            "Discord channel {channel} is {} for approvals: {detail}.",
            if trusted { "trusted" } else { "not trusted" }
        );
    }

    /// A ledger row written on behalf of a binding (`discord.*`), so its traffic
    /// sits in the same readable history as everything else.
    pub fn binding_ledger(&self, kind: &str, session_id: Option<&str>, data: Value) {
        if let Err(e) = self
            .store
            .append_ledger(&LedgerRow::new(kind, session_id, None, data))
        {
            tracing::warn!(error = %e, kind, "binding ledger append failed");
        }
    }
}
