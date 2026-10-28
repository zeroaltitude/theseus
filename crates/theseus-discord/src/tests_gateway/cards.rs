//! The needs-you cards (theseus-suvpe): every question that reaches the
//! owner's DM is a card with its own buttons, in plain words, settled when it
//! closes, and a reply to it answers it. Through the stand-in, a real core,
//! and a scripted model; the questions are approvals of a write that waits.

use std::sync::Arc;

use theseus_core::approval::{Client, Surface};
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};
use theseus_sim::fake_discord::{FakeDiscord, Msg, Typed};

use super::{bindings, guild, outside, write_script, Rig, ANA, ANA_DM, CY, LAB};
use crate::rpc_client::RpcClient;

/// The model's answers in order: `w` a write that waits, `t` a reply (what an
/// answered write's continuation reads); a few spare replies follow, for the
/// ordinary messages a test sends.
fn model(dir: &std::path::Path, spec: &str) -> Arc<dyn Provider> {
    let mut script: Vec<Scripted> = spec
        .chars()
        .flat_map(|c| match c {
            'w' => write_script(dir).into_iter().take(1).collect::<Vec<_>>(),
            _ => write_script(dir).into_iter().skip(1).collect(),
        })
        .collect();
    script.extend((0..8).map(|_| Scripted::text("Done.")));
    Arc::new(FakeProvider::scripted(script))
}

/// The classic text board (the DM's questions are classic posts here; a V2
/// card is `live_board`'s) and `n` questions to ask.
async fn rig(spec: &'static str, tweak: impl FnOnce(&mut theseus_core::Config)) -> Rig {
    Rig::start_tweaked(
        move |dir: &std::path::Path, _: Arc<FakeDiscord>| model(dir, spec),
        guild(true),
        &bindings(false),
        &[],
        |c| {
            c.discord.live_board = false;
            c.notify.unprompted_pace_secs = 60;
            tweak(c);
        },
    )
    .await
}

