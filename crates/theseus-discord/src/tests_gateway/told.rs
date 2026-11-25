//! A note of an act done unasked, with its Undo (issue 57), through the
//! stand-in's gateway: the note is the core's words with one Undo button whose
//! id is the post's; the owner's press in her DM is the undo's own method as
//! hers, once, and the note then says it was undone; a guest's press, and the
//! owner's in a shared place, undo nothing, and a second press calls nothing.

use std::sync::Arc;

use serde_json::json;
use theseus_core::approval::Answerer;
use theseus_core::owner_note::told::{self, Undo};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::{StandingAddParams, StandingKind, StandingScope};
use theseus_sim::fake_discord::Msg;

use super::{bindings, guild, Rig, ANA, ANA_DM, BEN, LAB};

fn model(_: &std::path::Path, _: Arc<theseus_sim::fake_discord::FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![]))
}

/// ana alone is the owner; ben may drive `#lab`, which is shared.
async fn rig() -> Rig {
    Rig::start_tweaked(
        model,
        guild(true),
        &bindings(false).replace(
            &format!("users = [\"{ANA}\"]"),
            &format!("users = [\"{ANA}\", \"{BEN}\"]"),
        ),
        &[],
        |c| c.places.owner = Some(vec![format!("discord:{ANA}")]),
    )
    .await
}

/// A standing order, as an act done unasked made it, and its note staged
/// through the helper: the order's id and the note's post id.
fn ordered(r: &Rig) -> (String, String) {
    let id = r
        .core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: "read the tide table in metres".into(),
                scope: StandingScope::Surface("terminal".into()),
                ..Default::default()
            },
            Answerer {
                label: "the CLI".into(),
                surface: theseus_core::approval::Surface::Cli,
                discord: None,
                agent: None,
            },
        )
        .unwrap()
        .record
        .id;
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus kept a standing order",
        "In “Tides”, without asking you",
        "Read the tide table in metres.",
        &Undo::StandingRevoke(id.clone()),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    (id, a.correlation_id)
}

/// How many revokes ended the order.
fn ends(r: &Rig, id: &str) -> usize {
    let all = r.core.runner.standing.all(&r.core.store).unwrap();
    all.iter().filter(|x| x.ends.as_deref() == Some(id)).count()
}

async fn note(r: &Rig, post: &str) -> Msg {
    let id = format!("told:undo:{post}");
    let has = |m: &Msg| m.buttons.iter().any(|b| b.custom_id == id);
    r.until("the note in ana's DM", || r.posted(ANA_DM).iter().any(has))
        .await;
    r.posted(ANA_DM).into_iter().find(has).unwrap()
}

async fn told_to(r: &Rig, pressed: &str, start: &str) {
    r.until(start, || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed)
                && x.content.as_deref().is_some_and(|c| c.starts_with(start))
        })
    })
    .await;
}

/// A copy of the note's button in `channel`, as the bot's message.
async fn forge(r: &Rig, channel: u64, custom_id: &str) -> String {
    let forged = json!({"content": "a copy of a note", "components": [{"type": 1,
        "components": [{"type": 2, "style": 2, "label": "Undo", "custom_id": custom_id}]}]});
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

/// The note is the core's words, with the command that undoes it (what every
/// surface without a button shows), and one Undo carrying the post id alone.
#[tokio::test]
async fn a_told_note_shows_its_words_its_command_and_one_undo() {
    let r = rig().await;
    let (id, post) = ordered(&r);
    let m = note(&r, &post).await;
    let (text, undo) = told::of(&r.core.kernel, &post).unwrap().unwrap();
    assert_eq!(undo, Undo::StandingRevoke(id.clone()));
    assert_eq!(m.content, text, "the words are the core's");
    assert!(theseus_core::owner_note::is_headed(&text), "{text}");
    assert!(text
        .starts_with("ℹ️ FYI · Theseus kept a standing order\nIn “Tides”, without asking you\n"));
    assert!(
        text.ends_with(&format!("\nUndo: `theseus standing revoke {id}`")),
        "{text}"
    );
    let got: Vec<(&str, &str)> = m
        .buttons
        .iter()
        .map(|b| (b.label.as_str(), b.custom_id.as_str()))
        .collect();
    assert_eq!(got, [("Undo", format!("told:undo:{post}").as_str())]);
    assert!(
        r.posted(LAB).iter().all(|x| x.versions[0] != text),
        "never in #lab"
    );
}

/// A guest's press and the owner's in a shared place undo nothing; the
/// owner's in her DM undoes once and the note says so; a second press calls
/// nothing.
#[tokio::test]
async fn the_owners_press_in_private_undoes_once_and_no_other_press_counts() {
    let r = rig().await;
    let (id, post) = ordered(&r);
    let m = note(&r, &post).await;
    let custom = format!("told:undo:{post}");

    let copy = forge(&r, LAB, &custom).await;
    let p = r.press(&copy, "Undo", (BEN, "ben"));
    told_to(&r, &p, "🔐 Your undo did not count").await;
    let p = r.press(&copy, "Undo", (ANA, "ana"));
    told_to(&r, &p, "🔐 Your undo did not count").await;
    assert_eq!(ends(&r, &id), 0, "nothing was revoked");
    assert!(told::undone(&r.core.store, &post).unwrap().is_none());

    let p = r.press(&m.id, "Undo", (ANA, "ana"));
    told_to(&r, &p, "↩️ Undone.").await;
    assert_eq!(ends(&r, &id), 1, "revoked once");
    r.until("the note says it was undone", || {
        r.fake.messages(ANA_DM).iter().any(|x| {
            x.id == m.id && x.content.ends_with("↩️ Undone by discord:ana.") && x.buttons.is_empty()
        })
    })
    .await;

    // A second press (a copy still showing the button) calls nothing.
    let again = forge(&r, ANA_DM, &custom).await;
    let p = r.press(&again, "Undo", (ANA, "ana"));
    told_to(&r, &p, "↩️ Already undone by discord:ana").await;
    assert_eq!(ends(&r, &id), 1, "not revoked again");
}
