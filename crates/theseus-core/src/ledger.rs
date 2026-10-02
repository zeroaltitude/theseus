//! Ledger rows. Every state transition, loop, and Advancer decision is a
//! row. In M0 rows are JSON in the embedded store. A row's kind is one of
//! the registry's (`theseus_protocol::LedgerKind`, theseus-j6qn), stored by
//! its name, so a row of a kind this build does not write still reads.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::LedgerKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRow {
    pub at_unix_ms: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl LedgerRow {
    pub fn new(
        kind: LedgerKind,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        data: Value,
    ) -> Self {
        Self {
            at_unix_ms: theseus_protocol::now_unix_ms(),
            kind: kind.as_str().into(),
            session_id: session_id.map(str::to_string),
            turn_id: turn_id.map(str::to_string),
            data,
        }
    }

    /// A row under any name, as an older build or a newer one wrote it: for
    /// a test of what a stored ledger holds. Writers name a `LedgerKind`.
    #[cfg(test)]
    pub fn named(kind: &str, session_id: Option<&str>, turn_id: Option<&str>, data: Value) -> Self {
        Self {
            kind: kind.into(),
            ..Self::new(LedgerKind::TurnStarted, session_id, turn_id, data)
        }
    }

    /// Whether this row answers a query for `kind`; a renamed kind matches
    /// under either of its names.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind == kind
            || LedgerKind::RENAMED.iter().any(|&(now, before)| {
                let now = now.as_str();
                (kind == now && self.kind == before) || (kind == before && self.kind == now)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored row keeps decoding whatever its kind (theseus-j6qn): one this
    /// build writes, one an older build wrote under a name since renamed, and
    /// one this build does not know. A row is written with its kind's name,
    /// byte for byte as before the registry.
    #[test]
    fn a_stored_row_decodes_whatever_its_kind() {
        let row = LedgerRow::new(
            LedgerKind::TurnStarted,
            Some("ses_a"),
            Some("turn_a"),
            Value::Null,
        );
        let bytes = serde_json::to_string(&row).unwrap();
        assert!(
            bytes.contains(r#""kind":"turn.started","session_id":"ses_a","turn_id":"turn_a""#),
            "{bytes}"
        );
        for (stored, reads_as) in [
            ("action.denied", Some(LedgerKind::ActionDeclined)),
            ("action.declined", Some(LedgerKind::ActionDeclined)),
            ("an.older_build", None),
        ] {
            let text = format!(r#"{{"at_unix_ms":1,"kind":"{stored}","data":{{"x":1}}}}"#);
            let r: LedgerRow = serde_json::from_str(&text).unwrap();
            assert_eq!(r.kind, stored);
            assert_eq!(LedgerKind::parse(&r.kind), reads_as);
            if reads_as == Some(LedgerKind::ActionDeclined) {
                assert!(r.is_kind("action.declined") && r.is_kind("action.denied"));
            }
        }
    }
}
