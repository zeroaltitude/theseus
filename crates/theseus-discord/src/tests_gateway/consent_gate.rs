//! The consent pattern's gate, through the fake gateway (issue 59), for the MCP sign-in, Google's
//! consent and the alerts' confirmation alike: a forged, unissued or stale step is refused in fixed
//! words before any request to the daemon; a press in a shared place is refused and shown nothing;
//! each interaction is acted on once, however often Discord delivers it; and a second owner cannot
//! send the first owner's form. `consent::SENT` is what the handlers sent the daemon.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::{Forged, Modal, Msg, DEFAULT_GUILD};

use super::{guild, Rig, ANA, ANA_DM, BEN, LAB};
use crate::runtime::consent::SENT;
use crate::runtime::{aws_alerts, google_sign_in, mcp_sign_in};

/// `#lab` bound private to ana and ben, both owners; ana's DM bound.
fn bindings_two_owners() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\", \"{BEN}\"]\nmention_only = false\nprivate = true\nsteps = \"open\"\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\nsteps = \"open\"\n"
    )
}

/// `#lab` shared, ana's DM bound.
fn bindings_shared_lab() -> String {
    super::bindings(false)
}

async fn rig(bindings: &str, open_lab: bool) -> Rig {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    Rig::start_tweaked(model, guild(open_lab), bindings, &[], |_| {}).await
}

fn sent() -> Vec<&'static str> {
    SENT.lock().unwrap().clone()
}

