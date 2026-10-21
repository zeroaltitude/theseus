//! The router's question in the owner's DM (theseus-rha2): the core's
//! `thread.asked` becomes one line with a button for each choice, a press is
//! `thread.answer` as the presser, which wakes the message that waits, and the
//! card settles into the answer.

use std::sync::Arc;
use std::time::Duration;

use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_core::thread_flow::Pending;
use theseus_core::thread_route::{AskChoice, Choice};
use theseus_protocol::thread_route::{AskOption, ThreadAsk};
use theseus_protocol::{Event, Message};

use super::{bindings, guild, Rig, ANA, ANA_DM};

/// The session the owner's DM runs on, once its first message opened it.
async fn dm_session(r: &Rig) -> String {
    let find = || {
        r.core
            .outbox
            .place_session(&format!("dm:{ANA}"))
            .ok()
            .flatten()
    };
    r.until("the DM's session", || find().is_some()).await;
    find().unwrap()
}

fn choice(key: &str, label: &str, c: Choice) -> AskChoice {
    AskChoice {
        choice: c,
        option: AskOption {
            key: key.into(),
            label: label.into(),
        },
    }
}

#[tokio::test]
async fn the_owners_press_wakes_the_message_and_the_card_becomes_the_answer() {
    let model = |_: &std::path::Path, _| -> Arc<dyn Provider> {
        Arc::new(FakeProvider::scripted(vec![Scripted::text("hello")]))
    };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    r.say((ANA, "ana"), None, "hello");
    r.until("the model's reply in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.starts_with("hello"))
    })
    .await;
    let session = dm_session(&r).await;
    let options = vec![
        choice("1", "pr round", Choice::Thread("pr-round".into())),
        choice("2", "security fix", Choice::Thread("security-fix".into())),
        choice("home", "home", Choice::Home),
    ];
    let (tx, rx) = tokio::sync::oneshot::channel();
    r.core.runner.threads.route.ask(
        "ask_card2",
        Pending {
            home: session.clone(),
            text: "why is the build red".into(),
            options: options.clone(),
            slugs: vec![],
            tx,
        },
    );
    let ask = ThreadAsk {
        ask_id: "ask_card2".into(),
        session_id: session.clone(),
        place: None,
        question: "Is this about the pr round or the security fix?".into(),
        options: options.iter().map(|o| o.option.clone()).collect(),
        message: "why is the build red".into(),
        expires_ms: 0,
        others: vec![],
    };
    r.core
        .bus
        .publish(&session, &Message::from(Event::ThreadAsked(ask)), None);
    let is_card = |m: &theseus_sim::fake_discord::Msg| m.content.contains("Is this about");
    r.until("the card", || r.posted(ANA_DM).iter().any(is_card))
        .await;
    let card = r.posted(ANA_DM).into_iter().find(is_card).unwrap();
    r.press(&card.id, "security fix", (ANA, "ana"));
    let got = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("answered")
        .unwrap();
    assert_eq!(got.choice, Choice::Thread("security-fix".into()));
    assert_eq!(got.who.label, "discord:ana");
    // The card becomes the answer: an update-message callback (type 7).
    r.until("the card settled", || {
        r.fake.replies().iter().any(|a| {
            a.response_type == Some(7)
                && a.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("\u{2705}"))
        })
    })
    .await;
}
