//! A turn's steps folded into one message (issue 56), end to end through the
//! stand-in's gateway: a place that says nothing folds its turn's tool lines
//! and thinking into one closed line with a Show steps button, its answer its
//! own message; open, the fold is the lines a place bound `steps = "open"`
//! shows for the same turn, cut whole-line into pages; Hide steps closes it;
//! a press after the place started again opens it from the session's record;
//! a card and a failed turn are never in it.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_core::provider::{FakeProvider, ProviderError, Scripted};
use theseus_sim::fake_discord::{Guild, Msg, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};

const OPEN: &str = "steps = \"open\"\n";

/// `#lab`, bound private with `lab`'s words, and ana's DM with none.
fn bindings(lab: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n{lab}\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// The same file without `#lab`.
fn without_lab() -> String {
    format!("guild_id = \"{DEFAULT_GUILD}\"\n[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n")
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA])
}

/// Two loops of calls, two reads and a search, then the answer. The files are
/// the rig's, so two turns of it show the same lines.
fn steps(dir: &Path, word: &str) -> Vec<Scripted> {
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("chart.txt"), "low tide 06:12\n").unwrap();
    std::fs::write(work.join("notes.txt"), "the pier floods\n").unwrap();
    let path = |f: &str| work.join(f).to_string_lossy().into_owned();
    vec![
        Scripted::tools(
            "",
            &[
                (
                    &format!("r1_{word}"),
                    "fs_read",
                    json!({"path": path("chart.txt")}),
                ),
                (
                    &format!("r2_{word}"),
                    "fs_read",
                    json!({"path": path("notes.txt")}),
                ),
            ],
        ),
        Scripted::tools(
            "",
            &[(
                &format!("g1_{word}"),
                "fs_grep",
                json!({"pattern": "tide", "path": work.to_string_lossy()}),
            )],
        ),
        Scripted::text(&format!("Low tide by the {word} chart is at 06:12.")),
    ]
}

const CLOSED: &str = "-# 3 steps: 2 reads, 1 search";

fn labels(m: &Msg) -> Vec<String> {
    m.buttons.iter().map(|b| b.label.clone()).collect()
}

/// The fold in `channel`: the one message with a fold's button.
fn fold(r: &Rig, channel: u64) -> Option<Msg> {
    let folds: Vec<Msg> = r
        .posted(channel)
        .into_iter()
        .filter(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("fold:")))
        .collect();
    assert!(folds.len() <= 1, "one fold a turn: {folds:#?}");
    folds.into_iter().next()
}

/// A place's tool messages: today's lines, one message a loop.
fn tool_messages(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.content.starts_with("-# ") && m.content.contains("`fs."))
        .collect()
}

