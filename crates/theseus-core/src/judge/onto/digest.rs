//! The morning note lists the night's ontology writes (theseus-0umqz): each
//! write the sweep or the dream made in the last day, one line, with the id
//! `theseus ontology undo` takes. A person who reads the note can undo any of
//! it without opening the ledger.
//!
//! - **From the rows alone.** A machine write's rows carry what undoes it
//!   (`fact::ontology::Undoable`: the write's id, the record as written and as
//!   it was), so [`lines`] is pure over them and needs no row of its own
//!   (`ontology.swept` is not written): a merge is a category row with
//!   `merged_into`, a split two new categories with a parent, a kind a kind
//!   row, a decay memberships that shrank, guidance a guidance row.
//! - **An undone write** says it was undone, from its `ontology.undo` row.
//! - **The model never rewords the ids**: `morning_note` appends these lines
//!   after the voiced words, verbatim, and so does its stand-in.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use theseus_ontology::CategoryId;
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page, Store as _};

use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// How far back the note reads: a day.
pub const DAY_MS: u64 = 86_400_000;

/// The ledger kinds a machine write's rows are, and the undo that takes one
/// back.
const KINDS: [LedgerKind; 5] = [
    LedgerKind::OntologyCategory,
    LedgerKind::OntologyMembership,
    LedgerKind::OntologyGuidance,
    LedgerKind::OntologyKind,
    LedgerKind::OntologyUndo,
];

/// The rows of one machine write.
struct Write<'a> {
    id: &'a str,
    /// Where its first row is in the ledger's order.
    first: usize,
    rows: Vec<&'a LedgerRow>,
}

/// The write a row belongs to, when it is the sweep's or the dream's.
fn write_of(row: &LedgerRow) -> Option<&str> {
    let w = row.data["undo"]["write"].as_str()?;
    (w.starts_with("sweep-") || w.starts_with("dream-")).then_some(w)
}

fn quoted(s: &str) -> String {
    format!("\"{s}\"")
}

/// A count and its noun.
fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// The words of one write, from its rows. `name_of` finds a category's name
/// by its id, as the ontology holds it now.
fn describe(w: &Write, name_of: &dyn Fn(&str) -> Option<String>) -> String {
    let name = |id: &str| name_of(id).unwrap_or_else(|| id.to_string());
    let of = |kind: &'static str| w.rows.iter().filter(move |r| r.kind == kind);
    let sessions = of("ontology.membership").count();
    let word = w.id.rsplit('-').next().unwrap_or_default();
    if w.id.starts_with("dream-") {
        let id = of("ontology.guidance")
            .find_map(|r| r.data["category"].as_str())
            .unwrap_or_default();
        let kind = id.split(':').next().unwrap_or_default();
        return format!("wrote guidance for {kind} {}", quoted(&name(id)));
    }
    match word {
        "kind" => {
            let made = of("ontology.kind")
                .find_map(|r| r.data["name"].as_str())
                .unwrap_or_default();
            format!(
                "made the kind {} from {}",
                quoted(made),
                count(sessions, "session", "sessions")
            )
        }
        "merge" => {
            let gone = of("ontology.category").find(|r| !r.data["merged_into"].is_null());
            let (kind, absorbed, into) = gone.map_or(("", "", ""), |r| {
                (
                    r.data["kind"].as_str().unwrap_or_default(),
                    r.data["name"].as_str().unwrap_or_default(),
                    r.data["merged_into"].as_str().unwrap_or_default(),
                )
            });
            format!(
                "merged {kind} {} into {} ({})",
                quoted(absorbed),
                quoted(&name(into)),
                count(sessions, "session", "sessions")
            )
        }
        "split" => {
            let kids: Vec<&Value> = of("ontology.category").map(|r| &r.data).collect();
            let parent = kids
                .first()
                .and_then(|d| d["parent"].as_str())
                .unwrap_or_default();
            let kind = kids
                .first()
                .and_then(|d| d["kind"].as_str())
                .unwrap_or_default();
            let names: Vec<String> = kids
                .iter()
                .filter_map(|d| d["name"].as_str())
                .map(quoted)
                .collect();
            format!(
                "split {kind} {} into {} ({} moved)",
                quoted(&name(parent)),
                names.join(" and "),
                count(sessions, "session", "sessions")
            )
        }
        "decay" => {
            let dropped: usize = of("ontology.membership")
                .map(|r| {
                    let held = |v: &Value| v["members"].as_array().map_or(0, Vec::len);
                    held(&r.data["undo"]["before"]).saturating_sub(held(&r.data["undo"]["wrote"]))
                })
                .sum();
            format!(
                "dropped {} unconfirmed for 30 days",
                count(dropped, "membership", "memberships")
            )
        }
        _ => format!(
            "changed the ontology ({})",
            count(w.rows.len(), "record", "records")
        ),
    }
}

