//! The rung table (own-benchmark): the models the routing table offers,
//! dearest first, and the rung one down from a model. Pure: the profiles and
//! prices are given.
//!
//! The table is the configured profiles that run a model Jev can choose
//! (`routing::model::OPTIONS`), one rung a distinct model, ordered by the
//! catalog's input price. A
//! cell's rung down is the dearest model strictly cheaper than its own; a
//! model the table does not hold has its rung down all the same, from its
//! price, and a cell on the table's cheapest model has none.

use std::collections::BTreeMap;

use crate::config::ProfileConfig;
use crate::routing::model::OPTIONS;

/// One rung: the profile that runs a model, and the model's price.
#[derive(Debug, Clone, PartialEq)]
pub struct Rung {
    pub profile: String,
    pub model: String,
    /// US dollars per million input tokens, then output: the order.
    pub price: (f64, f64),
}

/// Every configured profile that runs a model Jev can choose, the options'
/// order first, then by name.
pub fn profiles_of(profiles: &BTreeMap<String, ProfileConfig>) -> Vec<String> {
    OPTIONS
        .iter()
        .flat_map(|(_, model)| {
            profiles
                .iter()
                .filter(move |(_, p)| p.model == *model)
                .map(|(name, _)| name.clone())
        })
        .collect()
}

/// The table: each profile with its model and price (`of` gives them; none:
/// the profile is not usable and is left out), a model once, dearest first.
pub fn ladder(profiles: &[String], of: impl Fn(&str) -> Option<(String, (f64, f64))>) -> Vec<Rung> {
    let mut out: Vec<Rung> = Vec::new();
    for p in profiles {
        if let Some((model, price)) = of(p) {
            if !out.iter().any(|r| r.model == model) {
                out.push(Rung {
                    profile: p.clone(),
                    model,
                    price,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        b.price
            .partial_cmp(&a.price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// The rung one down from a model priced `price`: the dearest rung strictly
/// cheaper, or none.
pub fn down(ladder: &[Rung], price: (f64, f64)) -> Option<&Rung> {
    ladder.iter().find(|r| r.price < price)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<Rung> {
        let prices = |p: &str| match p {
            "fable" => Some(("fable-5-1".to_string(), (15.0, 75.0))),
            "sonnet" => Some(("sonnet-5-5".to_string(), (3.0, 15.0))),
            "haiku" => Some(("haiku-5-5".to_string(), (1.0, 5.0))),
            "twin" => Some(("sonnet-5-5".to_string(), (3.0, 15.0))),
            _ => None,
        };
        ladder(
            &["haiku", "sonnet", "unknown", "twin", "fable"].map(String::from),
            prices,
        )
    }

    /// Dearest first, a model once (the first profile naming it), an unusable
    /// profile left out.
    #[test]
    fn the_table_is_each_model_once_dearest_first() {
        let t = table();
        let names: Vec<&str> = t.iter().map(|r| r.profile.as_str()).collect();
        assert_eq!(names, ["fable", "sonnet", "haiku"]);
    }

    /// One rung down is the nearest strictly cheaper model; the cheapest has
    /// none; a model off the table gets the rung below its price.
    #[test]
    fn a_rung_down_is_the_nearest_cheaper_model() {
        let t = table();
        assert_eq!(down(&t, (15.0, 75.0)).unwrap().profile, "sonnet");
        assert_eq!(down(&t, (3.0, 15.0)).unwrap().profile, "haiku");
        assert!(down(&t, (1.0, 5.0)).is_none());
        assert_eq!(down(&t, (5.0, 25.0)).unwrap().profile, "sonnet");
    }

    /// Each option's model is reachable through the profiles that run it.
    #[test]
    fn the_options_profiles_are_those_that_run_a_choosable_model() {
        let cfg = crate::Config::example();
        let got = profiles_of(cfg.all_profiles());
        assert!(!got.is_empty());
        assert!(got.iter().any(|p| p == "sonnet"), "{got:?}");
        assert!(!got.iter().any(|p| p == "glm"), "{got:?}");
    }
}
