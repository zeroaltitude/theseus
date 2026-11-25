//! The folded default (issue 56) under the checks its open form has carried:
//! what a place shows (`show_tools`, `show_thinking`), read live; `steps`
//! read live; silence; the vault's values at the exit; and a card and its
//! settle around a fold. A fold never shows what its place is set not to
//! show now, though it was made when the place showed it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_sim::fake_discord::{Guild, Msg, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};

/// `#lab`, bound private with `lab`'s words, and ana's DM with `dm`'s.
fn bindings(lab: &str, dm: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n{lab}\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n{dm}"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA])
}

/// A loop that thinks, says a line and reads the chart; then the answer.
fn turn(dir: &Path, word: &str) -> Vec<Scripted> {
    let chart = dir.join("work").join("chart.txt");
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::write(&chart, "low tide 06:12\n").unwrap();
    vec![
        Scripted::Blocks {
            blocks: vec![
                json!({"type": "thinking", "thinking": format!("The {word} chart is in work/."), "signature": "sig"}),
                json!({"type": "text", "text": format!("Reading the {word} chart.")}),
                json!({"type": "tool_use", "id": format!("t_{word}"), "name": "fs_read",
                    "input": {"path": chart.to_string_lossy()}}),
            ],
            stop_reason: "tool_use".into(),
        },
        Scripted::text(&format!("Low tide by the {word} chart is at 06:12.")),
    ]
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

fn message(r: &Rig, channel: u64, id: &str) -> Msg {
    r.fake
        .messages(channel)
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

/// One exchange in `channel` (None: the DM), settled.
async fn exchange(r: &Rig, channel: Option<u64>, word: &str) {
    let at = channel.unwrap_or(ANA_DM);
    r.say(
        (ANA, "ana"),
        channel,
        &format!("When is low tide by the {word} chart?"),
    );
    r.until(&format!("the {word} exchange settled"), || {
        r.posted(at).iter().any(|m| {
            m.content
                .contains(&format!("the {word} chart is at 06:12."))
        }) && r.core.outbox.status("discord").pending == 0
    })
    .await;
}

/// Rewrite the bindings file and wait for it to be bound.
async fn rebind(r: &Rig, file: &str) {
    let revision = crate::bindings::Bindings::parse(file).unwrap().revision;
    std::fs::write(r.dir.path().join("bindings.toml"), file).unwrap();
    r.until("the change is bound", || {
        r.core
            .bindings
            .all()
            .first()
            .and_then(|b| b.revision.clone())
            == Some(revision.clone())
    })
    .await;
}

fn has(ms: &[Msg], s: &str) -> bool {
    ms.iter().any(|m| m.versions.iter().any(|v| v.contains(s)))
}

/// Folded, the default: `#lab`, hiding both, makes no fold and shows
/// nothing of either; the DM, showing both, folds both, its closed line
/// saying each, open their lines; a place hiding its tools alone folds its
/// thinking alone, with no step counted and no control.
#[tokio::test]
async fn each_half_of_a_fold_shows_only_under_its_word() {
    let file = bindings(
        "show_tools = false\nshow_thinking = false\n",
        "show_tools = false\n",
    );
    let r = rig(
        |dir| {
            let mut s = turn(dir, "lab");
            s.extend(turn(dir, "dm"));
            s
        },
        &file,
    )
    .await;
    exchange(&r, Some(LAB), "lab").await;
    let lab = r.posted(LAB);
    assert!(has(&lab, "Reading the lab chart."), "{lab:#?}");
    assert!(folds(&r, LAB).is_empty(), "no fold in #lab: {lab:#?}");
    assert!(!has(&lab, "fs.read") && !has(&lab, "💭") && !has(&lab, " step"));

    exchange(&r, None, "dm").await;
    r.until("the DM's fold", || {
        folds(&r, ANA_DM)
            .first()
            .is_some_and(|f| f.content.starts_with("-# 💭 thought for "))
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    assert!(!f.content.contains("step") && !f.content.contains('·'), "{f:#?}");
    assert_eq!(labels(&f), ["Show steps"], "no control without tools");
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("open", || {
        labels(&message(&r, ANA_DM, &f.id)) == ["Hide steps"]
    })
    .await;
    let open = message(&r, ANA_DM, &f.id);
    assert!(open.content.starts_with("-# 💭 thought for "), "{open:#?}");
    assert!(!has(&[open], "fs.read"), "no tool line in any version");
}

/// A fold made while its place showed both, opened; the place is then set
/// to hide its tools: the open fold loses its tool lines and its control at
/// once, closed it counts no step, and open again shows the thinking alone.
/// Set to hide both, it shows nothing, and a press opens nothing.
#[tokio::test]
async fn a_live_change_to_hide_a_half_reaches_an_old_fold() {
    let r = rig(|dir| turn(dir, "dm"), &bindings("", "")).await;
    exchange(&r, None, "dm").await;
    r.until("the DM's fold, closed", || {
        folds(&r, ANA_DM)
            .first()
            .is_some_and(|f| f.content.ends_with(" · 1 step: 1 read"))
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    assert!(labels(&f).len() > 1, "a notified call's control: {f:#?}");
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("open, with its tool line", || {
        message(&r, ANA_DM, &f.id).content.contains("`fs.read`")
    })
    .await;

    rebind(&r, &bindings("", "show_tools = false\n")).await;
    r.until("the open fold without its tool line", || {
        let m = message(&r, ANA_DM, &f.id);
        !m.content.contains("fs.read") && m.content.starts_with("-# 💭 thought for ")
    })
    .await;
    assert_eq!(labels(&message(&r, ANA_DM, &f.id)), ["Hide steps"]);
    r.press(&f.id, "Hide steps", (ANA, "ana"));
    r.until("closed, counting no step", || {
        let m = message(&r, ANA_DM, &f.id);
        m.content.starts_with("-# 💭 thought for ") && labels(&m) == ["Show steps"]
    })
    .await;
    assert!(!message(&r, ANA_DM, &f.id).content.contains("step:"));
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("open again, the thinking alone", || {
        labels(&message(&r, ANA_DM, &f.id)) == ["Hide steps"]
    })
    .await;
    assert!(!message(&r, ANA_DM, &f.id).content.contains("fs.read"));

    rebind(&r, &bindings("", "show_tools = false\nshow_thinking = false\n")).await;
    r.until("the fold shows nothing of either", || {
        let m = message(&r, ANA_DM, &f.id);
        !m.content.contains("💭") && !m.content.contains("fs.read") && labels(&m) == ["Show steps"]
    })
    .await;
    let closed = message(&r, ANA_DM, &f.id);
    let pressed = r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("the presser told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.ephemeral
                && x.content.as_deref() == Some("This place shows no steps now.")
        })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let m = message(&r, ANA_DM, &f.id);
    assert_eq!((m.content.as_str(), m.edits), (closed.content.as_str(), closed.edits));
}

/// A fold made while its place was folded, whose place is then bound
/// `steps = "open"`: its buttons still open and close it, and the place's
/// next turn writes today's messages, folding nothing new.
#[tokio::test]
async fn an_old_folds_buttons_work_after_its_place_says_open() {
    let r = rig(
        |dir| {
            let mut s = turn(dir, "first");
            s.extend(turn(dir, "second"));
            s
        },
        &bindings("", ""),
    )
    .await;
    exchange(&r, None, "first").await;
    r.until("the fold", || {
        folds(&r, ANA_DM)
            .first()
            .is_some_and(|f| f.content.ends_with(" · 1 step: 1 read"))
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    rebind(&r, &bindings("", "steps = \"open\"\n")).await;
    exchange(&r, None, "second").await;
    r.until("the second turn's tool message", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("`fs.read`") && m.id != f.id)
    })
    .await;
    assert_eq!(folds(&r, ANA_DM).len(), 1, "nothing new folded");
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("the old fold open", || {
        message(&r, ANA_DM, &f.id).content.contains("`fs.read`")
    })
    .await;
    r.press(&f.id, "Hide steps", (ANA, "ana"));
    r.until("the old fold closed", || {
        message(&r, ANA_DM, &f.id).content == f.content
    })
    .await;
}

/// `silent = ["tool_lines"]` quiets a fold's create, as it quieted a tool
/// line; the answer still rings. With nothing silenced the fold rang
/// (`tests_fold`).
#[tokio::test]
async fn a_place_that_silences_tool_lines_creates_its_fold_silent() {
    let r = rig(
        |dir| {
            std::fs::create_dir_all(dir.join("work")).unwrap();
            std::fs::write(dir.join("work").join("chart.txt"), "x\n").unwrap();
            let chart = dir.join("work").join("chart.txt");
            vec![
                Scripted::tools(
                    "",
                    &[("r1", "fs_read", json!({"path": chart.to_string_lossy()}))],
                ),
                Scripted::text("Low tide by the dm chart is at 06:12."),
            ]
        },
        &bindings("", "silent = [\"tool_lines\"]\n"),
    )
    .await;
    exchange(&r, None, "dm").await;
    r.until("the fold", || !folds(&r, ANA_DM).is_empty()).await;
    let f = folds(&r, ANA_DM).remove(0);
    assert_eq!(f.versions[0], "-# 1 step: 1 read", "{f:#?}");
    assert!(f.silent(), "{f:#?}");
    let answer = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("the dm chart is at 06:12."))
        .unwrap();
    assert!(!answer.silent(), "the answer rings: {answer:#?}");
}

/// A value the vault resolved, in a call's input and in the thinking: it is
/// in no version of the fold, closed, opened by a press, or edited live
/// while open; the open fold shows the call's line, masked.
#[tokio::test]
async fn a_vault_value_is_in_no_version_of_a_fold() {
    const VALUE: &str = "jev-test-key-0123456789";
    let r = rig(
        |dir| {
            std::fs::create_dir_all(dir.join("work")).unwrap();
            let p = dir.join("work").join(format!("{VALUE}.txt"));
            std::fs::write(&p, "x\n").unwrap();
            let q = dir.join("work").join(format!("again-{VALUE}.txt"));
            std::fs::write(&q, "x\n").unwrap();
            vec![
                Scripted::Blocks {
                    blocks: vec![
                        json!({"type": "thinking", "thinking": format!("The key is {VALUE}."), "signature": "sig"}),
                        json!({"type": "tool_use", "id": "v1", "name": "fs_read",
                            "input": {"path": p.to_string_lossy()}}),
                    ],
                    stop_reason: "tool_use".into(),
                },
                Scripted::tools(
                    "",
                    &[("v2", "proc_run", json!({"argv": ["sh", "-c", format!("sleep 2; cat {}", q.to_string_lossy())]}))],
                ),
                Scripted::text("Read it."),
            ]
        },
        &bindings("", ""),
    )
    .await;
    r.say((ANA, "ana"), None, "Read the key file.");
    r.until("the fold of the first call", || {
        folds(&r, ANA_DM)
            .first()
            .is_some_and(|f| f.content.contains("1 read"))
    })
    .await;
    let f = folds(&r, ANA_DM).remove(0);
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("open while the turn runs, then its last state", || {
        let m = message(&r, ANA_DM, &f.id);
        m.content.contains("`fs.read`")
            && r.posted(ANA_DM).iter().any(|m| m.content.contains("Read it."))
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let m = message(&r, ANA_DM, &f.id);
    assert!(m.versions.len() >= 3, "closed, opened, edited: {m:#?}");
    for v in &m.versions {
        assert!(!v.contains(VALUE), "the value left in {v:?}");
    }
    assert!(m.content.contains("`fs.read`"), "{m:#?}");
    r.press(&f.id, "Hide steps", (ANA, "ana"));
    r.until("closed", || labels(&message(&r, ANA_DM, &f.id)).contains(&"Show steps".to_string()))
        .await;
    assert!(message(&r, ANA_DM, &f.id)
        .versions
        .iter()
        .all(|v| !v.contains(VALUE)));
}

/// A call that waits for approval: the fold is written before the card,
/// the card is its own message with its buttons, and once approved the
/// card settles, the fold counts the call done, and the answer follows.
#[tokio::test]
async fn a_card_and_its_settle_keep_their_order_around_a_fold() {
    let r = rig(
        |dir| {
            let out = dir.join("outside").join("proof.txt");
            vec![
                Scripted::tools(
                    "",
                    &[(
                        "w1",
                        "fs_write",
                        json!({"path": out.to_string_lossy(), "content": "x"}),
                    )],
                ),
                Scripted::text("Written."),
            ]
        },
        &bindings("", ""),
    )
    .await;
    r.say((ANA, "ana"), None, "Write the proof.");
    r.until("the card and the fold that says it waits", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.versions[0].contains("**Approve?**"))
            && folds(&r, ANA_DM)
                .first()
                .is_some_and(|f| f.content.contains("1 waiting for approval"))
    })
    .await;
    let at = |id: &str| r.posted(ANA_DM).iter().position(|m| m.id == id).unwrap();
    let card = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.versions[0].contains("**Approve?**"))
        .unwrap();
    let f = folds(&r, ANA_DM).remove(0);
    assert!(at(&f.id) < at(&card.id), "the fold before its card");
    assert!(labels(&card).contains(&"Approve".to_string()));
    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("settled, done, and answered", || {
        let c = message(&r, ANA_DM, &card.id);
        let f = message(&r, ANA_DM, &f.id);
        c.buttons.is_empty()
            && !f.content.contains("waiting")
            && f.content.contains("1 step: 1 edit")
            && r.posted(ANA_DM).iter().any(|m| m.content.contains("Written."))
    })
    .await;
    let answer = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("Written."))
        .unwrap();
    assert!(at(&card.id) < at(&answer.id), "the answer after the card");
    assert_eq!(folds(&r, ANA_DM).len(), 1, "one fold for the turn");
}
