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
use theseus_sim::fake_discord::{Guild, Msg, DEFAULT_GUILD};

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

/// The notices channel, an invented id.
const NOTICES: u64 = 900_000_000_000_000_050;

/// `#lab`, a private notices channel (bound `notify = true` when `notify`)
/// and ana's DM; ana alone is the owner.
async fn noticed_rig(notify: bool) -> Rig {
    let file = format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[channel]]\nid = \"{NOTICES}\"\nname = \"notices\"\nusers = [\"{ANA}\"]\nprivate = true\nnotify = {notify}\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    );
    let g = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .private_channel(LAB, "lab", &[ANA])
        .private_channel(NOTICES, "notices", &[ANA]);
    Rig::start_tweaked(model, g, &file, &[], |c| {
        c.places.owner = Some(vec![format!("discord:{ANA}")])
    })
    .await
}

/// With a notices place bound the note goes there, not to the DM, and the
/// owner's Undo there (a private place) counts; with none bound it is in the
/// DM, as every FYI was.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_told_note_goes_to_the_notices_place_when_one_is_bound_and_else_to_the_dm() {
    for notify in [true, false] {
        let r = noticed_rig(notify).await;
        let (id, post) = ordered(&r);
        let custom = format!("told:undo:{post}");
        let has = |m: &Msg| m.buttons.iter().any(|b| b.custom_id == custom);
        let (to, not) = match notify {
            true => (NOTICES, ANA_DM),
            false => (ANA_DM, NOTICES),
        };
        r.until_on(&[to], "the note", || r.posted(to).iter().any(has))
            .await;
        assert!(
            !r.posted(not).iter().any(has),
            "notify={notify}: not in {not}"
        );
        let m = r.posted(to).into_iter().find(has).unwrap();
        let p = r.press(&m.id, "Undo", (ANA, "ana"));
        told_to(&r, &p, "↩️ Undone.").await;
        assert_eq!(ends(&r, &id), 1, "notify={notify}");
        r.stop();
    }
}

/// A note of `undo`, staged through the helper; its post id.
fn note_of(r: &Rig, undo: Undo) -> String {
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus did something",
        "In “Tides”, without asking you",
        "",
        &undo,
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    a.correlation_id
}

/// A reminder, in Theseus's home conversation; its wake id.
async fn wake(r: &Rig) -> String {
    let c = theseus_core::approval::Client::new("test", theseus_core::approval::Surface::Cli);
    let rpc = crate::rpc_client::RpcClient::connect(r.core.clone(), c).0;
    let made: theseus_protocol::wakes::WakeEditResult = rpc
        .call(
            theseus_protocol::method::WAKE_CREATE,
            theseus_protocol::wakes::WakeCreateParams {
                when: "in 2h".into(),
                note: "check the moorings".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    made.wake.wake_id
}

fn pending(r: &Rig, wake: &str) -> bool {
    let all = r.core.kernel.pending_wakes().unwrap();
    all.iter().any(|(_, w)| w.id == wake)
}

/// The judged door (issue 57): a wake note's Undo pressed by a guest, or by
/// the owner in a shared place, cancels nothing, though `wake.cancel` itself
/// judges no one; the owner's press in her DM cancels it once.
#[tokio::test]
async fn a_wake_notes_undo_counts_only_for_the_owner_in_private() {
    let r = rig().await;
    let id = wake(&r).await;
    let post = note_of(&r, Undo::WakeCancel(id.clone()));
    let m = note(&r, &post).await;
    let copy = forge(&r, LAB, &format!("told:undo:{post}")).await;
    let p = r.press(&copy, "Undo", (BEN, "ben"));
    told_to(&r, &p, "🔐 Your undo did not count").await;
    let p = r.press(&copy, "Undo", (ANA, "ana"));
    told_to(&r, &p, "🔐 Your undo did not count").await;
    assert!(pending(&r, &id), "nothing was cancelled");
    let p = r.press(&m.id, "Undo", (ANA, "ana"));
    told_to(&r, &p, "↩️ Undone.").await;
    assert!(!pending(&r, &id), "cancelled");
}

/// Each of the four undos runs through `told.undo` and does what its own
/// method does; a second `told.undo` of the same note is refused by the core
/// and does it no twice.
#[tokio::test]
async fn each_undo_runs_through_told_undo_and_a_second_is_refused_by_the_core() {
    use theseus_core::rpc::told::{ToldUndoParams, ToldUndoResult};
    use theseus_protocol::rsi::{SelfBacklogParams, SelfFileParams, SelfFileResult};
    use theseus_protocol::routines::{RoutineCreateParams, RoutineEditResult};
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |c| {
        c.self_improve.mode = theseus_core::config::self_improve::SelfMode::Act;
        c.self_improve.backlog_starts = Some(0);
    })
    .await;
    let c = theseus_core::approval::Client::new("test", theseus_core::approval::Surface::Cli);
    let rpc = crate::rpc_client::RpcClient::connect(r.core.clone(), c).0;
    let routine: RoutineEditResult = rpc
        .call(
            theseus_protocol::method::ROUTINE_CREATE,
            RoutineCreateParams {
                title: "Tides".into(),
                schedule: "weekdays 08:00".into(),
                brief: "Look at the tide table.".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let routine = routine.routine.id;
    let item: SelfFileResult = rpc
        .call(
            theseus_protocol::method::SELF_FILE,
            SelfFileParams {
                finding: "The tide table reads in feet; it should read in metres.".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let item = item.item;
    let wake = wake(&r).await;
    let (order, _) = ordered(&r);
    let open = |id: &str| {
        let all = r.core.self_backlog(&SelfBacklogParams { all: true }).unwrap().items;
        all.iter().any(|i| {
            i.id == id && matches!(i.state.as_str(), "proposed" | "waiting_human" | "in_progress")
        })
    };
    let listed = |id: &str| r.core.routine_list().unwrap().routines.iter().any(|x| x.id == id);
    assert!(pending(&r, &wake) && listed(&routine) && open(&item) && ends(&r, &order) == 0);

    for undo in [
        Undo::WakeCancel(wake.clone()),
        Undo::RoutineDelete(routine.clone()),
        Undo::StandingRevoke(order.clone()),
        Undo::SelfDrop(item.clone()),
    ] {
        let post = note_of(&r, undo.clone());
        let p = ToldUndoParams { post, ..Default::default() };
        let done: ToldUndoResult = rpc.call(theseus_protocol::method::TOLD_UNDO, &p).await.unwrap();
        assert!(done.text.contains("\n↩️ Undone by "), "{undo:?}: {}", done.text);
        assert!(!done.text.contains("Undo: `"), "{}", done.text);
        let again = rpc
            .call::<_, ToldUndoResult>(theseus_protocol::method::TOLD_UNDO, &p)
            .await
            .unwrap_err();
        assert!(again.data.get("undone_by").is_some(), "{undo:?}: {again:?}");
    }
    assert!(!pending(&r, &wake), "the wake was cancelled");
    assert!(!listed(&routine), "the routine was deleted");
    assert_eq!(ends(&r, &order), 1, "the order was revoked once");
    assert!(!open(&item), "the item was dropped");
}
