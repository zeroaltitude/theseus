//! Issue 57, A4: which settings cards take the owner's authenticator code, through
//! a real `config.propose` on a real settings file (the rig of `tests.rs`).
//!
//! A sensitive setting keeps its card; the code is kept for the gates, secrets and
//! money. A spending limit goes without it only when the change makes it tighter
//! by what it means: zero that means "never stop" is no limit at all, a removal
//! and a value set where there was none keep the code, and so does a catalog
//! price in either direction. The stricter rule wins in a change of several.

use serde_json::{json, Value};

use super::tests::{change, rig, Rig, FILE};
use crate::node::ResultStatus;

/// The settings file with `extra` after it.
fn file_with(r: &Rig, extra: &str) {
    std::fs::write(&r.file, format!("{FILE}\n{extra}")).unwrap();
}

/// Propose `call`, and the card it asked (declined again, so the next may ask).
async fn card_of(r: &Rig, call: Value) -> theseus_protocol::ConfirmRequest {
    let sid = r.session();
    r.turn(&sid, call.clone()).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{call}: {text}");
    let q = r
        .asked()
        .into_iter()
        .find(|q| q.session_id == sid)
        .expect("a card");
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
    q
}

/// Each of the three limits where zero means "never stop" keeps the code when it
/// is set to zero: that lifts the stop. Lowering each to a number above zero
/// takes the card alone.
#[tokio::test]
async fn a_spending_stop_set_to_zero_needs_the_code() {
    for (key, was, lower) in [
        ("spend_limit_hard_multiple", "10", json!(5)),
        ("daily_runaway_usd", "2000.0", json!(1000.0)),
        ("runaway_usd_per_10min", "200.0", json!(100.0)),
    ] {
        let r = rig();
        file_with(&r, &format!("[kernel]\n{key} = {was}\n"));
        let path = format!("kernel.{key}");
        let zero = match key {
            "spend_limit_hard_multiple" => json!(0),
            _ => json!(0.0),
        };
        let q = card_of(&r, change(&path, zero)).await;
        assert!(q.needs_proof, "{key} to 0: {}", q.reason);
        assert!(
            q.reason
                .contains(&format!("need a second proof as well as your yes: {path}")),
            "{}",
            q.reason
        );
        let q = card_of(&r, change(&path, lower)).await;
        assert!(!q.needs_proof, "{key} lowered: {}", q.reason);
        assert!(!q.reason.contains("second proof"), "{}", q.reason);
    }
}

/// A removal, and a value set where the file had none, cannot be read as a
/// tightening (the default may be higher), so they keep the code.
#[tokio::test]
async fn a_removed_limit_and_one_set_where_there_was_none_need_the_code() {
    let r = rig();
    file_with(&r, "[kernel]\ndaily_spend_ceiling_usd = 300.0\n");
    let removed = json!({
        "changes": [{"path": "kernel.daily_spend_ceiling_usd", "remove": true}],
        "reason": "the owner asked for it",
    });
    let q = card_of(&r, removed).await;
    assert!(q.needs_proof, "removed: {}", q.reason);
    let r = rig();
    let q = card_of(&r, change("kernel.daily_spend_ceiling_usd", json!(150.0))).await;
    assert!(q.needs_proof, "set from the default: {}", q.reason);
}

/// A catalog price decides what a call is charged and reserved: set to zero, or
/// raised, it needs the code.
#[tokio::test]
async fn a_catalog_price_set_to_zero_needs_the_code() {
    let r = rig();
    let path = "catalog.claude-sonnet-5-5.input_per_mtok";
    let q = card_of(&r, change(path, json!(0.0))).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason
            .contains("a price that decides what a model call is charged or reserved"),
        "{}",
        q.reason
    );
    let q = card_of(&r, change(path, json!(90.0))).await;
    assert!(q.needs_proof, "raised: {}", q.reason);
}

/// One change of two settings: a limit lowered (card alone) and a stop lifted
/// (code). The stricter rule wins.
#[tokio::test]
async fn in_a_change_of_several_the_stricter_rule_wins() {
    let r = rig();
    file_with(
        &r,
        "[kernel]\ndaily_spend_ceiling_usd = 300.0\ndaily_runaway_usd = 2000.0\n",
    );
    let both = json!({
        "changes": [
            {"path": "kernel.daily_spend_ceiling_usd", "value": 250.0},
            {"path": "kernel.daily_runaway_usd", "value": 0.0},
        ],
        "reason": "the owner asked for it",
    });
    let q = card_of(&r, both).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason.contains("kernel.daily_runaway_usd") && q.input["paths"].as_array().unwrap().len() == 2,
        "{}",
        q.reason
    );
}

/// A folder, prompt text, an address and a provider keep their card, with no
/// code (moved here from the second-proof test of `tests.rs`).
#[tokio::test]
async fn a_folder_prompt_text_an_address_and_a_provider_ask_with_the_card_alone() {
    let r = rig();
    for call in [
        change("tools.roots", json!(["/tmp/elsewhere"])),
        change("model.system", json!("be brief")),
        change("discord.rest_proxy", json!("evil.test:80")),
        change("model.provider", json!("anthropic")),
    ] {
        let q = card_of(&r, call.clone()).await;
        assert!(!q.needs_proof && !q.needs_fresh_proof, "{call}");
        assert!(!q.reason.contains("second proof"), "{call}: {}", q.reason);
    }
    assert_eq!(r.file_text(), FILE);
}
