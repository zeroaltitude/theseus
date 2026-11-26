//! Connecting Google and confirming the AWS alerts from Discord (issue 59), through the fake
//! gateway: each offer card reaches the owner's DM with its buttons; a form's value goes to the
//! daemon and is in no exit (a reply, a post, a ledger row, the outbox, the log at trace level);
//! a form from a shared channel is refused in fixed words. The consent and the confirmation
//! themselves are the core's tests (`google/tests_discord.rs`, `aws/tests_discord.rs`, with the
//! fakes); here no account is bound, so the daemon answers each call with a failure, and the
//! binding says it in its own fixed words.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::Modal;

use super::{bindings, guild, Rig, ANA, ANA_DM, LAB};
use crate::runtime::aws_alerts::{AWS_NO, NOT_HERE as ALERTS_NOT_HERE};
use crate::runtime::google_sign_in::{EXPIRED, NOT_HERE, NO_REVOKE};

/// Invented: the address a browser ended on, with a code in it. Never in any output.
const PASTED: &str = "http://127.0.0.1:41999/?code=INVENTED-GOOGLE-7713&state=abc";
const CODE: &str = "INVENTED-GOOGLE-7713";
/// Invented: SNS's token, in the link it was mailed in. Never in any output.
const ALERTS: &str = "https://sns.example.invalid/?Action=ConfirmSubscription&Token=7a7b7c7d7e7f7071727374757677787980818283848586878889909192939495";
const TOKEN: &str = "7a7b7c7d7e7f7071727374757677787980818283848586878889909192939495";

#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

fn told(r: &Rig, interaction: &str, words: &str) -> bool {
    r.fake.replies().iter().any(|x| {
        x.interaction.as_deref() == Some(interaction)
            && x.content.as_deref().is_some_and(|c| c.starts_with(words))
    })
}

async fn rig() -> Rig {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await
}

fn traced() -> (Arc<Mutex<Vec<u8>>>, tracing::subscriber::DefaultGuard) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_max_level(tracing::Level::TRACE)
        .finish();
    (buf, tracing::subscriber::set_default(sub))
}

/// The card a turn offers, as its offer tool stages it.
fn offered(r: &Rig, body: serde_json::Value) {
    let (post, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&post);
}

/// A form of one field, `value`, submitted by ana from her DM and from #lab.
fn submitted(r: &Rig, custom_id: &str, value: &str) -> (String, String) {
    let fields = [("address", value)];
    let from_dm = r
        .fake
        .submit_modal(
            &Sender {
                user: ANA,
                name: "ana",
                channel: None,
            },
            custom_id,
            &fields,
            &[],
            None,
        )
        .unwrap();
    let from_lab = r
        .fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: LAB,
            custom_id,
            fields: &fields,
        })
        .unwrap();
    (from_dm, from_lab)
}

#[tokio::test]
async fn the_google_card_and_form_say_fixed_words_and_the_address_is_in_no_exit() {
    let (buf, _guard) = traced();
    let r = rig().await;
    // The card a turn offers, as the offer tool stages it.
    let account = "owner@example.invalid";
    let body = json!({"kind": "google_sign_in", "account": account,
                      "text": theseus_core::google::offer::card_text(account)});
    offered(&r, body);
    let card_of = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("Connect Google for");
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(card_of)
    })
    .await;
    let card = r.posted(ANA_DM).into_iter().find(card_of).unwrap();
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Connect Google", "Revoke Google"]);
    assert!(r.posted(LAB).iter().all(|m| !card_of(m)), "never in #lab");

    // Revoke, with no account bound: fixed words.
    let out = r.press(&card.id, "Revoke Google", (ANA, "ana"));
    r.until("revoke's words", || told(&r, &out, NO_REVOKE))
        .await;

    // The form, from her DM: the daemon's failure in fixed words; from #lab: refused.
    // The form, for a consent this binding never began: refused before any request.
    let (from_dm, from_lab) = submitted(&r, "gsi:m:0123abcd", PASTED);
    r.until("ana is told in fixed words", || told(&r, &from_dm, EXPIRED))
        .await;
    r.until("the shared channel is refused", || {
        told(&r, &from_lab, NOT_HERE)
    })
    .await;

    in_no_exit(&r, &buf, &[CODE, "41999/?code"]);
}

#[tokio::test]
async fn the_alerts_card_and_form_say_fixed_words_and_the_token_is_in_no_exit() {
    let (buf, _guard) = traced();
    let r = rig().await;
    offered(
        &r,
        json!({"kind": "aws_alerts", "text": theseus_core::aws::offer::CARD}),
    );
    let card_of =
        |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("Confirm the AWS alerts");
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(card_of)
    })
    .await;
    let card = r.posted(ANA_DM).into_iter().find(card_of).unwrap();
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Confirm alerts"]);
    assert!(r.posted(LAB).iter().all(|m| !card_of(m)), "never in #lab");
    // Her press opens the form; sent from her DM, no account is bound, so AWS's failure in fixed
    // words; the same form from #lab is refused.
    let press = r.press(&card.id, "Confirm alerts", (ANA, "ana"));
    let form_of = || {
        r.fake
            .replies()
            .iter()
            .find(|x| x.interaction.as_deref() == Some(press.as_str()))
            .and_then(|x| x.modal.as_ref())
            .and_then(|m| m["custom_id"].as_str().map(str::to_string))
    };
    r.until("her form opens", || form_of().is_some()).await;
    let id = form_of().unwrap();
    let (from_dm, from_lab) = submitted(&r, &id, ALERTS);
    r.until("ana is told in fixed words", || told(&r, &from_dm, AWS_NO))
        .await;
    r.until("the shared channel is refused", || {
        told(&r, &from_lab, ALERTS_NOT_HERE)
    })
    .await;
    in_no_exit(&r, &buf, &[TOKEN]);
}

/// None of `values` is in any exit.
fn in_no_exit(r: &Rig, buf: &Arc<Mutex<Vec<u8>>>, values: &[&str]) {
    let replies = serde_json::to_string(&r.fake.replies()).unwrap();
    let posts: String = [ANA_DM, LAB]
        .iter()
        .flat_map(|c| r.posted(*c))
        .map(|m| format!("{m:?}"))
        .collect();
    r.core.store.flush_waiting();
    let rows: Vec<(u64, theseus_core::ledger::LedgerRow)> =
        r.core.store.ledger_tail(100_000).unwrap();
    let rows = format!("{rows:?}");
    let outbox = format!(
        "{:?}",
        r.core
            .outbox
            .open_for(theseus_core::outbox::OPERATOR_TARGET)
    );
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    for (what, text) in [
        ("a reply", &replies),
        ("a post", &posts),
        ("a ledger row", &rows),
        ("the outbox", &outbox),
        ("the log", &log),
    ] {
        for v in values {
            assert!(!text.contains(v), "a pasted value is in {what}");
        }
    }
}
