//! The static fan-out cue (theseus-z9mze.4): after the route verdict, a turn that may split its work
//! by area is told it can, and the helpers family (`task_fan_out`, `task_wait`) is admitted for it.
//!
//! Four things must hold: the route class is one of `deep_coding`, `review` or `sophisticated`;
//! the ask uses an open-ended word (`OPEN_WORDS`, a list in this file and nothing else: no model,
//! no judge, no spend); the turn is not a task's; and there is room for two helpers, which is what
//! `task_fan_out` itself would ask for (a task turn's free admission slots, and a carve of half
//! what the session may spend that affords two helpers at its floor).
//!
//! Every verdict of an eligible class is a `fan_out.cue` row, fired or not, so the misses can be
//! read beside the hits. The line the model reads is one tail block (`attach_hint`, as the
//! coding hint is), never the cached prefix.

use crate::compiler::{Compiled, RequestSpec};
use crate::fact::fan_out::FanOutCue;
use crate::tools_defer::{family, fit::Fit, want};
use crate::turn::{Turn, TurnRunner};

/// The route classes (the verdict's `mode`) that may fan out.
pub const CLASSES: [&str; 3] = ["deep_coding", "review", "sophisticated"];

/// The revision of `OPEN_WORDS`, written on every row (`words_rev`).
pub const WORDS_REV: u32 = 1;

/// Words that open a piece of work out across many places: asking for these, a person usually means
/// more areas than one turn reads well. Whole words, lower case; a word is said when it is a token
/// of the ask.
pub const OPEN_WORDS: &[&str] = &[
    "audit",
    "survey",
    "explore",
    "investigate",
    "inventory",
    "across",
    "everywhere",
    "throughout",
    "comprehensive",
    "exhaustive",
    "exhaustively",
    "thoroughly",
    "every",
    "each",
    "entire",
    "whole",
    "all",
];

/// The line a firing cue adds to the request's tail.
pub const LINE: &str = "You may fan out: if this work divides by area, `task_fan_out` starts helpers that \
each read one area at once and `task_wait` collects their reports (`tools_find` loads them if they are \
not listed). Do it only when the areas are independent; a small job is quicker done yourself.";

/// The open-ended words `ask` uses, in the list's order.
pub fn words_in(ask: &str) -> Vec<&'static str> {
    let said: Vec<String> = ask
        .chars()
        .take(want::ASK_CHARS)
        .collect::<String>()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_string)
        .collect();
    OPEN_WORDS
        .iter()
        .copied()
        .filter(|w| said.iter().any(|s| s == w))
        .collect()
}

/// What the cue reads, and nothing else.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub class: &'a str,
    pub ask: &'a str,
    pub task: bool,
    pub room: bool,
}

/// Whether the class is one that may fan out.
pub fn eligible(class: &str) -> bool {
    CLASSES.contains(&class)
}

/// The cue's verdict: its words, and whether all four conditions hold.
pub fn decide(look: &Look<'_>) -> (Vec<&'static str>, bool) {
    let words = words_in(look.ask);
    let fired = eligible(look.class) && !words.is_empty() && !look.task && look.room;
    (words, fired)
}

/// Room for two helpers: two task-class slots free beside this turn and none waiting, and a carve
/// that affords two at the floor (`total` is the carve `task_fan_out` would make).
pub fn room_for_two(slots_free: u32, waiting: usize, total_micros: u64, floor_micros: u64) -> bool {
    slots_free >= 2 && waiting == 0 && total_micros / 2 >= floor_micros.max(1)
}

impl TurnRunner {
    /// Whether this turn's session has room for two helpers: the slots the admission queue gives
    /// task turns, and the carve `task_fan_out`'s own split would make (half of what is left, or of
    /// the limit under a limit that notifies) against its floor.
    fn room_for_helpers(&self, t: &Turn<'_>) -> bool {
        let Ok(Some(parent)) = t.tc.kernel.execution(t.tc.execution_id) else {
            return false;
        };
        let left = parent.budget.available();
        let share = match t.tc.kernel.overdraws(&parent) {
            true => left.max(parent.budget.limit_micros),
            false => left,
        };
        let model = t.target.model.as_str();
        let call = self.catalog.get(model).map_or(0, |p| {
            2 * p.reserve_micros(
                self.cfg.tools.helper_max_output_tokens,
                crate::task_fan_out::FLOOR_CONTEXT_TOKENS,
            )
        });
        let floor = theseus_kernel::usd_to_micros(self.cfg.tools.helper_floor_usd).max(call);
        let adm = &self.tools.admission;
        let (in_use, waiting) = adm.load();
        room_for_two(
            adm.task_limit().saturating_sub(in_use),
            waiting,
            share / 2,
            floor,
        )
    }

