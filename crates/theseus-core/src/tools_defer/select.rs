//! The selector (theseus-7m7w.2): which families a place kind loads, from
//! its committed defaults and what the ledger shows it uses.
//!
//! Pure over the counts (`Stats`) and the clock it is given, so a test sets
//! both. The counts are in memory, read from the ledger once after serving
//! and kept by each turn's row; the selection is read where a conversation
//! recompiles, never per call.

use std::collections::{BTreeMap, BTreeSet};

use super::family::{self, PlaceKind};
use crate::config::defer::DeferConfig;

pub const DAY_MS: u64 = 86_400_000;
/// Fewest turns of a place kind the ledger must hold, in the days looked at,
/// before its use outvotes the defaults: a handful of turns learns nothing.
pub const MIN_TURNS: u32 = 10;

/// One day's counts for a place kind (days since the epoch).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Day {
    pub turns: u32,
    /// Turns in which the family was called. A search alone is not use: it is `searched`.
    pub used: BTreeMap<String, u32>,
    /// Turns in which the family was loaded.
    pub loaded: BTreeMap<String, u32>,
    /// Turns in which the family was loaded and the conversation had not
    /// used it yet.
    pub unused: BTreeMap<String, u32>,
    /// Searches that surfaced a tool of the family.
    pub searched: BTreeMap<String, u32>,
    /// Calls to a tool of the family that was not loaded and not searched for.
    pub missed: BTreeMap<String, u32>,
}

/// What the ledger shows of one place kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub days: BTreeMap<u64, Day>,
    /// When each family was last called.
    pub last_used: BTreeMap<String, u64>,
    /// The earliest row: how long the ledger has watched this kind.
    pub first_ms: Option<u64>,
}

impl Stats {
    pub fn day_mut(&mut self, at_ms: u64) -> &mut Day {
        self.first_ms = Some(self.first_ms.map_or(at_ms, |f| f.min(at_ms)));
        self.days.entry(at_ms / DAY_MS).or_default()
    }

    /// The counts of the days from `since_ms` on.
    pub fn since(&self, since_ms: u64) -> impl Iterator<Item = &Day> {
        self.days.range(since_ms / DAY_MS..).map(|(_, d)| d)
    }

    pub fn turns_since(&self, since_ms: u64) -> u32 {
        self.since(since_ms).map(|d| d.turns).sum()
    }

    pub fn sum_since(
        &self,
        since_ms: u64,
        pick: impl Fn(&Day) -> &BTreeMap<String, u32>,
    ) -> BTreeMap<String, u32> {
        let mut out = BTreeMap::new();
        for d in self.since(since_ms) {
            for (f, n) in pick(d) {
                *out.entry(f.clone()).or_insert(0) += n;
            }
        }
        out
    }

    /// Drop the days older than `keep_days` before `now_ms`.
    pub fn trim(&mut self, now_ms: u64, keep_days: u64) {
        let from = (now_ms / DAY_MS).saturating_sub(keep_days);
        self.days = self.days.split_off(&from);
    }
}

/// Why a family is in the set, or out of it.
#[derive(Debug, Clone, PartialEq)]
pub enum Why {
    /// Never hidden: `[tools.defer] always_loaded`, or a family that cannot be.
    Always,
    /// This place kind's committed default.
    Default,
    /// Called in at least `load_share` of the kind's turns.
    Learned { share: f64 },
    /// Asked for by a search of this conversation (the fallback's).
    Wanted,
    /// Offered per message by the caller (a seam: Jev's branches).
    Extra,
    /// Used enough to load, but the request would pass the place's budget with it: deferred
    /// (`fit`).
    OverBudget { share: f64 },
    /// A default, unused for this many days: hidden.
    Unused { days: u64 },
    /// Not a default, and not used enough: deferred.
    Deferred,
}

impl Why {
    pub fn loads(&self) -> bool {
        !matches!(
            self,
            Why::Unused { .. } | Why::Deferred | Why::OverBudget { .. }
        )
    }

    pub fn words(&self) -> String {
        match self {
            Why::Always => "always loaded".into(),
            Why::Default => "default for this place".into(),
            Why::Learned { share } => format!("used in {:.0}% of turns", share * 100.0),
            Why::Wanted => "asked for by a search".into(),
            Why::Extra => "offered for this message".into(),
            Why::OverBudget { share } => {
                format!(
                    "used in {:.0}% of turns, deferred to keep the request in budget",
                    share * 100.0
                )
            }
            Why::Unused { days } => format!("unused for {days} days"),
            Why::Deferred => "deferred".into(),
        }
    }
}