/// The three cards, as their offers stage them, each found in ana's DM.
async fn cards(r: &Rig) -> (Msg, Msg, Msg) {
    for body in [
        json!({"kind": "mcp_sign_in", "server": "docs",
               "text": theseus_core::mcp::offer::card_text("docs")}),
        json!({"kind": "google_sign_in", "account": "owner@example.invalid",
               "text": theseus_core::google::offer::card_text("owner@example.invalid")}),
        json!({"kind": "aws_alerts", "text": theseus_core::aws::offer::CARD}),
    ] {
        let (post, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
        r.core.store.append(&frame).unwrap();
        r.core.outbox.posted(&post);
    }
    let find = |words: &str| {
        r.posted(ANA_DM)
            .into_iter()
            .find(|m| m.versions[0].contains(words))
    };
    r.until("the three cards in ana's DM", || {
        find("Sign in to the MCP server").is_some()
            && find("Connect Google for").is_some()
            && find("Confirm the AWS alerts").is_some()
    })
    .await;
    (
        find("Sign in to the MCP server").unwrap(),
        find("Connect Google for").unwrap(),
        find("Confirm the AWS alerts").unwrap(),
    )
}

fn told(r: &Rig, interaction: &str, words: &str) -> bool {
    r.fake.replies().iter().any(|x| {
        x.interaction.as_deref() == Some(interaction)
            && x.content.as_deref().is_some_and(|c| c.starts_with(words))
    })
}

fn answers(r: &Rig, interaction: &str) -> usize {
    r.fake
        .replies()
        .iter()
        .filter(|x| x.interaction.as_deref() == Some(interaction))
        .count()
}

fn forged(r: &Rig, card: &Msg, custom_id: &str, channel: Option<u64>) -> String {
    r.fake
        .press_forged(&Forged {
            message: &card.id,
            custom_id,
            user: (ANA, "ana"),
            channel,
        })
        .unwrap()
}

fn form(r: &Rig, user: (u64, &str), channel: Option<u64>, custom_id: &str, value: &str) -> String {
    let fields = [("address", value)];
    match channel {
        None => r
            .fake
            .submit_modal(
                &Sender {
                    user: user.0,
                    name: user.1,
                    channel: None,
                },
                custom_id,
                &fields,
                &[],
                None,
            )
            .unwrap(),
        Some(c) => r
            .fake
            .modal(&Modal {
                user: user.0,
                name: user.1,
                channel: c,
                custom_id,
                fields: &fields,
            })
            .unwrap(),
    }
}

/// The modal the binding answered `interaction` with: its custom id.
fn opened(r: &Rig, interaction: &str) -> Option<String> {
    r.fake
        .replies()
        .iter()
        .find(|x| x.interaction.as_deref() == Some(interaction))
        .and_then(|x| x.modal.as_ref())
        .and_then(|m| m["custom_id"].as_str().map(str::to_string))
}

/// Ids this binding never issued, and an id that tries to carry an agreement, are refused in fixed
/// words, and nothing reaches the daemon.
#[tokio::test]
async fn forged_and_unissued_steps_are_refused_before_any_request() {
    let r = rig(&bindings_shared_lab(), true).await;
    let (mcp, google, _) = cards(&r).await;
    let agree = forged(&r, &mcp, "mcpin:agree:forged1", None);
    let paste = forged(&r, &mcp, "mcpin:paste:forged1", None);
    let gpaste = forged(&r, &google, "gsi:paste:forged2", None);
    let submits = [
        form(
            &r,
            (ANA, "ana"),
            None,
            "mcpin:m:forged1",
            "https://x.invalid/?code=c&state=s",
        ),
        form(
            &r,
            (ANA, "ana"),
            None,
            "mcpin:m:forged1:1",
            "https://x.invalid/?code=c&state=s",
        ),
        form(
            &r,
            (ANA, "ana"),
            None,
            "gsi:m:forged2",
            "http://127.0.0.1:1/?code=c&state=s",
        ),
        form(
            &r,
            (ANA, "ana"),
            None,
            "awsc:m:forged3",
            "0123456789abcdef0123456789abcdef",
        ),
    ];
    r.until("each forged step is refused", || {
        told(&r, &agree, mcp_sign_in::EXPIRED)
            && told(&r, &paste, mcp_sign_in::EXPIRED)
            && told(&r, &gpaste, google_sign_in::EXPIRED)
            && told(&r, &submits[0], mcp_sign_in::EXPIRED)
            && told(&r, &submits[1], mcp_sign_in::EXPIRED)
            && told(&r, &submits[2], google_sign_in::EXPIRED)
            && told(&r, &submits[3], aws_alerts::EXPIRED)
    })
    .await;
    for i in [&paste, &gpaste] {
        assert_eq!(opened(&r, i), None, "no form was opened");
    }
    assert!(
        sent().is_empty(),
        "nothing reached the daemon: {:?}",
        sent()
    );
}

/// A press in a shared channel, on any of the three, is refused and shown nothing: no address, no
/// form, no request.
#[tokio::test]
async fn a_press_in_a_shared_place_is_refused_and_shows_nothing() {
    let r = rig(&bindings_shared_lab(), true).await;
    let (mcp, google, alerts) = cards(&r).await;
    let presses = [
        (
            forged(&r, &mcp, "mcpin:in:docs", Some(LAB)),
            mcp_sign_in::NOT_HERE,
        ),
        (
            forged(&r, &mcp, "mcpin:out:docs", Some(LAB)),
            mcp_sign_in::NOT_HERE,
        ),
        (
            forged(&r, &google, "gsi:in:google", Some(LAB)),
            google_sign_in::NOT_HERE,
        ),
        (
            forged(&r, &google, "gsi:out:google", Some(LAB)),
            google_sign_in::NOT_HERE,
        ),
        (
            forged(&r, &alerts, "awsc:in:alerts", Some(LAB)),
            aws_alerts::NOT_HERE,
        ),
    ];
    r.until("each shared press is refused", || {
        presses.iter().all(|(i, w)| told(&r, i, w))
    })
    .await;
    for (i, _) in &presses {
        assert_eq!(opened(&r, i), None);
        assert_eq!(answers(&r, i), 1, "one refusal, nothing more");
    }
    assert!(sent().is_empty(), "{:?}", sent());
}

/// Each interaction is acted on once: a Sign out, a Revoke and a Confirm alerts press, and a form's
/// submit, each delivered again by Discord, are not acted on again.
#[tokio::test]
async fn each_interaction_is_acted_on_once() {
    let r = rig(&bindings_shared_lab(), true).await;
    let (mcp, google, alerts) = cards(&r).await;
    let out = r.press(&mcp.id, "Sign out", (ANA, "ana"));
    let revoke = r.press(&google.id, "Revoke Google", (ANA, "ana"));
    let confirm = r.press(&alerts.id, "Confirm alerts", (ANA, "ana"));
    r.until("each is answered", || {
        (told(&r, &out, mcp_sign_in::NOTHING_KEPT) || told(&r, &out, mcp_sign_in::NO_SIGN_OUT))
            && told(&r, &revoke, google_sign_in::NO_REVOKE)
            && opened(&r, &confirm).is_some()
    })
    .await;
    let id = opened(&r, &confirm).unwrap();
    let submit = form(
        &r,
        (ANA, "ana"),
        None,
        &id,
        "0123456789abcdef0123456789abcdef",
    );
    r.until("the form is answered", || {
        told(&r, &submit, aws_alerts::AWS_NO)
    })
    .await;
    let before: Vec<usize> = [&out, &revoke, &confirm, &submit]
        .iter()
        .map(|i| answers(&r, i))
        .collect();
    let was = sent();
    for i in [&out, &revoke, &confirm, &submit] {
        assert!(r.fake.redeliver(i), "delivered again");
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let after: Vec<usize> = [&out, &revoke, &confirm, &submit]
        .iter()
        .map(|i| answers(&r, i))
        .collect();
    assert_eq!(before, after, "no second answer");
    assert_eq!(sent(), was, "no second request");
    for m in ["mcp.logout", "google.revoke", "aws.confirm_alerts"] {
        assert_eq!(
            sent().iter().filter(|s| **s == m).count(),
            1,
            "{m}: {:?}",
            sent()
        );
    }
}

/// A second owner, pressing in a private place, cannot send the first owner's form: it is refused
/// before any request; the first owner's own submit is sent.
#[tokio::test]
async fn a_second_owner_cannot_send_the_first_owners_form() {
    let r = rig(&bindings_two_owners(), false).await;
    let (_, _, alerts) = cards(&r).await;
    let confirm = r.press(&alerts.id, "Confirm alerts", (ANA, "ana"));
    r.until("ana's form opens", || opened(&r, &confirm).is_some())
        .await;
    let id = opened(&r, &confirm).unwrap();
    let token = "0123456789abcdef0123456789abcdef";
    let bens = form(&r, (BEN, "ben"), Some(LAB), &id, token);
    r.until("ben is refused", || told(&r, &bens, aws_alerts::NOT_YOURS))
        .await;
    assert!(sent().is_empty(), "{:?}", sent());
    let anas = form(&r, (ANA, "ana"), None, &id, token);
    r.until("ana's is sent", || told(&r, &anas, aws_alerts::AWS_NO))
        .await;
    assert_eq!(sent(), vec!["aws.confirm_alerts"]);
}
