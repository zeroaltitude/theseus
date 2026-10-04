//! `rerank.v1`'s system label (M6 32d; design §2.9's table): the owner's
//! word on a recalled note (`theseus memory label <node> <label>`) grades
//! the rerank's answer about it. `useful` or `should_have` makes it `true`
//! (the note would have helped), `wrong` or `stale` `false`. The label's
//! `recall` names the recall whose rerank it grades; a label that names
//! none grades the newest rerank before it that asked about the node. Each
//! is written once, keyed by its judgment, its item's question, and the
//! memory label's position, at the system's weight: an owner's label on the
//! same question (1.0) beats it.

use serde_json::json;
use theseus_protocol::LedgerKind;
use theseus_store::kinds;

use super::labels::system_key;
use super::system::SystemLabel;
use super::{Scope, Seen};
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// The rule's name, in each label's row.
pub const RULE: &str = "memory_label";

/// One `memory.label` row, read back.
struct MemoryLabel {
    position: u64,
    node: String,
    word: String,
    truth: bool,
    recall: Option<String>,
}

/// What a memory label says of a rerank's answer about its note.
pub fn truth_of(word: &str) -> Option<bool> {
    match word {
        "useful" | "should_have" => Some(true),
        "wrong" | "stale" => Some(false),
        _ => None,
    }
}

impl Core {
    /// The rerank's system labels, from every memory label that grades one
    /// of `scope`'s answered judgments.
    pub(crate) fn rerank_labels(&self, scope: &Scope) -> Vec<SystemLabel> {
        let judged: Vec<&Seen> = scope
            .judgments
            .values()
            .flatten()
            .filter(|s| s.judgment.outcome == theseus_judge::Outcome::Answered)
            .collect();
        if judged.is_empty() {
            return Vec::new();
        }
        self.memory_labels()
            .into_iter()
            .filter_map(|l| label_for(&judged, &l))
            .collect()
    }

    /// Every memory label that says something of a note's use.
    fn memory_labels(&self) -> Vec<MemoryLabel> {
        let Ok(records) = self.store.scope_after(crate::recall::labels::SCOPE, 0) else {
            return Vec::new();
        };
        records
            .iter()
            .filter(|r| r.kind == kinds::LEDGER)
            .filter_map(|r| {
                let row: LedgerRow = r.decode().ok()?;
                if row.kind != LedgerKind::MemoryLabel.as_str() {
                    return None;
                }
                let s = |k: &str| row.data[k].as_str().map(str::to_string);
                let word = s("label")?;
                Some(MemoryLabel {
                    position: r.position,
                    node: s("node_id")?,
                    truth: truth_of(&word)?,
                    word,
                    recall: s("recall_id"),
                })
            })
            .collect()
    }
}

/// The node an item's key names (`<node>#<chunk>`).
fn node_of(key: &str) -> &str {
    key.rsplit_once('#').map_or(key, |(n, _)| n)
}

/// The system label `l` writes: on its recall's rerank, or the newest
/// rerank before it that asked about its node; none when no rerank asked.
fn label_for(judged: &[&Seen], l: &MemoryLabel) -> Option<SystemLabel> {
    let about = |s: &Seen| {
        s.judgment
            .answers
            .iter()
            .find(|a| a.about.as_deref().is_some_and(|k| node_of(k) == l.node))
            .cloned()
    };
    let (s, a, how) = match &l.recall {
        Some(r) => judged
            .iter()
            .filter(|s| s.context("recall") == Some(r.as_str()))
            .find_map(|s| Some((*s, about(s)?, format!("on recall {r}"))))?,
        None => judged
            .iter()
            .filter(|s| s.position < l.position)
            .filter_map(|s| Some((*s, about(s)?)))
            .max_by_key(|(s, _)| s.position)
            .map(|(s, a)| {
                (
                    s,
                    a,
                    "with no recall named: the newest rerank before it".to_string(),
                )
            })?,
    };
    let j = &s.judgment;
    Some(SystemLabel {
        id: system_key(&j.id, &a.question, &format!("{RULE}@{}", l.position)),
        judgment: j.id.clone(),
        pack: j.pack.clone(),
        session: s.context("session").map(str::to_string),
        question: a.question.clone(),
        about: a.about.clone(),
        label: json!(l.truth),
        rule: RULE,
        note: format!("the owner labeled {} {} ({how})", l.node, l.word),
    })
}
