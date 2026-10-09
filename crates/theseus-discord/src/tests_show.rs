//! What a place shows (theseus-l1y1), end to end through the stand-in's
//! gateway: its tool lines and its thinking, each on unless its binding says
//! `show_tools = false` or `show_thinking = false`, and off in that place
//! alone. The same turn's shape runs twice, once in `#lab`, which shows
//! neither, and once in ana's DM, which says nothing and shows both.

use std::path::Path;
use std::sync::Arc;

use theseus_core::provider::{FakeProvider, Scripted};
use theseus_sim::fake_discord::{Guild, Msg, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};

/// `#lab`, bound private with `lab`'s words, and ana's DM with none.
fn bindings(lab: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n{lab}\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// A loop that thinks, says a line, and reads the chart; then the answer.
fn turn(dir: &Path, word: &str) -> Vec<Scripted> {
    let chart = dir.join("work").join("chart.txt");
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::write(&chart, "low tide 06:12\n").unwrap();
    vec![
        Scripted::Blocks {
            blocks: vec![
                serde_json::json!({"type": "thinking", "thinking": format!("The {word} chart is in work/."), "signature": "sig"}),
                serde_json::json!({"type": "text", "text": format!("Reading the {word} chart.")}),
                serde_json::json!({"type": "tool_use", "id": format!("t_{word}"), "name": "fs_read",
                    "input": {"path": chart.to_string_lossy()}}),
            ],
            stop_reason: "tool_use".into(),
        },
        Scripted::text(&format!("Low tide by the {word} chart is at 06:12.")),
    ]
}

/// One exchange in `channel` (None: the DM), settled with its tool line
/// when `tools` is to show, and its messages there.
async fn exchange(r: &Rig, channel: Option<u64>, word: &str, tools: bool) -> Vec<Msg> {
    let at = channel.unwrap_or(ANA_DM);
    r.say(
        (ANA, "ana"),
        channel,
        &format!("When is low tide by the {word} chart?"),
    );
    r.until(&format!("the {word} exchange settled"), || {
        let got = r.posted(at);
        got.iter().any(|m| {
            m.content
                .contains(&format!("the {word} chart is at 06:12."))
        }) && (!tools || got.iter().any(|m| m.content.contains("fs.read")))
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    r.posted(at)
}

fn has(got: &[Msg], s: &str) -> bool {
    got.iter().any(|m| m.versions.iter().any(|v| v.contains(s)))
}

/// `#lab` shows neither tool lines nor thinking; the DM, saying nothing,
/// shows both; the turn in `#lab` runs as ever (its call read the chart,
/// and its reply posted).
#[tokio::test]
async fn a_place_that_hides_tools_and_thinking_hides_them_alone() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = turn(dir, "lab");
            s.extend(turn(dir, "dm"));
            Arc::new(FakeProvider::scripted(s))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings("show_tools = false\nshow_thinking = false\n"),
        &[],
    )
    .await;
    let lab = exchange(&r, Some(LAB), "lab", false).await;
    assert!(has(&lab, "Reading the lab chart."), "{lab:#?}");
    assert!(!has(&lab, "fs.read"), "no tool line in #lab: {lab:#?}");
    assert!(!has(&lab, "💭"), "no thinking in #lab: {lab:#?}");
    let dm = exchange(&r, None, "dm", true).await;
    assert!(has(&dm, "fs.read"), "the DM's tool line: {dm:#?}");
    let think = dm
        .iter()
        .find(|m| m.content.contains("💭 thinking"))
        .unwrap_or_else(|| panic!("the DM's thinking: {dm:#?}"));
    assert!(
        think.content.contains("The dm chart is in work/."),
        "{think:?}"
    );
    assert!(
        !think.silent(),
        "it notifies, as every message does by default"
    );
    // The thinking came before the loop's text.
    let at = |s: &str| dm.iter().position(|m| m.content.contains(s)).unwrap();
    assert!(at("💭 thinking") < at("Reading the dm chart."), "{dm:#?}");
    // Nothing of the lab's thinking or tools was made at all.
    // (Each create's row is written soon after it, off the lane's path.)
    let shown = || -> Vec<String> {
        r.ledger("discord.message.out")
            .iter()
            .filter_map(|d| d["part"].as_str().map(str::to_string))
            .filter(|p| p.ends_with(":tools") || p.ends_with(":think"))
            .collect()
    };
    r.until("the DM's two rows", || shown().len() >= 2).await;
    assert_eq!(shown().len(), 2, "the DM's two alone: {:?}", shown());
}

/// The words are read live with the rest of the file (theseus-ocwt): `#lab`
/// shows its tool line and thinking, then the file is rewritten to hide
/// both, and its next turn shows neither, with no restart.
#[tokio::test]
async fn a_rewritten_file_changes_what_a_place_shows() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = turn(dir, "first");
            s.extend(turn(dir, "second"));
            Arc::new(FakeProvider::scripted(s))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings(""),
        &[],
    )
    .await;
    let got = exchange(&r, Some(LAB), "first", true).await;
    assert!(has(&got, "💭 thinking") && has(&got, "fs.read"), "{got:#?}");
    let hidden = bindings("show_tools = false\nshow_thinking = false\n");
    let revision = crate::bindings::Bindings::parse(&hidden).unwrap().revision;
    std::fs::write(r.dir.path().join("bindings.toml"), &hidden).unwrap();
    r.until("the change is bound", || {
        r.core
            .bindings
            .all()
            .first()
            .and_then(|b| b.revision.clone())
            == Some(revision.clone())
    })
    .await;
    let before = got.len();
    let got = exchange(&r, Some(LAB), "second", false).await;
    let new: Vec<&Msg> = got[before..].iter().collect();
    assert!(
        new.iter()
            .any(|m| m.content.contains("Reading the second chart.")),
        "{new:#?}"
    );
    assert!(
        !new.iter()
            .any(|m| m.content.contains("💭") || m.content.contains("fs.read")),
        "the second turn shows neither: {new:#?}"
    );
}