/// A CLI turn that asks for a write's approval: the question's id and when
/// it was planned.
async fn ask(r: &Rig, input: &str) -> (String, u64) {
    let rpc = RpcClient::connect(r.core.clone(), Client::new("test", Surface::Cli)).0;
    let res: TurnSubmitResult = rpc
        .call(
            theseus_protocol::method::TURN_SUBMIT,
            TurnSubmitParams {
                dir: None,
                carried: false,
                prompt: None,
                session_id: None,
                input: input.into(),
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
    let corr = res.awaiting_confirm.expect("the write waits");
    let at = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    (corr, at)
}

/// The newest question's planned time, `n` questions asked.
async fn asked(r: &Rig, n: usize) -> (Vec<String>, u64) {
    let mut ids = Vec::new();
    let mut at = 0;
    for i in 0..n {
        let (corr, t) = ask(r, &format!("write it {i}")).await;
        ids.push(corr);
        at = at.max(t);
    }
    (ids, at)
}

/// The bot's messages in the DM holding `needle`.
fn dm(r: &Rig, needle: &str) -> Vec<Msg> {
    r.posted(ANA_DM)
        .into_iter()
        .filter(|m| m.versions[0].contains(needle))
        .collect()
}

fn labels(m: &Msg) -> Vec<&str> {
    m.buttons.iter().map(|b| b.label.as_str()).collect()
}

fn reply(r: &Rig, user: (u64, &str), channel: Option<u64>, text: &str, to: &str) -> String {
    r.fake
        .say_reply(
            &Typed {
                user: user.0,
                name: user.1,
                channel,
                content: text,
                file: None,
            },
            to,
        )
        .unwrap()
}

/// An approval escalates to the DM as a card: Approve, Decline and Details,
/// pinging. Pressing Approve as the owner answers it (the ledger's
/// `action.confirm_answered`, the held call runs), and the card says so with
/// no buttons.
#[tokio::test]
async fn a_dm_question_is_answered_by_its_cards_approve() {
    let r = rig("w", |_| {}).await;
    let (corr, at) = asked(&r, 1).await;
    assert_eq!(r.core.escalate_questions(at + 61_000), 1);
    r.until("the card", || !dm(&r, "Needs you").is_empty())
        .await;
    let card = dm(&r, "Needs you").remove(0);
    assert_eq!(labels(&card), ["Approve", "Decline", "Details"]);
    assert!(!card.silent(), "a lone question pings: {card:?}");
    assert!(card.versions[0].contains("write `"), "{}", card.versions[0]);
    assert!(
        !card.versions[0].contains("theseus answer"),
        "no shell hint"
    );

    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("the answer", || {
        r.ledger("action.confirm_answered")
            .iter()
            .any(|row| row["option"] == "approve")
    })
    .await;
    assert_eq!(r.core.confirm_list().unwrap().len(), 0);
    r.until("the card says so", || {
        dm(&r, "Needs you")
            .first()
            .is_some_and(|m| m.buttons.is_empty() && m.content.contains("Approved"))
    })
    .await;
    r.until("the held call ran", || outside(r.dir.path()).exists())
        .await;
    let _ = corr;
}

/// Details shows the presser alone the whole request: the tool, its target,
/// the rule that held it, and the question's id; the card keeps its buttons.
#[tokio::test]
async fn details_shows_the_whole_request_to_the_presser_alone() {
    let r = rig("w", |_| {}).await;
    let (corr, at) = asked(&r, 1).await;
    r.core.escalate_questions(at + 61_000);
    r.until("the card", || !dm(&r, "Needs you").is_empty())
        .await;
    let card = dm(&r, "Needs you").remove(0);
    let iid = r.press(&card.id, "Details", (ANA, "ana"));
    let shown = |r: &Rig| {
        r.fake
            .replies()
            .into_iter()
            .find(|x| x.interaction.as_deref() == Some(iid.as_str()) && x.ephemeral)
            .and_then(|x| x.content)
    };
    r.until("the details", || shown(&r).is_some()).await;
    let shown = shown(&r).unwrap();
    for part in ["**Details**", "fs.write", "**Rule:**", &corr[0]] {
        assert!(shown.contains(part), "{part}: {shown}");
    }
    assert_eq!(
        r.core.confirm_list().unwrap().len(),
        1,
        "it answers nothing"
    );
    assert_eq!(
        labels(&dm(&r, "Needs you")[0]).len(),
        3,
        "the card is as it was"
    );
}

/// Three questions due in one window are one header, which pings, and a card
/// each, which post silently; answering the second leaves the other two open.
#[tokio::test]
async fn a_batch_is_one_card_per_question() {
    let r = rig("www", |_| {}).await;
    let (_, at) = asked(&r, 3).await;
    assert_eq!(r.core.escalate_questions(at + 61_000), 3);
    r.until("the header and three cards", || {
        !dm(&r, "3 questions need you").is_empty() && dm(&r, "Needs you").len() == 3
    })
    .await;
    let cards = dm(&r, "Needs you");
    assert!(cards
        .iter()
        .all(|c| labels(c) == ["Approve", "Decline", "Details"]));
    assert!(cards.iter().all(Msg::silent), "the cards post silently");
    let pings = r
        .posted(ANA_DM)
        .into_iter()
        .filter(|m| !m.silent() && m.content.contains("need"))
        .count();
    assert_eq!(pings, 1, "one ping in all: the header");
    let second = cards
        .iter()
        .find(|c| c.versions[0].contains("2 of 3"))
        .expect("a second card");
    r.press(&second.id, "Approve", (ANA, "ana"));
    r.until("one answered", || r.core.confirm_list().unwrap().len() == 2)
        .await;
    r.until("its card settled", || {
        dm(&r, "2 of 3")
            .first()
            .is_some_and(|m| m.buttons.is_empty() && m.content.contains("Approved"))
    })
    .await;
    for other in ["1 of 3", "3 of 3"] {
        assert_eq!(labels(&dm(&r, other)[0]).len(), 3, "{other} stays open");
    }
}

/// A reply answers the question it replies to: `yes` to a card; `approve 2`
/// and `no 1` to the header; a bare `yes` to a header with several open
/// answers none and lists them; `yes, but wait` and a reply to a message that
/// holds no question are turns as ever.
#[tokio::test]
async fn a_reply_answers_the_question_it_replies_to() {
    let r = rig("wtwww", |_| {}).await;
    // One question, answered by `yes` to its card.
    let (_, at) = asked(&r, 1).await;
    r.core.escalate_questions(at + 61_000);
    r.until("the card", || !dm(&r, "Needs you").is_empty())
        .await;
    let card = dm(&r, "Needs you").remove(0);
    let turns = r.ledger("discord.message.in").len();
    reply(&r, (ANA, "ana"), None, "yes", &card.id);
    r.until("answered by the reply", || {
        r.core.confirm_list().unwrap().is_empty()
    })
    .await;
    assert_eq!(r.ledger("discord.message.in").len(), turns, "no turn");
    r.until("the card settled", || {
        dm(&r, "Needs you")[0].buttons.is_empty()
    })
    .await;
    r.until("the held call ran", || outside(r.dir.path()).exists())
        .await;

    // Three more, in one window: a header and three cards.
    let (ids, at) = asked(&r, 3).await;
    r.core.escalate_questions(at + 120_000);
    r.until("the header", || !dm(&r, "3 questions need you").is_empty())
        .await;
    let header = dm(&r, "3 questions need you").remove(0);
    reply(&r, (ANA, "ana"), None, "yes", &header.id);
    r.until("the numbers listed", || {
        dm(&r, "More than one is open").len() == 1
    })
    .await;
    assert_eq!(r.core.confirm_list().unwrap().len(), 3, "nothing answered");
    reply(&r, (ANA, "ana"), None, "approve 2", &header.id);
    reply(&r, (ANA, "ana"), None, "no 1", &header.id);
    r.until("two answered", || r.core.confirm_list().unwrap().len() == 1)
        .await;
    let rows = r.ledger("action.confirm_answered");
    assert!(rows
        .iter()
        .any(|x| x["correlation_id"] == ids[1] && x["option"] == "approve"));
    assert!(rows
        .iter()
        .any(|x| x["correlation_id"] == ids[0] && x["option"] == "decline"));

    // Not an answer: a sentence, and a reply to a message that holds nothing.
    let turns = r.ledger("discord.message.in").len();
    reply(&r, (ANA, "ana"), None, "yes, but wait", &header.id);
    let said = r.say((ANA, "ana"), None, "hello");
    reply(&r, (ANA, "ana"), None, "yes", &said);
    r.until("two turns", || {
        r.ledger("discord.message.in").len() == turns + 3
    })
    .await;
    assert_eq!(
        r.core.confirm_list().unwrap().len(),
        1,
        "the third stays open"
    );
}

/// Someone else's reply to a card is refused by the core: a collaborator in a
/// shared place. Nothing is answered, and the replier is told.
#[tokio::test]
async fn a_reply_from_anyone_else_is_refused() {
    let r = rig("w", |_| {}).await;
    let (_, at) = asked(&r, 1).await;
    r.core.escalate_questions(at + 61_000);
    r.until("the card", || !dm(&r, "Needs you").is_empty())
        .await;
    let card = dm(&r, "Needs you").remove(0);
    reply(&r, (CY, "cy"), Some(LAB), "yes", &card.id);
    r.until("cy is told", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("did not count"))
    })
    .await;
    assert_eq!(r.core.confirm_list().unwrap().len(), 1, "still open");
    assert!(r.ledger("action.confirm_answered").is_empty());
}

