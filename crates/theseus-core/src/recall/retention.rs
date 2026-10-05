//! FSRS-6 retention from the memory rows (M6 step 32a's wire-in; design
//! §2.7): each row the memory pass and the operator write becomes the
//! `AccessEvent` it is (`event_of`, pure), at the row's time and in its
//! position's order.
//!
//! | Row | Event | Review |
//! |---|---|---|
//! | `memory.labeled` (scope `memory:<session>`), by `durability` | first sight | Easy, Good, Hard, Again |
//! | `memory.used` (scope `recall:<session>`), `used` with its `outcome` | used | Good (`ok`), Hard (`unknown`), Again (`corrected`) |
//! | `memory.used`, not used | shown | none: exposure is no review |
//! | `memory.label` (scope `memory`) | labeled | Easy (`useful`, `should_have`), Again (`wrong`, `stale`) |
//!
//! `should_have` grades Easy: the operator says recall missed a node that
//! would have helped, the same vouching as `useful` (and §2.9's strongest
//! silver label), and Easy raises the node's stability, so that `+retention`
//! ranks it higher the next time: the remedy for a miss.

use theseus_memory::{Access, AccessEvent, Durability, Label, Outcome};
use theseus_protocol::LedgerKind;

use crate::ledger::LedgerRow;

/// The ledger kinds a node's retention is folded from.
pub const KINDS: [LedgerKind; 3] = [
    LedgerKind::MemoryLabeled,
    LedgerKind::MemoryUsed,
    LedgerKind::MemoryLabel,
];

/// The node a memory row is about, and the event it is, at the row's time;
/// `None` for a row of another kind, or one that does not read.
pub fn event_of(row: &LedgerRow) -> Option<(String, AccessEvent)> {
    let d = &row.data;
    let node = d["node_id"].as_str().filter(|n| !n.is_empty())?;
    let access = match row.kind.as_str() {
        k if k == LedgerKind::MemoryLabeled.as_str() => {
            Access::FirstSight(durability(d["durability"].as_str()?)?)
        }
        k if k == LedgerKind::MemoryUsed.as_str() => match d["used"].as_bool()? {
            false => Access::Shown,
            // A used item's row is written once its outcome is known; one
            // without reads as neither gone on nor corrected.
            true => Access::Used(
                d["outcome"]
                    .as_str()
                    .map_or(Some(Outcome::Unknown), outcome)?,
            ),
        },
        k if k == LedgerKind::MemoryLabel.as_str() => Access::Labeled(label(d["label"].as_str()?)?),
        _ => return None,
    };
    Some((
        node.to_string(),
        AccessEvent {
            at_ms: row.at_unix_ms,
            access,
        },
    ))
}

fn durability(s: &str) -> Option<Durability> {
    Some(match s {
        "high" => Durability::High,
        "medium" => Durability::Medium,
        "low" => Durability::Low,
        "floor" => Durability::Floor,
        _ => return None,
    })
}

fn outcome(s: &str) -> Option<Outcome> {
    Some(match s {
        "ok" => Outcome::Ok,
        "unknown" => Outcome::Unknown,
        "corrected" => Outcome::Corrected,
        _ => return None,
    })
}

fn label(s: &str) -> Option<Label> {
    Some(match s {
        "useful" => Label::Useful,
        "should_have" => Label::ShouldHave,
        "wrong" => Label::Wrong,
        "stale" => Label::Stale,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_memory::Grade;

    use super::*;

    fn row(kind: LedgerKind, data: serde_json::Value) -> LedgerRow {
        LedgerRow {
            at_unix_ms: 1_700_000_000_000,
            ..LedgerRow::new(kind, Some("ses_wren"), None, data)
        }
    }

    /// Each row to its event, row by row, and each event to its review by
    /// §2.7's table; a row that does not read is no event.
    #[test]
    fn each_memory_row_is_its_event_and_review() {
        use Access::{FirstSight as Seen, Labeled, Shown, Used};
        use Durability::{Floor, High, Low, Medium};
        use Grade::{Again, Easy, Good, Hard};
        use LedgerKind::{MemoryLabel as L, MemoryLabeled as F, MemoryUsed as U};
        let table = [
            (F, r#"{"durability": "high"}"#, Some(Seen(High)), Some(Easy)),
            (
                F,
                r#"{"durability": "medium"}"#,
                Some(Seen(Medium)),
                Some(Good),
            ),
            (F, r#"{"durability": "low"}"#, Some(Seen(Low)), Some(Hard)),
            (
                F,
                r#"{"durability": "floor"}"#,
                Some(Seen(Floor)),
                Some(Again),
            ),
            (F, r#"{"durability": "eternal"}"#, None, None),
            (F, r#"{}"#, None, None),
            (U, r#"{"used": false, "outcome": null}"#, Some(Shown), None),
            (
                U,
                r#"{"used": true, "outcome": "ok"}"#,
                Some(Used(Outcome::Ok)),
                Some(Good),
            ),
            (
                U,
                r#"{"used": true, "outcome": "unknown"}"#,
                Some(Used(Outcome::Unknown)),
                Some(Hard),
            ),
            (
                U,
                r#"{"used": true, "outcome": "corrected"}"#,
                Some(Used(Outcome::Corrected)),
                Some(Again),
            ),
            (
                U,
                r#"{"used": true}"#,
                Some(Used(Outcome::Unknown)),
                Some(Hard),
            ),
            (U, r#"{"used": true, "outcome": "praised"}"#, None, None),
            (U, r#"{"outcome": "ok"}"#, None, None),
            (
                L,
                r#"{"label": "useful"}"#,
                Some(Labeled(Label::Useful)),
                Some(Easy),
            ),
            (
                L,
                r#"{"label": "should_have"}"#,
                Some(Labeled(Label::ShouldHave)),
                Some(Easy),
            ),
            (
                L,
                r#"{"label": "wrong"}"#,
                Some(Labeled(Label::Wrong)),
                Some(Again),
            ),
            (
                L,
                r#"{"label": "stale"}"#,
                Some(Labeled(Label::Stale)),
                Some(Again),
            ),
            (L, r#"{"label": "remember"}"#, None, None),
            (
                LedgerKind::MemoryGated,
                r#"{"decision": "store"}"#,
                None,
                None,
            ),
        ];
        for (kind, data, access, grade) in table {
            let mut data: serde_json::Value = serde_json::from_str(data).unwrap();
            data["node_id"] = json!("nod_wren1");
            let got = event_of(&row(kind, data.clone())).map(|(n, e)| (n, e.access));
            let want = access.map(|a| ("nod_wren1".to_string(), a));
            assert_eq!(got, want, "{kind:?} {data}");
            assert_eq!(access.and_then(Access::grade), grade, "{kind:?} {data}");
        }
        // The row's time is the event's, and a row with no node is none.
        let (_, ev) =
            event_of(&row(L, json!({"node_id": "nod_wren1", "label": "useful"}))).unwrap();
        assert_eq!(ev.at_ms, 1_700_000_000_000);
        assert_eq!(event_of(&row(L, json!({"label": "useful"}))), None);
        assert_eq!(
            event_of(&row(L, json!({"node_id": "", "label": "useful"}))),
            None
        );
    }
}