/// Durations to one word, so two runs of a turn read alike.
fn norm(s: &str) -> String {
    s.lines()
        .map(|l| {
            l.split(" · ")
                .map(|seg| match seg.strip_suffix(" ms") {
                    Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => "<ms>",
                    _ => seg,
                })
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ask(r: &Rig, channel: Option<u64>, word: &str) {
    r.say(
        (ANA, "ana"),
        channel,
        &format!("When is low tide by the {word} chart?"),
    );
}

fn answered(r: &Rig, channel: u64, word: &str) -> bool {
    r.posted(channel).iter().any(|m| {
        m.content
            .contains(&format!("the {word} chart is at 06:12."))
    }) && r.core.outbox.status("discord").pending == 0
}

/// The parts of the rows of the binding's creates that end with `end`.
fn parts(r: &Rig, end: &str) -> Vec<String> {
    r.ledger("discord.message.out")
        .iter()
        .filter_map(|d| d["part"].as_str().map(str::to_string))
        .filter(|p| p.ends_with(end))
        .collect()
}

/// A turn of several calls in a place that says nothing is one fold,
/// closed, whose line counts the calls by kind and holds none of what they
/// were given or returned, and one answer; no message of the turn's tool
/// lines was ever made, and the fold's create pings as a tool line's did.
#[tokio::test]
async fn a_turn_of_several_calls_is_one_closed_fold_and_one_answer() {
    let r = Rig::start_on(
        |dir, _| Arc::new(FakeProvider::scripted(steps(dir, "dm"))),
        guild(),
        &bindings(""),
        &[],
    )
    .await;
    ask(&r, None, "dm");
    r.until("the answer and the fold's last line", || {
        answered(&r, ANA_DM, "dm") && fold(&r, ANA_DM).is_some_and(|f| f.content == CLOSED)
    })
    .await;
    let dm = r.posted(ANA_DM);
    let f = fold(&r, ANA_DM).unwrap();
    assert_eq!(
        labels(&f).last().map(String::as_str),
        Some("Show steps"),
        "{f:#?}"
    );
    for v in &f.versions {
        assert!(v.starts_with("-# ") && !v.contains('\n'), "one line: {v:?}");
        for never in ["`fs.", "chart.txt", "06:12", "pier"] {
            assert!(!v.contains(never), "{never} in the closed line: {v:?}");
        }
    }
    assert!(!f.silent(), "it pings as the tool line did: {f:#?}");
    let answer = dm
        .iter()
        .find(|m| m.content.contains("the dm chart is at 06:12."))
        .unwrap();
    assert!(answer
        .buttons
        .iter()
        .all(|b| !b.custom_id.starts_with("fold:")));
    assert!(
        dm.iter()
            .all(|m| m.versions.iter().all(|v| !v.contains("`fs."))),
        "no tool line was ever shown: {dm:#?}"
    );
    r.until("the fold's row", || !parts(&r, ":fold").is_empty())
        .await;
    assert_eq!(parts(&r, ":fold").len(), 1);
    assert!(parts(&r, ":tools").is_empty(), "{:?}", parts(&r, ":tools"));
}

/// The same turn in `#lab`, bound open, and in the DM, folded: open, the
/// fold shows `#lab`'s lines word for word, with `#lab`'s choices; Hide steps
/// gives the closed line back; `#lab` writes today's messages and no fold;
/// and the fold cost the DM no more writes than `#lab`'s messages cost it.
#[tokio::test]
async fn open_shows_the_lines_an_open_place_shows_and_hide_closes_it() {
    let r = Rig::start_on(
        |dir, _| {
            let mut s = steps(dir, "lab");
            s.extend(steps(dir, "dm"));
            Arc::new(FakeProvider::scripted(s))
        },
        guild(),
        &bindings(OPEN),
        &[],
    )
    .await;
    ask(&r, Some(LAB), "lab");
    r.until("#lab's answer and its lines' last state", || {
        let tools = tool_messages(&r, LAB);
        answered(&r, LAB, "lab")
            && tools.len() == 2
            && tools
                .iter()
                .all(|m| !m.content.contains('⏳') && !m.content.contains('▫'))
    })
    .await;
    let lab = tool_messages(&r, LAB);
    let want = norm(
        &lab.iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    assert!(fold(&r, LAB).is_none(), "an open place makes no fold");
    r.until("#lab's rows", || parts(&r, ":tools").len() == 2)
        .await;
    assert!(parts(&r, ":fold").is_empty());
    let mut choices: Vec<String> = lab.iter().flat_map(labels).collect();
    choices.dedup();

    ask(&r, None, "dm");
    r.until("the DM's answer and fold", || {
        answered(&r, ANA_DM, "dm") && fold(&r, ANA_DM).is_some_and(|f| f.content == CLOSED)
    })
    .await;
    let f = fold(&r, ANA_DM).unwrap();
    let writes = |ms: &[Msg]| ms.iter().map(|m| 1 + m.edits as usize).sum::<usize>();
    assert!(
        writes(std::slice::from_ref(&f)) <= writes(&lab),
        "the fold {} writes, #lab's lines {}",
        writes(std::slice::from_ref(&f)),
        writes(&lab)
    );
    let closed = labels(&f);
    assert_eq!(
        closed[..closed.len() - 1],
        choices[..],
        "#lab's choices, closed too"
    );

    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("the fold open", || {
        fold(&r, ANA_DM).is_some_and(|f| f.content.contains("`fs.read`"))
    })
    .await;
    let opened = fold(&r, ANA_DM).unwrap();
    assert_eq!(norm(&opened.content), want, "#lab's lines, word for word");
    let open_labels = labels(&opened);
    assert_eq!(open_labels.last().map(String::as_str), Some("Hide steps"));
    assert_eq!(open_labels[..open_labels.len() - 1], choices[..]);

    r.press(&f.id, "Hide steps", (ANA, "ana"));
    r.until("the fold closed again", || {
        fold(&r, ANA_DM).is_some_and(|f| f.content == CLOSED)
    })
    .await;
    assert_eq!(
        labels(&fold(&r, ANA_DM).unwrap())
            .last()
            .map(String::as_str),
        Some("Show steps")
    );
}

/// A fold made before its place started again (the bindings file dropped
/// `#lab` and took it back: a new place, its renderer and lane empty, over
/// the same session) opens: its lines are read back from the session's
/// record, from the press's id alone.
#[tokio::test]
async fn a_fold_opens_after_its_place_starts_again() {
    let r = Rig::start_on(
        |dir, _| Arc::new(FakeProvider::scripted(steps(dir, "lab"))),
        guild(),
        &bindings(""),
        &[],
    )
    .await;
    ask(&r, Some(LAB), "lab");
    r.until("the answer and the fold", || {
        answered(&r, LAB, "lab") && fold(&r, LAB).is_some_and(|f| f.content == CLOSED)
    })
    .await;
    let f = fold(&r, LAB).unwrap();
    let labels_bound = || -> Vec<String> {
        r.core
            .bindings
            .all()
            .iter()
            .flat_map(|b| b.places.iter().map(|p| p.label.clone()))
            .collect()
    };
    let file = r.dir.path().join("bindings.toml");
    std::fs::write(&file, without_lab()).unwrap();
    r.until("#lab unbound", || !labels_bound().contains(&"#lab".into()))
        .await;
    std::fs::write(&file, bindings("")).unwrap();
    r.until("#lab bound again", || {
        labels_bound().contains(&"#lab".into())
    })
    .await;
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("the fold open after the restart", || {
        fold(&r, LAB)
            .is_some_and(|f| f.content.contains("`fs.read`") && f.content.contains("`fs.grep`"))
    })
    .await;
    let opened = fold(&r, LAB).unwrap();
    assert_eq!(opened.content.lines().count(), 3, "{opened:#?}");
    assert!(
        opened.content.lines().all(|l| l.starts_with("-# ✅ `fs.")),
        "{opened:#?}"
    );
}

/// Three loops of ten calls each: open, the fold is cut whole-line into
/// pages, each under Discord's limit and saying how many more there are,
/// and Later and Earlier move through them.
#[tokio::test]
async fn a_long_fold_is_cut_whole_line_into_pages() {
    let r = Rig::start_on(
        |dir, _| {
            let work = dir.join("work");
            std::fs::create_dir_all(&work).unwrap();
            let mut s = Vec::new();
            for l in 0..3 {
                let calls: Vec<(String, Value)> = (0..10)
                    .map(|i| {
                        let p = work.join(format!(
                            "a-rather-long-name-for-a-file-the-fold-pages-{l}{i}.txt"
                        ));
                        std::fs::write(&p, "x\n").unwrap();
                        (format!("r{l}{i}"), json!({"path": p.to_string_lossy()}))
                    })
                    .collect();
                let calls: Vec<(&str, &str, Value)> = calls
                    .iter()
                    .map(|(id, input)| (id.as_str(), "fs_read", input.clone()))
                    .collect();
                s.push(Scripted::tools("", &calls));
            }
            s.push(Scripted::text("Read all thirty."));
            Arc::new(FakeProvider::scripted(s))
        },
        guild(),
        &bindings(""),
        &[],
    )
    .await;
    r.say((ANA, "ana"), None, "Read the thirty files.");
    r.until("the answer and the fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Read all thirty."))
            && fold(&r, ANA_DM).is_some_and(|f| f.content == "-# 30 steps: 30 reads")
    })
    .await;
    let f = fold(&r, ANA_DM).unwrap();
    r.press(&f.id, "Show steps", (ANA, "ana"));
    r.until("page 1", || {
        fold(&r, ANA_DM).is_some_and(|f| f.content.contains(" · page 1 of "))
    })
    .await;
    let page = |n: usize| {
        let m = fold(&r, ANA_DM).unwrap();
        let (body, more) = m.content.rsplit_once('\n').unwrap();
        assert!(
            m.content.chars().count() <= 2000,
            "{} characters",
            m.content.chars().count()
        );
        assert!(
            more.starts_with("-# … and ") && more.contains(&format!(" · page {n} of ")),
            "{more}"
        );
        assert!(
            body.lines().all(|l| l.starts_with("-# ✅ `fs.read` ")),
            "whole lines: {body}"
        );
        (body.lines().count(), more.to_string(), labels(&m))
    };
    let (first, more, l) = page(1);
    assert!(first < 30 && !more.ends_with("page 1 of 1"), "{more}");
    assert_eq!(l.last().map(String::as_str), Some("Hide steps"));
    assert!(l.contains(&"Later ▶".to_string()) && !l.contains(&"◀ Earlier".to_string()));
    r.press(&f.id, "Later ▶", (ANA, "ana"));
    r.until("page 2", || {
        fold(&r, ANA_DM).is_some_and(|f| f.content.contains(" · page 2 of "))
    })
    .await;
    let (_, _, l) = page(2);
    assert!(l.contains(&"◀ Earlier".to_string()), "{l:?}");
    r.press(&f.id, "◀ Earlier", (ANA, "ana"));
    r.until("page 1 again", || {
        fold(&r, ANA_DM).is_some_and(|f| f.content.contains(" · page 1 of "))
    })
    .await;
}

/// A call that waits for approval: its card is its own message with its
/// buttons, as ever, and the fold's line says one waits. A turn that fails:
/// its failure is its own message, never in the fold.
#[tokio::test]
async fn a_card_and_a_failed_turn_are_never_in_the_fold() {
    let r = Rig::start_on(
        |dir, _| {
            let out = dir.join("outside").join("proof.txt");
            std::fs::create_dir_all(dir.join("work")).unwrap();
            std::fs::write(dir.join("work").join("chart.txt"), "low tide\n").unwrap();
            let chart = dir.join("work").join("chart.txt");
            Arc::new(FakeProvider::scripted(vec![
                Scripted::tools(
                    "Writing it.",
                    &[(
                        "w1",
                        "fs_write",
                        json!({"path": out.to_string_lossy(), "content": "x"}),
                    )],
                ),
                Scripted::tools(
                    "",
                    &[("r1", "fs_read", json!({"path": chart.to_string_lossy()}))],
                ),
                Scripted::Fail(ProviderError::InvalidRequest {
                    status: 400,
                    message: "the request was refused".into(),
                }),
            ]))
        },
        guild(),
        &bindings(""),
        &[],
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), "Write the proof.");
    r.until("the card and the fold that says it waits", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.versions[0].contains("**Approve?**"))
            && fold(&r, LAB).is_some_and(|f| f.content.contains("1 waiting for approval"))
    })
    .await;
    let card = r
        .posted(LAB)
        .into_iter()
        .find(|m| m.versions[0].contains("**Approve?**"))
        .unwrap();
    assert!(labels(&card).contains(&"Approve".to_string()), "{card:#?}");
    let f = fold(&r, LAB).unwrap();
    assert_eq!(f.content, "-# 1 step: 1 edit · 1 waiting for approval");
    assert!(!f.content.contains("Approve?"));

    r.say((ANA, "ana"), None, "Read the chart.");
    r.until("the failure and the fold", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Turn failed"))
            && fold(&r, ANA_DM).is_some_and(|f| f.content == "-# 1 step: 1 read")
    })
    .await;
    let failed = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("Turn failed"))
        .unwrap();
    assert!(failed
        .buttons
        .iter()
        .all(|b| !b.custom_id.starts_with("fold:")));
    let f = fold(&r, ANA_DM).unwrap();
    assert!(
        f.versions.iter().all(|v| !v.contains("Turn failed")),
        "{f:#?}"
    );
}

