//! The task offer on Discord (theseus-bh3t.5): a reply whose post carries an
//! `offer` has `➕ Make this a task` on its last message in a private place; the
//! owner's press opens the new-task modal with the message as the brief; a
//! guest's is refused; a shared place shows no button; a task started from it
//! takes the button off the reply.

use super::commands::{ana, answered, said, started};
use super::*;
use crate::tests_cite::{assistant, reply_body};
use theseus_sim::fake_discord::Pressed;

const OPEN: u64 = 900_000_000_000_000_020;
const LATER: &str =
    "Clean-bench: we should clean up the bench reports later and re-run the async suite";
const BUTTON: &str = "tb:offer:turn_offer";

async fn offer_rig() -> Rig {
    let bindings = format!(
        "{}[[channel]]\nid = \"{OPEN}\"\nname = \"open\"\nusers = [\"{ANA}\"]\nmention_only = false\n",
        bindings()
    );
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![
                    Scripted::text("Hello."),
                    Scripted::text("Hello."),
                    Scripted::text("Hello."),
                ],
                vec![],
            )
        },
        guild().channel(OPEN, "open"),
        &bindings,
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, "hello");
    r.until("the DM's reply", || {
        r.posted(ANA_DM).iter().any(|m| m.content.contains("Hello"))
    })
    .await;
    r.say((ANA, "ana"), Some(OPEN), "hello");
    r.until("the open channel's reply", || {
        r.posted(OPEN).iter().any(|m| m.content.contains("Hello"))
    })
    .await;
    r.say((ANA, "ana"), Some(LAB), "hello");
    r.until("the lab's reply", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Hello"))
    })
    .await;
    r
}

/// A reply post carrying an offer, to `place` (a bound place's name) in `channel`;
/// the message it made.
async fn offered(r: &Rig, (place, target): (&str, String), channel: u64) -> Msg {
    let sid = r.core.outbox.place_session(place).unwrap().unwrap();
    let node = assistant(&r.core, &sid, "A reply that can become a task.");
    let mut body = reply_body("turn_offer", &node, &sid, vec![]);
    body["offer"] = json!({"brief": LATER});
    r.core.outbox.post(&sid, "", &target, body).unwrap();
    r.until("the offering reply", || {
        r.posted(channel)
            .iter()
            .any(|m| m.content.contains("can become a task"))
    })
    .await;
    r.posted(channel)
        .into_iter()
        .find(|m| m.content.contains("can become a task"))
        .unwrap()
}

fn press(r: &Rig, m: &Msg, who: (u64, &str)) -> String {
    r.fake
        .press(&Pressed {
            message: &m.id,
            button: BUTTON,
            user: who.0,
            name: who.1,
        })
        .unwrap()
}

#[tokio::test]
async fn the_press_opens_the_modal_with_the_message_as_the_brief_and_a_submit_starts_the_task() {
    let r = offer_rig().await;
    let m = offered(
        &r,
        (&format!("dm:{ANA}"), format!("discord:dm:{ANA}")),
        ANA_DM,
    )
    .await;
    let b = m.buttons.last().unwrap();
    assert_eq!(
        (b.label.as_str(), b.custom_id.as_str()),
        ("➕ Make this a task", BUTTON)
    );
    let iid = press(&r, &m, (ANA, "ana"));
    let modal = answered(&r, &iid).await.modal.expect("the new-task modal");
    let brief = modal["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["component"]["custom_id"] == "brief")
        .unwrap();
    assert_eq!(brief["component"]["value"], LATER);
    assert_eq!(modal["custom_id"], "nt:turn_offer");
    let sub = r
        .fake
        .submit_modal_from(
            &ana(),
            &m.id,
            "nt:turn_offer",
            &[("brief", LATER)],
            &[("budget", &["2"]), ("report", &["home"])],
        )
        .unwrap();
    r.until("started", || said(&r, &sub).contains("Started"))
        .await;
    assert!(started(&r, "Clean-bench").is_some());
    r.until("the button is gone", || {
        r.posted(ANA_DM)
            .iter()
            .find(|x| x.id == m.id)
            .is_some_and(|x| x.buttons.iter().all(|b| b.custom_id != BUTTON))
    })
    .await;
}

#[tokio::test]
async fn a_guests_press_is_refused_to_the_presser_alone() {
    let r = offer_rig().await;
    let m = offered(
        &r,
        (&format!("channel:{LAB}"), format!("discord:channel:{LAB}")),
        LAB,
    )
    .await;
    let iid = press(&r, &m, (CY, "cy"));
    let rep = answered(&r, &iid).await;
    assert!(rep.modal.is_none(), "{rep:?}");
    assert!(said(&r, &iid).contains("Only the owner can start a task here"));
}

#[tokio::test]
async fn a_shared_place_shows_no_button() {
    let r = offer_rig().await;
    let m = offered(
        &r,
        (
            &format!("channel:{OPEN}"),
            format!("discord:channel:{OPEN}"),
        ),
        OPEN,
    )
    .await;
    assert!(m.buttons.iter().all(|b| b.custom_id != BUTTON), "{m:?}");
}
