//! Adoption (26a, theseus-0j2.15): the packs that went live before the
//! ladder, outside it, stand as the owner's promotions of 2026-10-04. After
//! the ladder's first read (the warm read's, written between turns, never on
//! the start path or a turn's), each one this build wires live gets its row
//! once: `live`, `who: owner`, `why:
//! "decision of 2026-10-04"`. A pack the build lacks, or wires in shadow
//! because its live action has not joined, is adopted at the first read of
//! a build that has it.
//!
//! Their rules come from this table, keyed by pack id so a later version
//! keeps them, read beside each file's own (`rules::rules_of`); a rule a
//! pack's file declares itself wins over the table's of the same kind, so
//! the table's number is the one for a version whose file names none
//! (route.v12 names its own 5 pins a day). Each is a day's brake: its
//! rollback lapses at the next local midnight.

use theseus_judge::learn::RollbackRule;
use theseus_protocol::packs::PackModeRow;

use super::{Ladder, Loaded, Rung};
use crate::config::PackMode;

/// The adoption row's reason.
pub const WHY: &str = "decision of 2026-10-04";

/// Through what an adoption row came.
pub const VIA: &str = "adoption";

/// One pack the ladder adopts.
pub struct Adopted {
    /// The version adopted live.
    pub pack: &'static str,
    /// The adoption row's reason.
    pub why: &'static str,
}

/// The packs live before the ladder, the owner's decisions of 2026-10-04, and
/// the route versions that went live at once in place of the one before: each
/// the owner's word, its rollback rules guarding it. The earlier route versions
/// (`route.v1` to `route.v8`) were adopted the same way; their rows are in the
/// store, and the build no longer embeds them (`route.v5` to `route.v8` were
/// retired in issue 44, `route.v10` and `route.v11` in issue 46). A stored adoption of a
/// retired version is checked by its own file's rules (`rules::rules_of`).
pub const ADOPTED: &[Adopted] = &[
    Adopted {
        pack: "route.v12",
        why: WHY_ROUTE_V12,
    },
    Adopted {
        pack: "rerank.v1",
        why: WHY,
    },
    Adopted {
        pack: "security.v3",
        why: WHY,
    },
];

/// `route.v12`'s adoption row's reason: route.v11 without the questions
/// nothing read. It goes live on landing by the standing rule for a new
/// compiled version of a judgment pack (`docs/development.md`,
/// "Self-improvement"), under issue 46's line. (`route.v11`'s stored row
/// keeps its own reason.)
pub const WHY_ROUTE_V12: &str = "a new compiled version goes live on landing (docs/development.md); issue 46: the live packs lose the questions nothing reads";

/// The rules this table gives a pack id (every version of it). Most are the
/// adoption's, from the owner's decisions of 2026-10-04; `recall_cross` has
/// no adoption row (it went live with the ladder, not before it), and the
/// table is read for every pack, adopted or not (`rules::rules_of`), so its
/// rule stands beside them here:
/// - `route`: the owner pins another profile on 3 routed turns in a local
///   day (a pack that declares its own count, route.v10's 5, keeps it);
/// - `rerank`: its own breaker opens twice in a local day;
/// - `recall_cross`: the same (theseus-tbqpy). The cross judge is the other
///   canary at the `recall` point, on its own breaker under the same
///   `BreakerConfig::default()` as rerank's, so an open means the same
///   thing of either: the pack is failing or timing out often enough to
///   have tripped. Two in a local day is a pack that is not merely unlucky,
///   and the brake is a day's, so the count carries the same cost. Nothing
///   about the cross judge argues for a looser or a tighter number than its
///   neighbour's, so it takes its neighbour's.
/// - `security`: the notices' own rule (design §2.7): more than 30 notices,
///   or 3 judgments labeled `noise`, in a day.
pub fn rules(id: &str) -> Vec<RollbackRule> {
    match id {
        "route" => vec![RollbackRule::PinsPerDay { count: 3 }],
        "rerank" | "recall_cross" => vec![RollbackRule::OpensPerDay { count: 2 }],
        "security" => vec![
            RollbackRule::NoticesPerDay { max: 30 },
            RollbackRule::LabelsPerDay {
                label: "noise".into(),
                count: 3,
            },
        ],
        _ => vec![],
    }
}

/// The adoptions this build can make and the store lacks.
fn wanted<'a>(ladder: &'a Ladder, l: &'a Loaded) -> impl Iterator<Item = &'static Adopted> + 'a {
    ADOPTED.iter().filter(move |a| {
        theseus_judge::pack::by_name(a.pack).is_some()
            && ladder.wired(a.pack) == PackMode::Live
            && !l
                .rows
                .get(a.pack)
                .is_some_and(|rows| rows.iter().any(|r| r.via == VIA))
    })
}

/// Whether an adoption is missing (the warm read's question, before it
/// waits for a moment between turns to write them).
pub(crate) fn missing(ladder: &Ladder, l: &Loaded) -> bool {
    wanted(ladder, l).next().is_some()
}

/// Write each adoption this build can make and the store lacks, in one
/// frame: a turn that begins as they are written waits for one append.
pub(crate) fn adopt_missing(ladder: &Ladder, l: &mut Loaded) {
    let wanted: Vec<&Adopted> = wanted(ladder, l).collect();
    let rows: Vec<PackModeRow> = wanted
        .iter()
        .map(|a| PackModeRow {
            pack: a.pack.into(),
            mode: Rung::Live.as_str().into(),
            from: ladder.standing_in(l, a.pack).rung.as_str().into(),
            who: "owner".into(),
            by: "owner".into(),
            via: VIA.into(),
            why: a.why.into(),
            ..PackModeRow::default()
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    if let Err(e) = ladder.write_all_in(l, rows) {
        tracing::warn!(error = %format!("{e:#}"), "judge: the adoptions were not written");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every adopted rule can hold as the loader checks a pack file's.
    #[test]
    fn the_adopted_rules_pass_the_loaders_checks() {
        for a in ADOPTED {
            let id = super::super::id_of(a.pack);
            let rules = rules(id);
            assert!(!rules.is_empty(), "{id}");
            for r in rules {
                r.check().unwrap();
            }
        }
        assert!(rules("loop").is_empty());
    }

    /// The cross-recall pack's breaker has a day's brake of its own
    /// (theseus-tbqpy): the same rule and the same count as `rerank`'s, and
    /// `rules_of` carries it to a version whose file declares none, so an
    /// open of `recall_cross` is counted on the ladder.
    #[test]
    fn the_cross_recall_packs_breaker_opening_counts_on_the_ladder() {
        let table = rules("recall_cross");
        assert_eq!(table, vec![RollbackRule::OpensPerDay { count: 2 }]);
        for r in &table {
            r.check().unwrap();
        }
        assert_eq!(
            table,
            rules("rerank"),
            "the same shape and count as the neighbouring rerank family"
        );
        let merged = super::super::rules::rules_of("recall_cross.v1");
        assert_eq!(
            merged
                .iter()
                .filter(|r| **r == RollbackRule::OpensPerDay { count: 2 })
                .count(),
            1,
            "{merged:?}"
        );
    }
}
