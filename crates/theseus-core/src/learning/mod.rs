//! The learning ledger (M5 step 25c; design §2.9, spec §3.10): whether Jev's
//! judgments were right.
//!
//! - **Labels** (`labels`): `judge.label` rows, never edits, keyed `lbl_<id>`
//!   and scoped as their judgment. The operator's come through `judge.label`
//!   (weight 1.0; `theseus judge label`, the cockpit's buttons, and step 24's
//!   "should have asked" press); the system's are derived by each report's
//!   run (`system`, weight 0.5). Where a judgment's question has more than
//!   one label, the heaviest counts, and the newest of equal weight.
//! - **The report** (`report`): per pack version and question, from the
//!   `judge:<pack id>` scopes alone, never the whole history; every number
//!   from `theseus_judge::learn`. Its holdout is the closed window of the 14
//!   days before the report's local midnight, frozen into it with the
//!   judgments inside and their labels. Written as `judge.report` rows, one a
//!   pack version, keyed `rpt_<date>_<pack>`, in one frame with the run's
//!   system labels, then as `<state dir>/learning/<date>.json`, which the
//!   rows rebuild.
//! - **The tender** (`tender`): after serving, never within 10 minutes of a
//!   start, at `[judge] learning_hour` local time (a missed night runs once,
//!   10 minutes after the next start), on a thread of its own at nice 19 and
//!   about 5% of a core, holding the core weakly. Nothing runs with the judge
//!   off.

pub mod audit;
pub mod backfill;
pub mod items;
pub mod labels;
pub mod propose;
pub mod prove;
pub mod rebuild;
pub mod replay;
pub mod report;
pub mod rerank;
pub mod system;
pub mod tender;

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};
use theseus_judge::Judgment;
use theseus_protocol::LedgerKind;

use crate::ledger::LedgerRow;
use crate::store::Store;

/// The META key of the last run: `{"at_unix_ms", "date", "trigger"}`.
pub const LAST_RUN: &str = "learning.last_run";

/// The holdout's days by default (`[judge] holdout_days`).
pub const HOLDOUT_DAYS: u64 = 14;

/// A system label's weight; an operator's is 1.0 (design §2.9).
pub const SYSTEM_WEIGHT: f64 = 0.5;

/// One `judge.call` row, read back.
#[derive(Debug, Clone)]
pub struct Seen {
    pub position: u64,
    /// When its row was written; a backfilled one's, when its event
    /// happened (`context.event_at_ms`, 25d).
    pub at_ms: u64,
    pub judgment: Judgment,
}

impl Seen {
    pub fn context(&self, key: &str) -> Option<&str> {
        self.judgment.context.get(key).and_then(Value::as_str)
    }
}

/// One `judge.label` row, read back.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRow {
    pub position: u64,
    pub at_ms: u64,
    /// `lbl_…`: its key.
    pub id: String,
    pub judgment: String,
    pub pack: String,
    pub question: Option<String>,
    /// A per-item question's item key (32d).
    pub about: Option<String>,
    pub label: Value,
    pub source: String,
    pub weight: f64,
}

/// One pack's scope, read: its judgments by version (`loop.v1`), and every
/// label in it, by judgment.
#[derive(Debug, Default)]
pub struct Scope {
    pub judgments: BTreeMap<String, Vec<Seen>>,
    pub labels: HashMap<String, Vec<LabelRow>>,
    /// Every label's key, for the system labels' "written once".
    pub label_ids: std::collections::HashSet<String>,
}

