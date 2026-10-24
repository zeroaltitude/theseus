//! The week's measure (theseus-slvl.9): promises kept and the owner's
//! verdicts on what Theseus said first, from the ledger's rows. A read: the
//! rows of four kinds over seven days, paged by kind, never a turn's path.
//! `theseus persona` shows it; px-exam's initiative family reads the same
//! rows. The maintainer's week-long measure is promises kept at 95% or more
//! and the 👍 share.

use std::collections::HashMap;

use anyhow::Result;
use theseus_protocol::initiative::{InitiativeWeekParams, InitiativeWeekResult};
use theseus_protocol::LedgerKind;

use crate::rpc::Core;

const DAY_MS: u64 = 86_400_000;

/// A share, none while there is nothing to share over.
fn share(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

impl Core {
    /// `initiative.week`: the seven days to `until_ms` (now).
    pub fn initiative_week(&self, p: &InitiativeWeekParams) -> Result<InitiativeWeekResult> {
        let until = p.until_ms.unwrap_or_else(theseus_protocol::now_unix_ms);
        let since = until.saturating_sub(7 * DAY_MS);
        let rows = |kind| -> Result<Vec<crate::ledger::LedgerRow>> {
            Ok(self
                .synthesis_rows(kind, Some(since))?
                .into_iter()
                .filter(|r| r.at_unix_ms <= until)
                .collect())
        };
        let kept = rows(LedgerKind::PromiseKept)?.len() as u64;
        let unkept = rows(LedgerKind::PromiseUnkept)?.len() as u64;
        let spoke = rows(LedgerKind::InitiativeSpoke)?;
        // A message's last verdict stands: the owner may change their mind.
        let mut verdicts: HashMap<String, bool> = HashMap::new();
        for r in rows(LedgerKind::InitiativeRated)? {
            if let (Some(id), Some(rating)) = (r.data["id"].as_str(), r.data["rating"].as_str()) {
                verdicts.insert(id.to_string(), rating == "up");
            }
        }
        let up = verdicts.values().filter(|u| **u).count() as u64;
        let down = verdicts.len() as u64 - up;
        let voiced: Vec<f64> = spoke
            .iter()
            .filter_map(|r| r.data["cost_usd"].as_f64())
            .collect();
        Ok(InitiativeWeekResult {
            since_ms: since,
            until_ms: until,
            kept,
            unkept,
            kept_share: share(kept, kept + unkept),
            spoke: spoke.len() as u64,
            up,
            down,
            up_share: share(up, up + down),
            voiced: voiced.len() as u64,
            voiced_usd: voiced.iter().sum(),
            level: self
                .personas()
                .persona_for(None, None)
                .initiative
                .as_str()
                .into(),
        })
    }
}
