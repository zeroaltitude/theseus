//! The whole Discord DM task flow in one golden (theseus-hd8l2): the owner asks
//! in the DM, the task starts, asks for approval in its forum post, the owner
//! presses Approve, the task reports, and the board shows it done. Every
//! message the fake saw is written down in the order each place shows it, with
//! its buttons, and compared with `golden/dm_flow.txt`. The flow is locked by
//! the owner: a change to it needs the owner's OK first.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};
use theseus_core::provider::Scripted;
use theseus_sim::fake_discord::{FakeThread, Msg, BOT_ID};

use crate::render::board_v2;
use crate::tests_default_forum::{bindings, forum_up, guild, posts, rig_in};
use crate::tests_gateway::{Rig, ANA, ANA_DM};
use crate::tests_task_threads::{gate, open_gate, replied, start, until, ASK, REEF};

const V2: u64 = 1 << 15;
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/golden/dm_flow.txt");
const HEADER: &str = "\
# The Discord DM task flow, locked by the owner on 2026-10-22. Change this file only with the
# owner's OK (`THESEUS_GOLDEN=write cargo nextest run -p theseus-discord dm_flow`).
# Each place lists its messages in the order it shows them: its text when the test ended, its buttons as `[label (style) id]`, and its flags. Ids (a task's, a question's,
# a session's), times, durations and the temp path are normalized (see `Names`); everything else is as the fake Discord saw it.
# The DM's lines after the owner's message arrive in a different order from run to run, so they are sorted.
";

/// The task: it waits at the gate, then writes outside its tree, so it asks.
fn asks_to_write(dir: &Path) -> Vec<Scripted> {
    let path = dir.join("outside").join("reef.txt");
    vec![
        gate(dir),
        Scripted::tools(
            "",
            &[(
                "w1",
                "fs_write",
                json!({"path": path.to_string_lossy(), "content": "reef"}),
            )],
        ),
        Scripted::text("Written."),
    ]
}

/// The task home stands and the core was told (as `tests_task_home` waits).
async fn home_up(r: &Rig) -> u64 {
    let dm = format!("discord:dm:{ANA}");
    let record = || {
        let v: Option<Value> = r.core.store.get_meta("discord.task_home").ok().flatten();
        v.and_then(|v| v["channel"].as_u64())
    };
    until(r, "the task home stands", || {
        record().is_some() && r.core.outbox.report_target(Some(dm.clone())).1.is_some()
    })
    .await;
    record().unwrap()
}

/// The bot's V2 messages in `channel` with this button.
fn with_button(r: &Rig, channel: u64, prefix: &str) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.buttons.iter().any(|b| b.custom_id.starts_with(prefix)))
        .collect()
}

/// Ids and times to stable names, in the order they first appear.
struct Names {
    known: BTreeMap<String, String>,
    tmp: String,
    task: String,
    next: usize,
}

impl Names {
    fn name(&mut self, id: impl ToString, as_: &str) {
        self.known.insert(id.to_string(), as_.to_string());
    }

    /// Long digit runs (snowflakes) become their name, or `<id-N>`; `<t:..>`
    /// becomes `<time>`; the rig's temp directory becomes `<tmp>`.
    fn clean(&mut self, text: &str) -> String {
        let text = text
            .replace(&self.tmp, "<tmp>")
            .replace(&self.task, "task-1");
        let mut out = String::new();
        let mut rest = text.as_str();
        'scan: while !rest.is_empty() {
            if let Some(tail) = rest.strip_prefix("<t:") {
                if let Some(end) = tail.find('>') {
                    out.push_str("<time>");
                    rest = &tail[end + 1..];
                    continue;
                }
            }
            if let Some(tail) = rest.strip_prefix("wa:") {
                if tail.as_bytes().get(6) == Some(&b':') {
                    out.push_str("wa:<question>");
                    rest = &tail[6..];
                    continue;
                }
            }
            for pre in ["ses_", "turn_", "exe_"] {
                if let Some(tail) = rest.strip_prefix(pre) {
                    let hex = tail.bytes().take_while(u8::is_ascii_hexdigit).count();
                    if hex == 32 {
                        out.push_str(&format!("<{}>", pre.trim_end_matches('_')));
                        rest = &tail[hex..];
                        continue 'scan;
                    }
                }
            }
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && !out.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '.') {
                let frac = rest[digits..]
                    .strip_prefix('.')
                    .map_or(0, |f| 1 + f.bytes().take_while(u8::is_ascii_digit).count());
                let after = &rest[digits + frac..];
                for unit in [" ms", " s"] {
                    if let Some(t) = after.strip_prefix(unit) {
                        if !t.starts_with(|c: char| c.is_alphanumeric()) {
                            out.push_str("<dur>");
                            rest = t;
                            continue 'scan;
                        }
                    }
                }
            }
            if digits >= 15 {
                let id = &rest[..digits];
                let next = &mut self.next;
                let n = self.known.entry(id.to_string()).or_insert_with(|| {
                    *next += 1;
                    format!("id-{next}")
                });
                out.push_str(&format!("<{n}>"));
                rest = &rest[digits..];
                continue;
            }
            let c = rest.chars().next().unwrap();
            if digits > 0 {
                out.push_str(&rest[..digits]);
                rest = &rest[digits..];
            } else {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        out
    }
}