/// A turn that thinks: its fold says how long, and its create is silent, as
/// a process message that holds thinking is (the turn buzzes for its answer).
#[tokio::test]
async fn a_thinking_turns_fold_says_so_and_is_silent() {
    let r = Rig::start_on(
        |dir, _| {
            let chart = dir.join("work").join("chart.txt");
            std::fs::create_dir_all(dir.join("work")).unwrap();
            std::fs::write(&chart, "low tide 06:12\n").unwrap();
            Arc::new(FakeProvider::scripted(vec![
                Scripted::Blocks {
                    blocks: vec![
                        json!({"type": "thinking", "thinking": "The chart is in work/.", "signature": "sig"}),
                        json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": chart.to_string_lossy()}}),
                    ],
                    stop_reason: "tool_use".into(),
                },
                Scripted::text("Low tide by the dm chart is at 06:12."),
            ]))
        },
        guild(),
        &bindings(""),
        &[],
    )
    .await;
    ask(&r, None, "dm");
    r.until("the answer and the fold", || {
        answered(&r, ANA_DM, "dm")
            && fold(&r, ANA_DM).is_some_and(|f| {
                f.content.starts_with("-# 💭 thought for ")
                    && f.content.ends_with(" · 1 step: 1 read")
            })
    })
    .await;
    let f = fold(&r, ANA_DM).unwrap();
    assert!(f.silent(), "{f:#?}");
    assert!(
        f.versions.iter().all(|v| !v.contains("chart is in work")),
        "{f:#?}"
    );
}

