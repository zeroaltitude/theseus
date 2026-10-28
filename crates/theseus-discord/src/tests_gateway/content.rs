//! The one message a content label adds to the Discord flow (theseus-9ymz0,
//! design §6.3), through the stand-in's gateway: when the core raises a label
//! on the DM's session, one message appears in the DM with one Remove button;
//! the owner's press is `content.unlabel` as the presser, the core's, and
//! settles the message; a guest's press is refused, told alone, and removes
//! nothing; and a flag raised a second time says nothing more.

use std::sync::Arc;

use serde_json::json;
use theseus_core::content::{Flag, ItemKind, Label};
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_core::session::SessionRecord;
use theseus_judge::band::Band;
use theseus_sim::fake_discord::Msg;

use super::{bindings, guild, Rig, ANA, ANA_DM, BEN, LAB};

fn model(_: &std::path::Path, _: Arc<theseus_sim::fake_discord::FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![Scripted::text("Hello.")]))
}

/// A rig whose owner is ana alone (ben is a guest in the lab).
async fn rig() -> Rig {
    Rig::start_tweaked(
        model,
        guild(true),
        &bindings(false).replace(
            &format!("users = [\"{ANA}\"]"),
            &format!("users = [\"{ANA}\", \"{BEN}\"]"),
        ),
        &[],
        |c| {
            c.places.owner = Some(vec![format!("discord:{ANA}")]);
        },
    )
    .await
}

/// The DM's session, once the owner has spoken in it.
async fn dm_session(r: &Rig) -> String {
    r.say((ANA, "ana"), None, "hello");
    r.until("the reply", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.versions[0].contains("Hello."))
    })
    .await;
    r.core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .expect("the DM has a session")
}

fn label(flag: Flag) -> Label {
    Label {
        flag,
        node: Some("nod_e1".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch https://example.com/x".into(),
        confidence: 0.93,
        band: Band::Act,
        at_ms: 1,
        judgment: String::new(),
    }
}

/// The label on the session's record, then the core's one post about it.
fn raise(r: &Rig, sid: &str, l: &Label) -> bool {
    let mut rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    let mut flags = rec.data.take().map(|b| *b).unwrap_or_default();
    let raised = flags.raise(l.clone());
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(sid, &rec).unwrap();
    raised && r.core.tell_label(sid, l).unwrap()
}

fn carrying(r: &Rig, sid: &str, flag: Flag) -> bool {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    rec.data.is_some_and(|d| d.has(flag))
}

fn notes(r: &Rig) -> Vec<Msg> {
    r.posted(ANA_DM)
        .into_iter()
        .filter(|m| {
            m.buttons
                .iter()
                .any(|b| b.custom_id.starts_with("content:"))
        })
        .collect()
}

fn told(r: &Rig, pressed: &str, start: &str) -> impl Fn() -> bool {
    let fake = r.fake.clone();
    let (pressed, start) = (pressed.to_string(), start.to_string());
    move || {
        fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content.as_deref().is_some_and(|c| c.starts_with(&start))
        })
    }
}

/// A message in `channel` as the bot's, holding one button: a copy of the
/// note's, pressed where the note never is (the DM is the owner's alone).
async fn forge(r: &Rig, channel: u64, custom_id: &str) -> String {
    let forged = json!({"content": "a copy of a note", "components": [{"type": 1,
        "components": [{"type": 2, "style": 2, "label": "Remove", "custom_id": custom_id}]}]});
    let url = format!("http://{}/api/v10/channels/{channel}/messages", r.fake.addr);
    let posted: serde_json::Value = reqwest::Client::new()
        .post(url)
        .json(&forged)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    posted["id"].as_str().unwrap().to_string()
}

/// A raised label is one message in the DM with one Remove button, in the
/// words of the design; a flag raised again says nothing more.
#[tokio::test]
async fn a_label_is_one_message_with_one_remove_button() {
    let r = rig().await;
    let sid = dm_session(&r).await;
    assert!(raise(&r, &sid, &label(Flag::Injection)));
    r.until("the note", || !notes(&r).is_empty()).await;
    let m = notes(&r).remove(0);
    assert_eq!(
        m.versions.last().unwrap(),
        "⚑ **injection** — this session is holding: a fetched page from `http.fetch \
         https://example.com/x` tried to steer me, so writes, programs and sends are refused \
         until you clear it."
    );
    assert_eq!(m.buttons.len(), 1, "one button");
    assert_eq!(m.buttons[0].label, "Remove");
    assert_eq!(
        m.buttons[0].custom_id,
        format!("content:remove:{sid}:injection")
    );
    // A second raise of the same flag is not raised, so nothing is told.
    assert!(!raise(&r, &sid, &label(Flag::Injection)));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(notes(&r).len(), 1, "one message, once");
    // A cn-disqualifying flag is its own message, with its own word and no urgency.
    assert!(raise(&r, &sid, &label(Flag::Confidential)));
    r.until("the second note", || notes(&r).len() == 2).await;
    let c = notes(&r).remove(1);
    assert!(c
        .versions
        .last()
        .unwrap()
        .starts_with("⚑ **confidential** — China-origin models are off for this session"));
}

/// The owner's press removes the flag as the presser and settles the
/// message; a guest's press on a copy of it is refused, told alone, and
/// removes nothing (the core's rule, `judge_act`'s).
#[tokio::test]
async fn the_owners_press_removes_the_label_and_a_guests_is_refused() {
    let r = rig().await;
    let sid = dm_session(&r).await;
    raise(&r, &sid, &label(Flag::Injection));
    r.until("the note", || !notes(&r).is_empty()).await;
    let m = notes(&r).remove(0);

    // A guest, on a copy in the lab: refused, ledgered, and the label stays.
    let copy = forge(&r, LAB, &m.buttons[0].custom_id).await;
    let p = r.press(&copy, "Remove", (BEN, "ben"));
    r.until(
        "ben is refused",
        told(&r, &p, "🔐 Your removal did not count"),
    )
    .await;
    assert!(carrying(&r, &sid, Flag::Injection));

    // The owner, in the DM: the label goes and the message settles. The stand-in
    // does not apply an edit to a message, so the press's calls are what is read:
    // the acknowledgement (a deferred update), then the original response edited
    // to say who removed it, with no components.
    let pressed = r.press(&m.id, "Remove", (ANA, "ana"));
    r.until("the label is removed", || {
        !carrying(&r, &sid, Flag::Injection)
    })
    .await;
    r.until("the message settles", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.kind == "original"
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.ends_with("✅ Removed by discord:ana."))
        })
    })
    .await;
    let of_press: Vec<_> = r
        .fake
        .replies()
        .into_iter()
        .filter(|x| x.interaction.as_deref() == Some(pressed.as_str()))
        .collect();
    assert_eq!(of_press[0].kind, "callback", "{of_press:?}");
    assert_eq!(of_press[0].response_type, Some(6), "{of_press:?}");
    let rows: Vec<_> = r
        .core
        .store
        .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.kind == "content.removed")
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["flag"], "injection");
    assert_eq!(rows[0].data["via"], "discord:dm");
}
