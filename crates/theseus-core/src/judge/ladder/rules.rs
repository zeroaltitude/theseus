//! Rollback (design §2.7, "Moving down"): each pack's rules, its file's and
//! those its adoption gives it (`adopt`), checked as each event lands
//! (`learn::check_all`) and again by the nightly run. An event is a
//! `pack.event` row scoped to its pack id and local day, so a restart reads
//! the day's events back before it counts, and no count resets.
//!
//! A rollback is a `pack.mode` row (`rolled_back`, `who: system`, the rule,
//! its words), its sentence on the owner's surfaces, and health's line.
//! Moving down needs nobody. A rollback by an adopted rule is a day's
//! brake: its row's `until` is the next local midnight, when the pack
//! stands where it stood before. Every other rollback stands until a
//! promotion. The events a rule counts are today's after the version's
//! latest row, so a promotion starts the count again.

use std::collections::BTreeSet;

use anyhow::Result;
use theseus_judge::learn::{self, CanaryEvent, Fired, RollbackRule};
use theseus_protocol::packs::PackModeRow;
use theseus_protocol::LedgerKind;

use super::{adopt, id_of, Ladder, Loaded, Rung};
use crate::ledger::LedgerRow;
use crate::store::Store;

/// Who writes a rule's rollback.
pub const SYSTEM: &str = "system";

/// The local midnight after `now`.
pub fn next_midnight(now: u64) -> u64 {
    let today = crate::learning::local_midnight(now);
    // Half a day into tomorrow, then back to its midnight: a day of 23 or
    // 25 hours still lands on it.
    crate::learning::local_midnight(today + learn::DAY_MS + learn::DAY_MS / 2)
}

/// A pack version's rules: its file's, then those its adoption gives its
/// pack id.
pub fn rules_of(pack: &str) -> Vec<RollbackRule> {
    let mut out = theseus_judge::pack::by_name(pack)
        .map(|p| p.rollback.clone())
        .unwrap_or_default();
    for r in adopt::rules(id_of(pack)) {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// Pack ids braked today by their own step: the notices' brake is a
/// `judge.paused` row with `what: "notices"`, scoped `judge` (design §2.5),
/// naming its day. Its pack is `security`.
pub fn brakes_today(store: &Store, day: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(records) = store.scope_after("judge", 0) else {
        return out;
    };
    for r in records {
        let Ok(row) = r.decode::<LedgerRow>() else {
            continue;
        };
        let d = &row.data;
        if row.kind == LedgerKind::JudgePaused.as_str()
            && d["what"].as_str() == Some("notices")
            && d["day"].as_str() == Some(day)
        {
            out.insert(
                d["pack"]
                    .as_str()
                    .map_or("security", |p| id_of(p))
                    .to_string(),
            );
        }
    }
    out
}

impl Ladder {
    /// An event a pack's rules count, as it lands: kept as a `pack.event`
    /// row (scoped to its pack id and today), then every version of the pack
    /// that acts is checked. Returns the rollbacks it wrote.
    pub fn land(&self, pack: &str, event: CanaryEvent) -> Vec<PackModeRow> {
        self.with(|l, me| {
            let id = id_of(pack).to_string();
            let f = crate::fact::ladder::PackEventLanded {
                pack,
                event: &event,
            };
            let written = crate::fact::row(&f, None, None)
                .map(|r| r.scoped(&super::events_scope(&id, &l.day)))
                .and_then(|r| me.store().append(&[r]));
            match written {
                Ok(p) => l
                    .events
                    .entry(id.clone())
                    .or_default()
                    .push((p.first().copied().unwrap_or_default(), event)),
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), pack, "judge: an event the ladder counts was not written");
                }
            }
            me.check_in(l, Some(&id))
        })
    }

    /// Check every version that acts (of `only` the pack id given), and roll
    /// back each one a rule fires for.
    pub(crate) fn check_in(&self, l: &mut Loaded, only: Option<&str>) -> Vec<PackModeRow> {
        let versions: Vec<String> = l
            .rows
            .keys()
            .cloned()
            .chain(self.wired_packs().into_iter().map(|(p, _)| p))
            .filter(|p| only.is_none_or(|id| id_of(p) == id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut out = Vec::new();
        for pack in versions {
            let s = self.standing_in(l, &pack);
            if !s.rung.acts() {
                continue;
            }
            let events: Vec<CanaryEvent> = l
                .events
                .get(id_of(&pack))
                .map(|v| {
                    v.iter()
                        .filter(|(p, _)| *p > s.position)
                        .map(|(_, e)| e.clone())
                        .collect()
                })
                .unwrap_or_default();
            let Some(fired) = learn::check_all(&rules_of(&pack), &events)
                .into_iter()
                .next()
            else {
                continue;
            };
            match self.roll_back_in(l, &pack, s.rung, s.share, &fired) {
                Ok(row) => out.push(row),
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), pack, "judge: a rollback was not written");
                }
            }
        }
        out
    }

    /// A rule's rollback: its row, a brake's `until` the next midnight.
    fn roll_back_in(
        &self,
        l: &mut Loaded,
        pack: &str,
        from: Rung,
        share: Option<f64>,
        fired: &Fired,
    ) -> Result<PackModeRow> {
        let brake = adopt::rules(id_of(pack))
            .iter()
            .any(|r| r.name() == fired.rule);
        let row = PackModeRow {
            pack: pack.into(),
            mode: Rung::RolledBack.as_str().into(),
            from: from.as_str().into(),
            share,
            who: SYSTEM.into(),
            by: SYSTEM.into(),
            via: "ladder".into(),
            why: format!("rule {}", fired.rule),
            rule: Some(fired.rule.clone()),
            words: Some(fired.why.clone()),
            until_ms: brake.then(|| next_midnight(self.now())),
            ..PackModeRow::default()
        };
        self.write_in(l, row)
    }

    /// The nightly check (25c's run): read the ladder again from the store,
    /// and check every pack's rules on the day's events.
    pub fn recheck(&self) -> Vec<PackModeRow> {
        self.forget();
        self.with(|l, me| me.check_in(l, None))
    }
}
