//! A loop's thinking in its process message (theseus-l1y1), end to end
//! through the stand-in's gateway: the thinking at the top of the loop's
//! tool line message, folded to `💭 thought for N s` once the loop's text
//! starts, never a message of its own; a thinking turn buzzes for its
//! answer alone (every process message holding thinking is silent, its tool
//! lines with it: the owner's call on the fold, 2026-10-10), and makes one
//! create more only for its last loop, which thinks and answers (a thinking
//! turn of one loop: silent, and its answer pings).

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

fn thought(text: &str) -> serde_json::Value {
    serde_json::json!({"type": "thinking", "thinking": text, "signature": "sig"})
}

/// Two loops that each read a chart, then the answer; each loop thinks
/// first when `think`.
fn turn(dir: &Path, word: &str, think: bool) -> Vec<Scripted> {
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let mut out = Vec::new();
    for i in 0..2 {
        let chart = dir.join("work").join(format!("{word}{i}.txt"));
        std::fs::write(&chart, "low tide 06:12\n").unwrap();
        let mut blocks = Vec::new();
        if think {
            blocks.push(thought(&format!(
                "Chart {i} of the {word} pair is in work/."
            )));
        }
        blocks.push(
            serde_json::json!({"type": "tool_use", "id": format!("t_{word}{i}"), "name": "fs_read",
            "input": {"path": chart.to_string_lossy()}}),
        );
        out.push(Scripted::Blocks {
            blocks,
            stop_reason: "tool_use".into(),
        });
    }
    let answer = serde_json::json!({"type": "text", "text": format!("Low tide by the {word} charts is at 06:12.")});
    let blocks = if think {
        vec![thought("Both say 06:12."), answer]
    } else {
        vec![answer]
    };
    out.push(Scripted::Blocks {
        blocks,
        stop_reason: "end_turn".into(),
    });
    out
}

