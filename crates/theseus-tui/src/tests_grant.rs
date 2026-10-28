//! A plan question's other yes on the card (theseus-lyxy): the offer's line,
//! whole on a narrow pane, and its key. `e` approves and takes the offer,
//! through `work.answer`'s `approve_grant`, which names no pattern: the
//! daemon takes what it offered. With no offer on the card, `a` shows every
//! session as before.

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
async fn e_takes_the_plans_offer_and_a_is_the_filter_without_one() {
    let world = offering(GrantOffer::mode_edits());
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[e] approve the plan, and accept edits").await;
    // `a` is no key of this offer: it is the filter's, and answers nothing.
    rig.press(&[KeyCode::Char('a')]).await;
    assert!(rig.daemon().asked("work.answer").is_empty());
    rig.press(&[KeyCode::Char('e')]).await;
    rig.asked("work.answer", 1).await;
    assert_eq!(
        rig.daemon().asked("work.answer"),
        [json!({"question_id": "cor_spec01", "option": "approve_grant", "author": "the TUI"})]
    );
}
