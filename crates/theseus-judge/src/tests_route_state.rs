//! One definition of the route state (theseus-h66v4 row 3, finding B): what a
//! replay case builds and what the daemon sends are held to one list of fields,
//! so a replay cannot pass against a state shape the daemon never sends.

use crate::builders::{
    route_input, route_state, PricePeriod, RestrictedModels, RouteFacts, SwitchCost,
    EFFORT_STATE_KEYS, ROUTE_STATE_KEYS, WORK_STATE_KEYS,
};
use crate::state::NoScrub;
use serde_json::Value;

fn facts() -> RouteFacts {
    RouteFacts {
        price_period: PricePeriod::OffPeak,
        restricted_models: RestrictedModels::Allowed,
        last_reply_model: Some("claude-sonnet-5-5".into()),
        task_models: Vec::new(),
        est_tokens: Some(140_000),
        current_model: Some("claude-sonnet-5-5".into()),
        switch_cost: Some(SwitchCost::CheaperToStay),
        cheaper_model: None,
    }
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn a_cases_state_has_exactly_the_fields_the_list_names() {
    let i = route_input("thank you!", "discord_dm", facts());
    let p = route_state(&i, 3000, &NoScrub);
    let v = p.state.value();
    assert_eq!(keys(&v), ROUTE_STATE_KEYS, "{v}");
    assert_eq!(
        v["est_tokens"], 140_000,
        "the context size Jev reads the table against"
    );
    assert_eq!(v["current_model"], "claude-sonnet-5-5");
    assert_eq!(v["switch_cost"], "cheaper_to_stay");
    assert_eq!(v["author"], "operator");
    assert_eq!(v["place_kind"], "discord_dm");
}

#[test]
fn the_list_is_sorted_and_free_of_duplicates() {
    let mut s = ROUTE_STATE_KEYS.to_vec();
    s.sort_unstable();
    s.dedup();
    assert_eq!(s, ROUTE_STATE_KEYS);
}

#[test]
fn a_task_adds_its_models_and_no_other_field() {
    let mut f = facts();
    f.task_models = vec!["claude-sonnet-5-5".into(), "deepseek-flash".into()];
    let p = route_state(&route_input("brief", "task_brief", f), 3000, &NoScrub);
    let mut want: Vec<String> = ROUTE_STATE_KEYS.iter().map(|s| s.to_string()).collect();
    want.push("task_models".into());
    want.sort();
    assert_eq!(keys(&p.state.value()), want);
}

/// theseus-h66v4.8: the daemon names the cheaper model beside the word, and the
/// name is the only field that arrives with it. A state that says only
/// `cheaper_to_switch` does not say what to switch to.
#[test]
fn a_cheaper_model_is_the_only_field_a_switch_adds() {
    let mut f = facts();
    f.switch_cost = Some(SwitchCost::CheaperToSwitch);
    f.cheaper_model = Some("deepseek_flash (off-peak)".into());
    let i = route_input("rename it everywhere", "discord_dm", f);
    let v = route_state(&i, 3000, &NoScrub).state.value();
    let mut want: Vec<String> = ROUTE_STATE_KEYS.iter().map(|s| s.to_string()).collect();
    want.push("cheaper_model".into());
    want.sort();
    assert_eq!(keys(&v), want, "{v}");
    assert_eq!(v["switch_cost"], "cheaper_to_switch");
    assert_eq!(v["cheaper_model"], "deepseek_flash (off-peak)");
    // And it is absent where the daemon names none, as `facts()` does.
    let stay = route_state(
        &route_input("thanks!", "discord_dm", facts()),
        3000,
        &NoScrub,
    );
    assert!(stay.state.value().get("cheaper_model").is_none());
}

#[test]
fn the_estimate_is_left_out_where_there_is_none() {
    let mut f = facts();
    f.est_tokens = None;
    let p = route_state(&route_input("x", "job", f), 3000, &NoScrub);
    assert!(p.state.value().get("est_tokens").is_none());
}

/// theseus-h66v4.6: the other two packs of the family read their own slice of the same input, and
/// nothing the daemon knows about models or prices reaches either.
#[test]
fn the_effort_and_work_slices_are_their_own_states() {
    use crate::builders::{InboundInput, TaskInput};
    let mut f = facts();
    // A state that carries a cheaper model, so the check below is not vacuous.
    f.switch_cost = Some(SwitchCost::CheaperToSwitch);
    f.cheaper_model = Some("deepseek_flash (off-peak)".into());
    let mut i = route_input("thank you!", "discord_dm", f);
    i.previous_human_message = Some("did the build pass?".into());
    i.last_reply = Some("yes, it passed".into());
    i.live_tasks = vec![TaskInput {
        id: "t-1".into(),
        brief: "run the gate".into(),
        state: "running".into(),
    }];
    let effort = crate::builders::prepare(
        &crate::pack::by_name("effort.v1").unwrap(),
        &crate::builders::Input::Inbound(i.clone()),
        &NoScrub,
    )
    .unwrap();
    assert_eq!(keys(&effort.state.value()), EFFORT_STATE_KEYS, "effort");
    let work = crate::builders::prepare(
        &crate::pack::by_name("work.v1").unwrap(),
        &crate::builders::Input::Inbound(i.clone()),
        &NoScrub,
    )
    .unwrap();
    assert_eq!(keys(&work.state.value()), WORK_STATE_KEYS, "work");
    assert_eq!(work.state.value()["live_tasks"][0]["id"], "t-1");
    // The route slice has the facts and the recent messages, and no live tasks.
    let route = route_state(&i, 3000, &NoScrub).state.value();
    assert!(
        route.get("live_tasks").is_none()
            && route["previous_human_message"] == "did the build pass?"
    );
    // No other pack's slice names a fact the model call alone reads.
    for (name, v) in [
        ("effort", effort.state.value()),
        ("work", work.state.value()),
    ] {
        for k in [
            "price_period",
            "switch_cost",
            "cheaper_model",
            "current_model",
            "restricted_models",
            "est_tokens",
        ] {
            assert!(v.get(k).is_none(), "{name} carries {k}");
        }
    }
    // A bare input still builds all three, and the three states differ.
    let bare = InboundInput {
        message: "hi".into(),
        ..InboundInput::default()
    };
    let states: Vec<String> = ["route.v10", "effort.v1", "work.v1"]
        .iter()
        .map(|p| {
            crate::builders::prepare(
                &crate::pack::by_name(p).unwrap(),
                &crate::builders::Input::Inbound(bare.clone()),
                &NoScrub,
            )
            .unwrap()
            .state
            .json
            .clone()
        })
        .collect();
    assert!(states[0] != states[1] && states[1] != states[2] && states[0] != states[2]);
}
