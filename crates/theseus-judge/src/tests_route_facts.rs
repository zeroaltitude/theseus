//! route.v10's state facts and the job message builder (theseus-9f72b): the
//! four facts in the inbound state, the 3,000-token cap with all of them at
//! their worst, and a background job as the same ask every other site makes.

use serde_json::Value;

use crate::builders::{
    job, job_input, prepare, InboundInput, Input, JobInput, JobKind, PricePeriod, RestrictedModels,
    RouteFacts, TASK_MODELS,
};
use crate::pack::by_name;
use crate::state::NoScrub;

fn input() -> InboundInput {
    serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap()
}

fn facts() -> RouteFacts {
    RouteFacts {
        price_period: PricePeriod::OffPeak,
        restricted_models: RestrictedModels::Allowed,
        last_reply_model: Some("claude-sonnet-5-5".into()),
        task_models: vec!["claude-sonnet-5-5".into(), "deepseek-flash".into()],
        est_tokens: None,
    }
}

fn state_of(i: InboundInput) -> (Value, Vec<String>, u64) {
    let p = by_name("route.v8").unwrap();
    let b = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    (b.state.value(), b.state.fields.clone(), b.state.tokens)
}

#[test]
fn the_four_facts_reach_the_state() {
    let mut i = input();
    i.route = Some(facts());
    let (v, _, _) = state_of(i);
    assert_eq!(v["price_period"], "off_peak");
    assert_eq!(v["restricted_models"], "allowed");
    assert_eq!(v["last_reply_model"], "claude-sonnet-5-5");
    assert_eq!(
        v["task_models"],
        serde_json::json!(["claude-sonnet-5-5", "deepseek-flash"])
    );
}

#[test]
fn a_state_without_facts_has_none_of_the_four() {
    let (v, _, _) = state_of(input());
    for k in [
        "price_period",
        "restricted_models",
        "last_reply_model",
        "task_models",
    ] {
        assert!(v.get(k).is_none(), "{k} in a state with no facts");
    }
}

/// Outside a task, and before any reply, only the two the daemon always knows.
#[test]
fn the_last_reply_and_the_task_models_are_left_out_when_there_are_none() {
    let mut i = input();
    i.route = Some(RouteFacts {
        last_reply_model: None,
        task_models: Vec::new(),
        restricted_models: RestrictedModels::NotAllowed,
        price_period: PricePeriod::Peak,
        est_tokens: None,
    });
    let (v, _, _) = state_of(i);
    assert_eq!(v["price_period"], "peak");
    assert_eq!(v["restricted_models"], "not_allowed");
    assert!(v.get("last_reply_model").is_none());
    assert!(v.get("task_models").is_none());
}

#[test]
fn facts_parse_from_the_inputs_json_and_a_stray_key_is_refused() {
    let mut v: Value =
        serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap();
    v["route"] = serde_json::json!({"price_period": "peak", "restricted_models": "not_allowed"});
    let i: InboundInput = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(i.route.unwrap().price_period, PricePeriod::Peak);
    v["route"]["cost_now"] = 1.into();
    assert!(serde_json::from_value::<InboundInput>(v).is_err());
}

/// The worst a state can be: a megabyte message, the previous message and
/// reply, 2,000 live tasks, and every fact at its longest. The cap holds, and
/// the facts, which are small and sure, are the last things the cap cuts.
#[test]
fn the_state_stays_under_its_cap_with_every_fact_at_its_worst() {
    let big = "a very long line of text that goes on ".repeat(30_000);
    let item = "an item of two kilobytes or so ".repeat(64);
    let mut i = input();
    i.message = big.clone();
    i.previous_human_message = Some(big.clone());
    i.last_reply = Some(big);
    i.live_tasks = (0..2_000)
        .map(|n| crate::builders::TaskInput {
            id: format!("t{n}"),
            brief: item.clone(),
            state: item.clone(),
        })
        .collect();
    i.route = Some(RouteFacts {
        price_period: PricePeriod::OffPeak,
        restricted_models: RestrictedModels::NotAllowed,
        last_reply_model: Some("m".repeat(5_000)),
        task_models: (0..500)
            .map(|n| format!("{n}-{}", "model".repeat(400)))
            .collect(),
        est_tokens: Some(u64::MAX),
    });
    let p = by_name("route.v8").unwrap();
    assert_eq!(p.state_cap_tokens, 3000);
    let b = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    assert!(b.state.tokens <= 3000, "{} tokens", b.state.tokens);
    for k in [
        "price_period",
        "restricted_models",
        "last_reply_model",
        "task_models",
    ] {
        assert!(b.state.fields.iter().any(|f| f == k), "{k} was cut");
    }
    let v = b.state.value();
    let models = v["task_models"].as_array().unwrap();
    assert_eq!(models.len(), TASK_MODELS);
    assert!(models
        .iter()
        .all(|m| m.as_str().unwrap().chars().count() <= 60));
}