/// `[notify] dm_cards = false` is today's post: a lone question's one post
/// without buttons, and several in one nudge with no cards.
#[tokio::test]
async fn dm_cards_off_is_todays_post() {
    let r = rig("www", |c| c.notify.dm_cards = false).await;
    let (_, at) = asked(&r, 1).await;
    assert_eq!(r.core.escalate_questions(at + 61_000), 1);
    r.until("the post", || !dm(&r, "Needs you").is_empty())
        .await;
    let one = dm(&r, "Needs you").remove(0);
    assert!(one.buttons.is_empty(), "{one:?}");
    assert!(one.versions[0].contains("write"), "{}", one.versions[0]);
    let (_, at) = asked(&r, 2).await;
    r.core.escalate_questions(at + 120_000);
    r.until("the nudge", || !dm(&r, "2 questions need you").is_empty())
        .await;
    let nudge = dm(&r, "2 questions need you").remove(0);
    assert!(nudge.buttons.is_empty());
    assert_eq!(
        dm(&r, "eed").len(),
        2,
        "the post and the nudge: no card per question"
    );
}

/// An approval that expired unanswered: its card says so and offers one
/// button, "Ask again" (theseus-27p6w). The owner's press clears it and puts
/// the question to the session again, as a wake due now carrying a note (a
/// press by anyone else is the core's refusal: `tests_m3::ask::due`).
#[tokio::test]
async fn an_expired_card_offers_ask_again_and_the_owners_press_asks_it() {
    let r = rig("wt", |_| {}).await;
    let (corr, at) = asked(&r, 1).await;
    assert_eq!(r.core.escalate_questions(at + 61_000), 1);
    r.until("the card", || !dm(&r, "Needs you").is_empty())
        .await;
    let card = dm(&r, "Needs you").remove(0);
    // A call's question is held, not expired (theseus-v2dnc); what is asked
    // again is one an older build expired, made here as that build made it.
    assert_eq!(r.core.expire_questions(at + 20 * 60_000), 0, "held");
    let exec = r
        .core
        .kernel
        .action(&corr[0])
        .unwrap()
        .unwrap()
        .execution_id;
    r.core
        .kernel
        .frame(&[&exec], |k| {
            k.decline_action(&corr[0], "expiry", "nobody answered within 15 minutes")?;
            let _ = k.frame(&[&exec], |k| k.wake(&exec, "expired"));
            Ok(())
        })
        .unwrap();
    r.core
        .outbox
        .closed(&corr[0], theseus_core::outbox::Closed::new("expired", None))
        .unwrap();
    r.until("the card expires with its button", || {
        dm(&r, "Needs you")
            .first()
            .is_some_and(|m| m.content.contains("Expired") && labels(m) == ["Ask again"])
    })
    .await;

    assert!(
        r.ledger("wake.set").is_empty(),
        "nothing is asked before a press"
    );

    r.press(&card.id, "Ask again", (ANA, "ana"));
    r.until("the press answers by clearing the button", || {
        r.fake.replies().iter().any(|p| {
            p.response_type == Some(7)
                && p.content
                    .as_deref()
                    .is_some_and(|c| c.contains("asked again"))
        })
    })
    .await;
    // The wake is due now, so the driver may already have taken it from the
    // execution's pending list: the ledger's row of its being set is the record
    // that stays.
    let wakes = r.ledger("wake.set");
    assert_eq!(wakes.len(), 1, "{wakes:?}");
    assert!(wakes[0]["note"]
        .as_str()
        .is_some_and(|n| n.starts_with("The owner asked again for:")));
    // The ledger is rig-wide, so bind the row back to this card's execution:
    // otherwise any wake set anywhere in the rig would satisfy the assertion
    // above, which is weaker than the pending-list read it replaced (the join
    // review's nit 6).
    let exec = r
        .core
        .kernel
        .action(&corr[0])
        .unwrap()
        .unwrap()
        .execution_id;
    assert_eq!(
        wakes[0]["execution_id"],
        serde_json::json!(exec),
        "{wakes:?}"
    );
}