/// A fold whose turn this place cannot read back (no such turn) tells the
/// presser alone, in one line, and leaves the message as it was.
#[tokio::test]
async fn a_fold_too_old_to_read_back_says_so_to_the_presser() {
    let r = Rig::start_on(
        |_, _| Arc::new(FakeProvider::scripted(vec![])),
        guild(),
        &bindings(""),
        &[],
    )
    .await;
    let url = format!("http://{}/api/v10/channels/{ANA_DM}/messages", r.fake.addr);
    let posted: Value = reqwest::Client::new()
        .post(url)
        .json(&json!({"content": "-# 2 steps: 2 reads", "components": [{"type": 1, "components": [
            {"type": 2, "style": 2, "custom_id": "fold:o:0:turn_long_gone", "label": "Show steps"}]}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = posted["id"].as_str().unwrap().to_string();
    let pressed = r.press(&id, "Show steps", (ANA, "ana"));
    let too_old = "These steps are too old to show again";
    r.until("the presser told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.ephemeral
                && x.content.as_deref().is_some_and(|c| c.starts_with(too_old))
        })
    })
    .await;
    let m = r
        .fake
        .messages(ANA_DM)
        .into_iter()
        .find(|m| m.id == id)
        .unwrap();
    assert_eq!(m.content, "-# 2 steps: 2 reads");
}

/// An FYI note, as the core stages one to the operator lane: its correlation id.
fn stage_note(r: &Rig, body: Value) -> String {
    let (a, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    a.correlation_id
}

fn fyi(words: &str) -> Value {
    use theseus_core::owner_note::{note, Kind};
    json!({"kind": "notice", "text": note(Kind::Fyi, "Personas moved", "a start", words)})
}

/// The bot's DM message whose first version holds `s`.
fn dm_with(r: &Rig, s: &str) -> Option<Msg> {
    r.posted(ANA_DM).into_iter().find(|m| m.versions[0].contains(s))
}

/// Ana owns the rig; `dm` is her DM's extra lines, `notices` binds a notices
/// channel.
async fn note_rig(dm: &str, notices: bool) -> Rig {
    let more = if notices {
        format!(
            "[[channel]]\nid = \"{NOTICES}\"\nname = \"notices\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\nnotify = true\n"
        )
    } else {
        String::new()
    };
    let file = format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         {more}[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n{dm}"
    );
    let guild = guild().private_channel(NOTICES, "notices", &[ANA]);
    let r = Rig::start_tweaked(
        |_, _| Arc::new(FakeProvider::scripted(vec![])),
        guild,
        &file,
        &[],
        |c| c.places.owner = Some(vec![format!("discord:{ANA}")]),
    )
    .await;
    // The bind notice opens the DM, where the operator lane writes.
    r.until("the DM's bind notice", || !r.posted(ANA_DM).is_empty())
        .await;
    r
}

/// The notices channel, an invented id.
const NOTICES: u64 = 900_000_000_000_000_040;

/// A standalone FYI that stays in the DM is one closed line, said from its
/// kind, with Show note; open, it is the note's words as the DM got them
/// before; Hide note gives the line back.
#[tokio::test]
async fn a_standalone_fyi_in_the_dm_is_one_closed_line_that_opens() {
    let r = note_rig("", false).await;
    let words = "The persona files moved into the ontology.";
    stage_note(&r, fyi(words));
    r.until("the folded note", || dm_with(&r, "-# ℹ️ FYI: a note").is_some())
        .await;
    let m = dm_with(&r, "-# ℹ️ FYI: a note").unwrap();
    assert_eq!(m.content, "-# ℹ️ FYI: a note");
    assert_eq!(labels(&m), ["Show note"]);
    assert!(
        r.posted(ANA_DM).iter().all(|x| x.versions.iter().all(|v| !v.contains(words))),
        "the words were never shown closed"
    );
    r.press(&m.id, "Show note", (ANA, "ana"));
    r.until("the note open", || {
        dm_with(&r, "-# ℹ️ FYI: a note").is_some_and(|m| m.content.contains(words))
    })
    .await;
    let open = dm_with(&r, "-# ℹ️ FYI: a note").unwrap();
    use theseus_core::owner_note::{note, Kind};
    assert_eq!(open.content, note(Kind::Fyi, "Personas moved", "a start", words));
    assert_eq!(labels(&open), ["Hide note"]);
    r.press(&m.id, "Hide note", (ANA, "ana"));
    r.until("the note closed", || {
        dm_with(&r, "-# ℹ️ FYI: a note").is_some_and(|m| m.content == "-# ℹ️ FYI: a note")
    })
    .await;
}

/// A DM bound `steps = "open"` gets the note's words as before; a note the
/// notices place takes is its words there, never folded.
#[tokio::test]
async fn an_open_dm_and_the_notices_place_get_the_note_as_before() {
    use theseus_core::owner_note::{note, Kind};
    let r = note_rig(OPEN, false).await;
    let words = "Open words.";
    stage_note(&r, fyi(words));
    r.until("the note's words", || dm_with(&r, words).is_some())
        .await;
    let m = dm_with(&r, words).unwrap();
    assert_eq!(m.content, note(Kind::Fyi, "Personas moved", "a start", words));
    assert!(m.buttons.is_empty(), "{m:#?}");
    r.stop();

    let r = note_rig("", true).await;
    let words = "Words for the notices place.";
    stage_note(&r, fyi(words));
    r.until("the note in the notices place", || {
        r.posted(NOTICES).iter().any(|m| m.content.contains(words))
    })
    .await;
    let m = r
        .posted(NOTICES)
        .into_iter()
        .find(|m| m.content.contains(words))
        .unwrap();
    assert!(m.buttons.iter().all(|b| !b.custom_id.starts_with("fold:")), "{m:#?}");
}

/// What needs the owner, and a note with a button of its own, are written as
/// ever in a folded DM: a question with its buttons, an alert, and the
/// ontology's mint with its Undo.
#[tokio::test]
async fn a_question_an_alert_and_a_note_with_a_button_are_never_folded() {
    use theseus_core::owner_note::{note, Kind};
    let r = note_rig("", false).await;
    stage_note(
        &r,
        json!({"kind": "card_question", "card": {"id": "cdp_00000001", "ask": "add"},
               "text": note(Kind::Decision, "Keep this as a fact?", "a card waits on you", "The quay floods.")}),
    );
    stage_note(
        &r,
        json!({"kind": "hands_alert", "text": note(Kind::Alert, "A hand ran away", "its spend", "Spent a lot.")}),
    );
    stage_note(
        &r,
        json!({"kind": "onto_card", "write": "jdg_mint", "undo": true,
               "text": note(Kind::Fyi, "Theseus started keeping track of something new", "In pier", "The tide table.")}),
    );
    for words in ["The quay floods.", "Spent a lot.", "The tide table."] {
        r.until(words, || dm_with(&r, words).is_some()).await;
    }
    assert!(!labels(&dm_with(&r, "The quay floods.").unwrap()).is_empty());
    assert!(!labels(&dm_with(&r, "The tide table.").unwrap()).is_empty());
    assert!(
        r.posted(ANA_DM)
            .iter()
            .all(|m| m.buttons.iter().all(|b| !b.custom_id.starts_with("fold:"))),
        "nothing folded"
    );
}

/// A Show note button forged on another message, naming a real post, shows
/// nothing of that post: the presser is told, and the message is unchanged.
#[tokio::test]
async fn a_forged_note_press_shows_nothing() {
    let r = note_rig("", false).await;
    let words = "Words only their own message may show.";
    let post = stage_note(&r, fyi(words));
    r.until("the folded note", || dm_with(&r, "-# ℹ️ FYI: a note").is_some())
        .await;
    let url = format!("http://{}/api/v10/channels/{ANA_DM}/messages", r.fake.addr);
    let posted: Value = reqwest::Client::new()
        .post(url)
        .json(&json!({"content": "-# ℹ️ FYI: a note", "components": [{"type": 1, "components": [
            {"type": 2, "style": 2, "custom_id": format!("fold:no:{post}"), "label": "Show note"}]}]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = posted["id"].as_str().unwrap().to_string();
    let pressed = r.press(&id, "Show note", (ANA, "ana"));
    r.until("the presser told", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.ephemeral
                && x.content.as_deref() == Some(crate::runtime::fold::NOTE_GONE)
        })
    })
    .await;
    let m = r.fake.messages(ANA_DM).into_iter().find(|m| m.id == id).unwrap();
    assert!(!m.content.contains(words), "{m:#?}");
}