fn kinds() -> Vec<JobKind> {
    vec![
        JobKind::Compaction {
            oldest_tokens: 12_000,
            summary_tokens: 800,
            labels: vec!["code".into(), "confidential".into()],
        },
        JobKind::Synthesis {
            subject: "the tide tables".into(),
            sources: 3,
            owner_material: false,
        },
        JobKind::SelfBuilder {
            brief: "retry the flaky fetch\nwith a shorter wait".into(),
            private: false,
        },
        JobKind::SelfReviewer {
            diff_summary: "adds a retry to fetch.rs".into(),
            builder_model: Some("claude-sonnet-5-5".into()),
            private: false,
        },
    ]
}

fn job_in(kind: JobKind, f: RouteFacts) -> JobInput {
    JobInput { kind, facts: f }
}

#[test]
fn a_job_is_one_paragraph_asked_in_place_job_with_the_facts() {
    for kind in kinds() {
        let ji = job_input(&job_in(kind.clone(), facts()));
        assert_eq!(ji.place_kind, "job", "{kind:?}");
        assert!(!ji.message.trim().is_empty(), "{kind:?}");
        assert!(!ji.message.contains('\n'), "{kind:?}: {}", ji.message);
        assert!(ji.live_tasks.is_empty() && ji.live_threads.is_empty());
        assert_eq!(
            ji.route.as_ref().unwrap().price_period,
            PricePeriod::OffPeak
        );
        // The state, which the route pack reads, carries the place and facts.
        let p = by_name("route.v8").unwrap();
        let b = job(&job_in(kind.clone(), facts()), p.state_cap_tokens, &NoScrub);
        let v = b.state.value();
        assert_eq!(v["place_kind"], "job", "{kind:?}");
        assert_eq!(v["price_period"], "off_peak", "{kind:?}");
        assert!(b.state.tokens <= p.state_cap_tokens);
    }
}

#[test]
fn each_jobs_message_says_its_work() {
    let m = |k: JobKind| job_input(&job_in(k, facts())).message;
    let ks = kinds();
    let c = m(ks[0].clone());
    assert!(c.contains("oldest 12000 tokens"), "{c}");
    assert!(c.contains("at most 800 tokens"), "{c}");
    assert!(c.contains("code, confidential"), "{c}");
    let s = m(ks[1].clone());
    assert!(
        s.contains("3 sources") && s.contains("the tide tables"),
        "{s}"
    );
    let b = m(ks[2].clone());
    assert!(
        b.contains("retry the flaky fetch with a shorter wait"),
        "{b}"
    );
    let r = m(ks[3].clone());
    assert!(r.contains("adds a retry to fetch.rs"), "{r}");
}

/// The reviewer's state names the builder's model beside the task's, so the
/// pack can see who wrote the change.
#[test]
fn the_reviewers_state_names_the_builders_model() {
    let mut f = facts();
    f.task_models = Vec::new();
    let ji = job_input(&job_in(kinds()[3].clone(), f));
    assert_eq!(
        ji.route.unwrap().task_models,
        vec!["claude-sonnet-5-5".to_string()]
    );
}

/// The owner-material floor: private material lists US options only, and a
/// caller's `allowed` cannot widen it.
#[test]
fn private_material_is_never_widened_by_the_facts() {
    let private = [
        JobKind::Synthesis {
            subject: "owner notes".into(),
            sources: 2,
            owner_material: true,
        },
        JobKind::SelfBuilder {
            brief: "x".into(),
            private: true,
        },
        JobKind::SelfReviewer {
            diff_summary: "x".into(),
            builder_model: None,
            private: true,
        },
    ];
    for kind in private {
        let mut f = facts();
        f.restricted_models = RestrictedModels::Allowed;
        let ji = job_input(&job_in(kind.clone(), f));
        assert_eq!(
            ji.route.unwrap().restricted_models,
            RestrictedModels::NotAllowed,
            "{kind:?}"
        );
    }
}

/// And the facts never loosen themselves: `not_allowed` stays for any job.
#[test]
fn a_job_never_loosens_not_allowed() {
    for kind in kinds() {
        let mut f = facts();
        f.restricted_models = RestrictedModels::NotAllowed;
        let ji = job_input(&job_in(kind.clone(), f));
        assert_eq!(
            ji.route.unwrap().restricted_models,
            RestrictedModels::NotAllowed,
            "{kind:?}"
        );
    }
}

#[test]
fn a_huge_job_stays_under_the_cap() {
    let big = "a very long line of text that goes on ".repeat(30_000);
    let ji = job_in(
        JobKind::SelfBuilder {
            brief: big,
            private: false,
        },
        facts(),
    );
    let b = job(&ji, 3000, &NoScrub);
    assert!(b.state.tokens <= 3000, "{} tokens", b.state.tokens);
    assert!(b.state.fields.iter().any(|f| f == "price_period"));
}
