//! A question's grant on the card (theseus-drjs): the offer's line, whole on
//! a narrow pane, and its key. `a` (a command's prefix) or `e` (edits in the
//! project) approves and takes the offer, through `work.answer`'s
//! `approve_grant`, which names no pattern: the daemon grants what it
//! offered. With no offer on the card, `a` shows every session as before.

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::grants::GrantOffer;

use crate::tests::*;

fn offering(offer: GrantOffer) -> std::sync::Arc<std::sync::Mutex<World>> {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
        q.input = json!({"argv": ["python3", "-m", "unittest"]});
        q.grant_offer = Some(offer);
        w.board["confirms"] = json!([q]);
    }
    world
}

#[tokio::test]
async fn the_card_offers_the_grant_and_a_takes_it() {
    let words = ["python3", "-m", "unittest"].map(String::from);
    let world = offering(GrantOffer::prefix(&words));
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(80, 24, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("don't ask again").await;
    let screen = rig.screen();
    let line = screen
        .iter()
        .find(|l| l.contains("don't ask again: "))
        .expect("the offer's line");
    assert!(
        line.contains("[a] don't ask again: python3 -m unittest"),
        "{screen:?}"
    );
    assert!(
        screen
            .iter()
            .any(|l| l.contains("[a] approve + don't ask again")),
        "{screen:?}"
    );
    rig.press(&[KeyCode::Char('a')]).await;
    rig.asked("work.answer", 1).await;
    assert_eq!(
        rig.daemon().asked("work.answer"),
        [json!({"question_id": "cor_spec01", "option": "approve_grant", "author": "the TUI"})]
    );
}

#[tokio::test]
async fn e_takes_an_edits_offer_and_a_is_the_filter_without_one() {
    let world = offering(GrantOffer::edits("/work/app"));
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[e] don't ask again: edits under /work/app")
        .await;
    // `a` is no edit's key: it is the filter's, and answers nothing.
    rig.press(&[KeyCode::Char('a')]).await;
    assert!(rig.daemon().asked("work.answer").is_empty());
    rig.press(&[KeyCode::Char('e')]).await;
    rig.asked("work.answer", 1).await;
    assert_eq!(
        rig.daemon().asked("work.answer"),
        [json!({"question_id": "cor_spec01", "option": "approve_grant", "author": "the TUI"})]
    );
}
