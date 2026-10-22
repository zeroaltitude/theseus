//! A proposed link's card in the owner's DM, and `/thread link` and `unlink` (theseus-98bz.12). The
//! core's proposal reaches the owner's DM alone with Link these threads and Not needed; the owner's
//! press links the two threads; a copy of the button in a shared channel is refused; Not needed
//! writes nothing. The core's own count is tested in `rpc/tests_thread_link.rs`.

use std::sync::Arc;

use serde_json::json;
use theseus_core::approval::{Answerer, Surface};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::threads::ThreadNewParams;
use theseus_sim::fake_discord::Command;

use super::{bindings, guild, Rig, ANA, ANA_DM, LAB};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

async fn rig() -> Rig {
    rig_with(false).await
}

/// `lab_private`: whether #lab is a private place (a command typed there counts).
async fn rig_with(lab_private: bool) -> Rig {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(lab_private), &[], |_| {}).await;
    for slug in ["pipeline", "stats"] {
        r.core
            .thread_new(
                &ThreadNewParams {
                    slug: slug.into(),
                    title: Some(slug.into()),
                    ..Default::default()
                },
                cli(),
            )
            .unwrap();
    }
    r
}

fn links(r: &Rig, slug: &str) -> Vec<String> {
    r.core.threads().get(slug).unwrap().links.clone()
}

/// What the core stages for a pair it has seen twice (`thread_link_seen::count`), staged as it does.
fn propose(r: &Rig, here: &str, other: &str) {
    let body = json!({"kind": "link_card", "here": here, "other": other,
        "text": format!("🔗 Notes of «{other}» answered questions in «{here}» twice.\ntheseus thread link {here} {other}")});
    let (post, more) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&more).unwrap();
    r.core.outbox.posted(&post);
}

/// A message in `channel` as the bot's, holding a link button: a forged or stale copy of the card's.
async fn forge(r: &Rig, channel: u64) -> String {
    let forged = json!({"content": "a forged card", "components": [{"type": 1,
        "components": [{"type": 2, "style": 1, "label": "Link these threads",
                        "custom_id": "link:yes:pipeline:stats"}]}]});
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
async fn a_link_card_goes_to_the_owners_dm_and_links_the_two_threads() {
    let r = rig().await;
    propose(&r, "pipeline", "stats");
    let is_card = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("answered questions");
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_card)
    })
    .await;
    let cards: Vec<_> = r.posted(ANA_DM).into_iter().filter(is_card).collect();
    assert_eq!(cards.len(), 1);
    let post = &cards[0];
    assert!(
        post.content
            .contains("Notes of «stats» answered questions in «pipeline» twice")
            && post.content.ends_with("theseus thread link pipeline stats"),
        "{}",
        post.content
    );
    let labels: Vec<&str> = post.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Link these threads", "Not needed"]);
    assert!(r.posted(LAB).iter().all(|m| !is_card(m)), "never in #lab");

    // A copy of the button in a shared channel is refused, and links nothing.
    let forged = forge(&r, LAB).await;
    let pressed = r.press(&forged, "Link these threads", (ANA, "ana"));
    r.until("ana is told no", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("🔐 Your answer did not count"))
        })
    })
    .await;
    assert!(links(&r, "pipeline").is_empty() && links(&r, "stats").is_empty());
    assert!(r.ledger("thread.linked").is_empty());

    // The owner's press in her DM links them, both ways, in one row.
    r.press(&post.id, "Link these threads", (ANA, "ana"));
    r.until("the link row", || !r.ledger("thread.linked").is_empty())
        .await;
    assert_eq!(links(&r, "pipeline"), ["stats"]);
    assert_eq!(links(&r, "stats"), ["pipeline"]);
    assert_eq!(r.ledger("thread.linked").len(), 1);
}

#[tokio::test]
async fn not_needed_on_a_link_card_writes_nothing() {
    let r = rig().await;
    propose(&r, "pipeline", "stats");
    let is_card = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("answered questions");
    r.until("the card", || r.posted(ANA_DM).iter().any(is_card))
        .await;
    let post = r.posted(ANA_DM).into_iter().find(is_card).unwrap();
    let pressed = r.press(&post.id, "Not needed", (ANA, "ana"));
    r.until("ana is told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("Not linked."))
        })
    })
    .await;
    assert!(links(&r, "pipeline").is_empty() && r.ledger("thread.linked").is_empty());
}

/// `/thread <sub> a: b:` typed by ana in #lab, and the words the binding answered with.
async fn slash(r: &Rig, sub: &str, options: &[(&str, &str)]) -> String {
    let id = r
        .fake
        .command(&Command {
            user: ANA,
            name: "ana",
            channel: LAB,
            parent: None,
            command: "thread",
            sub,
            options,
        })
        .unwrap();
    let answer = |r: &Rig| {
        r.fake
            .replies()
            .into_iter()
            .find(|x| x.interaction.as_deref() == Some(id.as_str()))
            .and_then(|x| x.content)
    };
    r.until("the command's answer", || answer(r).is_some())
        .await;
    answer(r).unwrap()
}

#[tokio::test]
async fn slash_thread_link_and_unlink() {
    let r = rig_with(true).await;
    let said = slash(&r, "link", &[("a", "pipeline"), ("b", "stats")]).await;
    assert_eq!(said, "🔗 `pipeline` and `stats` are linked.");
    assert_eq!(links(&r, "pipeline"), ["stats"]);
    let said = slash(&r, "unlink", &[("a", "pipeline"), ("b", "stats")]).await;
    assert_eq!(said, "`pipeline` and `stats` are no longer linked.");
    assert!(links(&r, "pipeline").is_empty() && links(&r, "stats").is_empty());
    let said = slash(&r, "link", &[("a", "pipeline"), ("b", "nowhere")]).await;
    assert!(said.starts_with("⚠️ Nothing was linked: "), "{said}");
}