/// One line for each machine write in `rows` (in the ledger's order), oldest
/// first, by where its first row is: two writes in one millisecond keep the
/// order they were made in. `- merged topic
/// "release process" into "release checklist" (3 sessions) · undo: theseus
/// ontology undo sweep-…-merge`, or `· undone` for one that was.
pub fn lines(rows: &[LedgerRow], name_of: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let undone: BTreeSet<&str> = rows
        .iter()
        .filter(|r| r.kind == LedgerKind::OntologyUndo.as_str())
        .filter_map(|r| r.data["write"].as_str())
        .collect();
    let mut writes: BTreeMap<&str, Write> = BTreeMap::new();
    for (at, row) in rows.iter().enumerate() {
        let Some(id) = write_of(row) else {
            continue;
        };
        let w = writes.entry(id).or_insert_with(|| Write {
            id,
            first: at,
            rows: Vec::new(),
        });
        w.rows.push(row);
    }
    let mut ordered: Vec<&Write> = writes.values().collect();
    ordered.sort_by_key(|w| w.first);
    ordered
        .into_iter()
        .map(|w| {
            let how = match undone.contains(w.id) {
                true => "undone".to_string(),
                false => format!("undo: theseus ontology undo {}", w.id),
            };
            format!("- {} · {how}", describe(w, name_of))
        })
        .collect()
}

impl Core {
    /// The ledger rows of `kind` since `since_ms`, by kind through the store's
    /// pages; `None` while the index's shape is built after a start.
    pub(super) fn onto_rows(
        &self,
        kind: LedgerKind,
        since_ms: u64,
    ) -> anyhow::Result<Option<Vec<LedgerRow>>> {
        Ok(self
            .onto_rows_at(kind, since_ms)?
            .map(|rows| rows.into_iter().map(|(_, r)| r).collect()))
    }

    /// `onto_rows`, each row with its position in the store.
    fn onto_rows_at(
        &self,
        kind: LedgerKind,
        since_ms: u64,
    ) -> anyhow::Result<Option<Vec<(u64, LedgerRow)>>> {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(kind.as_str())],
            since_ms: Some(since_ms),
            limit: usize::MAX / 2,
            ..Page::default()
        };
        let Some(out) = self.store.inner().page(&page)? else {
            return Ok(None);
        };
        Ok(Some(
            out.records
                .iter()
                .filter_map(|r| Some((r.position, r.decode::<LedgerRow>().ok()?)))
                .filter(|(_, r)| r.at_unix_ms >= since_ms)
                .collect(),
        ))
    }

    /// The last day's machine writes, one line each (`lines`): nothing when
    /// there is none, or while the index is built. The kinds' rows are put
    /// back in the ledger's order, so the lines keep the order the writes
    /// were made in.
    pub(crate) fn onto_night_lines(&self, now_ms: u64) -> Vec<String> {
        let since = now_ms.saturating_sub(DAY_MS);
        let mut found = Vec::new();
        for kind in KINDS {
            if let Ok(Some(rows)) = self.onto_rows_at(kind, since) {
                found.extend(rows);
            }
        }
        found.sort_by_key(|(at, _)| *at);
        let rows: Vec<LedgerRow> = found.into_iter().map(|(_, r)| r).collect();
        let Ok(o) = self.runner.ontology.snapshot(&self.store) else {
            return Vec::new();
        };
        lines(&rows, &|id| {
            Some(o.category(&CategoryId::parse(id).ok()?)?.name.clone())
        })
    }
}
