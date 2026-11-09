//! The fan-out cue (theseus-z9mze.4, promoted by theseus-h66v4.6): after the route verdict, a turn whose
//! work Jev labelled `splits_by_area` is told it may fan out, and the helpers family (`task_fan_out`,
//! `task_wait`) is admitted for it. The owner's word (2026-10-30 16:01): "jev should also classify a fan out;
//! that would be the label that would trigger the llm to then use async calls and consider which were likely
//! to be embarrassingly parallel." Jev labels; the model decides how to split.
//!
//! The cue was a class match (`deep_coding`, `review`, `sophisticated`) plus a list of open-ended words
//! (`OPEN_WORDS`); both are deleted, and with them the cue's own test set of forty asks (it measured a word
//! list; `split.v1.cases.toml` carries the same forty as the label's cases). Three things must hold now: the
//! verdict carries `split.v1`'s answer and it is `splits_by_area` at the question's confirm bar; the turn is
//! not a task's; and there is room for two helpers, which is what `task_fan_out` itself would ask for (a task
//! turn's free admission slots, and a carve of half what the session may spend that affords two helpers at its
//! floor). Those last two are limits of the machinery, not readings of the ask.
//!
//! Every verdict that carries a split answer is a `fan_out.cue` row, fired or not, so the misses can be read
//! beside the hits. The line the model reads is one tail block (`attach_hint`, as the coding hint is), never
//! the cached prefix. Under route.v8 no verdict carries a split answer, so the cue is silent there: the word
//! list that fired it is gone, and that is a deliberate line, not a gap.

use crate::compiler::{Compiled, RequestSpec};
use crate::fact::fan_out::FanOutCue;
use crate::routing::Verdict;
use crate::tools_defer::{family, fit::Fit, want};
use crate::turn::{Turn, TurnRunner};

/// The option of `split.v1` that is the cue.
pub const SPLITS: &str = "splits_by_area";

/// The line a firing cue adds to the request's tail: the owner's hint to the model, then how.
pub const LINE: &str = concat!("This request looks parallelizable: consider starting tasks for the independent parts. \
`task_fan_out` starts helpers that each read one area at once and `task_wait` collects their reports \
(`tools_find` loads them if they are not listed). Do it only when the areas are independent; a small job is \
quicker done yourself. ",
    crate::content::context_not_the_owners!(),
    " Do not mention it to them.");

/// What the cue reads, and nothing else.
#[derive(Clone, Copy)]
pub struct Look {
    /// Jev's `split` answer is `splits_by_area` and reached its confirm bar.
    pub splits: bool,
    pub task: bool,
    pub room: bool,
}

/// Whether the verdict's split answer is the cue: `splits_by_area`, at the bar.
pub fn splits(v: &Verdict) -> bool {
    v.split
        .as_deref()
        .is_some_and(|p| p.sure && p.choice == SPLITS)
}

/// Whether all three conditions hold.
pub fn decide(look: &Look) -> bool {
    look.splits && !look.task && look.room
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
    /// line, `fan_out_line`) and admit the helpers family into the request's deferral. A verdict
    /// with no split answer (route.v8, a pinned turn, a failed ask) writes nothing.
    pub(in crate::turn) fn fan_out_cue(
        &self,
        t: &mut Turn<'_>,
        spec: &mut RequestSpec,
        verdict: Option<&Verdict>,
    ) {
        let Some((v, pick)) = verdict.and_then(|v| Some((v, v.split.as_deref()?))) else {
            return;
        };
        let look = Look {
            splits: splits(v),
            task: t.tc.task.is_some(),
            room: self.room_for_helpers(t),
        };
        let fired = decide(&look);
        t.route.fan_out = fired;
        t.record(&FanOutCue {
            split: &pick.choice,
            confidence: pick.confidence,
            sure: pick.sure,
            fired,
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

    fn verdict(split: Option<(&str, bool)>) -> Verdict {
        Verdict {
            mode: "sonnet".into(),
            confidence: 0.9,
            effort: None,
            judgment: "jdg_x".into(),
            turn: "trn_x".into(),
            persona: None,
            seal: None,
            personal: None,
            model: Some(Box::new(crate::routing::Pick {
                choice: "sonnet".into(),
                confidence: 0.9,
                sure: true,
                no_match: Some("sonnet".into()),
            })),
            work: None,
            // The cue reads only a sure answer, so `split.v1`'s declared
            // default does not reach it: theseus-ig13o builds the mechanism
            // and converts `model`, `effort` and `course`, and leaves this
            // site named rather than quietly changed.
            split: split.map(|(c, sure)| {
                Box::new(crate::routing::Pick {
                    choice: c.into(),
                    confidence: 0.9,
                    sure,
                    no_match: None,
                })
            }),
        }
    }

    #[test]
    fn only_a_sure_splits_by_area_is_the_cue() {
        assert!(splits(&verdict(Some(("splits_by_area", true)))));
        // Under the confirm bar it is not a label to act on.
        assert!(!splits(&verdict(Some(("splits_by_area", false)))));
        for other in ["one_area", "unclear"] {
            assert!(!splits(&verdict(Some((other, true)))), "{other}");
        }
        // No answer: route.v8, a pinned turn, a failed ask.
        assert!(!splits(&verdict(None)));
    }

    #[test]
    fn the_three_conditions_each_hold_the_cue_back() {
        let base = Look {
            splits: true,
            task: false,
            room: true,
        };
        assert!(decide(&base));
        assert!(!decide(&Look {
            splits: false,
            ..base
        }));
        // A task's own turn never fans out (theseus-z9mze.4: not already a task's).
        assert!(!decide(&Look { task: true, ..base }));
        // And with no room for two helpers the label alone does not fire it.
        assert!(!decide(&Look {
            room: false,
            ..base
        }));
    }

    /// The line says whose it is, so a model does not answer it to the owner (theseus-ectzg).
    #[test]
    fn the_line_says_it_is_the_harnesss_and_not_to_be_acknowledged() {
        assert!(
            LINE.contains(crate::content::CONTEXT_NOT_THE_OWNERS),
            "{LINE}"
        );
        assert!(LINE.contains("Do not mention it"), "{LINE}");
    }

    /// The owner's hint is the first thing the model reads, and no word list decides the cue:
    /// the module holds neither a class name nor an open-word list any more.
    #[test]
    fn the_line_opens_with_the_owners_hint_and_no_word_list_remains() {
        assert!(LINE.starts_with(
            "This request looks parallelizable: consider starting tasks for the independent parts."
        ));
        let src = include_str!("fan_out_cue.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        for gone in [
            "OPEN_WORDS",
            "CLASSES",
            "WORDS_REV",
            "words_in",
            "deep_coding",
        ] {
            let n = code.matches(gone).count();
            // The doc names what was deleted, once each at most.
            assert!(n <= 1, "{gone} appears {n} times in the cue's code");
        }
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
}