/// A button's style, from the components as the binding sent them.
fn styles(m: &Msg) -> BTreeMap<String, u64> {
    fn walk(v: &Value, out: &mut BTreeMap<String, u64>) {
        match v {
            Value::Array(a) => a.iter().for_each(|c| walk(c, out)),
            Value::Object(o) => {
                if o.get("type").and_then(Value::as_u64) == Some(2) {
                    let id = o.get("custom_id").and_then(Value::as_str).unwrap_or("");
                    out.insert(
                        id.into(),
                        o.get("style").and_then(Value::as_u64).unwrap_or(0),
                    );
                }
                o.values().for_each(|c| walk(c, out));
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    walk(&m.components_json, &mut out);
    out
}

/// One place's messages: the text they ended on, the buttons now, the flags. A
/// stream's first text and the edits between are timing, so only the end is kept;
/// the approval card, which is made whole, is also recorded as it asked.
fn place(n: &mut Names, title: &str, messages: &[Msg]) -> String {
    let mut out = format!("=== {title}\n");
    for (i, m) in messages.iter().enumerate() {
        out.push_str(&block(n, &(i + 1).to_string(), m));
    }
    out
}

fn block(n: &mut Names, label: &str, m: &Msg) -> String {
    let who = if m.author == BOT_ID.to_string() {
        "bot"
    } else {
        "owner"
    };
    let mut flags = Vec::new();
    if m.flags & V2 != 0 {
        flags.push("v2");
    }
    if m.silent() {
        flags.push("silent");
    }
    if m.pinned {
        flags.push("pinned");
    }
    let mut out = format!("--- {label} {who} [{}]\n", flags.join(" "));
    if m.reply_to.is_some() {
        out.push_str("(a reply)\n");
    }
    out.push_str(&n.clean(m.versions.last().unwrap()));
    out.push('\n');
    let style = styles(m);
    for b in &m.buttons {
        out.push_str(&format!(
            "[{} ({}) {}{}]\n",
            n.clean(&b.label),
            style.get(&b.custom_id).copied().unwrap_or(0),
            n.clean(&b.custom_id),
            if b.disabled { " disabled" } else { "" }
        ));
    }
    out
}

fn post_text(n: &mut Names, p: &FakeThread, tags: &[String]) -> String {
    format!(
        "post names: {}\npost tags: {}\n",
        p.names
            .iter()
            .map(|x| n.clean(x))
            .collect::<Vec<_>>()
            .join(" -> "),
        tags.join(" -> ")
    )
}

#[tokio::test]
async fn the_dm_task_flow_is_as_the_owner_approved_it() {
    let r = rig_in(
        guild(false),
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, asks_to_write(dir))],
        &bindings(true, ""),
        |_| {},
        |c| c.discord.task_home = "theseus-tasks".into(),
    )
    .await;
    let forum = forum_up(&r).await;
    let home = home_up(&r).await;
    r.say((ANA, "ana"), None, ASK);
    replied(&r, ANA_DM, "Started.").await;
    until(&r, "the task's post", || !posts(&r, forum).is_empty()).await;
    let post = posts(&r, forum).remove(0);
    open_gate(&r);
    until(&r, "the approval card", || {
        !with_button(&r, post.id, "wa:").is_empty()
    })
    .await;
    until(&r, "the board", || {
        !with_button(&r, home, board_v2::CLEAR).is_empty()
    })
    .await;
    let card = with_button(&r, post.id, "wa:").remove(0);
    let asked = card.clone();
    let approve = card
        .buttons
        .iter()
        .find(|b| b.custom_id.ends_with(":approve"))
        .unwrap()
        .custom_id
        .clone();
    r.press(&card.id, &approve, (ANA, "ana"));
    until(&r, "the card settles", || {
        with_button(&r, post.id, "wa:").is_empty()
    })
    .await;
    until(&r, "the report in the home", || {
        r.posted(home)
            .iter()
            .any(|m| m.content.contains("finished"))
    })
    .await;
    until(&r, "the post is done", || {
        posts(&r, forum)
            .first()
            .is_some_and(|p| p.names.last().is_some_and(|n| n.starts_with("✅ ")))
    })
    .await;
    until(&r, "the board says done", || {
        let b = r.posted(home).into_iter().find(|m| m.pinned);
        b.is_some_and(|m| m.content.contains("done"))
    })
    .await;

    // The one approved addition (theseus-9ymz0, design §11 test 30): a content
    // label raised on the DM's session is one message in the DM with one
    // button. It is recorded apart, after everything the owner locked, so the
    // golden differs from before by this block and by nothing else.
    let session = r
        .core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .expect("the DM's session");
    let label = theseus_core::content::Label {
        flag: theseus_core::content::Flag::Injection,
        node: None,
        kind: theseus_core::content::ItemKind::FetchedPage,
        source: "http.fetch https://example.com/x".into(),
        confidence: 0.93,
        band: theseus_judge::band::Band::Act,
        at_ms: 1,
        judgment: String::new(),
    };
    assert!(r.core.tell_label(&session, &label).unwrap());
    until(&r, "the label's message", || !labelled(&r).is_empty()).await;

    let got = record(&r, forum, home, &asked);
    check(&got);
}

/// The DM's messages that are the content label's.
fn labelled(r: &Rig) -> Vec<Msg> {
    with_button(r, ANA_DM, "content:")
}

/// Every place's messages, normalized, as one text.
fn record(r: &Rig, forum: u64, home: u64, asked: &Msg) -> String {
    let post = posts(r, forum).remove(0);
    let id = r.core.kernel.tasks(None).unwrap()[0].id.clone();
    let mut n = Names {
        known: BTreeMap::new(),
        tmp: r.dir.path().display().to_string(),
        task: id[id.len() - 6..].to_string(),
        next: 0,
    };
    n.name(ANA, "owner");
    n.name(BOT_ID, "bot");
    n.name(ANA_DM, "dm");
    n.name(forum, "forum");
    n.name(home, "home");
    n.name(post.id, "post");
    let tags: Vec<String> = post
        .tag_sets
        .iter()
        .map(|set| {
            set.iter()
                .map(|id| {
                    r.fake
                        .forum_tags(forum)
                        .into_iter()
                        .find(|t| &t.id == id)
                        .map_or_else(|| id.clone(), |t| t.name)
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect();
    let mut got = String::from(HEADER);
    // The bot's lines in the DM after the owner's message (the reply, the "started task"
    // line, the call's notice) land in whatever order the lanes win; they are sorted.
    let mut dm = r.fake.messages(ANA_DM);
    // The content label's message is recorded in its own section below.
    let note = dm.iter().position(|m| {
        m.buttons
            .iter()
            .any(|b| b.custom_id.starts_with("content:"))
    });
    let note = note.map(|i| dm.remove(i));
    let from = dm
        .iter()
        .position(|m| m.author != BOT_ID.to_string())
        .map_or(0, |i| i + 1);
    dm[from..].sort_by_key(|m| m.versions.last().cloned());
    got.push_str(&place(
        &mut n,
        "the owner's DM (after the owner's message, sorted by text)",
        &dm,
    ));
    got.push_str(&place(
        &mut n,
        "the task's forum post",
        &r.fake.messages(post.id),
    ));
    got.push_str(&post_text(&mut n, &post, &tags));
    got.push_str("=== the approval card, as it asked\n");
    got.push_str(&block(&mut n, "card", asked));
    got.push_str(&place(&mut n, "the task home", &r.fake.messages(home)));
    got.push_str(&place(
        &mut n,
        "the one message a content label adds (theseus-9ymz0), in the DM",
        &note.into_iter().collect::<Vec<_>>(),
    ));
    got
}

/// The record against the golden file, or written to it on `THESEUS_GOLDEN=write`.
fn check(got: &str) {
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(GOLDEN).unwrap_or_default();
    if got != want {
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(got.lines().count().min(want.lines().count()));
        panic!(
            "the DM task flow differs from {GOLDEN} at line {}:\n  got:  {}\n  want: {}\n({} lines got, {} want; \
             the flow is the owner's: THESEUS_GOLDEN=write rewrites it only with the owner's OK)\n--- got ---\n{got}",
            first + 1,
            got.lines().nth(first).unwrap_or("<end>"),
            want.lines().nth(first).unwrap_or("<end>"),
            got.lines().count(),
            want.lines().count()
        );
    }
}
