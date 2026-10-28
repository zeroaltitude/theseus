//! Drop and Halt on the self notes (theseus-o1h3f.7), through the stand-in's gateway: a
//! `self_digest` post in the owner's DM carries a Drop for each item it says it started and a
//! Halt, a `self_join` post a Halt alone, the notes' words are the core's byte for byte, and a
//! press is `self.drop` or `self.halt` as the presser, which the core judges.

use std::sync::Arc;

use serde_json::json;
use theseus_core::approval::Answerer;
use theseus_core::config::self_improve::SelfMode;
use theseus_core::ledger::LedgerRow;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::rsi::{SelfBacklogParams, SelfHaltParams, SelfResumeParams};
use theseus_protocol::LedgerKind;
use theseus_sim::fake_discord::Msg;

use super::{bindings, guild, Rig, ANA, ANA_DM, BEN, LAB};

fn model(_: &std::path::Path, _: Arc<theseus_sim::fake_discord::FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![]))
}

/// A rig whose owner is ana alone (ben is a guest), the self steps acting.
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
            c.self_improve.mode = SelfMode::Act;
            c.self_improve.backlog_starts = Some(0);
            c.places.owner = Some(vec![format!("discord:{ANA}")]);
        },
    )
    .await
}

/// The note as the core stages it, to the owner's DM.
fn stage(r: &Rig, kind: &str, text: &str) {
    let (a, frame) = r
        .core
        .outbox
        .stage_to_operator(None, json!({"kind": kind, "text": text}))
        .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
}

/// The digest's words as the core writes them (`night_text`), for items `(id, title)`.
fn digest(items: &[(String, String)]) -> String {
    let mut t = String::from("Night: 3 started\n");
    for (id, title) in items {
        t.push_str(&format!(
            "- started work on: {title}: seen 3 times since Oct 3 (item {id})\n"
        ));
    }
    t.push_str("Reorder or drop with `theseus self move <item> <rank>` / `theseus self drop <item>`; `theseus self backlog` lists it.");
    t
}

fn fake_items(n: usize) -> Vec<(String, String)> {
    (1..=n)
        .map(|i| (format!("tsk_{i}"), format!("Mend pier {i}")))
        .collect()
}

