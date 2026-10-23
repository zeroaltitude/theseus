//! A proposed seal's card in the owner's DM (theseus-f2g0): Jev recommends,
//! the owner seals. The core's proposal reaches the owner's DM alone with
//! Seal this thread and Not needed; a copy of its buttons in a shared channel
//! is refused; "Not needed" in the DM releases the thread, and the seal
//! there writes it.

use std::sync::Arc;

use serde_json::json;
use theseus_core::approval::{Answerer, Surface};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::threads::ThreadNewParams;

use super::{bindings, guild, Rig, ANA, ANA_DM, LAB};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A message in `channel` as the bot's, holding a seal button for `slug`: a
/// forged or stale copy of the card's.
async fn forge(r: &Rig, channel: u64, slug: &str) -> String {
    let forged = json!({"content": "a forged card", "components": [{"type": 1,
        "components": [{"type": 2, "style": 4, "label": "Seal this thread",
                        "custom_id": format!("seal:yes:{slug}")}]}]});
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

/// A forged copy of the seal button in #lab, bound shared: refused.
async fn forged_press_is_refused(r: &Rig) {
    let forged = forge(r, LAB, "security-fix").await;
    let pressed = r.press(&forged, "Seal this thread", (ANA, "ana"));
    r.until("ana is told no", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("🔐 Your answer did not count"))
        })
    })
    .await;
    assert!(r
        .core
        .threads()
        .get("security-fix")
        .unwrap()
        .sealed
        .is_none());
}

#[tokio::test]
async fn a_seal_card_goes_to_the_owners_dm_and_the_owner_alone_answers_it() {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "security-fix".into(),
                title: Some("security fix".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(r
        .core
        .thread_recommend_seal("security-fix", "it names an embargoed CVE")
        .unwrap());
    let is_card = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("looks embargoed");
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_card)
    })
    .await;
    let post = r.posted(ANA_DM).into_iter().find(is_card).unwrap();
    assert!(
        post.content
            .starts_with("❓ Decision · Seal thread «security fix»?\nThis looks embargoed: it names an embargoed CVE"),
        "{}",
        post.content
    );
    let labels: Vec<&str> = post.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Seal this thread", "Not needed"]);
    assert!(r.posted(LAB).iter().all(|m| !is_card(m)), "never in #lab");

    forged_press_is_refused(&r).await;

    // "Not needed" in her DM releases the thread and is a label row.
    r.press(&post.id, "Not needed", (ANA, "ana"));
    r.until("the label row", || {
        !r.ledger("thread.seal_dismissed").is_empty()
    })
    .await;
    assert!(r
        .core
        .threads()
        .get("security-fix")
        .unwrap()
        .seal_pending
        .is_none());

    // A new proposal, once the content changed, and her seal writes it.
    r.core
        .thread_edit(
            &theseus_protocol::threads::ThreadEditParams {
                slug: "security-fix".into(),
                card: Some(vec!["the CVE is assigned".into()]),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(r
        .core
        .thread_recommend_seal("security-fix", "assigned")
        .unwrap());
    r.until("the second card", || {
        r.posted(ANA_DM)
            .iter()
            .filter(|m| m.versions[0].contains("looks embargoed"))
            .count()
            == 2
    })
    .await;
    let second = r.posted(ANA_DM).into_iter().rfind(is_card).unwrap();
    r.press(&second.id, "Seal this thread", (ANA, "ana"));
    r.until("the seal", || !r.ledger("thread.sealed").is_empty())
        .await;
    let s = r
        .core
        .threads()
        .get("security-fix")
        .unwrap()
        .sealed
        .clone()
        .unwrap();
    assert!(s.by.contains(&ANA.to_string()), "{s:?}");
}
