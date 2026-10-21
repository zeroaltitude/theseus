//! A question with options on Discord (theseus-j7cje): the needs-you post a
//! question that waited reaches the owner's DM with (always pings, a button
//! per option, settled when answered), and the card of a question asked in
//! the DM itself (its options as buttons, no second post).

use std::sync::Arc;

use theseus_core::approval::{Client, Surface};
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};

use super::{bindings, guild, Rig, ANA, ANA_DM};
use crate::rpc_client::RpcClient;

fn model(_: &std::path::Path, _: Arc<theseus_sim::fake_discord::FakeDiscord>) -> Arc<dyn Provider> {
    let ask = serde_json::json!({
        "question": "Which plan for the harbour survey?",
        "options": [{"label": "Careful"}, {"label": "Ship it behind the flag"}],
        "default": 1,
    });
    Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t1", "owner_ask", ask)]),
        Scripted::text("Going on."),
    ]))
}

/// The classic card and text board (owner-ask was written before discord-board's
/// Components V2 card; with `live_board` on, a DM question's card is a V2 box).
async fn rig() -> Rig {
    // The model's owner_ask is deferred by default (its own family): these scripts call it directly.
    Rig::start_tweaked(model, guild(true), &bindings(false), &[], |c| {
        c.tools.defer.always_loaded.push("ask".into());
        c.discord.live_board = false;
    })
    .await
}

/// A CLI's turn: a session of no place asks the owner.
async fn asked_from_the_cli(r: &Rig) -> (String, u64) {
    let rpc = RpcClient::connect(r.core.clone(), Client::new("test", Surface::Cli)).0;
    let res: TurnSubmitResult = rpc
        .call(
            theseus_protocol::method::TURN_SUBMIT,
            TurnSubmitParams {
                dir: None,
                carried: false,
                prompt: None,
                session_id: None,
                input: "decide".into(),
                profile: None,
                provider: None,
                model: None,
                author: Some("test".into()),
                attachments: vec![],
                reply_to: None,
                opened_from: None,
                thread: None,
                speaking: None,
                in_reply_to: None,
                route_at_ms: None,
                surface: None,
                via: None,
            },
        )
        .await
        .unwrap();
    let corr = res.awaiting_confirm.expect("the question waits");
    let at = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    (corr, at)
}

fn is_it(m: &theseus_sim::fake_discord::Msg) -> bool {
    m.versions[0].contains("Needs you")
}

/// A CLI question that waited a minute reaches ana's DM: it pings, it has a
/// button for each option, a press answers it as ana, and the post is
/// edited to say who chose what.
#[tokio::test]
async fn a_question_that_waited_pings_the_dm_and_a_press_answers_it() {
    let r = rig().await;
    let (corr, at) = asked_from_the_cli(&r).await;
    assert!(
        r.posted(ANA_DM).iter().all(|m| !is_it(m)),
        "nothing at once"
    );
    assert_eq!(r.core.escalate_questions(at + 61_000), 1);
    r.until("the needs-you in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_it)
    })
    .await;
    let post = r.posted(ANA_DM).into_iter().find(is_it).unwrap();
    assert!(!post.silent(), "a needs-you always pings: {post:?}");
    assert!(
        post.versions[0].contains("Which plan for the harbour survey?"),
        "{}",
        post.content
    );
    let labels: Vec<&str> = post.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(
        labels,
        ["1. Careful", "2. Ship it behind the flag", "Details"]
    );

    r.press(&post.id, "2. Ship it behind the flag", (ANA, "ana"));
    r.until("the answer", || {
        r.ledger("action.confirm_answered")
            .iter()
            .any(|row| row["option"] == "choice:1")
    })
    .await;
    assert_eq!(r.core.confirm_list().unwrap().len(), 0, "it is answered");
    r.until("the post says who chose", || {
        r.posted(ANA_DM)
            .into_iter()
            .find(is_it)
            .is_some_and(|m| m.buttons.is_empty() && m.content.contains("Answered"))
    })
    .await;
    let _ = corr;
}

/// A question asked in the DM itself has its card there at once, with a
/// button for each option, and the owner's chat gets no second post for it.
#[tokio::test]
async fn a_question_asked_in_the_dm_has_its_card_and_no_second_post() {
    let r = rig().await;
    r.say((ANA, "ana"), None, "decide");
    r.until("the question's card", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.buttons.iter().any(|b| b.label.starts_with("1. ")))
    })
    .await;
    let card = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.buttons.iter().any(|b| b.label.starts_with("1. ")))
        .unwrap();
    assert!(card.content.contains("Which plan"), "{}", card.content);
    let corr = r.core.confirm_list().unwrap().remove(0).correlation_id;
    let at = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(
        r.core.escalate_questions(at + 600_000),
        0,
        "it has its card"
    );
    assert!(r.posted(ANA_DM).iter().all(|m| !is_it(m)));
    r.press(&card.id, "1. Careful", (ANA, "ana"));
    r.until("the answer", || {
        r.ledger("action.confirm_answered")
            .iter()
            .any(|row| row["option"] == "choice:0")
    })
    .await;
    r.until("the card is settled", || {
        r.posted(ANA_DM)
            .into_iter()
            .find(|m| m.id == card.id)
            .is_some_and(|m| m.buttons.is_empty() && m.content.contains("Answered"))
    })
    .await;
}
