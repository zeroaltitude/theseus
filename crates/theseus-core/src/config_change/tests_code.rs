//! Issue 57, A4: which settings cards take the owner's authenticator code, through
//! a real `config.propose` on a real settings file (the rig of `tests.rs`).
//!
//! A sensitive setting keeps its card; the code is kept for the gates, secrets and
//! money.

use serde_json::{json, Value};

use super::tests::{change, rig, Rig, FILE};
use crate::node::ResultStatus;

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
