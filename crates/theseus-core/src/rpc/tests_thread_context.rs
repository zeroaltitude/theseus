//! A thread carries only its own working set (theseus-h4a7) through the
//! core: the layers two threads and an unthreaded session share, the card's
//! snapshot and `thread.note`, and the cold resume.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::threads::ThreadNewParams;

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::compiler::Recompile;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_day_ceiling::{build, turn};
use crate::turn::TurnRequest;
use crate::Core;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn thread(core: &Core, slug: &str) -> String {
    core.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    core.thread_session(slug, cli()).unwrap().session_id
}

fn edit_topics(core: &Core, slug: &str, topics: &[&str]) {
    core.thread_edit(
        &theseus_protocol::threads::ThreadEditParams {
            slug: slug.into(),
            topics: Some(topics.iter().map(|t| t.to_string()).collect()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

async fn say(core: &Arc<Core>, sid: &str, input: &str, recompile: Option<Recompile>) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// A persona file and a context file, in `dir`, big enough that every layer
/// reaches the model's caching minimum.
fn files(dir: &std::path::Path, c: &mut crate::Config) {
    let (p, f) = (dir.join("surveyor.md"), dir.join("harbour.md"));
    std::fs::write(
        &p,
        format!("The surveyor measures. {}", "tide ".repeat(300)),
    )
    .unwrap();
    std::fs::write(&f, format!("The harbour log. {}", "wave ".repeat(300))).unwrap();
    c.context.files = vec![f.to_string_lossy().into_owned().into()];
    c.context.default_persona = Some("surveyor".into());
    c.personas.insert(
        "surveyor".into(),
        crate::config::PersonaConfig {
            files: vec![p.to_string_lossy().into_owned().into()],
            ..Default::default()
        },
    );
}

fn names(r: &ProviderRequest) -> Vec<String> {
    r.tools
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

fn texts(n: usize) -> Vec<Scripted> {
    (0..n)
        .map(|i| Scripted::text(&format!("noted {i}")))
        .collect()
}

fn marked(r: &ProviderRequest) -> Vec<bool> {
    r.system
        .iter()
        .map(|b| b.get("cache_control").is_some())
        .collect()
}

/// Two threads, and a thread and an unthreaded session, on one profile and
/// place class render the header and the persona layer byte for byte, and
/// the project layer when they share topics (here, none, and then one).
#[tokio::test]
async fn threads_and_sessions_share_the_first_layers_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(4), |c| files(dir.path(), c));
    let (a, b) = (thread(&core, "tide-gauges"), thread(&core, "moon-tables"));
    say(&core, &a, "hello from a", None).await;
    say(&core, &b, "hello from b", None).await;
    crate::tests_day_ceiling::bind_dm(&core);
    let (_, plain) = turn(&core, "hello from the DM").await;
    plain.unwrap();
    let r = fake.requests();
    assert_eq!(r.len(), 3);
    for other in &r[1..] {
        let (x, y) = (
            r[0].system[0]["text"].as_str().unwrap(),
            other.system[0]["text"].as_str().unwrap(),
        );
        let at = x
            .chars()
            .zip(y.chars())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        assert_eq!(
            r[0].system[0],
            other.system[0],
            "layer 1 differs at {at}: {:?} / {:?}",
            &x[at.saturating_sub(40)..(at + 80).min(x.len())],
            &y[at.saturating_sub(40)..(at + 80).min(y.len())]
        );
        assert_eq!(r[0].system[1], other.system[1], "layer 2");
        assert_eq!(r[0].system[2], other.system[2], "layer 3: no topics");
        assert_eq!(
            marked(other),
            [true, true, true, false],
            "{:?}",
            marked(other)
        );
    }
    assert!(r[0].system[0]["text"]
        .as_str()
        .unwrap()
        .contains(crate::turn::ASSEMBLY));
    assert!(r[0].system[1]["text"]
        .as_str()
        .unwrap()
        .contains("surveyor measures"));
    assert!(r[0].system[2]["text"]
        .as_str()
        .unwrap()
        .contains("harbour log"));
    // The tool is a thread's alone; the card heads a thread's messages.
    assert!(names(&r[0]).contains(&"thread_note".to_string()));
    assert!(names(&r[1]).contains(&"thread_note".to_string()));
    assert!(!names(&r[2]).contains(&"thread_note".to_string()));
    let head = |q: &ProviderRequest| {
        q.messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(
        head(&r[0]).starts_with("[Thread card: Title of tide-gauges"),
        "{}",
        head(&r[0])
    );
    assert!(head(&r[1]).starts_with("[Thread card: Title of moon-tables"));
    assert!(!head(&r[2]).contains("Thread card"));
    // Nothing of a thread's own is inside the layers.
    for q in &r {
        for layer in &q.system[..3] {
            let t = layer["text"].as_str().unwrap();
            for own in ["tide-gauges", "moon-tables", &a, &b, "Directory:"] {
                assert!(!t.contains(own), "{own} is inside a shared layer");
            }
        }
    }
}

/// The ontology guidance of a thread's topics is in its third layer, the
/// same bytes for two threads that share the topic, and not in a thread
/// that has none.
#[tokio::test]
async fn a_threads_topics_make_its_third_layer_and_threads_that_share_them_share_it() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(3), |c| files(dir.path(), c));
    core.ontology_category_add(
        &theseus_protocol::OntologyCategoryAddParams {
            name: "tides".into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    core.ontology_guidance_set(
        &theseus_protocol::OntologyGuidanceSetParams {
            category: "tides".into(),
            text: "Quote every level in metres.".into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    let (a, b, c) = (
        thread(&core, "gauges"),
        thread(&core, "buoys"),
        thread(&core, "moons"),
    );
    edit_topics(&core, "gauges", &["topic:tides"]);
    edit_topics(&core, "buoys", &["topic:tides"]);
    for (sid, text) in [(&a, "a"), (&b, "b"), (&c, "c")] {
        say(&core, sid, text, None).await;
    }
    let r = fake.requests();
    let third = |q: &ProviderRequest| q.system[2]["text"].as_str().unwrap().to_string();
    assert!(
        third(&r[0]).contains("Quote every level in metres."),
        "{}",
        third(&r[0])
    );
    assert_eq!(
        r[0].system[2], r[1].system[2],
        "the same topic, the same layer"
    );
    assert!(!third(&r[2]).contains("Quote every level"));
    assert_eq!(r[0].system[0], r[2].system[0]);
    assert_eq!(r[0].system[1], r[2].system[1]);
}

fn note(id: &str, line: &str) -> Scripted {
    Scripted::tools("", &[(id, "thread_note", json!({"line": line}))])
}

/// `thread.note` changes nothing the prefix holds: the card at the head and
/// every message before the call are the same bytes in the request that
/// follows it, and the next recompile shows the line in the card.
#[tokio::test]
async fn a_note_leaves_the_prefix_alone_and_shows_in_the_next_recompile() {
    let dir = tempfile::tempdir().unwrap();
    let mut script = texts(1);
    script.push(note("n1", "gauge 4 reads high after the swap"));
    script.extend(texts(2));
    let (core, fake) = build(dir.path(), script, |c| files(dir.path(), c));
    let a = thread(&core, "gauges");
    say(&core, &a, "first", None).await;
    say(&core, &a, "log a finding", None).await;
    let r = fake.requests();
    assert_eq!(
        r.len(),
        3,
        "turn one, then the loop with the call, then its result"
    );
    let card = |q: &ProviderRequest| {
        q.messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(!card(&r[1]).contains("gauge 4 reads high"));
    assert_eq!(card(&r[1]), card(&r[2]), "the card did not move mid-turn");
    assert_eq!(
        serde_json::to_string(&r[1].messages[..]).unwrap()[..400],
        serde_json::to_string(&r[2].messages[..]).unwrap()[..400],
    );
    assert_eq!(
        serde_json::to_string(&r[1].messages)
            .unwrap()
            .trim_end_matches(']'),
        serde_json::to_string(&r[2].messages[..r[1].messages.len()])
            .unwrap()
            .trim_end_matches(']'),
        "every message of the earlier request is the same bytes in the later one"
    );
    let result = r[2].messages.last().unwrap()["content"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(result.contains("Noted on the card of gauges"), "{result}");
    // The record holds the line, in a version of its own.
    let board = core.runner.threads.all(&core.store).unwrap();
    assert_eq!(
        board.get("gauges").unwrap().card,
        ["gauge 4 reads high after the swap"]
    );
    // A turn that does not recompile still renders the snapshot; a recompile
    // takes the card as it stands.
    say(&core, &a, "and again", None).await;
    let r = fake.requests();
    assert!(
        !card(&r[3]).contains("gauge 4 reads high"),
        "an append keeps its snapshot"
    );
    say(
        &core,
        &a,
        "start over from the record",
        Some(Recompile::Transcript),
    )
    .await;
    let r = fake.requests();
    let c = card(&r[4]);
    assert!(c.contains("- gauge 4 reads high after the swap"), "{c}");
}

/// A line is one sentence of at most a card line's length; the card keeps
/// the newest twelve; an unthreaded session's note is refused.
#[tokio::test]
async fn a_note_is_bounded_and_the_card_keeps_the_newest_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut script = Vec::new();
    for k in 0..14 {
        script.push(note(&format!("n{k}"), &format!("line {k}")));
        script.push(Scripted::text("ok"));
    }
    let (core, _fake) = build(dir.path(), script, |c| files(dir.path(), c));
    let a = thread(&core, "gauges");
    for k in 0..14 {
        say(&core, &a, &format!("note {k}"), None).await;
    }
    let card = core
        .runner
        .threads
        .all(&core.store)
        .unwrap()
        .get("gauges")
        .unwrap()
        .card
        .clone();
    assert_eq!(card.len(), 12);
    assert_eq!((card[0].as_str(), card[11].as_str()), ("line 2", "line 13"));
    let rows: Vec<_> = crate::tests_day_ceiling::rows(&core, "thread.edited");
    assert_eq!(rows.len(), 14);
    assert_eq!(rows[0].data["via"], "thread.note");
}

/// The history a cold-resume test starts from: a thread of `turns` long
/// replies, its last call `ago_ms` ago.
async fn long_thread(
    core: &Arc<Core>,
    fake: &Arc<FakeProvider>,
    sid: &str,
    turns: usize,
    ago_ms: u64,
) -> usize {
    for k in 0..turns {
        say(core, sid, &format!("question {k}"), None).await;
    }
    core.store
        .update_session(sid, |r: &mut SessionRecord| {
            r.last_active_ms = theseus_protocol::now_unix_ms() - ago_ms;
            Ok(vec![])
        })
        .unwrap();
    fake.requests().len()
}

fn long(i: usize) -> Scripted {
    Scripted::text(&format!("answer {i}: {}", "silt ".repeat(2_000)))
}

fn compiled_rows(core: &Core, sid: &str) -> Vec<Value> {
    crate::tests_day_ceiling::rows(core, "context.compiled")
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(sid))
        .map(|r| r.data)
        .collect()
}

const HOUR_PLUS: u64 = 2 * 3_600_000;

/// A thread whose next message finds its cache window passed is rewritten
/// small: under its working tokens, the newest turns kept; a warm one is
/// appended to as it was.
#[tokio::test]
async fn a_cold_thread_resumes_small_and_a_warm_one_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let script: Vec<Scripted> = (0..20).map(long).collect();
    let (core, fake) = build(dir.path(), script, |c| {
        files(dir.path(), c);
        c.threads.working_tokens = Some(22_000);
        c.threads.resume_keep_turns = 1;
        c.memory.summary_profile = Some("off".into());
    });
    let (cold, warm) = (thread(&core, "cold"), thread(&core, "warm"));
    let n = long_thread(&core, &fake, &cold, 6, HOUR_PLUS).await;
    let m = long_thread(&core, &fake, &warm, 6, 1_000).await;
    assert!(n > 0 && m > n);
    let before = fake.requests().len();
    say(&core, &cold, "back after lunch", None).await;
    say(&core, &warm, "still here", None).await;
    let r = fake.requests();
    assert_eq!(r.len(), before + 2);
    let rows = |sid| compiled_rows(&core, sid);
    let last = |sid| rows(sid).last().cloned().unwrap();
    let c = last(&cold);
    assert_eq!(c["decision"], "recompile", "{c}");
    assert_eq!(c["trigger"], "cold_resume");
    assert!(
        c["est_tokens"].as_u64().unwrap() <= 22_000,
        "{}",
        c["est_tokens"]
    );
    let kept = r[before]
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .count();
    assert!(kept <= 2, "the newest turns only: {kept}");
    let w = last(&warm);
    assert_eq!(w["decision"], "append", "{w}");
    assert!(
        w["est_tokens"].as_u64().unwrap() > 22_000,
        "a warm thread is not shrunk for its size"
    );
}

/// A cold resume keeps the newest twenty turns whole by default (theseus-d9je7): a thread of
/// thirty turns resume with the newest twenty, not six.
#[tokio::test]
async fn a_cold_resume_keeps_twenty_turns() {
    let dir = tempfile::tempdir().unwrap();
    let script: Vec<Scripted> = (0..32).map(long).collect();
    let (core, fake) = build(dir.path(), script, |c| {
        files(dir.path(), c);
        c.threads.working_tokens = Some(90_000);
        c.memory.summary_profile = Some("off".into());
    });
    let cold = thread(&core, "cold");
    long_thread(&core, &fake, &cold, 30, HOUR_PLUS).await;
    let before = fake.requests().len();
    say(&core, &cold, "back after lunch", None).await;
    let r = fake.requests();
    assert_eq!(r.len(), before + 1);
    let c = compiled_rows(&core, &cold).last().cloned().unwrap();
    assert_eq!(c["trigger"], "cold_resume", "{c}");
    let kept = r[before]
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .count();
    assert_eq!(
        kept, 20,
        "the newest twenty turns, the new message among them"
    );
}

/// With no `working_tokens`, a cold resume works to `working_percent` of the model's window
/// (theseus-d9je7): at 1 per cent only a turn or two fit.
#[tokio::test]
async fn a_cold_resume_works_to_a_share_of_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let script: Vec<Scripted> = (0..32).map(long).collect();
    let (core, fake) = build(dir.path(), script, |c| {
        files(dir.path(), c);
        c.threads.working_percent = 1;
        c.memory.summary_profile = Some("off".into());
    });
    let cold = thread(&core, "cold");
    long_thread(&core, &fake, &cold, 30, HOUR_PLUS).await;
    let before = fake.requests().len();
    say(&core, &cold, "back after lunch", None).await;
    let c = compiled_rows(&core, &cold).last().cloned().unwrap();
    assert_eq!(c["trigger"], "cold_resume", "{c}");
    let kept = fake.requests()[before]
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .count();
    assert!(
        kept <= 3,
        "1 per cent of the window is a turn or two: {kept}"
    );
}

/// An unthreaded session that has been away is not rewritten by this: a
/// cold-compact is a separate decision.
#[tokio::test]
async fn an_unthreaded_session_is_not_resumed_small() {
    let dir = tempfile::tempdir().unwrap();
    let script: Vec<Scripted> = (0..10).map(long).collect();
    let (core, fake) = build(dir.path(), script, |c| {
        files(dir.path(), c);
        c.threads.working_tokens = Some(22_000);
        c.memory.summary_profile = Some("off".into());
    });
    let (sid, first) = turn(&core, "question 0").await;
    first.unwrap();
    for k in 1..5 {
        say(&core, &sid, &format!("question {k}"), None).await;
    }
    core.store
        .update_session(&sid, |r: &mut SessionRecord| {
            r.last_active_ms = theseus_protocol::now_unix_ms() - HOUR_PLUS;
            Ok(vec![])
        })
        .unwrap();
    say(&core, &sid, "back after lunch", None).await;
    let last = compiled_rows(&core, &sid).last().cloned().unwrap();
    assert_eq!(last["decision"], "append", "{last}");
    assert!(fake.requests().len() >= 6);
}
