//! A fold through an outage (issue 56), end to end through the real lane's
//! drain: an update whose own edit gets Discord's 503 is dropped, as live
//! progress always was, and nothing is kept for it; once the lane is back and
//! every post that waited has gone, the place writes each fold it still holds
//! once more, as it shows now. (A press not taken within its wait is
//! cancelled: `runtime/fold.rs`'s tests drive that wait on a clock they
//! control.)

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_sim::fake_discord::{Guild, Mode, Msg, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};

/// The renderer's newest turns (`render.rs`'s `RECENT_TURNS`): the most
/// folds a place holds here, where no older turn has a call still pending.
/// (Up to `render/held.rs`'s `PINNED_TURNS` older turns with a pending call
/// are held besides.)
const HELD: usize = 8;

fn bindings(dm: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n{dm}"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA])
}

async fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>, file: &str) -> Rig {
    Rig::start_on(
        |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
        guild(),
        file,
        &[],
    )
    .await
}

/// A loop that thinks, says a line and runs a notified command; then the
/// answer.
fn command_turn(word: &str) -> Vec<Scripted> {
    vec![
        Scripted::Blocks {
            blocks: vec![
                json!({"type": "thinking", "thinking": format!("The {word} chart is a command away."), "signature": "sig"}),
                json!({"type": "text", "text": format!("Reading the {word} chart.")}),
                json!({"type": "tool_use", "id": format!("c_{word}"), "name": "proc_run",
                    "input": {"argv": ["sh", "-c", format!("echo {word}")]}}),
            ],
            stop_reason: "tool_use".into(),
        },
        Scripted::text(&format!("Low tide by the {word} chart is at 06:12.")),
    ]
}

fn labels(m: &Msg) -> Vec<String> {
    m.buttons.iter().map(|b| b.label.clone()).collect()
}

fn folds(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("fold:")))
        .collect()
}

