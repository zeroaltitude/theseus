//! Issue 57, A4: which settings cards take the owner's authenticator code, through
//! a real `config.propose` on a real settings file (the rig of `tests.rs`), and
//! the yes judged by the same rule the card was asked by.
//!
//! Every sensitive setting keeps its card. The code is dropped only for prompt
//! text and for tightening one of the five listed kernel limits, compared in the
//! kernel's own units; groups G1 to G3 keep it pending the owner's word.

use serde_json::{json, Value};

use super::tests::{change, rig, Rig, FILE};
use crate::node::ResultStatus;
use crate::Config;

/// The settings file with `extra` after it.
fn file_with(r: &Rig, extra: &str) {
    std::fs::write(&r.file, format!("{FILE}\n{extra}")).unwrap();
}

/// Propose `call`, and the card it asked.
async fn card_of(r: &Rig, call: Value) -> theseus_protocol::ConfirmRequest {
    let sid = r.session();
    r.turn(&sid, call.clone()).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{call}: {text}");
    r.asked()
        .into_iter()
        .find(|q| q.session_id == sid)
        .expect("a card")
}

/// The card is declined, so the next may ask.
fn decline(r: &Rig, q: &theseus_protocol::ConfirmRequest) {
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
}

/// What the kernel would hold for the settings file `text`.
fn kernel_of(text: &str) -> theseus_kernel::KernelConfig {
    Config::parse(text).unwrap().0.kernel.to_kernel_config()
}

/// (D) Prompt text asks with its card alone, and the owner's yes alone writes
/// it: the card's mark and the yes's check agree.
#[tokio::test]
async fn prompt_text_takes_the_card_alone_and_the_yes_alone_writes_it() {
    let r = rig();
    let q = card_of(&r, change("model.system", json!("be brief"))).await;
    assert!(!q.needs_proof && !q.needs_fresh_proof, "{q:?}");
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(r.file_text().contains("be brief"), "{}", r.file_text());
}

/// (G1, G2, G3) An address, a provider, a gate and a folder keep the code
/// pending the owner's word: the card is marked, and the yes alone (no app
/// paired) is refused, so nothing is written.
#[tokio::test]
async fn the_pending_groups_keep_the_code_at_the_card_and_at_the_yes() {
    let r = rig();
    for call in [
        change("discord.rest_proxy", json!("proxy.example:80")),
        change("model.provider", json!("anthropic")),
        change("judge.enabled", json!(false)),
        change("tools.roots", json!(["/tmp/elsewhere"])),
    ] {
        let q = card_of(&r, call.clone()).await;
        assert!(q.needs_proof, "{call}: {}", q.reason);
        let e = r
            .core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap_err();
        assert!(
            e.to_string().contains("theseus owner proof enroll"),
            "{call}: {e:#}"
        );
        decline(&r, &q);
    }
    assert_eq!(r.file_text(), FILE);
}

/// Finding 1: each of the four dollar limits, from a stop of $50, to a positive
/// value below half a micro-dollar, which the kernel holds as zero (no limit):
/// the code. At the rounding boundary, one that converts to one micro-dollar is
/// a tighter stop: the card alone, and the yes alone writes it.
#[tokio::test]
async fn a_dollar_limit_that_converts_to_zero_micro_dollars_needs_the_code() {
    for key in [
        "spend_limit_usd",
        "daily_spend_ceiling_usd",
        "daily_runaway_usd",
        "runaway_usd_per_10min",
    ] {
        let r = rig();
        file_with(&r, &format!("[kernel]\n{key} = 50.0\n"));
        let path = format!("kernel.{key}");
        let field = |k: theseus_kernel::KernelConfig| match key {
            "spend_limit_usd" => k.spend_limit_micros,
            "daily_spend_ceiling_usd" => k.daily_ceiling_micros,
            "daily_runaway_usd" => k.daily_runaway_micros,
            _ => k.burn_line_micros,
        };
        let held = |v: f64| field(kernel_of(&format!("{FILE}\n[kernel]\n{key} = {v}\n")));
        assert_eq!(held(0.0000004), 0, "{key}: the kernel holds no limit");
        assert_eq!(held(0.0000006), 1, "{key}: one micro-dollar");
        let q = card_of(&r, change(&path, json!(0.0000004))).await;
        assert!(q.needs_proof, "{key} to 0.0000004: {}", q.reason);
        decline(&r, &q);
        let q = card_of(&r, change(&path, json!(0.0000006))).await;
        assert!(!q.needs_proof, "{key} to 0.0000006: {}", q.reason);
        r.core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap();
        assert_eq!(
            field(kernel_of(&r.file_text())),
            1,
            "{key}: written, one micro-dollar"
        );
    }
}

/// The fifth limit, the hard multiple, is a whole number: lowering it is a
/// tighter stop; zero (never stop) needs the code.
#[tokio::test]
async fn the_hard_multiple_is_its_own_whole_number_case() {
    let r = rig();
    file_with(&r, "[kernel]\nspend_limit_hard_multiple = 10\n");
    let q = card_of(&r, change("kernel.spend_limit_hard_multiple", json!(0))).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert_eq!(
        kernel_of(&format!(
            "{FILE}\n[kernel]\nspend_limit_hard_multiple = 0\n"
        ))
        .spend_hard_multiple,
        0,
        "the kernel holds no stop"
    );
    decline(&r, &q);
    let q = card_of(&r, change("kernel.spend_limit_hard_multiple", json!(5))).await;
    assert!(!q.needs_proof, "{}", q.reason);
    decline(&r, &q);
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
    decline(&r, &q);
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
    decline(&r, &q);
    let q = card_of(&r, change(path, json!(90.0))).await;
    assert!(q.needs_proof, "raised: {}", q.reason);
}

/// One change of several: a limit made tighter, prompt text, and a stop that
/// converts to zero micro-dollars. The stricter rule wins; without the last,
/// the card alone.
#[tokio::test]
async fn in_a_change_of_several_the_stricter_rule_wins() {
    let r = rig();
    file_with(
        &r,
        "[kernel]\ndaily_spend_ceiling_usd = 300.0\ndaily_runaway_usd = 2000.0\n",
    );
    let both = |runaway: f64| {
        json!({
            "changes": [
                {"path": "kernel.daily_spend_ceiling_usd", "value": 250.0},
                {"path": "model.system", "value": "be brief"},
                {"path": "kernel.daily_runaway_usd", "value": runaway},
            ],
            "reason": "the owner asked for it",
        })
    };
    let q = card_of(&r, both(0.0000001)).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason.contains("kernel.daily_runaway_usd"),
        "{}",
        q.reason
    );
    decline(&r, &q);
    let q = card_of(&r, both(1000.0)).await;
    assert!(!q.needs_proof, "{}", q.reason);
}
