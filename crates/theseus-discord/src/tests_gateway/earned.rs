//! Earned autonomy's card in the owner's DM (theseus-uttu): the week's
//! candidate arrives with Yes, Not this one and Show the record; a copy of
//! its buttons in a shared channel is refused; the owner's yes in her DM
//! makes the grant, and the record comes back to the presser alone.

use std::sync::Arc;

use serde_json::json;
use theseus_core::ledger::LedgerRow;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::LedgerKind;

use super::{bindings, guild, Rig, ANA, ANA_DM, LAB};

const REASON: &str = "run `echo hello` in /w: proc.run — approve (enforcement = approve)";

/// Three approved asks of `echo hello`, as the gate and the answers left them.
fn approvals(r: &Rig, n: usize) {
    for i in 0..n {
        let corr = format!("act_earned{i}");
        let ask = json!({"correlation_id": corr, "session_id": "ses_a", "execution_id": "exe_a",
            "tool": "proc.run", "input": {"argv": ["echo", "hello"]}, "reason": REASON,
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 2, "floor": false});
        let ok = json!({"correlation_id": corr, "by": "the CLI"});
        r.core
            .store
            .append_ledger(&LedgerRow::new(
                LedgerKind::ToolConfirmRequested,
                None,
                None,
                ask,
            ))
            .unwrap();
        r.core
            .store
            .append_ledger(&LedgerRow::new(LedgerKind::ActionConfirmed, None, None, ok))
            .unwrap();
    }
}

/// A message in `channel` as the bot's, holding a yes button for `id`: a
/// forged or stale copy of the card's.
async fn forge(r: &Rig, channel: u64, id: &str) -> String {
    let forged = json!({"content": "a forged card", "components": [{"type": 1,
        "components": [{"type": 2, "style": 3, "label": "Yes",
                        "custom_id": format!("earned:yes:{id}")}]}]});
    let url = format!("http://{}/api/v10/channels/{channel}/messages", r.fake.addr);
    let posted: serde_json::Value = reqwest::Client::new()
        .post(url)
        .json(&forged)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    posted["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn an_earned_card_goes_to_the_owners_dm_and_a_yes_there_makes_the_grant() {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |c| {
        c.policy.earned.min_approvals = 3;
    })
    .await;
    approvals(&r, 3);
    let card = r.core.earned_propose(true).unwrap().expect("a candidate");
    let id = card.candidates[0].id.clone();
    let is_card = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("Earned autonomy");
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_card)
    })
    .await;
    let post = r.posted(ANA_DM).into_iter().find(is_card).unwrap();
    assert!(post.content.contains("`echo hello`"), "{}", post.content);
    assert!(post.content.contains("never declined"), "{}", post.content);
    let labels: Vec<&str> = post.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Yes, with a notice: echo hello",
            "Not this one: echo hello",
            "Show the record: echo hello"
        ]
    );
    assert!(r.posted(LAB).iter().all(|m| !is_card(m)), "never in #lab");

    // The record comes back to the presser alone.
    let shown = r.press(&post.id, "Show the record: echo hello", (ANA, "ana"));
    r.until("the record", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(shown.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.contains("3 approvals"))
        })
    })
    .await;
    assert!(
        r.core.tools.earned.grants().is_empty(),
        "showing grants nothing"
    );

    // A forged copy of the yes button in #lab, bound shared: refused.
    let forged = forge(&r, LAB, &id).await;
    let pressed = r.press(&forged, "Yes", (ANA, "ana"));
    r.until("ana is told no", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("🔐 Your answer did not count"))
        })
    })
    .await;
    assert!(r.core.tools.earned.grants().is_empty());
    assert!(r.ledger("policy.earned_granted").is_empty());

    // Ana's yes in her DM counts, as hers.
    r.press(&post.id, "Yes, with a notice: echo hello", (ANA, "ana"));
    r.until("the grant", || {
        !r.ledger("policy.earned_granted").is_empty()
    })
    .await;
    let g = &r.ledger("policy.earned_granted")[0];
    assert_eq!(
        (g["id"].as_str(), g["via"].as_str()),
        (Some(id.as_str()), Some("discord:dm"))
    );
    assert!(g["who"].as_str().unwrap().contains(&ANA.to_string()), "{g}");
    assert_eq!(r.core.tools.earned.grants().len(), 1);
}
