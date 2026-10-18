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

/// The reply of `turn_offer` posted with no offer in it, as the core writes it
/// when the judge's batch has not landed: the message it made.
async fn plain_reply(r: &Rig) -> Msg {
    let place = format!("dm:{ANA}");
    let sid = r.core.outbox.place_session(&place).unwrap().unwrap();
    let node = assistant(&r.core, &sid, "A reply that can become a task.");
    let body = reply_body("turn_offer", &node, &sid, vec![]);
    r.core
        .outbox
        .post(&sid, "", &format!("discord:dm:{ANA}"), body)
        .unwrap();
    r.until("the plain reply", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("can become a task"))
    })
    .await;
    r.posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("can become a task"))
        .unwrap()
}

/// theseus-ypnxt.5: the judge's batch lands after the reply; the core's `offer`
/// post makes the lane edit the reply once, to add the button, and the press
/// then opens the modal with the brief.
#[tokio::test]
async fn a_late_offer_adds_the_button_to_the_reply_by_one_edit() {
    let r = offer_rig().await;
    let m = plain_reply(&r).await;
    assert!(m.buttons.iter().all(|b| b.custom_id != BUTTON), "{m:?}");
    let sid = r
        .core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .unwrap();
    r.core
        .outbox
        .post(
            &sid,
            "",
            &format!("discord:dm:{ANA}"),
            json!({"kind": "offer", "turn_id": "turn_offer", "brief": LATER}),
        )
        .unwrap();
    r.until("the button by an edit", || {
        r.posted(ANA_DM)
            .iter()
            .find(|x| x.id == m.id)
            .is_some_and(|x| x.buttons.iter().any(|b| b.custom_id == BUTTON))
    })
    .await;
    let after = r.posted(ANA_DM).into_iter().find(|x| x.id == m.id).unwrap();
    assert_eq!(after.edits, m.edits + 1, "one edit: {after:?}");
    assert_eq!(after.content, m.content, "the words are the same");
    assert_eq!(
        r.posted(ANA_DM).len(),
        r.posted(ANA_DM).iter().filter(|x| x.id != after.id).count() + 1,
        "no message was added"
    );
    let iid = press(&r, &after, (ANA, "ana"));
    let modal = answered(&r, &iid).await.modal.expect("the new-task modal");
    let brief = modal["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["component"]["custom_id"] == "brief")
        .unwrap();
    assert_eq!(brief["component"]["value"], LATER);
}

/// A late offer for a reply this lane does not hold, or one that has its
/// button, or in a shared place, changes nothing.
#[tokio::test]
async fn a_late_offer_changes_nothing_for_an_unknown_or_a_chipped_or_a_shared_reply() {
    let r = offer_rig().await;
    let sid = r
        .core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .unwrap();
    let late = |turn: &str| json!({"kind": "offer", "turn_id": turn, "brief": LATER});
    // A turn the lane never replied to.
    r.core
        .outbox
        .post(&sid, "", &format!("discord:dm:{ANA}"), late("turn_unknown"))
        .unwrap();
    // A reply that carried its offer already.
    let m = offered(
        &r,
        (&format!("dm:{ANA}"), format!("discord:dm:{ANA}")),
        ANA_DM,
    )
    .await;
    r.core
        .outbox
        .post(&sid, "", &format!("discord:dm:{ANA}"), late("turn_offer"))
        .unwrap();
    // A shared place's reply.
    let lab = r
        .core
        .outbox
        .place_session(&format!("channel:{LAB}"))
        .unwrap()
        .unwrap();
    let node = assistant(&r.core, &lab, "A shared reply.");
    r.core
        .outbox
        .post(
            &lab,
            "",
            &format!("discord:channel:{LAB}"),
            reply_body("turn_shared", &node, &lab, vec![]),
        )
        .unwrap();
    r.until("the shared reply", || {
        r.posted(LAB)
            .iter()
            .any(|x| x.content.contains("A shared reply"))
    })
    .await;
    r.core
        .outbox
        .post(
            &lab,
            "",
            &format!("discord:channel:{LAB}"),
            late("turn_shared"),
        )
        .unwrap();
    let c = r.core.clone();
    r.until("the outbox drains", move || {
        c.outbox.open_for(&format!("discord:dm:{ANA}")).is_empty()
            && c.outbox
                .open_for(&format!("discord:channel:{LAB}"))
                .is_empty()
    })
    .await;
    let shared = r
        .posted(LAB)
        .into_iter()
        .find(|x| x.content.contains("A shared reply"))
        .unwrap();
    assert!(
        shared.buttons.iter().all(|b| b.custom_id != BUTTON),
        "{shared:?}"
    );
    let same = r.posted(ANA_DM).into_iter().find(|x| x.id == m.id).unwrap();
    assert_eq!(same.edits, m.edits, "no edit: {same:?}");
}

/// The replies a late offer can join are a private place's alone (the join's
/// fix): sixteen shared-place replies after a DM's do not push the DM's out
/// of the list, so its late offer still adds the button.
#[tokio::test]
async fn shared_replies_never_push_a_dms_reply_out_of_a_late_offers_reach() {
    let r = offer_rig().await;
    let m = plain_reply(&r).await;
    let lab = r
        .core
        .outbox
        .place_session(&format!("channel:{LAB}"))
        .unwrap()
        .unwrap();
    for i in 0..16 {
        let node = assistant(&r.core, &lab, &format!("Shared reply {i}."));
        let body = reply_body(&format!("turn_shared_{i}"), &node, &lab, vec![]);
        r.core
            .outbox
            .post(&lab, "", &format!("discord:channel:{LAB}"), body)
            .unwrap();
    }
    r.until("the sixteen shared replies", || {
        r.posted(LAB)
            .iter()
            .filter(|x| x.content.contains("Shared reply"))
            .count()
            == 16
    })
    .await;
    let sid = r
        .core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .unwrap();
    let late = json!({"kind": "offer", "turn_id": "turn_offer", "brief": LATER});
    r.core
        .outbox
        .post(&sid, "", &format!("discord:dm:{ANA}"), late)
        .unwrap();
    r.until("the button by an edit", || {
        r.posted(ANA_DM)
            .iter()
            .find(|x| x.id == m.id)
            .is_some_and(|x| x.buttons.iter().any(|b| b.custom_id == BUTTON))
    })
    .await;
}
