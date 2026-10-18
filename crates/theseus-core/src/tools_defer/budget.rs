//! A learned family never takes a request past its place's budget
//! (theseus-buz3h).
//!
//! The selector is pure over counts and knows no sizes, so it loads every
//! family the ledger shows used. This step, run once a turn after it, puts
//! the request's size back: while the system and the loaded tools are over
//! the place kind's first-request budget (`PlaceKind::tools_budget_tokens`,
//! the owner's 8,000 for cli and task, 15,000 for the DM), the learned
//! families go back to deferred, the largest first, one search away. A
//! default, an always-loaded family and what a message or a plan asked for
//! are never demoted here.

use serde_json::Value;

use super::select::{Selection, Why};
use super::{resolve, tools_bytes, Deferral, Mechanism, PlaceKind};

/// What a request costs besides its tools, and the rate its tools are counted at.
pub struct Fit<'a> {
    pub kind: PlaceKind,
    pub mechanism: Mechanism,
    /// The whole catalog the place is offered.
    pub catalog: &'a [Value],
    /// The system blocks, in tokens.
    pub system_tokens: f64,
    /// Bytes of tool JSON to a token.
    pub json_rate: f64,
}

impl Fit<'_> {
    fn deferral(&self, sel: &Selection) -> Deferral {
        Deferral {
            mechanism: self.mechanism,
            loaded: sel.loaded(),
            resolved: false,
        }
    }

    /// The request's tokens with `sel` loaded: its system and its tools as sent.
    pub fn tokens(&self, sel: &Selection) -> f64 {
        let tools = tools_bytes(&resolve(self.catalog, &self.deferral(sel)));
        self.system_tokens + tools as f64 / self.json_rate
    }
}

/// Demote `sel`'s learned families until the request fits the budget of its
/// place kind: largest first, the lowest use share first among equals.
pub fn demote(sel: &mut Selection, fit: &Fit) {
    let budget = fit.kind.tools_budget_tokens() as f64;
    let learned: Vec<(String, f64)> = sel
        .families
        .iter()
        .filter_map(|(f, w)| match w {
            Why::Learned { share } => Some((f.clone(), *share)),
            _ => None,
        })
        .collect();
    if learned.is_empty() || fit.tokens(sel) <= budget {
        return;
    }
    let bytes = |fam: &str| -> u64 {
        let own: Vec<Value> = fit
            .catalog
            .iter()
            .filter(|t| super::family_of(t) == fam)
            .cloned()
            .collect();
        tools_bytes(&own)
    };
    let mut sized: Vec<(String, f64, u64)> = learned
        .into_iter()
        .map(|(f, share)| {
            let b = bytes(&f);
            (f, share, b)
        })
        .collect();
    sized.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.total_cmp(&b.1)));
    for (fam, share, _) in sized {
        if fit.tokens(sel) <= budget {
            break;
        }
        sel.families.insert(fam, Why::OverBudget { share });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn def(name: &str, pad: usize) -> Value {
        json!({"name": name, "description": "x".repeat(pad), "input_schema": {"type": "object"}})
    }

    fn selection(whys: &[(&str, Why)]) -> Selection {
        Selection {
            families: whys
                .iter()
                .map(|(f, w)| (f.to_string(), w.clone()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    /// The families: files 1 KB, tasks 10 KB, aws 3 KB.
    fn catalog() -> Vec<Value> {
        vec![
            def("fs_read", 1_000),
            def("task_create", 10_000),
            def("aws_call", 3_000),
        ]
    }

    fn fit<'a>(kind: PlaceKind, catalog: &'a [Value], system: f64) -> Fit<'a> {
        Fit {
            kind,
            mechanism: Mechanism::Search,
            catalog,
            system_tokens: system,
            json_rate: 2.4,
        }
    }

    #[test]
    fn learned_loads_stay_inside_the_place_budget() {
        let catalog = catalog();
        let learned = |share| Why::Learned { share };
        let mut sel = selection(&[
            ("files", Why::Always),
            ("tasks", learned(0.5)),
            ("aws", learned(0.2)),
        ]);
        // Everything loaded is 14 KB of tools (5,900 tokens) and 1,300 of system: under 8,000.
        let roomy = fit(PlaceKind::Cli, &catalog, 1_300.0);
        assert!(roomy.tokens(&sel) < 8_000.0);
        let before = sel.clone();
        demote(&mut sel, &roomy);
        assert_eq!(sel, before, "inside the budget, nothing moves");
        // A system of 2,500 tokens leaves no room for tasks: the largest learned family goes first.
        let tight = fit(PlaceKind::Cli, &catalog, 2_500.0);
        assert!(tight.tokens(&sel) > 8_000.0);
        demote(&mut sel, &tight);
        assert_eq!(sel.families["tasks"], Why::OverBudget { share: 0.5 });
        assert_eq!(sel.families["aws"], learned(0.2), "aws fits once tasks is out");
        assert!(tight.tokens(&sel) <= 8_000.0);
        assert!(!sel.loaded().contains("tasks") && sel.loaded().contains("aws"));
        // Nothing fits at all: every learned family goes, and the always-loaded one stays.
        let none = fit(PlaceKind::Cli, &catalog, 7_800.0);
        let mut sel = before;
        demote(&mut sel, &none);
        assert!(matches!(sel.families["tasks"], Why::OverBudget { .. }));
        assert!(matches!(sel.families["aws"], Why::OverBudget { .. }));
        assert_eq!(sel.families["files"], Why::Always);
        // The DM's budget is larger: the same system leaves room there.
        let mut sel = selection(&[("files", Why::Always), ("tasks", learned(0.5))]);
        demote(&mut sel, &fit(PlaceKind::Dm, &catalog, 7_800.0));
        assert_eq!(sel.families["tasks"], learned(0.5));
    }

    #[test]
    fn a_default_is_never_demoted_here() {
        let catalog = catalog();
        let mut sel = selection(&[
            ("files", Why::Always),
            ("tasks", Why::Default),
            ("aws", Why::Extra),
        ]);
        let before = sel.clone();
        demote(&mut sel, &fit(PlaceKind::Cli, &catalog, 9_000.0));
        assert_eq!(sel, before);
    }

    #[test]
    fn the_largest_goes_first_and_a_tie_goes_to_the_least_used() {
        let catalog = vec![
            def("fs_read", 100),
            def("task_create", 6_000),
            def("aws_call", 6_000),
        ];
        let learned = |share| Why::Learned { share };
        let mut sel = selection(&[
            ("files", Why::Always),
            ("tasks", learned(0.9)),
            ("aws", learned(0.2)),
        ]);
        // Both are 6 KB: one of them must go, and it is the one used least.
        demote(&mut sel, &fit(PlaceKind::Cli, &catalog, 4_000.0));
        assert_eq!(sel.families["aws"], Why::OverBudget { share: 0.2 });
        assert_eq!(sel.families["tasks"], learned(0.9));
    }
}
