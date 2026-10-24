//! A persona added without a soul asks the owner for it in her DM (theseus-utfnl): one card
//! with Write its soul and Decline. The modal's text becomes the file and the persona goes live;
//! a modal submitted in a shared channel is refused and writes nothing; Decline retires it. The
//! core's writes are tested in `tests_persona_write.rs`; the DM task flow is not touched.

use std::sync::Arc;

use theseus_core::approval::{Answerer, Surface};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::PersonaAddParams;
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::Modal;

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
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await
}

fn add(r: &Rig, key: &str) {
    r.core
        .persona_add(
            &PersonaAddParams {
                key: key.into(),
                ..Default::default()
            },
            &cli(),
        )
        .unwrap();
}

fn card_of(m: &theseus_sim::fake_discord::Msg, key: &str) -> bool {
    m.versions[0].contains(&format!("theseus persona soul {key}"))
}

#[tokio::test]
async fn the_modals_text_becomes_the_soul_and_the_persona_goes_live() {
    let r = rig().await;
    add(&r, "pilot");
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(|m| card_of(m, "pilot"))
    })
    .await;
    let card = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| card_of(m, "pilot"))
        .unwrap();
    assert!(
        card.content
            .starts_with("✋ Action · New persona: Pilot\nyou added it from the CLI"),
        "{}",
        card.content
    );
    assert!(theseus_core::owner_note::is_headed(&card.content));
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Write its soul", "Decline"]);
    assert!(
        r.posted(LAB).iter().all(|m| !card_of(m, "pilot")),
        "never in #lab"
    );

    // The press opens the modal.
    let pressed = r.press(&card.id, "Write its soul", (ANA, "ana"));
    r.until("the modal", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.modal
                    .as_ref()
                    .is_some_and(|m| m["custom_id"] == "psoul:m:pilot")
        })
    })
    .await;

    // A submit from a shared channel is refused, and writes nothing.
    let fields = [("soul", "A dry harbour pilot. ⚓")];
    let from_lab = r
        .fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: LAB,
            custom_id: "psoul:m:pilot",
            fields: &fields,
        })
        .unwrap();
    r.until("ana is told no", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(from_lab.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("🔐 Your soul did not count"))
        })
    })
    .await;
    assert!(!r.core.personas().is_live("pilot"));
    assert!(r.ledger("persona.soul").is_empty());

    // From her DM it is the file, and the persona is live.
    let from_dm = r
        .fake
        .submit_modal(
            &Sender {
                user: ANA,
                name: "ana",
                channel: None,
            },
            "psoul:m:pilot",
            &fields,
            &[],
            None,
        )
        .unwrap();
    r.until("the soul", || !r.ledger("persona.soul").is_empty())
        .await;
    r.until("ana is told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(from_dm.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("✅ **pilot** has its soul"))
        })
    })
    .await;
    let book = r.core.personas();
    assert!(book.is_live("pilot"));
    let path = book.get("pilot").unwrap().files[0].path().to_string();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "A dry harbour pilot. ⚓\n"
    );
}

#[tokio::test]
async fn decline_retires_the_persona() {
    let r = rig().await;
    add(&r, "scout");
    r.until("the card", || {
        r.posted(ANA_DM).iter().any(|m| card_of(m, "scout"))
    })
    .await;
    let card = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| card_of(m, "scout"))
        .unwrap();
    let pressed = r.press(&card.id, "Decline", (ANA, "ana"));
    r.until("ana is told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("↩️ Declined"))
        })
    })
    .await;
    assert!(!r.core.personas().exists("scout"));
}
