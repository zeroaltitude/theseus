//! One definition of the route state (theseus-h66v4 row 3, finding B): what a
//! replay case builds and what the daemon sends are held to one list of fields,
//! so a replay cannot pass against a state shape the daemon never sends.

use crate::builders::{
    inbound, route_input, PricePeriod, RestrictedModels, RouteFacts, SwitchCost, ROUTE_STATE_KEYS,
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
    let p = inbound(&i, 3000, &NoScrub);
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
    let p = inbound(&route_input("brief", "task_brief", f), 3000, &NoScrub);
    let mut want: Vec<String> = ROUTE_STATE_KEYS.iter().map(|s| s.to_string()).collect();
    want.push("task_models".into());
    want.sort();
    assert_eq!(keys(&p.state.value()), want);
}

#[test]
fn the_estimate_is_left_out_where_there_is_none() {
    let mut f = facts();
    f.est_tokens = None;
    let p = inbound(&route_input("x", "job", f), 3000, &NoScrub);
    assert!(p.state.value().get("est_tokens").is_none());
}
