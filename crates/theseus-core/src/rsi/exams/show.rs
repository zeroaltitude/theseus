//! `self.exams` (theseus-pw1q.11): the counted items by family and half, and
//! today's candidates with their reviews. It reads rows and the open shelf,
//! so a sealed item is only ever a number or an id: there is nothing of its
//! text to show.

use std::collections::BTreeMap;

use anyhow::Result;
use theseus_protocol::rsi::{SelfExamFamily, SelfExamToday, SelfExamsParams, SelfExamsResult};
use theseus_protocol::LedgerKind;

use super::{Family, Shelf};
use crate::rpc::Core;

impl Core {
    /// The counts and today's candidates.
    pub fn self_exams(&self, _p: &SelfExamsParams) -> Result<SelfExamsResult> {
        let now = theseus_protocol::now_unix_ms();
        let midnight = crate::learning::local_midnight(now);
        let mut counts: BTreeMap<Family, (u64, u64)> =
            Family::ALL.into_iter().map(|f| (f, (0, 0))).collect();
        let tasks: BTreeMap<String, String> = Shelf::new(&self.cfg.state_dir())
            .items()
            .into_iter()
            .map(|i| (i.id.0, i.task))
            .collect();
        let mut today = Vec::new();
        for (_, row) in super::harvest::rows_of(self, LedgerKind::SelfExamCounted, 0, now)? {
            let d = &row.data;
            let (sealed, fam) = (
                d["sealed"].as_bool().unwrap_or(false),
                d["family"].as_str().and_then(Family::parse),
            );
            if let Some(f) = fam {
                let c = counts.entry(f).or_default();
                if sealed {
                    c.1 += 1;
                } else {
                    c.0 += 1;
                }
            }
            if row.at_unix_ms >= midnight {
                today.push(entry(&row.data, "counted", &tasks));
            }
        }
        for (_, row) in super::harvest::rows_of(self, LedgerKind::SelfExamRejected, midnight, now)?
        {
            today.push(entry(&row.data, "rejected", &tasks));
        }
        let mut sealed_dropped = 0;
        for (_, row) in super::harvest::rows_of(self, LedgerKind::SelfExamPass, midnight, now)? {
            sealed_dropped += row.data["sealed_dropped"].as_u64().unwrap_or(0);
        }
        Ok(SelfExamsResult {
            families: counts
                .into_iter()
                .map(|(f, (open, sealed))| SelfExamFamily {
                    family: f.as_str().into(),
                    open,
                    sealed,
                })
                .collect(),
            today,
            sealed_dropped,
            state: self.self_state(),
        })
    }
}

/// A day's entry. An open item shows its task; a sealed one never does.
fn entry(d: &serde_json::Value, verdict: &str, tasks: &BTreeMap<String, String>) -> SelfExamToday {
    let sealed = d["sealed"].as_bool().unwrap_or(false);
    let item = d["item"].as_str().unwrap_or("?").to_string();
    SelfExamToday {
        task: (!sealed).then(|| tasks.get(&item).cloned()).flatten(),
        item,
        family: d["family"].as_str().unwrap_or("?").into(),
        sealed,
        verdict: verdict.into(),
        reviewer: d["reviewer"].as_str().map(str::to_string),
        reason: d["reason"].as_str().map(str::to_string),
    }
}
