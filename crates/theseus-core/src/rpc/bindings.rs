//! Channel bindings (M3c): each binding's latest status, which health shows.
//!
//! Nothing waits for a binding to start (theseus-q4v): what must reach its
//! channels is in the outbox, and it delivers that when it can.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde_json::Value;
use theseus_protocol::{BindingStatus, LedgerKind};

use super::Core;
use crate::ledger::LedgerRow;

#[derive(Default)]
pub struct BindingBoard {
    status: RwLock<BTreeMap<String, BindingStatus>>,
}

impl BindingBoard {
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
            LedgerKind::ApprovalChannelChecked,
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

    /// The Discord binding read who can view a guild channel (M4 19a): the
    /// audience of the session there. `viewers` is every member who can view
    /// it, the bot aside, or None when they cannot be read (`why`), and then
    /// the channel counts as public. Kept in META, and recorded, only when
    /// it changed; the session's next turn recompiles for it (`audience`).
    pub fn place_viewers(
        &self,
        channel: u64,
        name: Option<String>,
        viewers: Option<Vec<u64>>,
        why: Option<&str>,
    ) {
        let place = format!("discord:{channel}");
        let read = crate::labels::PlaceViewers {
            name,
            viewers: viewers.map(|v| v.into_iter().map(|u| format!("discord:{u}")).collect()),
        };
        match self.runner.places.set(&self.store, &place, read.clone()) {
            Ok(false) => {}
            Ok(true) => self.rec(None).record(&crate::fact::label::AudienceRead {
                place: &place,
                read: &read,
                why,
            }),
            Err(e) => tracing::warn!(place, error = %format!("{e:#}"),
                "a place's viewers could not be kept: its sessions keep the audience they had"),
        }
    }

    /// A ledger row written on behalf of a binding (`discord.*`), so its traffic
    /// sits in the same readable history as everything else.
    pub fn binding_ledger(&self, kind: LedgerKind, session_id: Option<&str>, data: Value) {
        if let Err(e) = self
            .store
            .append_ledger(&LedgerRow::new(kind, session_id, None, data))
        {
            tracing::warn!(error = %e, kind = kind.as_str(), "binding ledger append failed");
        }
    }
}
