//! Silent by default (theseus-l1y1), end to end through the stand-in's
//! gateway: a message the owner types is answered with one ping, its first
//! text part, and the turn's tool line and later parts go out silent; a
//! burst of cards in one place pings once, its later cards silent and their
//! buttons live. Every create's `flags` is what the stand-in recorded, and
//! each `discord.message.out` row says what the table asked for (`ping`) and
//! whether the window held it back (`held`), so a silent row is told from a
//! ping the window took.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use theseus_core::provider::{FakeProvider, Scripted};
use theseus_sim::fake_discord::{Guild, Msg, Pressed, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, LAB};

/// `#lab`, bound private, where ana, an owner, drives Theseus, and her DM.
fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA])
}

async fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    Rig::start_on(
        |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
        guild(),
        &bindings(),
        &[],
    )
    .await
}

/// A write under the rig's approve list, so it waits for its card.
fn outside(dir: &Path, name: &str) -> PathBuf {
    dir.join("outside").join(name)
}

/// The `discord.message.out` row of the create whose key ends with `tail`.
fn row(r: &Rig, tail: &str) -> serde_json::Value {
    r.ledger("discord.message.out")
        .into_iter()
        .find(|d| d["part"].as_str().is_some_and(|p| p.ends_with(tail)))
        .unwrap_or_else(|| panic!("no create of a key ending {tail}"))
}

fn first(r: &Rig, starts: &str) -> Msg {
    r.posted(LAB)
        .into_iter()
        .find(|m| m.content.starts_with(starts))
        .unwrap_or_else(|| panic!("no message beginning {starts:?}: {:#?}", r.posted(LAB)))
}

/// The owner's message is answered with one ping: its reply's first part,
/// which replies to it. The tool line and the second loop's text, the part
/// the footer rides on, are silent, and the table asked for no ping for
/// them (the window held nothing back).
#[tokio::test]
async fn the_answer_to_the_owners_message_pings_once_and_its_tool_line_and_later_parts_do_not() {
    let r = rig(|dir| {
        let chart = dir.join("work").join("chart.txt");
        std::fs::create_dir_all(dir.join("work")).unwrap();
        std::fs::write(&chart, "low tide 06:12\n").unwrap();
        vec![
            Scripted::tools(
                "Reading the tide chart.",
                &[(
                    "t1",
                    "fs_read",
                    serde_json::json!({"path": chart.to_string_lossy()}),
                )],
            ),
            Scripted::text("Low tide is at 06:12."),
        ]
    })
    .await;
    let typed = r.say((ANA, "ana"), Some(LAB), "When is low tide?");
    r.until("the answer, its tool line, and its reply settled", || {
        r.posted(LAB).iter().any(|m| m.content.contains("06:12."))
            && r.posted(LAB).iter().any(|m| m.content.contains("fs.read"))
            && r.core.outbox.status("discord").pending == 0
            // Each create's row is written soon after it, off the lane's path.
            && [":L0:p0", ":L0:tools", ":L1:p0"].iter().all(|t| {
                r.ledger("discord.message.out")
                    .iter()
                    .any(|d| d["part"].as_str().is_some_and(|p| p.ends_with(t)))
            })
    })
    .await;
    let answer = first(&r, "Reading the tide chart.");
    assert_eq!(answer.reply_to.as_deref(), Some(typed.as_str()));
    assert!(!answer.silent(), "the answer pings: {answer:?}");
    let tools = r
        .posted(LAB)
        .into_iter()
        .find(|m| m.versions[0].contains("fs.read"))
        .unwrap();
    assert!(tools.silent(), "a tool line is silent: {tools:?}");
    let later = first(&r, "Low tide is at 06:12.");
    assert!(later.silent(), "a later part is silent: {later:?}");
    // The bind notice too.
    let bound = first(&r, "🔗");
    assert!(bound.silent(), "{bound:?}");
    // What the table asked for, before the window.
    assert_eq!(row(&r, ":L0:p0")["ping"], true);
    for tail in [":L0:tools", ":L1:p0"] {
        let d = row(&r, tail);
        assert_eq!(
            (&d["ping"], &d["held"]),
            (&false.into(), &false.into()),
            "{tail}: {d}"
        );
    }
}

/// Three cards inside the window, in one place: the first pings, the second
/// and third go out silent, the window holding their pings back, and their
/// buttons work as always. A turn asks one question at a time, so each card
/// is approved as it lands, by its own button, and the next call asks.
#[tokio::test]
async fn a_burst_of_three_cards_pings_once_and_every_card_keeps_its_buttons() {
    let r = rig(|dir| {
        let w = |id: &str, name: &str| {
            Scripted::tools(
                "",
                &[(
                    id,
                    "fs_write",
                    serde_json::json!({"path": outside(dir, name).to_string_lossy(), "content": name}),
                )],
            )
        };
        vec![
            w("t1", "a.txt"),
            w("t2", "b.txt"),
            w("t3", "c.txt"),
            Scripted::text("All three written."),
        ]
    })
    .await;
    r.say((ANA, "ana"), Some(LAB), "Write the three tide files.");
    let cards = || -> Vec<Msg> {
        r.posted(LAB)
            .into_iter()
            .filter(|m| m.versions[0].contains("**Approve?**"))
            .collect()
    };
    for n in 1..=3 {
        r.until(&format!("card {n}"), || cards().len() == n).await;
        let card = cards().pop().unwrap();
        let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Approve", "Decline"], "{card:?}");
        r.fake
            .press(&Pressed {
                message: &card.id,
                button: "Approve",
                user: ANA,
                name: "ana",
            })
            .unwrap();
    }
    r.until("all three written and the reply posted", || {
        outside(r.dir.path(), "c.txt").exists()
            && r.posted(LAB)
                .iter()
                .any(|m| m.content.contains("All three written."))
    })
    .await;
    let got = cards();
    let loud: Vec<&Msg> = got.iter().filter(|m| !m.silent()).collect();
    assert_eq!(loud.len(), 1, "one ping for the burst: {got:#?}");
    assert_eq!(loud[0].id, got[0].id, "the first card's");
    let card_rows = || -> Vec<serde_json::Value> {
        r.ledger("discord.message.out")
            .into_iter()
            .filter(|d| {
                d["part"]
                    .as_str()
                    .is_some_and(|p| p.starts_with("confirm:"))
            })
            .collect()
    };
    // Each create's row is written soon after it, off the lane's path.
    r.until("the three cards' rows", || card_rows().len() == 3)
        .await;
    let rows = card_rows();
    let said: Vec<(bool, bool)> = rows
        .iter()
        .map(|d| (d["ping"] == true, d["held"] == true))
        .collect();
    assert_eq!(
        said,
        [(true, false), (false, true), (false, true)],
        "the window held the second and third: {rows:?}"
    );
    let ok = r.ledger("discord.confirm");
    assert_eq!(ok.len(), 3, "each card's press counted: {ok:?}");
    assert!(ok.iter().all(|d| d["ok"] == true), "{ok:?}");
}
