//! Capture's cards in the owner's DM (theseus-slvl.5): a record kept from the
//! owner's words arrives with Undo, and an ask with Keep and Not this; the
//! owner's press counts as hers (Undo ends the record, Keep writes it), and a
//! copy of a card's buttons in a shared channel is refused.

use std::sync::Arc;

use theseus_core::provider::{FakeProvider, Provider};
use theseus_core::session::SessionRecord;
use theseus_judge::band::Band;
use theseus_protocol::SessionKind;
use theseus_protocol::StandingKind;

use super::{bindings, guild, Rig, ANA, ANA_DM};

/// ana's DM session, bound as the binding binds it, and a verdict on a
/// message she sent in it.
fn heard(r: &Rig, text: &str, band: Band, confidence: f64) -> theseus_core::capture::act::Done {
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
    let m = theseus_core::capture::act::Message {
        session: sid,
        node: format!("msg_{}", text.len()),
        text: text.into(),
        place_kind: "discord_dm (private)".into(),
    };
    let v = theseus_core::capture::Verdict {
        heard: theseus_core::capture::Heard::Preference,
        band,
        confidence,
        p_correction: 0.0,
        said: theseus_core::capture::Said::Everywhere,
        sure_of_scope: true,
        judgment: "jdg_gateway".into(),
        pack: "directive.v1".into(),
    };
    r.core.capture_now(&m, &v).unwrap()
}

#[tokio::test]
async fn a_kept_card_has_undo_and_an_ask_has_keep_and_the_owners_press_counts() {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    // A sure preference: kept, with a card that has Undo.
    let kept = heard(&r, "short answers, code first", Band::Act, 0.95);
    let theseus_core::capture::act::Done::Kept(rec) = kept else {
        panic!("{kept:?}")
    };
    let is_kept = |m: &theseus_sim::fake_discord::Msg| m.content.contains("Kept as a standing");
    r.until("the kept card in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_kept)
    })
    .await;
    let card = r.posted(ANA_DM).into_iter().find(is_kept).unwrap();
    assert!(
        card.content.contains("short answers, code first"),
        "{}",
        card.content
    );
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Undo"]);
    // Her Undo ends it.
    let shown = r.press(&card.id, "Undo", (ANA, "ana"));
    r.until("the record ended", || {
        r.core
            .runner
            .standing
            .all(&r.core.store)
            .map(|a| a.iter().any(|x| x.ends.as_deref() == Some(rec.as_str())))
            .unwrap_or(false)
    })
    .await;
    r.until("ana hears it", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(shown.as_str())
                && x.content.as_deref().is_some_and(|c| c.contains("Undone"))
        })
    })
    .await;
    // An unsure one is asked: Keep, Keep everywhere is not offered (it is
    // everywhere already), Not this.
    let asked = heard(&r, "reply briefly", Band::Confirm, 0.6);
    let theseus_core::capture::act::Done::Asked(ask) = asked else {
        panic!("{asked:?}")
    };
    let is_ask = |m: &theseus_sim::fake_discord::Msg| m.content.contains("Keep as a standing");
    r.until("the ask in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_ask)
    })
    .await;
    let q = r.posted(ANA_DM).into_iter().find(is_ask).unwrap();
    let labels: Vec<&str> = q.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Keep", "Not this"]);
    // Another member's press on it counts for nothing: the ask stays open.
    r.press(&q.id, "Keep", (super::BEN, "ben"));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(r.ledger("standing.answered").is_empty());
    r.press(&q.id, "Keep", (ANA, "ana"));
    r.until("the kept record", || {
        r.ledger("standing.answered")
            .iter()
            .any(|a| a["ask"] == ask.as_str() && a["answer"] == "keep")
    })
    .await;
    let live = r.core.runner.standing.all(&r.core.store).unwrap();
    assert!(live
        .iter()
        .any(|x| x.text == "reply briefly" && x.kind == StandingKind::Preference));
}
