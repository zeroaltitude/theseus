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
    /// The Discord binding's places, from its bindings file (the place rule,
    /// theseus-nbsh): each one's class follows from them. Told as the binding
    /// starts, before it reads a message from any of them; until then a guild
    /// place is shared.
    pub fn bind_places(&self, places: Vec<crate::places::BoundPlace>) {
        // Each place's given category, at its first bind (theseus-8kk.1).
        if let Err(e) = self.bind_categories(&places) {
            tracing::warn!(error = %format!("{e:#}"), "the places' categories were not made");
        }
        // A ceiling's family that names no tool here offers nothing (step
        // 38a): the binding's start says so, in health too, with the rest of
        // what it finds (`place_warnings`, theseus-ext.11).
        self.runner.place_rule.bind(places);
    }

    /// The guilds the bindings file trusts whole (theseus-rdqg; each has its
    /// own word, step 38a), by id: told with its places, so health names each
    /// private channel there as in a trusted guild, which is never read.
    pub fn trust_guilds(&self, trusted: std::collections::BTreeSet<String>) {
        self.runner.place_rule.trust_guilds(trusted);
    }

    /// Who can view a guild channel bound `private = true`, outside a trusted
    /// guild, as the binding read it at its start (the place rule's one
    /// check): everyone who can, the bot aside, by id and name, or why that
    /// cannot be read. Health warns while anyone besides the owner can;
    /// recorded once a start.
    pub fn private_place_viewed(
        &self,
        channel: u64,
        name: &str,
        viewers: std::result::Result<Vec<(u64, String)>, String>,
    ) {
        let owners = self.runner.place_rule.owners(&self.cfg);
        let viewed = match viewers {
            Ok(v) => crate::places::Viewed::Others(
                v.into_iter()
                    .filter(|(id, _)| !owners.contains(&format!("discord:{id}")))
                    .map(|(_, name)| name)
                    .collect(),
            ),
            Err(why) => crate::places::Viewed::Unread(why),
        };
        let place = format!("discord:channel:{channel}");
        if let crate::places::Viewed::Others(o) = &viewed {
            if !o.is_empty() {
                tracing::warn!(place, others = ?o,
                    "a channel bound private can be viewed by people besides the owner");
            }
        }
        self.rec(None).record(&crate::fact::place::PlaceViewed {
            place: &place,
            name,
            viewed: &viewed,
        });
        self.runner.place_rule.viewed(&place, viewed);
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

    /// `binding_ledger` for a row on the path a person waits on
    /// (theseus-ck0n): a message out, before its lane's next write. The row
    /// is appended on a blocking thread and the lane goes on, so its frame's
    /// sync (1.4 s under a neighbour's IO, 2026-10-04) never holds the
    /// reply's next edit; rows queued together share one sync. It still takes
    /// the store's writer, so a frame queued behind it waits as before
    /// (a message in, written just before its turn's admission frame, gains
    /// nothing from this, and stays `binding_ledger`). Outside a runtime it is
    /// written at once.
    pub fn binding_ledger_soon(&self, kind: LedgerKind, session_id: Option<&str>, data: Value) {
        let row = LedgerRow::new(kind, session_id, None, data);
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            if let Err(e) = self.store.append_ledger(&row) {
                tracing::warn!(error = %e, kind = kind.as_str(), "binding ledger append failed");
            }
            return;
        };
        let store = self.store.clone();
        rt.spawn_blocking(move || {
            if let Err(e) = store.append_ledger(&row) {
                tracing::warn!(error = %e, kind = kind.as_str(), "binding ledger append failed");
            }
        });
    }
}
