//! The owner card's live questions in the owner's DM (theseus-6f8t0): the core's poster stages a
//! `card_question` post, the DM gets one message with its buttons (never a shared channel), and the
//! owner's press is `card.answer` as the presser: Keep, Edit (a modal holding the words), Not this,
//! or End it on a stale question. Another member's press, and a modal submitted in a shared
//! channel, are refused by the core and change nothing. The poster's own rules are tested in the
//! core's `card/tests_dm.rs`; the DM task flow is not touched.

use std::sync::Arc;

use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::{
    CardAsk, CardProposal, CardSource, ProposalState, StandingKind, StandingScope,
};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::Modal;

use super::{bindings, guild, Rig, ANA, ANA_DM, BEN, LAB};

async fn rig() -> Rig {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    Rig::start_tweaked(model, guild(true), &bindings(false), &[], |c| {
        c.initiative.morning_hour = 0;
        c.card.dm_per_day = 10;
    })
    .await
}

fn proposal(n: u64, ask: CardAsk) -> CardProposal {
    let text = format!("Moors at the north jetty, case {n}");
    let now = theseus_protocol::now_unix_ms();
    CardProposal {
        id: format!("cdp_{n:08}"),
        kind: StandingKind::Fact,
        scope: StandingScope::Everywhere,
        question: format!("Add to your card: '{text}' (from 2 sources)?"),
        text,
        options: theseus_protocol::card_options(),
        sources: vec![CardSource {
            episode: format!("ep_{n}"),
            node: format!("nod_{n:0>10}"),
            kind: "decision".into(),
            at_ms: 1,
        }],
        support: 2,
        created_ms: n,
        expires_ms: now + 30 * 86_400_000,
        state: ProposalState::Open,
        run: "crd_1".into(),
        answered_ms: None,
        answered_by: None,
        kept_text: None,
        standing_id: None,
        ask,
        record: None,
        observations: vec![format!("cob_{n:08}")],
        posted_ms: None,
    }
}

/// A question written and posted; its message in ana's DM.
async fn asked(r: &Rig, n: u64, ask: CardAsk) -> theseus_sim::fake_discord::Msg {
    let p = proposal(n, ask);
    let rec = theseus_core::card::state::record(&p).unwrap();
    r.core.store.append(&[rec]).unwrap();
    // Each question a pacer window (30 minutes) after the one before it.
    let at = theseus_protocol::now_unix_ms() + n * 3_600_000;
    assert_eq!(r.core.card_dm_tick(at).await.unwrap(), 1);
    let mine = |m: &theseus_sim::fake_discord::Msg| m.content.contains(&p.text);
    r.until("the question in ana's DM", || {
        r.posted(ANA_DM).iter().any(mine)
    })
    .await;
    assert!(r.posted(LAB).iter().all(|m| !mine(m)), "never in #lab");
    r.posted(ANA_DM).into_iter().find(mine).unwrap()
}

fn state_of(r: &Rig, id: &str) -> CardProposal {
    theseus_core::card::state::proposals(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

fn labels(m: &theseus_sim::fake_discord::Msg) -> Vec<&str> {
    m.buttons.iter().map(|b| b.label.as_str()).collect()
}

#[tokio::test]
async fn an_add_shows_keep_edit_not_this_and_keep_writes_the_record() {
    let r = rig().await;
    let q = asked(&r, 1, CardAsk::Add).await;
    assert!(q.content.starts_with("❓ Decision · "), "{}", q.content);
    assert!(theseus_core::owner_note::is_headed(&q.content));
    assert_eq!(labels(&q), ["Keep", "Edit", "Not this"]);
    // Another member's press counts for nothing.
    r.press(&q.id, "Keep", (BEN, "ben"));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(state_of(&r, "cdp_00000001").state, ProposalState::Open);
    // The owner's does, and the buttons go.
    let pressed = r.press(&q.id, "Keep", (ANA, "ana"));
    r.until("kept", || {
        state_of(&r, "cdp_00000001").state == ProposalState::Kept
    })
    .await;
    r.until("ana hears it", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("📌 Kept on your card"))
        })
    })
    .await;
    // The press is acknowledged first (a type 6 deferred update), before `card.answer`
    // writes between turns; the card itself is then edited (the original response) with
    // no components. The stand-in does not apply an edit to a message, so the calls are
    // what is read.
    let of_press: Vec<_> = r
        .fake
        .replies()
        .into_iter()
        .filter(|x| x.interaction.as_deref() == Some(pressed.as_str()))
        .collect();
    assert_eq!(of_press[0].kind, "callback", "{of_press:?}");
    assert_eq!(of_press[0].response_type, Some(6), "{of_press:?}");
    assert!(
        of_press.iter().any(|x| x.kind == "original"),
        "{of_press:?}"
    );
}

#[tokio::test]
async fn not_this_declines_and_edit_opens_a_modal_holding_the_words() {
    let r = rig().await;
    let q = asked(&r, 2, CardAsk::Pattern).await;
    assert_eq!(labels(&q), ["Keep", "Edit", "Not this"]);
    // Edit: the modal holds the proposal's words.
    let pressed = r.press(&q.id, "Edit", (ANA, "ana"));
    let id = "card:m:cdp_00000002";
    r.until("the modal", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.modal.as_ref().is_some_and(|m| {
                    m["custom_id"] == id
                        && m["components"][0]["component"]["value"]
                            == "Moors at the north jetty, case 2"
                })
        })
    })
    .await;
    // A submit from a shared channel is refused and changes nothing.
    let fields = [("words", "Moors at the south jetty")];
    let from_lab = r
        .fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: LAB,
            custom_id: id,
            fields: &fields,
        })
        .unwrap();
    r.until("ana is told no", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(from_lab.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("🔐 Your answer did not count"))
        })
    })
    .await;
    assert_eq!(state_of(&r, "cdp_00000002").state, ProposalState::Open);
    // From her DM it is kept in her words.
    r.fake
        .submit_modal(
            &Sender {
                user: ANA,
                name: "ana",
                channel: None,
            },
            id,
            &fields,
            &[],
            None,
        )
        .unwrap();
    r.until("kept in her words", || {
        let p = state_of(&r, "cdp_00000002");
        p.state == ProposalState::Kept && p.kept_text.as_deref() == Some("Moors at the south jetty")
    })
    .await;
    // Not this, on another.
    let q = asked(&r, 3, CardAsk::Add).await;
    r.press(&q.id, "Not this", (ANA, "ana"));
    r.until("declined", || {
        state_of(&r, "cdp_00000003").state == ProposalState::Declined
    })
    .await;
}

#[tokio::test]
async fn a_stale_question_shows_end_it_and_the_press_reaches_card_answer_as_end() {
    let r = rig().await;
    let q = asked(&r, 4, CardAsk::Stale).await;
    assert_eq!(labels(&q), ["Keep", "Edit", "End it"]);
    let pressed = r.press(&q.id, "End it", (ANA, "ana"));
    // The press is `card.answer { option: end }` as ana. Until the live pass's handler is joined
    // the core names `end` as an option it does not know; either way ana is told, alone.
    r.until("ana is told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("↩️ Ended") || c.contains("\"end\""))
        })
    })
    .await;
}
