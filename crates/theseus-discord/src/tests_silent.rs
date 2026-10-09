//! Which creates ping (theseus-l1y1), end to end through the stand-in's
//! gateway: today's pings by default, and silence is per category, per
//! place. With no config every message pings, as it always has: the answer
//! to the owner's message, its tool line, its later parts, the bind notice,
//! and every card, however close together. A `silent` list (the place's
//! binding's, else `[discord]`'s) names the categories that post silent
//! instead, each its own alone, and `ping_window_secs` holds a place to one
//! ping in that long. Every create's `flags` is what the stand-in recorded,
//! and each `discord.message.out` row says whether it pinged (`ping`) and
//! whether the window held it (`held`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use theseus_core::config::discord::Category;
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_sim::fake_discord::{Guild, Msg, Pressed, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, LAB};

/// `#lab`, bound private with `lab`'s own lines, where ana, an owner,
/// drives Theseus, and her DM.
fn bindings(lab: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n{lab}\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA])
}

/// The daemon's `[discord]` words, and `#lab`'s own lines in the bindings
/// file.
#[derive(Default)]
struct Words {
    silent: Vec<Category>,
    window_secs: u64,
    lab: &'static str,
}

async fn rig(w: Words, script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    Rig::start_tweaked(
        |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
        guild(),
        &bindings(w.lab),
        &[],
        move |c| {
            c.discord.silent = w.silent;
            c.discord.ping_window_secs = w.window_secs;
        },
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

/// What one exchange's messages did, with the words set: whether each went
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
async fn exchange(w: Words) -> Exchange {
    let r = rig(w, |dir| {
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

/// With no config every message of the exchange pings, as it did before
/// `[discord] silent`.
#[tokio::test]
async fn with_no_config_the_answer_its_tool_line_and_its_later_parts_all_notify() {
    let got = exchange(Words::default()).await;
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

fn silent(c: &[Category]) -> Words {
    Words {
        silent: c.to_vec(),
        ..Words::default()
    }
}

/// Each category silences its own message of the exchange, and only it;
/// the redesign's pair, `tool_lines` and `later_parts`, silences both and
/// leaves the answer pinging.
#[tokio::test]
async fn each_category_silences_only_its_own_message() {
    let all_loud = Exchange {
        bind: false,
        answer: false,
        tools: false,
        later: false,
    };
    for (c, want) in [
        (
            Category::ToolLines,
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
            Category::LaterParts,
            Exchange {
                later: true,
                ..all_loud
            },
        ),
        (
            Category::Notices,
            Exchange {
                bind: true,
                ..all_loud
            },
        ),
        (Category::Cards, all_loud),
    ] {
        assert_eq!(exchange(silent(&[c])).await, want, "{c:?}");
    }
    let pair = [Category::ToolLines, Category::LaterParts];
    assert_eq!(
        exchange(silent(&pair)).await,
        Exchange {
            tools: true,
            later: true,
            ..all_loud
        },
        "the answer still pings"
    );
}

/// A place's own `silent` list takes the daemon's place there: `#lab`'s
/// `tool_lines` silences its tool line with nothing silent daemon-wide, and
/// its empty list makes every message ping under a daemon that silences
/// tool lines and later parts.
#[tokio::test]
async fn a_places_own_list_takes_the_daemons_place() {
    let all_loud = Exchange {
        bind: false,
        answer: false,
        tools: false,
        later: false,
    };
    let own = Words {
        lab: "silent = [\"tool_lines\"]\n",
        ..Words::default()
    };
    assert_eq!(
        exchange(own).await,
        Exchange {
            tools: true,
            ..all_loud
        }
    );
    let none = Words {
        silent: vec![Category::ToolLines, Category::LaterParts],
        lab: "silent = []\n",
        ..Words::default()
    };
    assert_eq!(exchange(none).await, all_loud);
}

/// What three cards did: each card's create silent or not, in order, and
/// its row's `ping` and `held`.
#[derive(Debug, PartialEq)]
struct Cards {
    silent: Vec<bool>,
    ping: Vec<bool>,
    held: Vec<bool>,
}

/// The three cards of one turn's three writes, each approved by its own
/// button as it lands, all inside 20 s; their buttons work whatever pings.
async fn three_cards(w: Words) -> Cards {
    let r = rig(w, |dir| {
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
    let began = std::time::Instant::now();
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
    let took = began.elapsed();
    assert!(took.as_secs() < 20, "three cards inside 20 s: {took:?}");
    r.until("all three written and the reply posted", || {
        outside(r.dir.path(), "c.txt").exists()
            && r.posted(LAB)
                .iter()
                .any(|m| m.content.contains("All three written."))
    })
    .await;
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
    let ok = r.ledger("discord.confirm");
    assert_eq!(ok.len(), 3, "each card's press counted: {ok:?}");
    assert!(ok.iter().all(|d| d["ok"] == true), "{ok:?}");
    let rows = card_rows();
    Cards {
        silent: cards().iter().map(Msg::silent).collect(),
        ping: rows.iter().map(|d| d["ping"] == true).collect(),
        held: rows.iter().map(|d| d["held"] == true).collect(),
    }
}

/// With no config each of three cards inside 20 s pings, however close
/// together (no window), and with `cards` silent none does.
#[tokio::test]
async fn every_card_notifies_by_default_and_cards_silences_them_all() {
    let loud = three_cards(Words::default()).await;
    assert_eq!(
        loud,
        Cards {
            silent: vec![false; 3],
            ping: vec![true; 3],
            held: vec![false; 3],
        }
    );
    let quiet = three_cards(silent(&[Category::Cards])).await;
    assert_eq!(
        quiet,
        Cards {
            silent: vec![true; 3],
            ping: vec![false; 3],
            held: vec![false; 3],
        },
        "silenced, not held"
    );
}

/// `ping_window_secs = 30` holds the place to one ping: of three cards
/// inside 20 s the first pings, and the window holds the other two (each
/// row says `held`), set daemon-wide or by the place's own binding. So that
/// the cards are the place's first pings, its bind notice is silenced (a
/// silent write takes no ping from the window) and it shows no tool lines.
#[tokio::test]
async fn a_window_of_30_s_lets_one_of_three_cards_ping() {
    let once = Cards {
        silent: vec![false, true, true],
        ping: vec![true, false, false],
        held: vec![false, true, true],
    };
    let daemon = Words {
        silent: vec![Category::Notices],
        window_secs: 30,
        lab: "show_tools = false\n",
    };
    assert_eq!(three_cards(daemon).await, once);
    let place = Words {
        silent: vec![Category::Notices],
        window_secs: 0,
        lab: "show_tools = false\nping_window_secs = 30\n",
    };
    assert_eq!(three_cards(place).await, once, "the place's own window");
}