/// One exchange in `channel` (None: the DM), settled with its `tools` tool
/// lines done, its `folds` thinkings folded, its answer posted and its rows
/// written; the messages it made.
async fn exchange(
    r: &Rig,
    channel: Option<u64>,
    word: &str,
    tools: usize,
    folds: usize,
) -> Vec<Msg> {
    let at = channel.unwrap_or(ANA_DM);
    let before = r.posted(at).len();
    r.say(
        (ANA, "ana"),
        channel,
        &format!("When is low tide by the {word} charts?"),
    );
    let rows = || {
        r.ledger("discord.message.out")
            .iter()
            .filter(|d| d["part"].as_str().is_some_and(|p| p.ends_with(":tools")))
            .count()
    };
    let rows_before = rows();
    r.until(&format!("the {word} exchange settled"), || {
        let got = r.posted(at);
        let new = &got[before.min(got.len())..];
        new.iter().any(|m| {
            m.content
                .contains(&format!("the {word} charts is at 06:12."))
        }) && new
            .iter()
            .filter(|m| m.content.contains("✅ `fs.read`"))
            .count()
            == tools
            && new
                .iter()
                .filter(|m| m.content.starts_with("-# 💭 thought for"))
                .count()
                == folds
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    r.until("its process rows", || {
        rows() >= rows_before + tools.max(folds)
    })
    .await;
    r.posted(at)[before..].to_vec()
}

fn pings(got: &[Msg]) -> usize {
    got.iter().filter(|m| !m.silent()).count()
}

/// The fold, against the same turn without thinking (main's messages, as a
/// model that does not think makes them): the thinking is at the top of
/// each loop's tool line message and folds to one line; no message holds
/// thinking of its own; every process message is silent, so the answer is
/// the turn's one buzz where the turn without thinking buzzes for each tool
/// line too; the creates are the same but for the last loop's thinking, one
/// silent create; and the answer's message holds no thinking.
#[tokio::test]
async fn a_thinking_turn_buzzes_for_its_answer_alone_and_folds_its_thinking() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = turn(dir, "plain", false);
            s.extend(turn(dir, "deep", true));
            Arc::new(FakeProvider::scripted(s))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings(""),
        &[],
    )
    .await;
    let plain = exchange(&r, Some(LAB), "plain", 2, 0).await;
    assert!(!plain
        .iter()
        .any(|m| m.versions.iter().any(|v| v.contains("💭"))));
    assert_eq!((plain.len(), pings(&plain)), (3, 3), "{plain:#?}");
    let deep = exchange(&r, Some(LAB), "deep", 2, 3).await;
    assert_eq!(deep.len(), plain.len() + 1, "one create more: {deep:#?}");
    assert_eq!(pings(&deep), 1, "the answer alone: {deep:#?}");
    // Every message with thinking is a process message: its thinking on top.
    let thinking: Vec<&Msg> = deep
        .iter()
        .filter(|m| m.versions.iter().any(|v| v.contains("💭")))
        .collect();
    assert_eq!(thinking.len(), 3, "{deep:#?}");
    for m in &thinking {
        assert!(m.silent(), "a process message with thinking: {m:#?}");
        assert!(m.versions.iter().all(|v| v.starts_with("-# 💭 ")), "{m:#?}");
        assert!(
            m.content.starts_with("-# 💭 thought for "),
            "folded: {m:#?}"
        );
    }
    // Loops 0 and 1: the fold, then the tool line, kept.
    for m in &thinking[..2] {
        let lines: Vec<&str> = m.content.lines().collect();
        assert_eq!(lines.len(), 2, "{m:#?}");
        assert!(lines[1].starts_with("✅ `fs.read`"), "{m:#?}");
    }
    // The last loop's: its thinking alone.
    assert_eq!(thinking[2].content.lines().count(), 1, "{:#?}", thinking[2]);
    let answer = deep
        .iter()
        .find(|m| m.content.contains("the deep charts"))
        .unwrap();
    assert!(!answer.silent(), "{answer:#?}");
    assert!(!answer
        .versions
        .iter()
        .any(|v| v.contains("💭") || v.contains("Both say")));
    // Where the last loop's message lands against the answer is best
    // effort, as a tool line's always was: a thinking-made create goes before
    // the lane's next post (`Lane::take`), but under load the reply's post can
    // reach the lane before the place has sent the thinking at all.
    // No key of its own: every row with thinking is a process message's.
    for d in r.ledger("discord.message.out") {
        let part = d["part"].as_str().unwrap_or_default();
        assert!(!part.ends_with(":think"), "{d}");
    }
}

/// A turn that only thinks and answers: its process message, created
/// silent, then the answer, which pings.
#[tokio::test]
async fn a_thinking_answer_is_silent_and_its_answer_pings() {
    let r = Rig::start_on(
        |_, _| {
            Arc::new(FakeProvider::scripted(vec![Scripted::Blocks {
                blocks: vec![
                    thought("The chart says 06:12."),
                    serde_json::json!({"type": "text", "text": "Low tide by the lone chart is at 06:12."}),
                ],
                stop_reason: "end_turn".into(),
            }]))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings(""),
        &[],
    )
    .await;
    r.say(
        (ANA, "ana"),
        Some(LAB),
        "When is low tide by the lone chart?",
    );
    r.until("the answer and the fold", || {
        let got = r.posted(LAB);
        got.iter()
            .any(|m| m.content.contains("lone chart is at 06:12."))
            && got
                .iter()
                .any(|m| m.content.starts_with("-# 💭 thought for"))
    })
    .await;
    let got = r.posted(LAB);
    let think = got.iter().find(|m| m.content.starts_with("-# 💭")).unwrap();
    assert!(think.silent(), "{got:#?}");
    let answer = got
        .iter()
        .find(|m| m.content.contains("lone chart"))
        .unwrap();
    assert!(!answer.silent(), "{got:#?}");
    assert!(!answer.content.contains("💭"));
}

/// `show_thinking = false` in `#lab` shows no thinking there, and its tool
/// lines make and ping the messages they make without thinking; the DM,
/// saying nothing, shows the same turn's thinking.
#[tokio::test]
async fn a_place_that_hides_thinking_shows_its_tool_lines_as_before() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = turn(dir, "lab", true);
            s.extend(turn(dir, "dm", true));
            Arc::new(FakeProvider::scripted(s))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings("show_thinking = false\n"),
        &[],
    )
    .await;
    let lab = exchange(&r, Some(LAB), "lab", 2, 0).await;
    assert!(
        !lab.iter()
            .any(|m| m.versions.iter().any(|v| v.contains("💭"))),
        "{lab:#?}"
    );
    assert_eq!((lab.len(), pings(&lab)), (3, 3), "{lab:#?}");
    let dm = exchange(&r, None, "dm", 2, 3).await;
    let folded = dm
        .iter()
        .filter(|m| m.content.starts_with("-# 💭 thought for"))
        .count();
    assert_eq!(folded, 3, "{dm:#?}");
    assert_eq!((dm.len(), pings(&dm)), (4, 1), "{dm:#?}");
}