async fn note(r: &Rig, text: &str) -> Msg {
    r.until("the note in ana's DM", || {
        r.posted(ANA_DM).iter().any(|m| m.versions[0] == text)
    })
    .await;
    r.posted(ANA_DM)
        .into_iter()
        .find(|m| m.versions[0] == text)
        .unwrap()
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

/// A digest naming three started items: a Drop each, in order, and one Halt; the words are the
/// core's, unchanged (planted: one word moved, or no arm in `jev_post`).
#[tokio::test]
async fn a_digest_gets_a_drop_for_each_started_item_and_a_halt_and_keeps_its_words() {
    let r = rig().await;
    let text = digest(&fake_items(3));
    stage(&r, "self_digest", &text);
    let m = note(&r, &text).await;
    assert_eq!(m.content, text, "the words are byte for byte the core's");
    let got: Vec<(&str, &str)> = m
        .buttons
        .iter()
        .map(|b| (b.label.as_str(), b.custom_id.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("Drop: Mend pier 1", "snote:drop:tsk_1"),
            ("Drop: Mend pier 2", "snote:drop:tsk_2"),
            ("Drop: Mend pier 3", "snote:drop:tsk_3"),
            ("Halt self-improvement", "snote:halt"),
        ]
    );
    assert!(
        r.posted(LAB).iter().all(|x| x.versions[0] != text),
        "never in #lab"
    );
}

/// A join note: Halt, and no Drop (planted: give it Drops).
#[tokio::test]
async fn a_join_note_gets_a_halt_and_no_drop_and_keeps_its_words() {
    let r = rig().await;
    // Even a join that quotes a started line has nothing to drop.
    let text = format!("Joined a change.\n{}", digest(&fake_items(2)));
    stage(&r, "self_join", &text);
    let m = note(&r, &text).await;
    assert_eq!(m.content, text);
    let ids: Vec<&str> = m.buttons.iter().map(|b| b.custom_id.as_str()).collect();
    assert_eq!(ids, ["snote:halt"]);
}

/// A digest past the cap posts, with the commands the note already names for the rest.
#[tokio::test]
async fn a_digest_over_the_cap_posts_and_names_where_the_rest_are() {
    let r = rig().await;
    let text = digest(&fake_items(30));
    stage(&r, "self_digest", &text);
    let m = note(&r, &text).await;
    assert_eq!(m.buttons.len(), crate::runtime::self_note::MAX_DROPS + 1);
    assert!(m.content.contains("`theseus self drop <item>`"));
    assert!(m.content.contains("`theseus self backlog`"));
}

/// A message in `channel` as the bot's, holding one button: a copy of a note's, pressed where
/// the note never is (the DM is the owner's alone, so a guest meets a note only so).
async fn forge(r: &Rig, channel: u64, custom_id: &str, label: &str) -> String {
    let forged = json!({"content": "a copy of a note", "components": [{"type": 1,
        "components": [{"type": 2, "style": 2, "label": label, "custom_id": custom_id}]}]});
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

/// Real items from a seeded day's ledger.
fn real_items(r: &Rig) -> Vec<(String, String)> {
    let now = theseus_protocol::now_unix_ms();
    for i in 0..3u64 {
        let mut row = LedgerRow::new(
            LedgerKind::TurnFailed,
            None,
            None,
            json!({"loops": 2, "reason": "timeout: the provider did not answer",
                   "cost_usd": 0.4, "tool_calls": 1}),
        );
        row.at_unix_ms = now - (i + 1) * 3_600_000;
        r.core.store.append_ledger(&row).unwrap();
    }
    for tool in ["fs.read", "text.diff"] {
        for i in 0..2u64 {
            let mut row = LedgerRow::new(
                LedgerKind::ToolInvalidInput,
                None,
                None,
                json!({"tool": tool, "reason": "not json"}),
            );
            row.at_unix_ms = now - (5 + i) * 3_600_000;
            r.core.store.append_ledger(&row).unwrap();
        }
    }
    r.core
        .self_resume(
            &SelfResumeParams::default(),
            &Answerer {
                label: "the CLI".into(),
                surface: theseus_core::approval::Surface::Cli,
                discord: None,
                agent: None,
            },
        )
        .unwrap();
    r.core
        .backlog_night(theseus_protocol::now_unix_ms())
        .unwrap();
    let items = r
        .core
        .self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items;
    assert!(items.len() >= 2, "{items:?}");
    items.into_iter().map(|i| (i.id, i.title)).collect()
}

/// Presses: a Drop is `self.drop` for its item, answered to the presser; a second says it is
/// already dropped; a guest's is refused as the sibling cards refuse (the core's rule, the
/// owner's word: `judge_act`'s `SelfBacklog` arm); a Halt is `self.halt`, anyone's, and a
/// second says it is already halted. (Planted: swap the methods; drop the `changed` guard;
/// skip the author on the call.)
#[tokio::test]
async fn presses_drop_and_halt_as_the_presser_and_are_idempotent() {
    let r = rig().await;
    let items = real_items(&r);
    let (first, second) = (&items[0], &items[1]);
    let text = digest(&items);
    stage(&r, "self_digest", &text);
    let m = note(&r, &text).await;
    let label = |t: &str| format!("Drop: {t}");
    let open = |id: &str| {
        r.core
            .self_backlog(&SelfBacklogParams { all: true })
            .unwrap()
            .items
            .iter()
            .any(|i| {
                i.id == id
                    && matches!(
                        i.state.as_str(),
                        "proposed" | "waiting_human" | "in_progress"
                    )
            })
    };

    // A guest's drop is the owner's word, refused with the sibling shape; nothing changes.
    let copy = forge(&r, LAB, &format!("snote:drop:{}", second.0), "Drop").await;
    let p = r.press(&copy, "Drop", (BEN, "ben"));
    r.until("ben is refused", told(&r, &p, "🔐 Your drop did not count"))
        .await;
    assert!(open(&second.0));

    // The owner's drop counts, as hers.
    let p = r.press(&m.id, &label(&first.1), (ANA, "ana"));
    r.until("the drop", told(&r, &p, "↩️ Dropped")).await;
    assert!(!open(&first.0));
    assert!(open(&second.0), "only that item");

    // A second press says so and does not error.
    let p = r.press(&m.id, &label(&first.1), (ANA, "ana"));
    r.until("already dropped", told(&r, &p, &format!("↩️ {}", first.0)))
        .await;
    assert!(r.fake.replies().iter().any(|x| x
        .content
        .as_deref()
        .is_some_and(|c| c.ends_with("was already dropped."))));

    // A halt is anyone's: the guest's counts, the second says it was halted already.
    assert!(!r.core.self_state().halted);
    let copy = forge(&r, LAB, "snote:halt", "Halt").await;
    let p = r.press(&copy, "Halt", (BEN, "ben"));
    r.until("halted", told(&r, &p, "🛑 Self-improvement halted"))
        .await;
    assert!(r.core.self_state().halted);
    let p = r.press(&m.id, "Halt self-improvement", (ANA, "ana"));
    r.until(
        "already halted",
        told(&r, &p, "🛑 Self-improvement was already halted"),
    )
    .await;
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: theseus_core::approval::Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// The self-halt note (theseus-6s7ll): the core's words byte for byte, one Resume button and
/// no other, and a ping (planted: no arm for the kind, an empty row, or `quiet` carried over).
#[tokio::test]
async fn the_halt_note_pings_and_carries_one_resume_button() {
    let r = rig().await;
    let text = "Self builds are halted, and they need you.";
    stage(&r, "self_halt", text);
    let m = note(&r, text).await;
    assert_eq!(m.content, text);
    let got: Vec<(&str, &str)> = m
        .buttons
        .iter()
        .map(|b| (b.label.as_str(), b.custom_id.as_str()))
        .collect();
    assert_eq!(got, [("Resume self-improvement", "snote:resume")]);
    assert!(!m.silent(), "the halt note pings");
}

/// The guard on the exception: the digest and the join note carry no Resume (passes at the
/// base too; not a red-first test; the quiet default's guard is the core's).
#[tokio::test]
async fn no_other_self_note_gets_a_resume() {
    let r = rig().await;
    let text = digest(&fake_items(2));
    stage(&r, "self_digest", &text);
    let m = note(&r, &text).await;
    assert!(m.buttons.iter().all(|b| b.custom_id != "snote:resume"));
    let join = "Joined a change.";
    stage(&r, "self_join", join);
    let m = note(&r, join).await;
    assert!(m.buttons.iter().all(|b| b.custom_id != "snote:resume"));
}

/// A Resume press is `self.resume` as the presser, the typed words' own path: a guest's is
/// refused, the owner's resumes, and a second press (or one after resuming elsewhere) says it
/// was not halted and changes nothing (planted: no arm in `parse`, or a second call that errs).
#[tokio::test]
async fn a_resume_press_resumes_once_and_a_second_press_is_harmless() {
    let r = rig().await;
    r.core
        .self_halt(&SelfHaltParams::default(), &cli())
        .unwrap();
    assert!(r.core.self_state().halted);
    let text = "Self builds are halted, and they need you.";
    stage(&r, "self_halt", text);
    let m = note(&r, text).await;
    let label = "Resume self-improvement";

    let copy = forge(&r, LAB, "snote:resume", "Resume").await;
    let p = r.press(&copy, "Resume", (BEN, "ben"));
    r.until("ben is refused", told(&r, &p, "🛑")).await;
    assert!(
        r.core.self_state().halted,
        "a guest's press changes nothing"
    );

    let p = r.press(&m.id, label, (ANA, "ana"));
    r.until("resumed", told(&r, &p, "▶️ Self-improvement resumed"))
        .await;
    assert!(!r.core.self_state().halted);

    let p = r.press(&m.id, label, (ANA, "ana"));
    r.until(
        "already resumed",
        told(&r, &p, "▶️ Self-improvement was not halted"),
    )
    .await;
    assert!(!r.core.self_state().halted);
}