/// A place's `silent` list is read live too (theseus-l1y1): `#lab`'s first
/// turn pings its thinking, its tool line and its answer; the file is
/// rewritten with `silent = ["tool_lines", "thinking"]`, and the next turn's
/// thinking and tool line post silent, its answer still pinging, with no
/// restart.
#[tokio::test]
async fn a_rewritten_file_changes_what_a_place_silences() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = turn(dir, "first");
            s.extend(turn(dir, "second"));
            Arc::new(FakeProvider::scripted(s))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings(""),
        &[],
    )
    .await;
    let silent = |got: &[Msg], s: &str| -> bool {
        got.iter()
            .find(|m| m.content.contains(s))
            .unwrap_or_else(|| panic!("no {s:?}: {got:#?}"))
            .silent()
    };
    let got = exchange(&r, Some(LAB), "first", true).await;
    for s in ["💭 thinking", "fs.read", "Reading the first chart."] {
        assert!(!silent(&got, s), "{s} pings with no config");
    }
    let quiet = bindings("silent = [\"tool_lines\", \"thinking\"]\n");
    let revision = crate::bindings::Bindings::parse(&quiet).unwrap().revision;
    std::fs::write(r.dir.path().join("bindings.toml"), &quiet).unwrap();
    r.until("the change is bound", || {
        r.core
            .bindings
            .all()
            .first()
            .and_then(|b| b.revision.clone())
            == Some(revision.clone())
    })
    .await;
    let before = got.len();
    exchange(&r, Some(LAB), "second", true).await;
    // The exchange's wait sees the first turn's tool line: wait for this one.
    let lines = || {
        r.posted(LAB)
            .iter()
            .filter(|m| m.content.contains("fs.read"))
            .count()
    };
    r.until("the second tool line", || lines() == 2).await;
    let got = r.posted(LAB);
    let new = &got[before..];
    assert!(silent(new, "💭 thinking"), "{new:#?}");
    assert!(silent(new, "fs.read"), "{new:#?}");
    assert!(
        !silent(new, "Reading the second chart."),
        "the answer pings"
    );
}
