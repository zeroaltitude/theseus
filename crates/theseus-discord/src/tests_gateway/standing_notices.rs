//! Capture's note of a record kept (issue 57), through the stand-in's gateway: every record kept
//! is told, whether its words were said in the owner's DM or typed at the CLI, in his DM with its
//! Undo. The card carries his own words, so it never goes to a notices place, bound shared or
//! private. His Undo in the DM counts; a copy of the button pressed in a shared place is refused,
//! privately, for him as for a guest.

use std::sync::Arc;

use theseus_core::capture::act::{Done, Message};
use theseus_core::capture::{Heard, Said, Verdict};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_core::session::SessionRecord;
use theseus_judge::band::Band;
use theseus_protocol::SessionKind;
use serde_json::json;
use theseus_sim::fake_discord::{FakeDiscord, Guild, Msg, DEFAULT_GUILD};

use super::{Rig, ANA, ANA_DM, BEN, LAB};

/// The notices channel, an invented id.
const NOTICES: u64 = 900_000_000_000_000_060;

fn model(_: &std::path::Path, _: Arc<FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![]))
}

/// `#lab`, the notices channel (bound `notify = true` when `notify`, private when `private`, ana
/// and ben admitted) and ana's DM; ana alone is the owner.
async fn rig(notify: bool, private: bool) -> Rig {
    let file = format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[channel]]\nid = \"{NOTICES}\"\nname = \"notices\"\nusers = [\"{ANA}\", \"{BEN}\"]\nmention_only = false\nprivate = {private}\nnotify = {notify}\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    );
    let g = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .private_channel(LAB, "lab", &[ANA]);
    let g = match private {
        true => g.private_channel(NOTICES, "notices", &[ANA, BEN]),
        false => g.channel(NOTICES, "notices"),
    };
    Rig::start_tweaked(model, g, &file, &[], |c| {
        c.places.owner = Some(vec![format!("discord:{ANA}")])
    })
    .await
}

fn verdict(heard: Heard, band: Band, confidence: f64) -> Verdict {
    Verdict {
        heard,
        band,
        confidence,
        p_correction: if heard == Heard::Correction { 0.4 } else { 0.0 },
        said: Said::Everywhere,
        sure_of_scope: true,
        judgment: "jdg_gateway".into(),
        pack: "directive.v2".into(),
    }
}

/// Words ana said in her DM, as capture hears them.
fn in_dm(r: &Rig, text: &str, v: &Verdict) -> Done {
    let place = format!("dm:{ANA}");
    let sid = match r.core.outbox.place_session(&place).unwrap() {
        Some(s) => s,
        None => {
            let rec = SessionRecord::new(SessionKind::Conversation, None);
            r.core.store.put_session(&rec.session_id, &rec).unwrap();
            r.core.outbox.bind_place(&place, &rec.session_id).unwrap();
            rec.session_id
        }
    };
    let m = Message {
        session: sid,
        node: format!("msg_{}", text.len()),
        text: text.into(),
        place_kind: "discord_dm (private)".into(),
    };
    r.core.capture_now(&m, v).unwrap()
}

/// Words the owner typed at the CLI, in a conversation of no place.
fn at_the_cli(r: &Rig, text: &str, v: &Verdict) -> Done {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let m = Message {
        session: rec.session_id,
        node: format!("msg_cli_{}", text.len()),
        text: text.into(),
        place_kind: "cli (private)".into(),
    };
    r.core.capture_now(&m, v).unwrap()
}

fn kept(d: Done) -> String {
    match d {
        Done::Kept(id) => id,
        other => panic!("{other:?}"),
    }
}

/// How many revokes ended record `id`.
fn ends(r: &Rig, id: &str) -> usize {
    let all = r.core.runner.standing.all(&r.core.store).unwrap();
    all.iter().filter(|x| x.ends.as_deref() == Some(id)).count()
}

