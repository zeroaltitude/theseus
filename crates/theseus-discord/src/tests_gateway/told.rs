//! A note of an act done unasked, with its Undo (issue 57), through the
//! stand-in's gateway: the note is the core's words, masked at the exit, with
//! one Undo button whose id is the post's; the owner's press in her DM is
//! `told.undo` as hers, once, and the note then says it was undone; a guest's
//! press, and the owner's in a shared place, undo nothing, and a second press
//! calls nothing.

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
    let id = order(r);
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

/// A standing order the owner's terminal keeps; its id.
fn order(r: &Rig) -> String {
    r.core
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
        .id
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

/// Each undo runs through `told.undo` and does what its own method does; a
/// second `told.undo` of the same note is refused by the core and does not
/// do it twice.
#[tokio::test]
async fn each_undo_runs_through_told_undo_and_a_second_is_refused_by_the_core() {
    use theseus_core::rpc::told::{ToldUndoParams, ToldUndoResult};
    use theseus_protocol::rsi::{SelfBacklogParams, SelfFileParams, SelfFileResult};
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |c| {
        c.self_improve.mode = theseus_core::config::self_improve::SelfMode::Act;
        c.self_improve.backlog_starts = Some(0);
    })
    .await;
    let c = theseus_core::approval::Client::new("test", theseus_core::approval::Surface::Cli);
    let rpc = crate::rpc_client::RpcClient::connect(r.core.clone(), c).0;
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
    let (order, _) = ordered(&r);
    let open = |id: &str| {
        let all = r
            .core
            .self_backlog(&SelfBacklogParams { all: true })
            .unwrap()
            .items;
        all.iter().any(|i| {
            i.id == id
                && matches!(
                    i.state.as_str(),
                    "proposed" | "waiting_human" | "in_progress"
                )
        })
    };
    assert!(open(&item) && ends(&r, &order) == 0);

    for undo in [
        Undo::StandingRevoke(order.clone()),
        Undo::SelfDrop(item.clone()),
    ] {
        let post = note_of(&r, undo.clone());
        let p = ToldUndoParams {
            post,
            ..Default::default()
        };
        let done: ToldUndoResult = rpc
            .call(theseus_protocol::method::TOLD_UNDO, &p)
            .await
            .unwrap();
        assert!(
            done.text.contains("\n↩️ Undone by "),
            "{undo:?}: {}",
            done.text
        );
        assert!(!done.text.contains("Undo: `"), "{}", done.text);
        let again = rpc
            .call::<_, ToldUndoResult>(theseus_protocol::method::TOLD_UNDO, &p)
            .await
            .unwrap_err();
        assert!(again.data.get("undone_by").is_some(), "{undo:?}: {again:?}");
    }
    assert_eq!(ends(&r, &order), 1, "the order was revoked once");
    assert!(!open(&item), "the item was dropped");
}

/// The exit's masking holds on every word a note sends: a vault value in the
/// note's title, its second line and its body reaches Discord in none of the
/// first send, the edit after the owner's Undo, a repeat press's edit, or any
/// reply to a presser. The core's stored note keeps the words as written.
#[tokio::test]
async fn a_told_notes_send_its_undo_edit_and_its_replies_are_masked() {
    const SECRET: &str = "fake-token-not-a-secret";
    let r = rig().await;
    assert!(r.core.exits.on());
    let id = order(&r);
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        &format!("Kept an order about {SECRET}"),
        &format!("In “{SECRET}”, without asking you"),
        &format!("It mentions {SECRET}."),
        &Undo::StandingRevoke(id.clone()),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    let post = a.correlation_id;
    let m = note(&r, &post).await;
    assert!(m.content.contains("[redacted:"), "{}", m.content);
    let copy = forge(&r, LAB, &format!("told:undo:{post}")).await;
    let p = r.press(&copy, "Undo", (BEN, "ben"));
    told_to(&r, &p, "🔐 Your undo did not count").await;
    let p = r.press(&m.id, "Undo", (ANA, "ana"));
    told_to(&r, &p, "↩️ Undone.").await;
    assert_eq!(ends(&r, &id), 1, "revoked once");
    // A repeat press, on a copy still showing the button: the core's answer
    // edits the copy too, and that edit is masked as well.
    let again = forge(&r, ANA_DM, &format!("told:undo:{post}")).await;
    let p = r.press(&again, "Undo", (ANA, "ana"));
    told_to(&r, &p, "↩️ Already undone by discord:ana").await;
    for at in [&m.id, &again] {
        let edited = r.posted(ANA_DM).into_iter().find(|x| x.id == *at).unwrap();
        assert!(
            edited.content.contains("\n↩️ Undone by discord:ana."),
            "{}",
            edited.content
        );
        assert!(edited.content.contains("[redacted:"), "{}", edited.content);
        assert!(edited.versions.len() >= 2, "{:?}", edited.versions);
    }
    for at in [ANA_DM, LAB] {
        for x in r.posted(at) {
            assert!(x.versions.iter().all(|v| !v.contains(SECRET)), "{x:?}");
        }
    }
    for x in r.fake.replies() {
        let c = x.content.unwrap_or_default();
        assert!(!c.contains(SECRET), "{c}");
    }
    let (text, _) = told::of(&r.core.kernel, &post).unwrap().unwrap();
    assert!(text.contains(SECRET), "the stored note keeps its words");
}

/// A note that names itself (an undo with no method of its own writes
/// `told::by_door()`) reaches Discord with its own post id filled in, so
/// its `theseus told undo` works as written.
#[tokio::test]
async fn a_note_naming_itself_reaches_discord_with_its_own_post_id() {
    let r = rig().await;
    let id = order(&r);
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus took something back",
        "In “Tides”, without asking you",
        &format!("Or from a terminal: `{}`", told::by_door()),
        &Undo::StandingRevoke(id),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    let post = a.correlation_id;
    let m = note(&r, &post).await;
    assert!(
        m.content
            .contains(&format!("Or from a terminal: `theseus told undo {post}`")),
        "{}",
        m.content
    );
    assert!(!m.content.contains(told::THIS_POST), "{}", m.content);
}
