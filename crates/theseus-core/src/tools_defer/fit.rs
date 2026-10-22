//! Learned loads inside the place's budget (theseus-buz3h).
//!
//! `select` is pure over counts and sees no sizes, so a family the ledger
//! learned loads whatever it costs. The owner's rule is the other way round: the
//! first request is at most the place's budget, and a family is always loaded
//! only with evidence of use. [`Fit`] is what the selection is held to: the
//! request's cost besides its tools, and what each family's tools cost. A
//! learned family that would take the request past the budget is demoted to
//! deferred, largest first, one `tool_search` from loaded again. The
//! defaults, the always-loaded families and what a search or a message asked
//! for are never demoted here.
//!
//! A family a message names, or Jev judges relevant, may take the request past
//! the budget (theseus-xlri5, `want::extra`): by exactly its own size, while the
//! whole stays under `ceiling`. The learned families stay held to the budget
//! plus that raise: admitting one neither evicts a learned family nor lets one
//! ride the raise.

use std::collections::BTreeMap;

use serde_json::Value;

use super::family::{self, PlaceKind};
use super::select::{Selection, Why};
use super::{Mechanism, DEFERRED_BYTES, FIND, SEARCH_BYTES};
use crate::catalog::TokenRates;
use crate::compiler::RequestSpec;

/// Tokens a system block's framing costs (`provider::MESSAGE_TOKENS`).
const BLOCK_TOKENS: u64 = crate::provider::MESSAGE_TOKENS;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fit {
    /// The place kind's budget for a whole first request, in tokens.
    pub budget: u64,
    /// The hard ceiling admitted families may raise it to:
    /// `floor(budget × [tools.defer] admit_ceiling)`.
    pub ceiling: u64,
    /// What the admitted families took the request past the budget, in tokens.
    pub raised: u64,
    /// The system's text, in tokens.
    pub system: u64,
    /// What deferring costs: the search tool and the deferred tools' constant.
    pub held: u64,
    /// The tokens of each family's tool definitions.
    pub cost: BTreeMap<String, u64>,
}

