//! A fold's press (issue 56), end to end through the stand-in's gateway: the
//! place's lane is the only writer of a fold's message, for a press as for
//! a live update, in the place's order; a press while the lane waits out
//! Discord tells the presser and changes nothing; and a press is honoured
//! only on the lane's own fold message for the turn its id names. Anyone the
//! place answers may press a fold, and only a fold: an acting control stays
//! the bound people's; a place not bound here answers nothing.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_sim::fake_discord::{Guild, Msg, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, CY, LAB};

/// What a presser is told when the lane waits out Discord.
const AWAY: &str = "Discord is not taking this conversation's messages just now";
/// What a presser is told when the pressed message is not the turn's fold.
const NOT_HERE: &str = "This button is not this message's";
/// What a presser is told when no fold of the turn is known here.
const TOO_OLD: &str = "These steps are too old to show again";

/// An unbound channel of the guild, an invented id.
const ELSEWHERE: u64 = 900_000_000_000_000_050;

/// `#lab`, bound with `lab`'s words for `users`, and ana's DM: both fold.
fn bindings(lab: &str, users: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [{users}]\nmention_only = false\n{lab}\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(CY, "cy")
        .channel(LAB, "lab")
        .channel(ELSEWHERE, "elsewhere")
}

async fn rig(script: impl FnOnce(&std::path::Path) -> Vec<Scripted>, lab: &str) -> Rig {
    Rig::start_on(
        |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
        guild(),
        &bindings(lab, &format!("\"{ANA}\"")),
        &[],
    )
    .await
}

/// A loop of `n` reads of files in `work/`, named `<word>-<i>.txt`.
fn reads(dir: &std::path::Path, word: &str, n: usize) -> Scripted {
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let calls: Vec<(String, Value)> = (0..n)
        .map(|i| {
            let p = work.join(format!(
                "{word}-a-rather-long-file-name-for-paging-{i:02}.txt"
            ));
            std::fs::write(&p, "x\n").unwrap();
            (
                format!("r_{word}_{i}"),
                json!({"path": p.to_string_lossy()}),
            )
        })
        .collect();
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, input)| (id.as_str(), "fs_read", input.clone()))
        .collect();
    Scripted::tools("", &calls)
}

fn labels(m: &Msg) -> Vec<String> {
    m.buttons.iter().map(|b| b.label.clone()).collect()
}

/// The bot's folds in `channel`, oldest first.
fn folds(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("fold:")))
        .collect()
}

/// The turn a fold's buttons name.
fn turn_of(f: &Msg) -> String {
    let id = &f
        .buttons
        .iter()
        .find(|b| b.custom_id.starts_with("fold:"))
        .unwrap()
        .custom_id;
    id.rsplit(':').next().unwrap().to_string()
}