/// Read a pack's scope (`judge:<pack id>`): its `judge.call` and
/// `judge.label` rows, and nothing else. A row that does not read is
/// skipped.
pub fn read_scope(store: &Store, pack_id: &str) -> anyhow::Result<Scope> {
    let mut s = Scope::default();
    for r in store.scope_after(&format!("judge:{pack_id}"), 0)? {
        let Ok(row) = r.decode::<LedgerRow>() else {
            continue;
        };
        if row.kind == LedgerKind::JudgeCall.as_str() {
            if let Ok(judgment) = serde_json::from_value::<Judgment>(row.data) {
                // A backfilled judgment's time is its event's (25d): the
                // holdout's split reads when the judged thing happened.
                let at_ms = judgment
                    .context
                    .get("event_at_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(row.at_unix_ms);
                s.judgments
                    .entry(judgment.pack.clone())
                    .or_default()
                    .push(Seen {
                        position: r.position,
                        at_ms,
                        judgment,
                    });
            }
        } else if row.kind == LedgerKind::JudgeLabel.as_str() {
            if let Some(l) = label_row(r.position, row.at_unix_ms, &row.data) {
                s.label_ids.insert(l.id.clone());
                s.labels.entry(l.judgment.clone()).or_default().push(l);
            }
        }
    }
    Ok(s)
}

fn label_row(position: u64, at_ms: u64, d: &Value) -> Option<LabelRow> {
    let s = |k: &str| d.get(k).and_then(Value::as_str).map(str::to_string);
    Some(LabelRow {
        position,
        at_ms,
        id: s("id")?,
        judgment: s("judgment")?,
        pack: s("pack").unwrap_or_default(),
        question: s("question"),
        about: s("about"),
        label: label_of(d),
        source: s("source").unwrap_or_else(|| "operator".into()),
        weight: d.get("weight").and_then(Value::as_f64).unwrap_or(1.0),
    })
}

/// A row's label in the ledger's form. 28b's answer to a `categorize.v1`
/// proposal (`fact::judge::ProposalLabel`) says `accepted` or `rejected`
/// beside the judgment's `answer`: an accept names that answer the right
/// class, a reject a wrong one. Any other row's label stands.
fn label_of(d: &Value) -> Value {
    let label = d.get("label").cloned().unwrap_or(Value::Null);
    match (label.as_str(), d.get("answer").and_then(Value::as_str)) {
        (Some("accepted"), Some(answer)) => json!(answer),
        (Some("rejected"), Some(answer)) => json!({ "not": answer }),
        _ => label,
    }
}

/// The pack ids whose scopes a report reads: every pack this build embeds.
pub fn pack_ids() -> Vec<String> {
    let mut ids: Vec<String> = theseus_judge::pack::embedded()
        .as_ref()
        .map(|packs| packs.iter().map(|p| p.id.clone()).collect())
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

/// The local midnight that begins `unix_ms`'s day.
pub fn local_midnight(unix_ms: u64) -> u64 {
    let l = crate::wake::local(unix_ms);
    let into = (u64::from(l.hour) * 3600 + u64::from(l.minute) * 60 + u64::from(l.second)) * 1000
        + unix_ms % 1000;
    unix_ms.saturating_sub(into)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_judge::band::band;
    use theseus_judge::client::Answer;
    use theseus_judge::judge::AnswerRecord;

    use super::labels::{truth, Truth};
    use crate::fact::{judge::ProposalLabel, Fact};

    /// 28b's answers to Jev's proposals, as its writer writes them, read as
    /// labels on the answer: an accept grades the proposed topic right, a
    /// reject wrong (not a class named `accepted` or `rejected`).
    #[test]
    fn a_proposals_answer_reads_as_a_label_on_its_topic() {
        let row = |label, topic| {
            ProposalLabel {
                id: "lbl_1",
                judgment: "jdg_1",
                pack: "categorize.v1",
                question: "topic",
                label,
                answer: "harbor",
                topic,
                who: "owner",
                via: "cli",
                note: None,
            }
            .row()
        };
        let accepted = super::label_row(1, 0, &row("accepted", Some("harbor"))).unwrap();
        let rejected = super::label_row(2, 0, &row("rejected", None)).unwrap();
        assert_eq!(accepted.label, json!("harbor"));
        assert_eq!(rejected.label, json!({"not": "harbor"}));
        assert_eq!(accepted.question.as_deref(), Some("topic"));
        assert_eq!(
            (accepted.source.as_str(), accepted.weight),
            ("operator", 1.0)
        );
        let answer = Answer::Choice {
            choice: "harbor".into(),
            probabilities: vec![("harbor".into(), 0.9), ("new_topic".into(), 0.1)],
            confidence: 0.9,
        };
        let answer = AnswerRecord {
            question: "topic".into(),
            def: "topic".into(),
            about: None,
            band: band(
                &answer,
                theseus_judge::Thresholds {
                    act: 0.9,
                    confirm: 0.6,
                },
            ),
            answer,
        };
        assert_eq!(
            truth(&accepted.label, &answer),
            Some(Truth::Class("harbor".into()))
        );
        assert_eq!(
            truth(&rejected.label, &answer),
            Some(Truth::NotClass("harbor".into()))
        );
        // A label in the ledger's own form stands.
        let plain = json!({"id": "lbl_3", "judgment": "jdg_1", "label": "wrong"});
        assert_eq!(
            super::label_row(3, 0, &plain).unwrap().label,
            json!("wrong")
        );
    }
}
