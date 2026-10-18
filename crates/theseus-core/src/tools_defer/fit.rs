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
    /// The system's text, in tokens.
    pub system: u64,
    /// What deferring costs: the search tool and the deferred tools' constant.
    pub held: u64,
    /// The tokens of each family's tool definitions.
    pub cost: BTreeMap<String, u64>,
}

impl Fit {
    /// The fit of a request on `spec` in a `kind` of place, at the model's `rates`.
    pub fn of(kind: PlaceKind, spec: &RequestSpec, rates: TokenRates, how: Mechanism) -> Fit {
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
        Fit {
            budget: kind.tools_budget_tokens(),
            system,
            held: at(held, rates.json),
            cost,
        }
    }

    fn tokens(&self, family: &str) -> u64 {
        self.cost.get(family).copied().unwrap_or(0)
    }

    /// Demote the learned families of `sel` until what it loads fits: the
    /// largest first, the least used of equals.
    pub fn demote(&self, sel: &mut Selection) {
        let mut total =
            self.system + self.held + sel.loaded().iter().map(|f| self.tokens(f)).sum::<u64>();
        while total > self.budget {
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
}