/// The choice for one place kind: each known family with its reason.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    pub families: BTreeMap<String, Why>,
}

impl Selection {
    /// The families that load.
    pub fn loaded(&self) -> BTreeSet<String> {
        self.families
            .iter()
            .filter(|(_, w)| w.loads())
            .map(|(f, _)| f.clone())
            .collect()
    }
}

/// Choose the families of `known` that `kind` loads at `now_ms`.
/// `extra` is the seam for what a message adds (Jev's branches, later): those
/// load whatever the ledger says.
pub fn select(
    stats: Option<&Stats>,
    kind: PlaceKind,
    cfg: &DeferConfig,
    now_ms: u64,
    known: &BTreeSet<String>,
    extra: &BTreeSet<String>,
) -> Selection {
    let window_ms = u64::from(cfg.window_days) * DAY_MS;
    let hide_ms = u64::from(cfg.hide_after_days) * DAY_MS;
    let window_from = now_ms.saturating_sub(window_ms);
    let hide_from = now_ms.saturating_sub(hide_ms);
    let turns = stats.map_or(0, |s| s.turns_since(window_from));
    let used = stats.map_or_else(BTreeMap::new, |s| s.sum_since(window_from, |d| &d.used));
    // How long the ledger has watched this kind, and its turns in the hide
    // window: hiding needs both, so an empty ledger hides nothing.
    let watched = stats
        .and_then(|s| s.first_ms)
        .is_some_and(|f| f <= hide_from);
    let recent_turns = stats.map_or(0, |s| s.turns_since(hide_from));
    let defaults = kind.defaults();
    let mut out = Selection::default();
    for fam in known {
        let why = if !family::hideable(fam) || cfg.always_loaded.iter().any(|a| a == fam) {
            Why::Always
        } else if extra.contains(fam) {
            Why::Extra
        } else if turns >= MIN_TURNS
            && f64::from(used.get(fam).copied().unwrap_or(0)) / f64::from(turns) >= cfg.load_share
            && used.get(fam).copied().unwrap_or(0) > 0
        {
            Why::Learned {
                share: f64::from(used[fam]) / f64::from(turns),
            }
        } else if defaults.contains(&fam.as_str()) {
            let last = stats.and_then(|s| s.last_used.get(fam)).copied();
            let idle = last.is_none_or(|l| l < hide_from);
            if watched && recent_turns >= MIN_TURNS && idle {
                let since = last.or(stats.and_then(|s| s.first_ms)).unwrap_or(now_ms);
                Why::Unused {
                    days: now_ms.saturating_sub(since) / DAY_MS,
                }
            } else {
                Why::Default
            }
        } else {
            Why::Deferred
        };
        out.families.insert(fam.clone(), why);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 100 * DAY_MS;

    fn known() -> BTreeSet<String> {
        [
            "files", "commands", "web", "memory", "tasks", "aws", "other",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn cfg() -> DeferConfig {
        DeferConfig::default()
    }

    /// A kind watched for `days` days, `per_day` turns each, in which the
    /// listed families were used in `share` of the turns.
    fn watched(days: u64, per_day: u32, used: &[(&str, u32)]) -> Stats {
        let mut s = Stats::default();
        for d in 0..days {
            let at = NOW - d * DAY_MS - 1000;
            let day = s.day_mut(at);
            day.turns += per_day;
            for (f, n) in used {
                *day.used.entry(f.to_string()).or_insert(0) += n;
            }
            for (f, _) in used {
                let last = s.last_used.entry(f.to_string()).or_insert(0);
                *last = (*last).max(at);
            }
        }
        s
    }

    fn pick(sel: &Selection, f: &str) -> Why {
        sel.families[f].clone()
    }

    #[test]
    fn with_no_ledger_the_defaults_load_and_the_rest_defer() {
        let none = BTreeSet::new();
        let sel = select(None, PlaceKind::Dm, &cfg(), NOW, &known(), &none);
        assert_eq!(pick(&sel, "files"), Why::Always);
        assert_eq!(pick(&sel, "commands"), Why::Always);
        assert_eq!(pick(&sel, "other"), Why::Always);
        assert_eq!(pick(&sel, "web"), Why::Default);
        assert_eq!(pick(&sel, "aws"), Why::Deferred);
        let loaded = sel.loaded();
        assert!(loaded.contains("memory") && !loaded.contains("aws"));
    }

    #[test]
    fn a_family_used_in_enough_of_the_turns_loads() {
        let none = BTreeSet::new();
        // aws in 2 of 10 turns a day over two weeks: 20%, over the 10% line.
        let s = watched(14, 10, &[("aws", 2)]);
        let sel = select(Some(&s), PlaceKind::Cli, &cfg(), NOW, &known(), &none);
        assert!(matches!(pick(&sel, "aws"), Why::Learned { share } if (share - 0.2).abs() < 1e-9));
        // 1 in 20: 5%, under it.
        let s = watched(14, 20, &[("aws", 1)]);
        let sel = select(Some(&s), PlaceKind::Cli, &cfg(), NOW, &known(), &none);
        assert_eq!(pick(&sel, "aws"), Why::Deferred);
    }

    #[test]
    fn a_few_turns_learn_nothing() {
        let none = BTreeSet::new();
        // 5 turns, all using aws: below MIN_TURNS.
        let s = watched(1, 5, &[("aws", 5)]);
        let sel = select(Some(&s), PlaceKind::Cli, &cfg(), NOW, &known(), &none);
        assert_eq!(pick(&sel, "aws"), Why::Deferred);
    }

    #[test]
    fn a_default_unused_for_the_hide_days_hides() {
        let none = BTreeSet::new();
        // The kind has been watched 30 days, 10 turns a day, using only tasks.
        let mut s = watched(30, 10, &[("tasks", 5)]);
        // Memory was used once, 20 days ago: older than the 14.
        s.last_used.insert("memory".into(), NOW - 20 * DAY_MS);
        let sel = select(Some(&s), PlaceKind::Dm, &cfg(), NOW, &known(), &none);
        assert!(matches!(pick(&sel, "memory"), Why::Unused { days } if days >= 19));
        assert!(!sel.loaded().contains("memory"));
        // Web too: never used in the 30 days watched.
        assert!(matches!(pick(&sel, "web"), Why::Unused { .. }));
        // The always-loaded families never hide.
        assert_eq!(pick(&sel, "files"), Why::Always);
    }

    #[test]
    fn a_ledger_too_young_to_judge_hides_nothing() {
        let none = BTreeSet::new();
        // Three days watched: fewer than hide_after_days.
        let s = watched(3, 50, &[("tasks", 5)]);
        let sel = select(Some(&s), PlaceKind::Dm, &cfg(), NOW, &known(), &none);
        assert_eq!(pick(&sel, "web"), Why::Default);
        assert_eq!(pick(&sel, "memory"), Why::Default);
    }

    #[test]
    fn use_older_than_the_window_does_not_count() {
        let none = BTreeSet::new();
        let mut s = Stats::default();
        // Heavy aws use 40 days ago, none since.
        let old = s.day_mut(NOW - 40 * DAY_MS);
        old.turns = 100;
        old.used.insert("aws".into(), 100);
        let recent = s.day_mut(NOW - DAY_MS);
        recent.turns = 20;
        let sel = select(Some(&s), PlaceKind::Cli, &cfg(), NOW, &known(), &none);
        assert_eq!(pick(&sel, "aws"), Why::Deferred);
    }

    #[test]
    fn the_per_message_seam_and_the_always_list_win() {
        let extra: BTreeSet<String> = ["aws".to_string()].into();
        let sel = select(None, PlaceKind::Headless, &cfg(), NOW, &known(), &extra);
        assert_eq!(pick(&sel, "aws"), Why::Extra);
        let mut c = cfg();
        c.always_loaded.push("web".into());
        let sel = select(
            None,
            PlaceKind::Headless,
            &c,
            NOW,
            &known(),
            &BTreeSet::new(),
        );
        assert_eq!(pick(&sel, "web"), Why::Always);
    }

    #[test]
    fn trimming_keeps_the_recent_days() {
        let mut s = watched(60, 1, &[]);
        s.trim(NOW, 30);
        assert!(s.days.len() <= 31);
        assert!(s.days.keys().all(|d| *d >= NOW / DAY_MS - 30));
    }
}