fn message(r: &Rig, channel: u64, id: &str) -> Msg {
    r.fake
        .messages(channel)
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

/// Whether the presser of `pressed` was told, alone, a line starting `s`.
fn told(r: &Rig, pressed: &str, s: &str) -> bool {
    r.fake.replies().iter().any(|x| {
        x.interaction.as_deref() == Some(pressed)
            && x.ephemeral
            && x.content.as_deref().is_some_and(|c| c.starts_with(s))
    })
}

/// Give message `id` in `channel` one button, `custom_id`, as a forged
/// interaction would carry it: its words stay as they were.
async fn forge(r: &Rig, channel: u64, id: &str, custom_id: &str) {
    let url = format!(
        "http://{}/api/v10/channels/{channel}/messages/{id}",
        r.fake.addr
    );
    let ok = reqwest::Client::new()
        .patch(url)
        .json(&json!({"components": [{"type": 1, "components": [
            {"type": 2, "style": 2, "custom_id": custom_id, "label": "Forged"}]}]}))
        .send()
        .await
        .unwrap()
        .status()
        .is_success();
    assert!(ok, "the forged button is on the message");
}

/// Whether the press `pressed` was acknowledged: its place had it then, as
/// a press is sent to its place before it is acknowledged
/// (`runtime/fold.rs`).
fn acked(r: &Rig, pressed: &str) -> bool {
    r.fake
        .replies()
        .iter()
        .any(|x| x.interaction.as_deref() == Some(pressed) && x.kind == "callback")
}

/// The fake's writes held so far.
fn held(r: &Rig) -> usize {
    r.fake.seen().iter().filter(|s| s.outcome == "held").count()
}

/// The lane is the one writer: while it waits on a write Discord holds, a
/// press its place already has changes nothing at Discord; two opposing
/// presses then end as the place took them, the message and the place
/// agreeing (the next press does what its label says). Each step waits for
/// what shows it: the held request at the fake, each press's
/// acknowledgement (its place had it), then the hold's release.
#[tokio::test]
async fn a_press_waits_for_the_lanes_held_write_and_opposing_presses_agree() {
    let r = rig(
        |dir| {
            vec![
                reads(dir, "a", 10),
                reads(dir, "b", 10),
                reads(dir, "c", 10),
                Scripted::text("Read all thirty."),
                Scripted::text("Second answer, hold-me."),
            ]
        },
        "",
    )
    .await;
    r.say((ANA, "ana"), None, "Read the thirty files.");
    r.until("the answer and the fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Read all thirty."))
            && folds(&r, ANA_DM)
                .first()
                .is_some_and(|f| f.content == "-# 30 steps: 30 reads")
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("page 1", || {
        message(&r, ANA_DM, &f.id).content.contains(" · page 1 of ")
    })
    .await;
    let page1 = message(&r, ANA_DM, &f.id);

    // The lane is held on the next turn's answer.
    r.fake.hold_writes_containing(Some("hold-me"));
    let before = held(&r);
    r.say((ANA, "ana"), None, "And again?");
    r.until("the lane's write held", || held(&r) > before).await;
    let later = r.press(&f.id, "Later ▶", (ANA, "ana"));
    let hide = r.press(&f.id, "Hide steps", (ANA, "ana"));
    r.until("both presses with their place", || {
        acked(&r, &later) && acked(&r, &hide)
    })
    .await;
    let during = message(&r, ANA_DM, &f.id);
    assert_eq!(
        (during.content.as_str(), during.edits),
        (page1.content.as_str(), page1.edits),
        "no press wrote while the lane's write was held"
    );

    r.fake.hold_writes_containing(None);
    r.until("the presses written, in the place's order", || {
        let m = message(&r, ANA_DM, &f.id);
        m.content == "-# 30 steps: 30 reads" && labels(&m) == ["Show steps"]
    })
    .await;
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("page 1 again", || {
        let m = message(&r, ANA_DM, &f.id);
        m.content.contains(" · page 1 of ") && labels(&m).contains(&"Hide steps".to_string())
    })
    .await;
}

