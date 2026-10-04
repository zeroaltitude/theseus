//! The index rows (spec §6: "the DynamoDB index", rebuildable from the
//! segments and never the truth): `BatchWriteItem`s of at most 25 puts, each
//! batch's unprocessed items sent again after a backoff until none are
//! left, or the tries run out and the pass fails (and is run again).
//!
//! Every key starts with the deployment, so one table holds several
//! Theseuses and the tender's session may write only its own
//! (`dynamodb:LeadingKeys`):
//!
//! | pk | sk | what |
//! |---|---|---|
//! | `<deployment>#wal` | `<segment>` | a sealed segment shipped whole |
//! | `<deployment>#wal` | `<segment>.tail.<from>` | a tail of the open one |
//! | `<deployment>#blob` | `<sha256>` | a blob |
//! | `<deployment>#rec#<kind>` | its key | a keyed record's latest position and segment |

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value};
use theseus_store::Record;

use super::super::session::Kind;
use super::super::{Account, Request, Signer};
use super::TENDER;

/// The most puts one `BatchWriteItem` takes.
pub const BATCH: usize = 25;

/// An item, from its attributes: a string is `S`, a number `N`.
pub fn item(pk: String, sk: String, attrs: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    m.insert("pk".into(), json!({ "S": pk }));
    m.insert("sk".into(), json!({ "S": sk }));
    for (k, v) in attrs {
        let typed = match v {
            Value::Number(n) => json!({ "N": n.to_string() }),
            Value::String(s) => json!({ "S": s }),
            Value::Bool(b) => json!({ "BOOL": b }),
            _ => continue,
        };
        m.insert((*k).to_string(), typed);
    }
    Value::Object(m)
}

/// A segment's or a tail's sort key.
pub fn wal_sk(segment: u32, tail_from: Option<u64>) -> String {
    match tail_from {
        None => format!("{segment:09}"),
        Some(from) => format!("{segment:09}.tail.{from:012}"),
    }
}

/// Each keyed record's row, the latest of a key alone (a batch must not
/// name one key twice), with the segment that holds it: `spans` and `ends`
/// as the follower read them, `after` the last position already written.
pub fn record_rows(
    deployment: &str,
    records: &[Record],
    spans: &[(u32, u64, u64)],
    ends: &[u64],
    after: u64,
) -> Vec<Value> {
    let mut latest: BTreeMap<(String, String), Value> = BTreeMap::new();
    let mut i = 0usize;
    for r in records {
        while i + 1 < ends.len() && r.position > ends[i] {
            i += 1;
        }
        let Some(key) = r.key.as_deref().filter(|_| r.position > after) else {
            continue;
        };
        let kind = theseus_store::kinds::name(r.kind);
        let mut attrs = vec![
            ("position", json!(r.position)),
            ("at_unix_ms", json!(r.at_unix_ms)),
        ];
        if let Some((seg, ..)) = spans.get(i) {
            attrs.push(("segment", json!(seg)));
        }
        if let Some(s) = &r.scope {
            attrs.push(("scope", json!(s)));
        }
        let pk = format!("{deployment}#rec#{kind}");
        latest.insert(
            (pk.clone(), key.to_string()),
            item(pk, key.to_string(), &attrs),
        );
    }
    latest.into_values().collect()
}

/// The table, as the tender writes it.
pub struct Table {
    pub account: Arc<Account>,
    pub name: String,
    pub region: String,
    /// Tries of one batch's unprocessed items, and the first backoff
    /// (doubled each time).
    pub tries: u32,
    pub backoff: Duration,
}

impl Table {
    /// Write every item, 25 to a request, retrying what DynamoDB left
    /// unprocessed. The rows written.
    pub async fn write(&self, items: Vec<Value>) -> Result<u64, String> {
        let mut written = 0u64;
        for chunk in items.chunks(BATCH) {
            let mut pending: Vec<Value> = chunk
                .iter()
                .map(|i| json!({"PutRequest": {"Item": i}}))
                .collect();
            let mut wait = self.backoff;
            let mut tries = 0u32;
            while !pending.is_empty() {
                let input = json!({"RequestItems": {self.name.clone(): pending.clone()}});
                let r = Request {
                    service: "dynamodb",
                    operation: "BatchWriteItem",
                    input: &input,
                    region: &self.region,
                    pages: 1,
                    class: "write",
                    signer: Signer::As(Kind::Tender(TENDER)),
                };
                let out = self
                    .account
                    .request(None, &r)
                    .await
                    .map_err(|f| format!("dynamodb BatchWriteItem: {f}"))?;
                let left: Vec<Value> = out.body["UnprocessedItems"][&self.name]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                written += (pending.len() - left.len().min(pending.len())) as u64;
                pending = left;
                if pending.is_empty() {
                    break;
                }
                tries += 1;
                if tries >= self.tries {
                    return Err(format!(
                        "dynamodb BatchWriteItem left {} items unprocessed after {tries} tries",
                        pending.len()
                    ));
                }
                tokio::time::sleep(wait).await;
                wait = wait.saturating_mul(2);
            }
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(position: u64, kind: u16, key: Option<&str>) -> Record {
        Record {
            position,
            kind,
            schema: 0,
            key: key.map(String::from),
            scope: None,
            at_unix_ms: 1000 + position,
            payload: Vec::new(),
        }
    }

    #[test]
    fn a_keys_latest_record_is_its_row_with_the_segment_that_holds_it() {
        use theseus_store::kinds::{LEDGER, SESSION};
        let records = vec![
            rec(1, SESSION, Some("s1")),
            rec(2, LEDGER, None),
            rec(3, SESSION, Some("s2")),
            rec(4, SESSION, Some("s1")),
        ];
        // Positions 1 and 2 in segment 1, 3 and 4 in segment 2.
        let spans = [(1, 100, 200), (2, 0, 80)];
        let rows = record_rows("dep", &records, &spans, &[2, 4], 0);
        assert_eq!(rows.len(), 2, "one row per key, and none for a ledger row");
        let s1 = rows.iter().find(|r| r["sk"]["S"] == "s1").unwrap();
        assert_eq!(s1["pk"]["S"], "dep#rec#session");
        assert_eq!(s1["position"]["N"], "4");
        assert_eq!(s1["segment"]["N"], "2");
        // Already written through position 3: only s1's newer row is left.
        let rows = record_rows("dep", &records, &spans, &[2, 4], 3);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["position"]["N"], "4");
    }
}