impl Fit {
    /// The fit of a request on `spec` in a `kind` of place, at the model's
    /// `rates`, under a ceiling of `multiple` times the place's budget.
    pub fn of(
        kind: PlaceKind,
        spec: &RequestSpec,
        rates: TokenRates,
        how: Mechanism,
        multiple: f64,
    ) -> Fit {
        let at = |bytes: u64, per: f64| (bytes as f64 / per).ceil() as u64;
        let texts = [
            &spec.system_text,
            &spec.persona_text,
            &spec.context_text,
            &spec.session_text,
        ];
        let system: u64 = texts
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| at(t.len() as u64, rates.text) + BLOCK_TOKENS)
            .sum();
        let held = match how {
            Mechanism::Search => SEARCH_BYTES + DEFERRED_BYTES,
            Mechanism::Find => {
                serde_json::to_string(&super::find_definition()).map_or(0, |s| s.len() as u64)
            }
        };
        let mut cost: BTreeMap<String, u64> = BTreeMap::new();
        for t in &spec.tools {
            let Some(name) = t.get("name").and_then(Value::as_str).filter(|n| *n != FIND) else {
                continue;
            };
            let bytes = serde_json::to_string(t).map_or(0, |s| s.len() as u64);
            *cost.entry(family::of_wire(name)).or_default() += at(bytes, rates.json);
        }
        let budget = kind.tools_budget_tokens();
        Fit {
            budget,
            ceiling: (budget as f64 * multiple).floor() as u64,
            raised: 0,
            system,
            held: at(held, rates.json),
            cost,
        }
    }

    pub fn tokens(&self, family: &str) -> u64 {
        self.cost.get(family).copied().unwrap_or(0)
    }

    /// The whole first request when `families` are loaded: its system, what
    /// deferring costs, and their tools.
    pub fn total<'a>(&self, families: impl IntoIterator<Item = &'a String>) -> u64 {
        self.system + self.held + families.into_iter().map(|f| self.tokens(f)).sum::<u64>()
    }

    /// Demote the learned families of `sel` until what it loads fits the
    /// budget and the raise: the largest first, the least used of equals.
    pub fn demote(&self, sel: &mut Selection) {
        let mut total = self.total(&sel.loaded());
        while total > self.budget + self.raised {
            let next = sel
                .families
                .iter()
                .filter_map(|(f, w)| match w {
                    Why::Learned { share } => Some((f.clone(), *share)),
                    _ => None,
                })
                .max_by(|a, b| {
                    self.tokens(&a.0)
                        .cmp(&self.tokens(&b.0))
                        .then(b.1.total_cmp(&a.1))
                });
            let Some((fam, share)) = next else { break };
            total -= self.tokens(&fam);
            sel.families.insert(fam, Why::OverBudget { share });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn learned(shares: &[(&str, f64)]) -> Selection {
        let mut sel = Selection::default();
        sel.families.insert("files".into(), Why::Always);
        sel.families.insert("memory".into(), Why::Default);
        for (f, share) in shares {
            sel.families
                .insert(f.to_string(), Why::Learned { share: *share });
        }
        sel
    }

    fn fit(budget: u64, costs: &[(&str, u64)]) -> Fit {
        Fit {
            budget,
            ceiling: budget * 2,
            raised: 0,
            system: 900,
            held: 100,
            cost: costs.iter().map(|(f, n)| (f.to_string(), *n)).collect(),
        }
    }

    #[test]
    fn learned_loads_stay_inside_the_place_budget() {
        let costs = [
            ("files", 4_000),
            ("memory", 500),
            ("tasks", 3_000),
            ("aws", 1_200),
        ];
        let mut sel = learned(&[("tasks", 0.3), ("aws", 0.2)]);
        fit(8_000, &costs).demote(&mut sel);
        // 1,000 + 4,000 + 500 + 3,000 + 1,200 is 9,700: the largest learned goes.
        assert_eq!(sel.families["tasks"], Why::OverBudget { share: 0.3 });
        assert_eq!(sel.families["aws"], Why::Learned { share: 0.2 });
        assert!(sel.loaded().contains("aws") && !sel.loaded().contains("tasks"));
        // Defaults and the always-loaded are never demoted, even over the budget.
        let mut sel = learned(&[]);
        fit(100, &costs).demote(&mut sel);
        assert!(sel.loaded().contains("files") && sel.loaded().contains("memory"));
        // Room for all: nothing moves.
        let mut sel = learned(&[("tasks", 0.3), ("aws", 0.2)]);
        let before = sel.clone();
        fit(20_000, &costs).demote(&mut sel);
        assert_eq!(sel, before);
    }

    #[test]
    fn equal_costs_demote_the_least_used_first() {
        let costs = [("tasks", 2_000), ("aws", 2_000)];
        let mut sel = learned(&[("tasks", 0.5), ("aws", 0.1)]);
        fit(4_500, &costs).demote(&mut sel);
        assert!(matches!(sel.families["aws"], Why::OverBudget { .. }));
        assert!(matches!(sel.families["tasks"], Why::Learned { .. }));
    }

    /// theseus-xlri5: with and without 1,200 tokens admitted past the budget,
    /// the same learned families load: the raise is the admitted family's,
    /// never a learned one's, and admitting it evicts none.
    #[test]
    fn a_raise_never_evicts_or_admits_a_learned_family() {
        let costs = [
            ("files", 4_000),
            ("memory", 500),
            ("tasks", 3_000),
            ("aws", 1_200),
            ("terminals", 1_200),
        ];
        // 1,000 + 4,000 + 500 + 1,200 is 6,700 with the learned aws: tasks would pass 8,000.
        let alone = {
            let mut sel = learned(&[("tasks", 0.3), ("aws", 0.2)]);
            fit(8_000, &costs).demote(&mut sel);
            sel
        };
        assert!(alone.loaded().contains("aws") && !alone.loaded().contains("tasks"));
        // The terminals admitted (as `want::extra` adds them, Extra): 7,900, under the
        // budget, raises nothing; the same learned set loads.
        let admitted = |raised: u64, base: u64| {
            let mut sel = learned(&[("tasks", 0.3), ("aws", 0.2)]);
            sel.families.insert("terminals".into(), Why::Extra);
            let mut f = fit(base, &costs);
            f.raised = raised;
            f.demote(&mut sel);
            sel
        };
        let learned_of = |sel: &Selection| -> Vec<String> {
            sel.families
                .iter()
                .filter(|(_, w)| matches!(w, Why::Learned { .. }))
                .map(|(f, _)| f.clone())
                .collect()
        };
        assert_eq!(learned_of(&admitted(0, 8_000)), learned_of(&alone));
        // At a budget of 6,700 the terminals take the request 1,200 past it: raised by
        // exactly that, aws stays and tasks does not ride the raise.
        let base = {
            let mut sel = learned(&[("tasks", 0.3), ("aws", 0.2)]);
            fit(6_700, &costs).demote(&mut sel);
            sel
        };
        assert_eq!(learned_of(&base), learned_of(&alone));
        let raised = admitted(1_200, 6_700);
        assert_eq!(learned_of(&raised), learned_of(&base));
        assert!(raised.loaded().contains("terminals"));
        assert!(!raised.loaded().contains("tasks"));
    }
}