/// A press on a fold whose create Discord holds, around the call's end: the
/// fold's create is held at the fake (seen there), the press reaches its
/// place (acknowledged), the gated command then ends (failed), and the turn
/// with it; only then is the hold released. Whichever the place took first,
/// the press or the call's end, the lane writes the fold's states in that
/// order after the create: the fold ends open, and stays open, though the
/// turn ended with it closed.
#[tokio::test]
async fn a_press_outlasts_the_fold_update_the_lane_still_holds() {
    let r = rig(
        |dir| {
            std::fs::create_dir_all(dir.join("work")).unwrap();
            let gate = dir.join("work").join("gate");
            let wait = format!(
                "while [ ! -f {} ]; do sleep 0.1; done; exit 3",
                gate.to_string_lossy()
            );
            vec![
                Scripted::tools(
                    "",
                    &[("m1", "proc_run", json!({"argv": ["sh", "-c", wait]}))],
                ),
                Scripted::text("No such chart."),
            ]
        },
        "",
    )
    .await;
    // The fold's create waits at Discord, applied but unanswered: the lane
    // holds the turn's later updates of it meanwhile.
    let before = held(&r);
    r.fake.hold_writes_containing(Some("-# 1 step: 1 command"));
    r.say((ANA, "ana"), None, "Run the failing count.");
    r.until("the fold's create held at the fake", || {
        held(&r) > before && !folds(&r, ANA_DM).is_empty()
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    let pressed = r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("the press with its place", || acked(&r, &pressed))
        .await;
    std::fs::write(r.dir.path().join("work").join("gate"), "").unwrap();
    r.until("the call ended and the turn with it", || {
        !r.ledger("turn.ended").is_empty()
    })
    .await;
    r.fake.hold_writes_containing(None);
    r.until("the answer delivered", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("No such chart."))
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    r.until("the fold open", || {
        message(&r, ANA_DM, &f.id).content.contains("`proc.run`")
    })
    .await;
    // An observation of absence: a late closed write would show here. It
    // cannot fail a correct lane, only give a wrong one time to show.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let m = message(&r, ANA_DM, &f.id);
    assert!(m.content.contains("`proc.run`"), "still open: {m:#?}");
    assert!(
        !m.content.contains('⏳') && !m.content.contains("▫️"),
        "the call's end is in it: {m:#?}"
    );
    assert_eq!(
        labels(&m).last().map(String::as_str),
        Some("Hide steps"),
        "{m:#?}"
    );
}

/// While the place's lane waits out Discord, a press is told so, alone, and
/// changes nothing: the fold is as it was after the wait, and a press then
/// opens it.
#[tokio::test]
async fn a_press_while_the_lane_backs_off_is_told_and_changes_nothing() {
    let r = rig(
        |dir| {
            vec![
                reads(dir, "a", 2),
                Scripted::text("Read both."),
                Scripted::text("Second answer."),
            ]
        },
        "",
    )
    .await;
    r.say((ANA, "ana"), None, "Read both files.");
    r.until("the answer and the fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Read both."))
            && folds(&r, ANA_DM)
                .first()
                .is_some_and(|f| f.content == "-# 2 steps: 2 reads")
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    let failed = || {
        r.fake
            .seen()
            .iter()
            .filter(|s| s.outcome == "failed")
            .count()
    };
    r.fake.fail_creates_in(ANA_DM, Some(503));
    r.say((ANA, "ana"), None, "And again?");
    r.until("the lane refused by Discord", || failed() > 0)
        .await;
    let pressed = r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("the presser told", || told(&r, &pressed, AWAY))
        .await;
    let m = message(&r, ANA_DM, &f.id);
    assert_eq!(
        (m.content.as_str(), m.edits),
        (f.content.as_str(), f.edits),
        "{m:#?}"
    );
    r.fake.fail_creates_in(ANA_DM, None);
    r.until("the second answer, once Discord is back", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Second answer."))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let m = message(&r, ANA_DM, &f.id);
    assert_eq!(m.content, "-# 2 steps: 2 reads", "unchanged: {m:#?}");
    assert_eq!(labels(&m), ["Show steps"]);
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("open", || {
        message(&r, ANA_DM, &f.id).content.contains("`fs.read`")
    })
    .await;
}

/// A fold's press id, forged onto a card of the same turn: refused, in one
/// line to the presser, and the card's words are as they were.
#[tokio::test]
async fn a_fold_id_forged_onto_a_card_edits_nothing() {
    let r = rig(
        |dir| {
            let out = dir.join("outside").join("proof.txt");
            vec![Scripted::tools(
                "Writing it.",
                &[(
                    "w1",
                    "fs_write",
                    json!({"path": out.to_string_lossy(), "content": "x"}),
                )],
            )]
        },
        "",
    )
    .await;
    r.say((ANA, "ana"), None, "Write the proof.");
    r.until("the card and the fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.versions[0].contains("**Approve?**"))
            && !folds(&r, ANA_DM).is_empty()
    })
    .await;
    let card = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.versions[0].contains("**Approve?**"))
        .unwrap();
    let turn = turn_of(&folds(&r, ANA_DM)[0]);
    forge(&r, ANA_DM, &card.id, &format!("fold:o:0:{turn}")).await;
    // The card as the forged press finds it (a needs-you card is all
    // components, so its words are those the forged row left).
    let card = message(&r, ANA_DM, &card.id);
    let pressed = r.press(&card.id, "Forged", (ANA, "ana"));
    r.until("the presser told", || told(&r, &pressed, NOT_HERE))
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let m = message(&r, ANA_DM, &card.id);
    assert_eq!(
        (m.content.as_str(), m.edits),
        (card.content.as_str(), card.edits),
        "{m:#?}"
    );
    assert_eq!(labels(&m), ["Forged"], "no fold's buttons: {m:#?}");
}

/// A fold whose button names another turn: refused, and neither fold
/// changes.
#[tokio::test]
async fn a_fold_pressed_for_another_turn_edits_nothing() {
    let r = rig(
        |dir| {
            vec![
                reads(dir, "a", 1),
                Scripted::text("First."),
                reads(dir, "b", 2),
                Scripted::text("Second."),
            ]
        },
        "",
    )
    .await;
    for (ask, said) in [("One?", "First."), ("Two?", "Second.")] {
        r.say((ANA, "ana"), None, ask);
        r.until(said, || {
            r.posted(ANA_DM).iter().any(|m| m.content.contains(said))
                && r.core.outbox.status("discord").pending == 0
        })
        .await;
    }
    r.until("two folds", || folds(&r, ANA_DM).len() == 2).await;
    let (a, b) = {
        let mut f = folds(&r, ANA_DM);
        let b = f.remove(1);
        (f.remove(0), b)
    };
    forge(&r, ANA_DM, &a.id, &format!("fold:o:0:{}", turn_of(&b))).await;
    let a = message(&r, ANA_DM, &a.id);
    let pressed = r.press(&a.id, "Forged", (ANA, "ana"));
    r.until("the presser told", || told(&r, &pressed, NOT_HERE))
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let now = message(&r, ANA_DM, &a.id);
    assert_eq!(
        (now.content.as_str(), now.edits),
        (a.content.as_str(), a.edits)
    );
    let now = message(&r, ANA_DM, &b.id);
    assert_eq!(
        (now.content.as_str(), now.edits),
        (b.content.as_str(), b.edits)
    );
}

