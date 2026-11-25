//! His declaration undone (issue 51; `theseus ledger topics
//! --undo-declared`, the same method and the same owner's act in person as
//! the filing, `rpc/exchanges.rs`). A declared answer's row says
//! `declared: unmarked` (`history.rs`); this ends every touch such an
//! answer came to, and nothing else.
//!
//! - **Which touches**: a touch of a category of a kind `onto` asks, by an
//!   exchange a declared answer was about, of a category that answer chose
//!   (whatever its confidence, and though the category was retired since):
//!   so a touch the rebuild moved is found by its root and its turn, and a
//!   category the answer chose that got no touch ends nothing. No other
//!   writer touches those exchanges: the filing asks only an exchange that
//!   touched no such category before, and the live point touches only the
//!   exchange of a message as it arrives.
//! - **What it writes**: each touch's end, in frames, as the rebuild ends a
//!   moved touch. The rows stay, as history: no row, record, person, place
//!   or membership is written or ended. A later run without the declaration
//!   never writes a declared answer's touches again; one with it does, from
//!   the rows, with no call.
//! - **A dry run** counts and ends nothing. A second undo finds nothing left
//!   and ends nothing. Counts only: no word of a message, no name, no id.

use std::collections::{HashMap, HashSet};

use theseus_judge::Judgment;
use theseus_protocol::exchange::ExchangeTopicsResult;
use theseus_store::{kinds, NewRecord, Store as _};

use super::decide;
use super::history::{history_rows, DECLARED};
use crate::exchange::{Root, SEP, TOUCH_PREFIX};
use crate::rpc::Core;

/// Touches ended in one frame.
const BATCH: usize = 512;

/// The roots of the categories `j` chose, whatever the confidence and
/// whether or not the ontology still holds them: a listed candidate, never
/// a project (a task, with no touch) and never a `new` (the filing writes
/// none for it).
fn named(j: &Judgment) -> impl Iterator<Item = String> + '_ {
    let kinds = &j.context["onto"]["kinds"];
    theseus_judge::builders::ONTO_KINDS
        .into_iter()
        .filter(|kind| *kind != "project")
        .filter_map(move |kind| {
            let (answer, _) = decide::choice(j, kind)?;
            let options = decide::strings(&kinds[kind]);
            let id = decide::nth(&options, &answer, 'c')?;
            Some(Root::Thing(id.as_str()).key())
        })
}

impl Core {
    /// `exchange.topics` with `undo_declared`: see the module's doc. The
    /// caller has judged it the owner's act, and holds the filing's lock.
    pub(crate) fn topics_undo(&self, dry_run: bool) -> anyhow::Result<ExchangeTopicsResult> {
        let rows: Vec<Judgment> = history_rows(&self.store)?
            .into_iter()
            .filter(|j| j.context["declared"].as_str() == Some(DECLARED))
            .collect();
        let mut chose: HashMap<&str, HashSet<String>> = HashMap::new();
        for j in &rows {
            if let Some(turn) = j.context["turn"].as_str() {
                chose.entry(turn).or_default().extend(named(j));
            }
        }
        let mut ended = Vec::new();
        let mut turns = HashSet::new();
        let prefix = format!("{TOUCH_PREFIX}{}", Root::Thing("").key());
        for r in self
            .store
            .inner()
            .latest_with_prefix(kinds::META, &prefix)?
        {
            let Some(key) = r.key else {
                continue;
            };
            let mut parts = key.rsplitn(3, SEP);
            let (Some(turn), Some(_), Some(root)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let root = root.strip_prefix(TOUCH_PREFIX).unwrap_or(root);
            if chose.get(turn).is_some_and(|c| c.contains(root)) {
                turns.insert(turn.to_string());
                ended.push(key);
            }
        }
        let r = ExchangeTopicsResult {
            dry_run,
            undo_declared: true,
            declared_rows: rows.len() as u64,
            undone: turns.len() as u64,
            ended: ended.len() as u64,
            ..ExchangeTopicsResult::default()
        };
        if !dry_run {
            for keys in ended.chunks(BATCH) {
                let frame: Vec<NewRecord> =
                    keys.iter().map(|k| NewRecord::delete_meta(k)).collect();
                self.store.append(&frame)?;
            }
        }
        Ok(r)
    }
}
