//! The job builder's six owner-material kinds (theseus-ej6bj, route.v10 row 4,
//! design §2 row 13): card, initiative, people, threads, the learn writer and
//! the ontology's naming. Each is one paragraph in the place `job`, and each
//! works on the owner's private material, so each lists US options only
//! whatever the caller's facts say.

use crate::builders::{job_input, JobInput, JobKind, PricePeriod, RestrictedModels, RouteFacts};

fn facts(r: RestrictedModels) -> RouteFacts {
    RouteFacts {
        price_period: PricePeriod::Peak,
        restricted_models: r,
        last_reply_model: None,
        task_models: Vec::new(),
        est_tokens: None,
        current_model: None,
        switch_cost: None,
        cheaper_model: None,
        previous_turn: None,
    }
}

fn owner_kinds() -> Vec<(JobKind, &'static str)> {
    vec![
        (JobKind::Card { clusters: 7 }, "7 clusters"),
        (JobKind::CardObserve { lines: 11 }, "11 lines"),
        (
            JobKind::Initiative {
                kind: "the morning note".into(),
            },
            "the morning note",
        ),
        (JobKind::People { lines: 42 }, "42 lines"),
        (JobKind::Threads { episodes: 9 }, "9 episodes"),
        (
            JobKind::Learn {
                pack: "classify.v4".into(),
            },
            "classify.v4",
        ),
        (
            JobKind::OntologyName {
                kind: "project".into(),
            },
            "project",
        ),
    ]
}

/// Each says its work in one paragraph, naming what it was given.
#[test]
fn each_owner_kind_is_one_paragraph_that_says_its_work() {
    for (kind, said) in owner_kinds() {
        let ji = job_input(&JobInput {
            kind: kind.clone(),
            facts: facts(RestrictedModels::NotAllowed),
        });
        assert_eq!(ji.place_kind, "job", "{kind:?}");
        assert!(ji.message.contains(said), "{kind:?}: {}", ji.message);
        assert!(!ji.message.contains('\n'), "{kind:?}: {}", ji.message);
        assert!(ji.message.split_whitespace().count() >= 8, "{kind:?}");
    }
}

/// The facts go through as the daemon gives them: no job floors
/// `restricted_models` since the restricted-origin rule was deleted (2026-11-21).
#[test]
fn a_jobs_facts_are_the_daemons_own() {
    for (kind, _) in owner_kinds() {
        for r in [RestrictedModels::Allowed, RestrictedModels::NotAllowed] {
            let ji = job_input(&JobInput {
                kind: kind.clone(),
                facts: facts(r),
            });
            assert_eq!(ji.route.unwrap().restricted_models, r, "{kind:?}");
        }
    }
}

/// A part of the paragraph is cut, so a long pack name or kind cannot grow it.
#[test]
fn a_long_free_text_part_is_cut() {
    let long = "x".repeat(10_000);
    for kind in [
        JobKind::Learn { pack: long.clone() },
        JobKind::OntologyName { kind: long.clone() },
        JobKind::Initiative { kind: long },
    ] {
        let ji = job_input(&JobInput {
            kind,
            facts: facts(RestrictedModels::NotAllowed),
        });
        assert!(ji.message.chars().count() < 2_000, "{}", ji.message.len());
    }
}
