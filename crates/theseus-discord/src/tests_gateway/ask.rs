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
                deadline_secs: None,
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
    let pressed = r.fake.replies().into_iter().find_map(|x| x.interaction).unwrap();
    r.until("the press is over", || crate::runtime::handled(&pressed))
        .await;
    assert_settled_alone(&r, &post, &pressed);
}

/// The card of a question asked in ana's DM, once it is posted.
async fn the_dm_card(r: &Rig) -> theseus_sim::fake_discord::Msg {
    r.say((ANA, "ana"), None, "decide");
    r.until("the question's card", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.buttons.iter().any(|b| b.label.starts_with("1. ")))
    })
    .await;
    r.posted(ANA_DM)
        .into_iter()
        .find(|m| m.buttons.iter().any(|b| b.label.starts_with("1. ")))
        .unwrap()
}

fn card_now(r: &Rig, id: &str) -> theseus_sim::fake_discord::Msg {
    r.posted(ANA_DM).into_iter().find(|m| m.id == id).unwrap()
}

/// One writer per card: the card was written twice, once when it was posted and once by the daemon's settle, and
/// the press that answered it wrote nothing: it acknowledged (an update of the card) and that is all.
fn assert_settled_alone(r: &Rig, card: &theseus_sim::fake_discord::Msg, pressed: &str) {
    let now = card_now(r, &card.id);
    assert_eq!(now.versions.len(), 2, "posted once, settled once: {now:?}");
    assert_eq!(now.versions[0], card.content, "{now:?}");
    let settle = &now.versions[1];
    assert!(settle.starts_with("✅ **Answered** by "), "{settle}");
    assert!(settle.contains("Which plan for the harbour survey?"), "the question's own words: {settle}");
    assert_eq!(&now.content, settle, "the card says the settle, word for word");
    assert!(!now.content.contains("chose"), "the press wrote none: {now:?}");
    assert!(now.buttons.is_empty(), "the settle took the buttons: {now:?}");
    let mine: Vec<_> = r
        .fake
        .replies()
        .into_iter()
        .filter(|x| x.interaction.as_deref() == Some(pressed))
        .collect();
    assert!(
        mine.iter().all(|x| x.kind == "callback"),
        "the press only acknowledged: {mine:?}"
    );
    assert!(
        mine.iter().any(|x| x.response_type == Some(6)),
        "an update of the card: {mine:?}"
    );
}

/// A question asked in the DM itself has its card there at once, with a
/// button for each option, and the owner's chat gets no second post for it.
#[tokio::test]
async fn a_question_asked_in_the_dm_has_its_card_and_no_second_post() {
    let r = rig().await;
    let card = the_dm_card(&r).await;
    assert!(card.content.contains("Which plan"), "{}", card.content);
    let corr = r.core.confirm_list().unwrap().remove(0).correlation_id;
    let at = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(
        r.core.escalate_questions(at + 600_000),
        0,
        "it has its card"
    );
    assert!(r.posted(ANA_DM).iter().all(|m| !is_it(m)));
    let pressed = r.press(&card.id, "1. Careful", (ANA, "ana"));
    r.until("the answer", || {
        r.ledger("action.confirm_answered")
            .iter()
            .any(|row| row["option"] == "choice:0")
    })
    .await;
    r.until("the card is settled", || {
        card_now(&r, &card.id).content.contains("Answered")
    })
    .await;
    r.until("the press is over", || crate::runtime::handled(&pressed))
        .await;
    assert_settled_alone(&r, &card, &pressed);
}

/// The two writers of a card meet in either order, and the card ends with the daemon's settle either way. First:
/// the stand-in applies the settle only after the press has finished (it acknowledged, answered, and wrote
/// nothing), and until then the card is what it was posted as.
#[tokio::test]
async fn a_pressed_card_ends_with_the_settle_when_the_settle_lands_after_the_press_is_over() {
    let r = rig().await;
    let card = the_dm_card(&r).await;
    r.fake.withhold_edits(true);
    let pressed = r.press(&card.id, "1. Careful", (ANA, "ana"));
    r.until("the press is over and the settle waits", || {
        crate::runtime::handled(&pressed) && r.fake.withheld() == 1
    })
    .await;
    let waiting = card_now(&r, &card.id);
    assert_eq!(waiting.content, card.content, "the press wrote nothing: {waiting:?}");
    assert!(!waiting.buttons.is_empty(), "its buttons stay: {waiting:?}");
    r.fake.withhold_edits(false);
    r.until("the settle lands", || {
        card_now(&r, &card.id).content.contains("Answered")
    })
    .await;
    assert_settled_alone(&r, &card, &pressed);
}

/// And the other order: the settle lands first, and the stand-in applies whatever the press writes after it, as
/// late as it can. The press writes nothing, so the card is the settle.
#[tokio::test]
async fn a_pressed_card_ends_with_the_settle_when_the_press_completes_after_the_settle() {
    let r = rig().await;
    let card = the_dm_card(&r).await;
    r.fake.withhold_completions(true);
    let pressed = r.press(&card.id, "1. Careful", (ANA, "ana"));
    r.until("the settle lands", || {
        card_now(&r, &card.id).content.contains("Answered")
    })
    .await;
    r.fake.withhold_completions(false);
    r.until("the press is over", || crate::runtime::handled(&pressed))
        .await;
    assert_settled_alone(&r, &card, &pressed);
}