/// `silent = ["tool_lines"]` in `#lab` silences its tool lines there: a
/// thinking turn's process messages are silent anyway, and a turn without
/// thinking pings for its answer alone too. The DM, saying nothing, pings
/// for each tool line and the answer, as main does.
#[tokio::test]
async fn a_place_that_silences_tool_lines_silences_its_process_messages_alone() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = turn(dir, "lab", true);
            s.extend(turn(dir, "calm", false));
            s.extend(turn(dir, "dm", false));
            Arc::new(FakeProvider::scripted(s))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings("silent = [\"tool_lines\"]\n"),
        &[],
    )
    .await;
    let lab = exchange(&r, Some(LAB), "lab", 2, 3).await;
    assert_eq!((lab.len(), pings(&lab)), (4, 1), "{lab:#?}");
    for m in lab.iter().filter(|m| m.content.starts_with("-# 💭")) {
        assert!(m.silent(), "{m:#?}");
    }
    let answer = lab
        .iter()
        .find(|m| m.content.contains("the lab charts"))
        .unwrap();
    assert!(!answer.silent(), "{answer:#?}");
    let calm = exchange(&r, Some(LAB), "calm", 2, 0).await;
    assert_eq!((calm.len(), pings(&calm)), (3, 1), "{calm:#?}");
    let dm = exchange(&r, None, "dm", 2, 0).await;
    assert_eq!((dm.len(), pings(&dm)), (3, 3), "{dm:#?}");
}

/// A thinking answer whose text the stream had not written when the reply's
/// post came (its process message's create answered late) is still the
/// answer: the post's first part takes the answer's category, not a later
/// part's, though the stream wrote a key of the turn (its thinking). With
/// `silent = ["later_parts"]` it pings.
#[tokio::test]
async fn an_answer_the_post_writes_after_the_thinking_is_still_the_answer() {
    let r = Rig::start_on(
        |_, _| {
            Arc::new(FakeProvider::scripted(vec![Scripted::Blocks {
                blocks: vec![
                    thought("The chart says 06:12."),
                    serde_json::json!({"type": "text", "text": "Low tide by the held chart is at 06:12."}),
                ],
                stop_reason: "end_turn".into(),
            }]))
        },
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings("silent = [\"later_parts\"]\n"),
        &[],
    )
    .await;
    // The lane waits on the thinking's create while the turn ends.
    r.fake.hold_writes_containing(Some("💭 thinking"));
    r.say(
        (ANA, "ana"),
        Some(LAB),
        "When is low tide by the held chart?",
    );
    r.until("the thinking's create, held, and the reply's post", || {
        r.posted(LAB).iter().any(|m| m.content.starts_with("-# 💭"))
            && r.core.outbox.status("discord").pending > 0
    })
    .await;
    r.fake.hold_writes_containing(None);
    r.until("the answer and the fold", || {
        let got = r.posted(LAB);
        got.iter()
            .any(|m| m.content.contains("held chart is at 06:12."))
            && got
                .iter()
                .any(|m| m.content.starts_with("-# 💭 thought for"))
            && r.core.outbox.status("discord").pending == 0
    })
    .await;
    let got = r.posted(LAB);
    let answer = got
        .iter()
        .find(|m| m.content.contains("held chart"))
        .unwrap();
    assert!(!answer.silent(), "the answer pings: {got:#?}");
}