/// A fold whose button names a turn of another place: refused, and neither
/// fold changes.
#[tokio::test]
async fn a_fold_pressed_for_another_places_turn_edits_nothing() {
    let r = rig(
        |dir| {
            vec![
                reads(dir, "lab", 1),
                Scripted::text("Lab's."),
                reads(dir, "dm", 1),
                Scripted::text("The DM's."),
            ]
        },
        "",
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), "Lab?");
    r.until("#lab's fold", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Lab's.")) && folds(&r, LAB).len() == 1
    })
    .await;
    r.say((ANA, "ana"), None, "DM?");
    r.until("the DM's fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("The DM's."))
            && folds(&r, ANA_DM).len() == 1
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let lab = folds(&r, LAB).remove(0);
    let dm = folds(&r, ANA_DM).remove(0);
    forge(&r, ANA_DM, &dm.id, &format!("fold:o:0:{}", turn_of(&lab))).await;
    let dm = message(&r, ANA_DM, &dm.id);
    let pressed = r.press(&dm.id, "Forged", (ANA, "ana"));
    r.until("the presser told", || told(&r, &pressed, TOO_OLD))
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let now = message(&r, ANA_DM, &dm.id);
    assert_eq!(
        (now.content.as_str(), now.edits),
        (dm.content.as_str(), dm.edits)
    );
    let now = message(&r, LAB, &lab.id);
    assert_eq!(
        (now.content.as_str(), now.edits),
        (lab.content.as_str(), lab.edits)
    );
}

/// In a shared channel, someone it does not list may open and close a fold,
/// since anyone there could read the lines; that person's press of the
/// fold's "Should I have asked?" control, which acts, is refused as before.
/// A fold's press in a channel not bound here gets no answer at all.
#[tokio::test]
async fn a_non_member_may_press_a_fold_but_not_its_control_and_an_unbound_place_answers_nothing() {
    let r = rig(
        |_| {
            vec![
                Scripted::tools(
                    "",
                    &[("c1", "proc_run", json!({"argv": ["sh", "-c", "echo lab"]}))],
                ),
                Scripted::text("Read."),
            ]
        },
        "private = true\n",
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), "Read them?");
    r.until("#lab's fold", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Read."))
            && folds(&r, LAB)
                .first()
                .is_some_and(|f| f.content == "-# 1 step: 1 command")
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let f = folds(&r, LAB).remove(0);
    let control = labels(&f)
        .into_iter()
        .find(|l| l != "Show steps")
        .expect("a notified call's control on the fold");
    r.press(&f.id, "Show steps", (CY, "cy"));
    r.until("open, by cy", || {
        message(&r, LAB, &f.id).content.contains("`proc.run`")
    })
    .await;
    r.press(&f.id, "Hide steps", (CY, "cy"));
    r.until("closed, by cy", || {
        message(&r, LAB, &f.id).content == "-# 1 step: 1 command"
    })
    .await;
    let pressed = r.press(&f.id, &control, (CY, "cy"));
    r.until("cy refused the control", || {
        told(
            &r,
            &pressed,
            "Only the people this place is bound to can do that.",
        )
    })
    .await;

    // A fold's button in a channel bound nowhere here.
    let url = format!(
        "http://{}/api/v10/channels/{ELSEWHERE}/messages",
        r.fake.addr
    );
    let posted: Value = reqwest::Client::new()
        .post(url)
        .json(&json!({"content": "-# 1 step: 1 command", "components": [{"type": 1, "components": [
            {"type": 2, "style": 2, "custom_id": format!("fold:o:0:{}", turn_of(&f)), "label": "Show steps"}]}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = posted["id"].as_str().unwrap().to_string();
    let pressed = r.press(&id, "Show steps", (ANA, "ana"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        r.fake
            .replies()
            .iter()
            .all(|x| x.interaction.as_deref() != Some(pressed.as_str())),
        "no answer at all"
    );
    assert_eq!(message(&r, ELSEWHERE, &id).edits, 0);
}
