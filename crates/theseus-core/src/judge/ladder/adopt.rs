//! Adoption (26a, theseus-0j2.15): the packs that went live before the
//! ladder, outside it, stand as the owner's promotions of 2026-10-04. At the
//! ladder's first read (after serving, never on the start path), each one
//! this build wires live gets its row once: `live`, `who: owner`, `why:
//! "decision of 2026-10-04"`. A pack the build lacks, or wires in shadow
//! because its live action has not joined, is adopted at the first read of
//! a build that has it.
//!
//! Their rules come from this table, keyed by pack id so a later version
//! keeps them, read beside each file's own (`rules::rules_of`). Each is a
//! day's brake: its rollback lapses at the next local midnight.

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
}

/// The packs live before the ladder.
pub const ADOPTED: &[Adopted] = &[
    Adopted { pack: "route.v1" },
    Adopted { pack: "rerank.v1" },
    Adopted {
        pack: "security.v3",
    },
];

/// The rules adoption gives a pack id (every version of it):
/// - `route`: the owner pins another profile on 3 routed turns in a local
///   day;
/// - `rerank`: its own breaker opens twice in a local day;
/// - `security`: the notices' own rule (design §2.7): more than 30 notices,
///   or 3 judgments labeled `noise`, in a day.
pub fn rules(id: &str) -> Vec<RollbackRule> {
    match id {
        "route" => vec![RollbackRule::PinsPerDay { count: 3 }],
        "rerank" => vec![RollbackRule::OpensPerDay { count: 2 }],
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

/// Write each adoption this build can make and the store lacks.
pub(crate) fn adopt_missing(ladder: &Ladder, l: &mut Loaded) {
    for a in ADOPTED {
        if theseus_judge::pack::by_name(a.pack).is_none() || ladder.wired(a.pack) != PackMode::Live
        {
            continue;
        }
        let done = l
            .rows
            .get(a.pack)
            .is_some_and(|rows| rows.iter().any(|r| r.via == VIA));
        if done {
            continue;
        }
        let from = ladder.standing_in(l, a.pack).rung;
        let row = PackModeRow {
            pack: a.pack.into(),
            mode: Rung::Live.as_str().into(),
            from: from.as_str().into(),
            who: "owner".into(),
            by: "owner".into(),
            via: VIA.into(),
            why: WHY.into(),
            ..PackModeRow::default()
        };
        if let Err(e) = ladder.write_in(l, row) {
            tracing::warn!(error = %format!("{e:#}"), pack = a.pack, "judge: an adoption was not written");
        }
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
}