fn message(r: &Rig, channel: u64, id: &str) -> Msg {
    r.fake
        .messages(channel)
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

/// Waits for `f` up to `secs`, outside the rig's patience: an outage's
/// backoff is part of what these tests wait through.
async fn within(secs: u64, what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Whether an edit of message `id` was answered with the fake's failure.
fn edit_failed(r: &Rig, id: &str) -> bool {
    r.fake
        .seen()
        .iter()
        .any(|s| s.method == "PATCH" && s.path.ends_with(id) && s.outcome == "failed")
}

/// One settled exchange in the DM, and its fold.
async fn settled(r: &Rig, word: &str) -> Msg {
    r.say(
        (ANA, "ana"),
        None,
        &format!("When is low tide by the {word} chart?"),
    );
    r.until("the exchange and its fold", || {
        r.posted(ANA_DM).iter().any(|m| {
            m.content
                .contains(&format!("the {word} chart is at 06:12."))
        }) && folds(r, ANA_DM)
            .last()
            .is_some_and(|f| f.content.ends_with(" · 1 step: 1 command"))
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    folds(r, ANA_DM).pop().unwrap()
}

/// Rewrite the bindings file and wait for it to be bound.
async fn rebind(r: &Rig, file: &str) {
    let revision = crate::bindings::Bindings::parse(file).unwrap().revision;
    std::fs::write(r.dir.path().join("bindings.toml"), file).unwrap();
    within(20, "the change is bound", || {
        r.core
            .bindings
            .all()
            .first()
            .and_then(|b| b.revision.clone())
            == Some(revision.clone())
    })
    .await;
}

/// An open fold; its place is set to hide its tools while Discord answers
/// every call with 503, so the fold's rebuilt edit itself fails. Nothing else
/// happens after: once Discord is back, the fold shows what its place shows
/// now, with no tool line.
#[tokio::test]
async fn a_hide_whose_edit_fails_reaches_discord_once_the_lane_is_back() {
    let r = rig(|_| command_turn("dm"), &bindings("")).await;
    let f = settled(&r, "dm").await;
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("open", || {
        message(&r, ANA_DM, &f.id).content.contains("`proc.run`")
    })
    .await;
    r.fake.set_mode(Mode::Fail);
    rebind(&r, &bindings("show_tools = false\n")).await;
    within(20, "the hide's edit refused", || edit_failed(&r, &f.id)).await;
    assert!(message(&r, ANA_DM, &f.id).content.contains("`proc.run`"));
    r.fake.set_mode(Mode::Up);
    within(30, "the hidden fold written once the lane is back", || {
        let m = message(&r, ANA_DM, &f.id);
        !m.content.contains("proc.run") && m.content.starts_with("-# 💭 thought for ")
    })
    .await;
    assert_eq!(labels(&message(&r, ANA_DM, &f.id)), ["Hide steps"]);
}

/// A press the lane took, whose own edit Discord refused: once Discord is
/// back, with no event after, the fold shows what the press asked.
#[tokio::test]
async fn an_accepted_show_whose_edit_fails_is_written_once_the_lane_is_back() {
    let r = rig(|_| command_turn("dm"), &bindings("")).await;
    let f = settled(&r, "dm").await;
    r.fake.set_mode(Mode::Fail);
    r.press(&f.id, "Show steps", (ANA, "ana"));
    within(20, "the press's edit refused", || edit_failed(&r, &f.id)).await;
    assert_eq!(message(&r, ANA_DM, &f.id).content, f.content);
    r.fake.set_mode(Mode::Up);
    within(30, "the fold open once the lane is back", || {
        message(&r, ANA_DM, &f.id).content.contains("`proc.run`")
    })
    .await;
}

/// Forty turns while Discord refuses the DM's creates, the last waiting on
/// an approval card. Nothing is kept for their folds: once Discord is back,
/// every post that waited goes first, the card before any fold, and then at
/// most the turns the place still holds get a fold, the newest among them.
#[tokio::test]
async fn forty_turns_in_an_outage_leave_at_most_the_held_folds_and_the_card_goes_first() {
    let r = rig(
        |dir| {
            let work = dir.join("work");
            std::fs::create_dir_all(&work).unwrap();
            let mut s = Vec::new();
            for i in 0..41 {
                let p = work.join(format!("f{i}.txt"));
                std::fs::write(&p, "x\n").unwrap();
                s.push(Scripted::tools(
                    "",
                    &[(
                        &format!("r{i}"),
                        "fs_read",
                        json!({"path": p.to_string_lossy()}),
                    )],
                ));
                s.push(Scripted::text(&format!("Answer {i}.")));
            }
            let out = dir.join("outside").join("proof.txt");
            s.push(Scripted::tools(
                "",
                &[(
                    "w1",
                    "fs_write",
                    json!({"path": out.to_string_lossy(), "content": "x"}),
                )],
            ));
            s
        },
        &bindings(""),
    )
    .await;
    r.say((ANA, "ana"), None, "Turn 0?");
    r.until("the first turn and its fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Answer 0."))
            && !folds(&r, ANA_DM).is_empty()
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let before: Vec<String> = r.posted(ANA_DM).into_iter().map(|m| m.id).collect();
    r.fake.fail_creates_in(ANA_DM, Some(503));
    let ended = || r.ledger("turn.ended").len();
    for i in 1..41 {
        let n = ended();
        r.say((ANA, "ana"), None, &format!("Turn {i}?"));
        within(20, &format!("turn {i} ended"), || ended() > n).await;
    }
    r.say((ANA, "ana"), None, "Write the proof.");
    within(20, "the card's call waits", || {
        r.core.confirm_list().is_ok_and(|l| !l.is_empty())
    })
    .await;
    let dm = format!("discord:dm:{ANA}");
    within(20, "the card's post waits with the answers", || {
        r.core
            .outbox
            .open_for(&dm)
            .iter()
            .any(|a| theseus_core::outbox::kind_of(a) == "card")
    })
    .await;
    r.fake.fail_creates_in(ANA_DM, None);
    within(60, "every answer and the card, then the folds", || {
        let all = r.posted(ANA_DM);
        all.iter().any(|m| m.content.contains("Answer 40."))
            && all.iter().any(|m| m.versions[0].contains("**Approve?**"))
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let new_fold = |m: &Msg| {
        !before.contains(&m.id) && m.buttons.iter().any(|b| b.custom_id.starts_with("fold:"))
    };
    within(30, "a fold written again", || {
        r.posted(ANA_DM).iter().any(new_fold)
    })
    .await;
    // An observation of absence: more folds than the place holds would show
    // here. It cannot fail a correct lane, only give a wrong one time to show.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let all = r.posted(ANA_DM);
    let id = |m: &Msg| m.id.parse::<u64>().unwrap();
    let card = all
        .iter()
        .find(|m| m.versions[0].contains("**Approve?**"))
        .unwrap();
    let new_folds: Vec<&Msg> = all
        .iter()
        .filter(|m| !before.contains(&m.id))
        .filter(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("fold:")))
        .collect();
    assert!(!new_folds.is_empty(), "the held turns' folds were written");
    assert!(
        new_folds.len() <= HELD,
        "{} folds after the outage: {new_folds:#?}",
        new_folds.len()
    );
    for f in &new_folds {
        assert!(id(card) < id(f), "the card before any fold: {f:#?}");
    }
}

/// A general question around a turn's fold: the fold is written before the
/// question's card, the card is its own message with its options, and once
/// answered the card is settled before the answer is written.
#[tokio::test]
async fn a_question_and_its_settle_keep_their_order_around_a_fold() {
    let ask = json!({
        "question": "Which plan for the harbour survey?",
        "options": [{"label": "Careful"}, {"label": "Ship it behind the flag"}],
        "default": 1,
    });
    let r = Rig::start_tweaked(
        |_, _| {
            Arc::new(FakeProvider::scripted(vec![
                Scripted::tools(
                    "",
                    &[("c1", "proc_run", json!({"argv": ["sh", "-c", "echo plan"]}))],
                ),
                Scripted::tools("", &[("q1", "owner_ask", ask)]),
                Scripted::text("Going on, carefully."),
            ]))
        },
        guild(),
        &bindings(""),
        &[],
        |c| {
            c.tools.defer.always_loaded.push("ask".into());
            c.discord.live_board = false;
        },
    )
    .await;
    r.say((ANA, "ana"), None, "decide");
    let is_card = |m: &Msg| m.buttons.iter().any(|b| b.label.starts_with("1. "));
    r.until("the question's card and the fold", || {
        r.posted(ANA_DM).iter().any(is_card) && !folds(&r, ANA_DM).is_empty()
    })
    .await;
    let at = |id: &str| r.posted(ANA_DM).iter().position(|m| m.id == id).unwrap();
    let card = r.posted(ANA_DM).into_iter().find(is_card).unwrap();
    let f = folds(&r, ANA_DM).remove(0);
    assert!(at(&f.id) < at(&card.id), "the fold before the question");
    assert!(card.content.contains("Which plan"), "{}", card.content);
    assert!(!f.content.contains("Which plan"), "{f:#?}");
    r.press(&card.id, "1. Careful", (ANA, "ana"));
    r.until("settled and answered", || {
        let c = message(&r, ANA_DM, &card.id);
        c.buttons.is_empty()
            && c.content.contains("Answered")
            && r.posted(ANA_DM)
                .iter()
                .any(|m| m.content.contains("Going on, carefully."))
    })
    .await;
    let answer = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("Going on, carefully."))
        .unwrap();
    assert!(
        at(&card.id) < at(&answer.id),
        "the answer after the question"
    );
    let seen = r.fake.seen();
    let settle = seen
        .iter()
        .position(|s| s.method == "PATCH" && s.path.ends_with(&card.id))
        .expect("the card's settle");
    let made = seen
        .iter()
        .position(|s| s.message_id.as_deref() == Some(answer.id.as_str()))
        .expect("the answer's create");
    assert!(settle < made, "the settle before the answer");
    assert_eq!(folds(&r, ANA_DM).len(), 1, "one fold for the turn");
}