    /// After the route verdict: look, write the row, and on a fire note it on the turn (the tail
    /// line, `fan_out_line`) and admit the helpers family into the request's deferral.
    pub(in crate::turn) fn fan_out_cue(
        &self,
        t: &mut Turn<'_>,
        spec: &mut RequestSpec,
        class: Option<&str>,
    ) {
        let Some(class) = class.filter(|c| eligible(c)) else {
            return;
        };
        let look = Look {
            class,
            ask: &t.route.ask,
            task: t.tc.task.is_some(),
            room: self.room_for_helpers(t),
        };
        let (words, fired) = decide(&look);
        t.route.fan_out = fired;
        t.record(&FanOutCue {
            class,
            words: &words,
            fired,
            words_rev: WORDS_REV,
        });
        if fired {
            self.defer_cued(t, spec);
        }
    }

    /// The cue's line, one tail block after the cached prefix, when the cue fired this turn.
    pub(in crate::turn) fn fan_out_line(t: &Turn<'_>, compiled: &mut Compiled) {
        if t.route.fan_out {
            Self::attach_hint(compiled, LINE);
        }
    }

    /// Admit the helpers family into the request's own deferral (`By::Cue`), held to the same
    /// ceiling as any admission; the next compile sees the changed tools, as for a found family.
    pub(in crate::turn) fn defer_cued(&self, t: &Turn<'_>, spec: &mut RequestSpec) {
        let kind = self.place_kind(t.tc.session_id, t.tc.task.is_some());
        let rates = self.catalog.get(&spec.model).map_or_else(
            || crate::catalog::TokenRates::of(&spec.model),
            |e| e.bytes_per_token,
        );
        let multiple = t
            .route
            .injection
            .as_ref()
            .map_or(self.cfg.tools.defer.admit_ceiling, |b| b.admit_ceiling);
        let Some(how) = spec
            .defer
            .as_ref()
            .filter(|d| !d.resolved)
            .map(|d| d.mechanism)
        else {
            return;
        };
        let fit = Fit::of(kind, spec, rates, how, multiple);
        let Some(d) = spec.defer.as_mut() else {
            return;
        };
        if let Some(a) = want::cued(family::HELPERS, &spec.tools, &d.loaded, &fit) {
            d.loaded.insert(a.family.clone());
            d.added.insert(a.family);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 40 asks, each labelled as one that should fire the cue (work that spreads over areas, said
    /// in open-ended words) or not. Offline: the cue is a word list, so this set is the whole
    /// check of its precision and recall, with no model and no spend.
    const SET: [(&str, bool); 40] = [
        ("audit every crate for unwrap calls on user input", true),
        (
            "survey the repository and report which modules lack tests",
            true,
        ),
        (
            "explore how errors propagate across the daemon and the cli",
            true,
        ),
        (
            "investigate why the startup is slow and check each phase",
            true,
        ),
        (
            "review all the open changes under crates and list the risky ones",
            true,
        ),
        (
            "inventory the config keys used throughout the workspace",
            true,
        ),
        (
            "do a comprehensive review of the store's recovery paths",
            true,
        ),
        (
            "exhaustively check the protocol methods for missing docs",
            true,
        ),
        ("look everywhere for places that read the wall clock", true),
        ("go through each toollet and say what it refuses", true),
        (
            "find every caller of the retry helper and judge its bound",
            true,
        ),
        ("thoroughly review the admission queue and its tests", true),
        ("check the entire cockpit for stale protocol types", true),
        (
            "read the whole spec chapter set and list contradictions",
            true,
        ),
        ("audit the gate across all the scripts", true),
        ("survey how each provider handles refusals", true),
        ("explore the bench adapters and compare their reports", true),
        ("investigate every failing test in the suite", true),
        (
            "go through the repository and report the dead modules",
            true,
        ),
        (
            "find who calls the retry decision from anywhere in the tree",
            true,
        ),
        ("fix the typo in the readme", false),
        (
            "rename the function load_cfg to load_config in config.rs",
            false,
        ),
        ("why does the build fail with a linker error", false),
        ("what does the steer module do", false),
        ("add a test for the empty case of parse_ask", false),
        ("bump the version line in the first chapter", false),
        ("explain this stack trace", false),
        ("write a commit message for my staged changes", false),
        ("make the footer line one character shorter", false),
        ("run the formatter and tell me what changed", false),
        ("change the default timeout from 30 to 45 seconds", false),
        ("summarise this function in two sentences", false),
        ("revert the last commit", false),
        ("can you check that the socket path is right", false),
        ("open the config and show me the models section", false),
        ("implement the todo on line 120 of kernel.rs", false),
        ("update the changelog with today's fix", false),
        ("make the whole line say ready instead", false),
        ("review my diff for the typo fix", false),
        ("fix all the typos in the readme", false),
    ];

    fn fires(ask: &str) -> bool {
        let look = Look {
            class: "deep_coding",
            ask,
            task: false,
            room: true,
        };
        decide(&look).1
    }

    /// The cue's precision and recall on the labelled set, held at a floor so a word added or cut
    /// that makes it worse fails here. It prints both (`--nocapture`). The bar is design §18.4's:
    /// at least 18 of the 20 labelled-true asks fire and at least 18 of the 20 labelled-false stay
    /// silent. The set is the author's own, so it guards the word list's stability, not the cue's
    /// accuracy.
    #[test]
    fn the_labelled_set_of_forty_asks_scores_as_reported() {
        let (mut tp, mut fp, mut fn_) = (0u32, 0u32, 0u32);
        for (ask, should) in SET {
            match (fires(ask), should) {
                (true, true) => tp += 1,
                (true, false) => {
                    fp += 1;
                    println!("false positive: {ask}");
                }
                (false, true) => {
                    fn_ += 1;
                    println!("miss: {ask}");
                }
                (false, false) => {}
            }
        }
        let precision = f64::from(tp) / f64::from((tp + fp).max(1));
        let recall = f64::from(tp) / f64::from((tp + fn_).max(1));
        println!("precision {precision:.3} recall {recall:.3} (tp {tp}, fp {fp}, fn {fn_})");
        assert_eq!(SET.len(), 40);
        let fired = SET
            .iter()
            .filter(|(ask, should)| *should && fires(ask))
            .count();
        let silent = SET
            .iter()
            .filter(|(ask, should)| !*should && !fires(ask))
            .count();
        assert_eq!(SET.iter().filter(|(_, should)| *should).count(), 20);
        assert!(fired >= 18, "{fired} of the 20 labelled-true asks fired");
        assert!(
            silent >= 18,
            "{silent} of the 20 labelled-false asks stayed silent"
        );
    }

    #[test]
    fn the_four_conditions_each_hold_the_cue_back() {
        let ask = "audit every crate for unwrap";
        let base = Look {
            class: "deep_coding",
            ask,
            task: false,
            room: true,
        };
        assert!(decide(&base).1);
        for class in ["review", "sophisticated"] {
            assert!(decide(&Look { class, ..base }).1, "{class}");
        }
        for class in ["trivial", "quick", "chat", "routine_coding", "other", ""] {
            assert!(!decide(&Look { class, ..base }).1, "{class}");
        }
        assert!(
            !decide(&Look {
                ask: "fix the typo",
                ..base
            })
            .1
        );
        // A task's own turn never fans out (theseus-z9mze.4: not already a task's).
        assert!(!decide(&Look { task: true, ..base }).1);
        // And with no room for two helpers the words alone do not fire it.
        assert!(
            !decide(&Look {
                room: false,
                ..base
            })
            .1
        );
        // The words are returned whether or not it fired, for the row.
        let (words, fired) = decide(&Look {
            room: false,
            ..base
        });
        assert_eq!((words, fired), (vec!["audit", "every"], false));
    }

    #[test]
    fn room_is_two_free_slots_nothing_waiting_and_a_carve_that_affords_two() {
        // 7 slots for tasks, 1 in use: room. 6 in use leaves 1: none.
        assert!(room_for_two(2, 0, 4_000_000, 1_000_000));
        assert!(!room_for_two(1, 0, 4_000_000, 1_000_000));
        assert!(!room_for_two(0, 0, 4_000_000, 1_000_000));
        // A turn waiting for a slot means no room.
        assert!(!room_for_two(5, 1, 4_000_000, 1_000_000));
        // The carve must afford two floors: 2,000,000 / 2 is one floor, and one is short.
        assert!(room_for_two(5, 0, 2_000_000, 1_000_000));
        assert!(!room_for_two(5, 0, 1_999_998, 1_000_000));
    }

    #[test]
    fn words_are_whole_tokens_of_the_ask() {
        assert_eq!(words_in("Audit EVERY crate"), ["audit", "every"]);
        assert!(words_in("the eachother caller, a vacation").is_empty());
        assert!(words_in("").is_empty());
        for w in OPEN_WORDS {
            assert_eq!(*w, w.to_lowercase(), "{w}");
        }
    }
}
