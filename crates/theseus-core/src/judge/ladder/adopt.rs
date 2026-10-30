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
//! (route.v10 names its own 5 pins a day). Each is a day's brake: its
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
/// the owner's word, its rollback rules guarding it (`route.v5` in place of
/// `route.v4`, theseus-c1uz; `route.v6` in place of `route.v5`,
/// theseus-a1wvq.2; `route.v7` in place of `route.v6`, theseus-a1wvq; `route.v8` in place of `route.v7`, theseus-f9qa8). The earlier route versions (`route.v1` to `route.v4`) were
/// adopted the same way; their rows are in the store, and the build no longer
/// embeds them.
pub const ADOPTED: &[Adopted] = &[
    Adopted {
        pack: "route.v5",
        why: WHY_ROUTE_V5,
    },
    Adopted {
        pack: "route.v6",
        why: WHY_ROUTE_V6,
    },
    Adopted {
        pack: "route.v7",
        why: WHY_ROUTE_V7,
    },
    Adopted {
        pack: "route.v8",
        why: WHY_ROUTE_V8,
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

/// `route.v5`'s adoption row's reason: it is route.v4 word for word with two
/// questions that decide nothing for its promotion, `persona` and `seal`
/// (theseus-c1uz); the owner's word of 2026-10-09 that Jev engages the persona.
pub const WHY_ROUTE_V5: &str = "decision of 2026-10-13";

/// `route.v6`'s adoption row's reason: route.v5 word for word and the `data`
/// question, which decides nothing for its promotion and acts from the day the
/// pack does (theseus-a1wvq.2).
pub const WHY_ROUTE_V6: &str = "the owner's decision of 2026-10-17";

/// `route.v7`'s adoption row's reason: route.v6 word for word but the `data`
/// question's wording, which decides nothing for its promotion (theseus-a1wvq,
/// v1 patch1).
pub const WHY_ROUTE_V7: &str = "the owner's decision of 2026-10-19";

/// `route.v8`'s adoption row's reason: route.v7 word for word but the `data`
/// question's `open` and `unclear` options, which decides nothing for its
/// promotion (theseus-f9qa8, v1 patch2).
pub const WHY_ROUTE_V8: &str = "the owner's decision of 2026-10-23";

/// The rules adoption gives a pack id (every version of it):
/// - `route`: the owner pins another profile on 3 routed turns in a local
///   day (a pack that declares its own count, route.v10's 5, keeps it);
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
}
