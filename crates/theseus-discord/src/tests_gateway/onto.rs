//! Jev's ontology cards in the owner's DM (theseus-xzxqv, theseus-d0c7h): a
//! card names the categories Jev minted from a message, so it is the owner's
//! to see. Both cards the core sends, the mint with its Undo and the question
//! with none, reach the owner's DM alone and never a shared channel.

use std::sync::Arc;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};

use super::{bindings, guild, Rig, ANA_DM, LAB};

#[tokio::test]
async fn an_onto_card_goes_to_the_owners_dm_and_never_a_shared_channel() {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    r.core
        .outbox
        .to_operator(
            None,
            json!({"kind": "onto_card", "write": "jdg_mint", "undo": true,
                "text": "🗂 Jev started a new topic and filed this session under it: onto-mint-card"}),
        )
        .unwrap();
    r.core
        .outbox
        .to_operator(
            None,
            json!({"kind": "onto_card", "write": "jdg_ask", "undo": false,
                "text": "🗂 Is this onto-ask-card? (60% sure)"}),
        )
        .unwrap();
    let has = |m: &theseus_sim::fake_discord::Msg, w: &str| m.versions[0].contains(w);
    r.until("both cards in ana's DM", || {
        let dm = r.posted(ANA_DM);
        dm.iter().any(|m| has(m, "onto-mint-card")) && dm.iter().any(|m| has(m, "onto-ask-card"))
    })
    .await;
    let dm = r.posted(ANA_DM);
    let mint = dm.iter().find(|m| has(m, "onto-mint-card")).unwrap();
    let labels: Vec<&str> = mint.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Undo"], "a mint has its undo: {mint:?}");
    assert_eq!(
        mint.buttons[0].custom_id, "onto:undo:jdg_mint",
        "the press names the write"
    );
    let ask = dm.iter().find(|m| has(m, "onto-ask-card")).unwrap();
    assert!(ask.buttons.is_empty(), "a question has no button: {ask:?}");
    // Both are delivered, so a card posted elsewhere would be there by now.
    let anywhere = |w: &str| r.fake.messages(LAB).iter().any(|m| has(m, w));
    assert!(
        !anywhere("onto-mint-card") && !anywhere("onto-ask-card"),
        "never in #lab"
    );
}
