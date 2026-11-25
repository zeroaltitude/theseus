//! Capture's note of a record kept (issue 57), through the stand-in's gateway: every record kept
//! is told, whether its words were said in the owner's DM or typed at the CLI, and the note goes
//! to the notices place when one is bound and to the DM when none is, with its Undo. A question
//! of capture's (a Decision) stays in the DM. The owner's Undo from a private place counts; from a
//! shared notices place it is refused, privately, as a guest's is.

use std::sync::Arc;

use theseus_core::capture::act::{Done, Message};
use theseus_core::capture::{Heard, Said, Verdict};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_core::session::SessionRecord;
use theseus_judge::band::Band;
use theseus_protocol::SessionKind;
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

/// With a notices place bound, a record kept from ana's DM and one kept from the CLI are each told
/// there and not in the DM; with none bound, in the DM. Her Undo where it landed (a private place)
/// ends the record once. A question stays in the DM either way.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_records_note_goes_to_the_notices_place_when_one_is_bound_and_else_to_the_dm() {
    let sure = verdict(Heard::Preference, Band::Act, 0.9);
    let unsure = verdict(Heard::Fact, Band::Confirm, 0.6);
    for notify in [true, false] {
        let r = rig(notify, true).await;
        let (to, not) = match notify {
            true => (NOTICES, ANA_DM),
            false => (ANA_DM, NOTICES),
        };
        let said = "answers in metric units please";
        let typed = "I work nights on tuesdays";
        let ids = [
            (kept(in_dm(&r, said, &sure)), said),
            (kept(at_the_cli(&r, typed, &unsure)), typed),
        ];
        for (id, text) in &ids {
            let has = |m: &Msg| m.content.contains(text);
            r.until_on(&[to], text, || r.posted(to).iter().any(has))
                .await;
            assert!(
                !r.posted(not).iter().any(has),
                "notify={notify}: {text} not in {not}"
            );
            let m = r.posted(to).into_iter().find(has).unwrap();
            assert!(m.content.starts_with("ℹ️ FYI · "), "{}", m.content);
            let p = r.press(&m.id, "Undo", (ANA, "ana"));
            told_to(&r, &p, "↩️ Undone").await;
            assert_eq!(ends(&r, id), 1, "notify={notify}: {text}");
        }
        // A question of capture's is a Decision: the DM, notices place or none.
        let twice = verdict(Heard::Correction, Band::Escalate, 0.4);
        assert_eq!(
            in_dm(&r, "stop using tables for lists", &twice),
            Done::Noted
        );
        let asked = in_dm(&r, "again, no tables for lists please", &twice);
        assert!(matches!(asked, Done::Asked(_)), "{asked:?}");
        let ask = |m: &Msg| m.content.contains("said this twice");
        r.until("the ask in ana's DM", || r.posted(ANA_DM).iter().any(ask))
            .await;
        assert!(!r.posted(NOTICES).iter().any(ask), "notify={notify}");
        r.stop();
    }
}

/// In a shared notices place, a kept record's Undo counts for no one: a guest's press and the
/// owner's are each refused, privately, and the record holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_records_undo_in_a_shared_notices_place_is_refused_privately() {
    let r = rig(true, false).await;
    let said = "answers in metric units please";
    let id = kept(in_dm(&r, said, &verdict(Heard::Preference, Band::Act, 0.9)));
    let has = |m: &Msg| m.content.contains(said);
    r.until_on(&[NOTICES], "the note", || r.posted(NOTICES).iter().any(has))
        .await;
    let m = r.posted(NOTICES).into_iter().find(has).unwrap();
    let p = r.press(&m.id, "Undo", (BEN, "ben"));
    told_to(&r, &p, "🔐 Your answer did not count").await;
    let p = r.press(&m.id, "Undo", (ANA, "ana"));
    told_to(&r, &p, "🔐 Your answer did not count").await;
    assert_eq!(ends(&r, &id), 0, "nothing was undone");
    r.stop();
}