/// The presser hears `start`, alone: the press's first answer is private.
async fn told_to(r: &Rig, pressed: &str, start: &str) {
    r.until(start, || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed)
                && x.content.as_deref().is_some_and(|c| c.starts_with(start))
        })
    })
    .await;
    let first = r
        .fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(pressed) && x.kind == "callback")
        .expect("the press's first answer");
    assert!(first.ephemeral, "{start}: the presser alone hears it");
}

/// An FYI that does go to a bound notices place: staged after the captures, its landing there
/// says the notices lane has passed them.
fn marker(r: &Rig) {
    use theseus_core::owner_note::{note, Kind};
    let body = json!({"kind": "notice",
        "text": note(Kind::Fyi, "The tide sweep has stopped for today", "its day budget",
                     "fyi-notices-marker")});
    let (a, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
}

/// With a notices place bound, shared or private, a record kept from ana's DM and one kept from
/// the CLI each come to her DM with their Undo, and their words never reach the notices place.
/// Her Undo in the DM ends the record once, and she alone hears so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_records_card_comes_to_the_dm_and_never_to_a_notices_place() {
    let sure = verdict(Heard::Preference, Band::Act, 0.9);
    let unsure = verdict(Heard::Fact, Band::Confirm, 0.6);
    for private in [true, false] {
        let r = rig(true, private).await;
        let said = "answers in metric units please";
        let typed = "I work nights on tuesdays";
        let ids = [
            (kept(in_dm(&r, said, &sure)), said),
            (kept(at_the_cli(&r, typed, &unsure)), typed),
        ];
        marker(&r);
        r.until_on(&[NOTICES], "the marker", || {
            r.posted(NOTICES)
                .iter()
                .any(|m| m.content.contains("fyi-notices-marker"))
        })
        .await;
        for (id, text) in &ids {
            let has = |m: &Msg| m.content.contains(text);
            r.until(text, || r.posted(ANA_DM).iter().any(has)).await;
            assert!(
                !r.posted(NOTICES).iter().any(has),
                "private={private}: {text} is never in the notices place"
            );
            let m = r.posted(ANA_DM).into_iter().find(has).unwrap();
            assert!(m.content.starts_with("ℹ️ FYI · "), "{}", m.content);
            let p = r.press(&m.id, "Undo", (ANA, "ana"));
            told_to(&r, &p, "↩️ Undone").await;
            assert_eq!(ends(&r, id), 1, "private={private}: {text}");
        }
        r.stop();
    }
}

/// A copy of a kept card's Undo in a shared place counts for no one: a guest's press and the
/// owner's are each refused, privately, and the record holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_cards_undo_copied_into_a_shared_place_is_refused_privately() {
    let r = rig(true, false).await;
    let said = "answers in metric units please";
    let id = kept(in_dm(&r, said, &verdict(Heard::Preference, Band::Act, 0.9)));
    let has = |m: &Msg| m.content.contains(said);
    r.until("the card in the DM", || r.posted(ANA_DM).iter().any(has))
        .await;
    let card = r.posted(ANA_DM).into_iter().find(has).unwrap();
    let custom = card.buttons[0].custom_id.clone();
    let forged = json!({"content": "a copy of a card", "components": [{"type": 1,
        "components": [{"type": 2, "style": 2, "label": "Undo", "custom_id": custom}]}]});
    let url = format!("http://{}/api/v10/channels/{NOTICES}/messages", r.fake.addr);
    let posted: serde_json::Value = reqwest::Client::new()
        .post(url)
        .json(&forged)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let copy = posted["id"].as_str().unwrap().to_string();
    let p = r.press(&copy, "Undo", (BEN, "ben"));
    told_to(&r, &p, "🔐 Your answer did not count").await;
    let p = r.press(&copy, "Undo", (ANA, "ana"));
    told_to(&r, &p, "🔐 Your answer did not count").await;
    assert_eq!(ends(&r, &id), 0, "nothing was undone");
    r.stop();
}
