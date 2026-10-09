//! Which creates notify (theseus-l1y1), end to end through the stand-in's
//! gateway. With no config every message notifies, as it always has: the
//! answer to the owner's message, its tool line, its later parts, the bind
//! notice, and every card, however close together. `[discord] silent` names
//! the kinds that post silent instead, and each name silences its own kind
//! alone. Every create's `flags` is what the stand-in recorded, and each
//! `discord.message.out` row says whether it pinged (`ping`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use theseus_core::config::discord::Category;
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

async fn rig(silent: &[Category], script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    let silent = silent.to_vec();
    Rig::start_tweaked(
        |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
        guild(),
        &bindings(),
        &[],
        move |c| c.discord.silent = silent,
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

/// What one exchange's messages did, with `silent` set: whether each went
/// out silent, by the stand-in's flags, and its row's `ping` agreeing.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Exchange {
    bind: bool,
    answer: bool,
    tools: bool,
    later: bool,
}

/// The owner types in `#lab`; the turn reads a file (a tool line), then
/// answers in a second loop (a later part, the footer on it).
async fn exchange(silent: &[Category]) -> Exchange {
    let r = rig(silent, |dir| {
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
    let tools = r
        .posted(LAB)
        .into_iter()
        .find(|m| m.versions[0].contains("fs.read"))
        .unwrap();
    let later = first(&r, "Low tide is at 06:12.");
    for (tail, m) in [
        (":L0:p0", &answer),
        (":L0:tools", &tools),
        (":L1:p0", &later),
    ] {
        let d = row(&r, tail);
        assert_eq!(d["ping"], !m.silent(), "{tail}'s row and flags agree: {d}");
    }
    Exchange {
        bind: first(&r, "🔗").silent(),
        answer: answer.silent(),
        tools: tools.silent(),
        later: later.silent(),
    }
}

/// With no config every message of the exchange notifies, as it did before
/// `[discord] silent`.
#[tokio::test]
async fn with_no_config_the_answer_its_tool_line_and_its_later_parts_all_notify() {
    let got = exchange(&[]).await;
    assert_eq!(
        got,
        Exchange {
            bind: false,
            answer: false,
            tools: false,
            later: false
        }
    );
}

/// Each category silences its own message of the exchange, and only it.
#[tokio::test]
async fn each_category_silences_only_its_own_message() {
    let all_loud = Exchange {
        bind: false,
        answer: false,
        tools: false,
        later: false,
    };
    for (silent, want) in [
        (
            Category::Tools,
            Exchange {
                tools: true,
                ..all_loud
            },
        ),
        (
            Category::Answer,
            Exchange {
                answer: true,
                ..all_loud
            },
        ),
        (
            Category::Replies,
            Exchange {
                later: true,
                ..all_loud
            },
        ),
        (
            Category::Notes,
            Exchange {
                bind: true,
                ..all_loud
            },
        ),
        (Category::Cards, all_loud),
    ] {
        assert_eq!(exchange(&[silent]).await, want, "{silent:?}");
    }
}

/// The three cards of one turn's three writes, each approved by its own
/// button as it lands: with no config each notifies, however close together
/// (no window), and with `cards` silent none does; their buttons work
/// either way.
#[tokio::test]
async fn every_card_notifies_by_default_and_cards_silences_them_all() {
    for silent in [vec![], vec![Category::Cards]] {
        let r = rig(&silent, |dir| {
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
        let quiet = !silent.is_empty();
        let got = cards();
        assert!(
            got.iter().all(|m| m.silent() == quiet),
            "{silent:?}: {got:#?}"
        );
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
        assert!(rows.iter().all(|d| d["ping"] == !quiet), "{rows:?}");
        let ok = r.ledger("discord.confirm");
        assert_eq!(ok.len(), 3, "each card's press counted: {ok:?}");
        assert!(ok.iter().all(|d| d["ok"] == true), "{ok:?}");
    }
}
