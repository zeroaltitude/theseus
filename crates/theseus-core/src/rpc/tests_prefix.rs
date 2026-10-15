//! The prefix library through the whole core (theseus-rr15): layers 1 to 3
//! the same bytes for two sessions, two threads and two places on one
//! project; a second thread on the project reuses its header verbatim with
//! its topic after it, and repeats it on its appends; a near-miss project is
//! never reused; a changed source is a new header; the off switch renders
//! each session's own layers; and a pick rides the compilation's frame.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams};
use theseus_protocol::{StandingAddParams, StandingKind, StandingScope};

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_day_ceiling::build;
use crate::turn::TurnRequest;
use crate::{Config, Core};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
    }
}

fn thread(core: &Core, slug: &str, topics: &[&str]) -> String {
    core.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    if !topics.is_empty() {
        core.thread_edit(
            &ThreadEditParams {
                slug: slug.into(),
                topics: Some(topics.iter().map(|t| t.to_string()).collect()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    }
    core.thread_session(slug, cli()).unwrap().session_id
}

fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn say(core: &Arc<Core>, sid: &str, input: &str) {
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
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
            surface: None,
        })
        .await
        .unwrap();
}

/// The project: one system-level context file, and a persona with its own.
fn project(dir: &std::path::Path, c: &mut Config) {
    let (p, f) = (dir.join("surveyor.md"), dir.join("harbour.md"));
    std::fs::write(
        &p,
        format!("The surveyor measures. {}", "tide ".repeat(300)),
    )
    .unwrap();
    std::fs::write(&f, format!("The harbour log. {}", "wave ".repeat(600))).unwrap();
    c.context.files = vec![f.to_string_lossy().into_owned().into()];
    c.context.default_persona = Some("surveyor".into());
    c.personas.insert(
        "surveyor".into(),
        crate::config::PersonaConfig {
            files: vec![p.to_string_lossy().into_owned().into()],
        },
    );
}

fn topic(core: &Core, name: &str, guidance: &str) {
    core.ontology_category_add(
        &theseus_protocol::OntologyCategoryAddParams {
            name: name.into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    core.ontology_guidance_set(
        &theseus_protocol::OntologyGuidanceSetParams {
            category: name.into(),
            text: guidance.into(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
}

fn texts(n: usize) -> Vec<Scripted> {
    (0..n)
        .map(|i| Scripted::text(&format!("noted {i}")))
        .collect()
}

fn layers(r: &ProviderRequest) -> Vec<Value> {
    r.system[..3].to_vec()
}

fn text(r: &ProviderRequest, i: usize) -> String {
    r.system[i]["text"].as_str().unwrap_or_default().to_string()
}

fn picks(core: &Core) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind("prefix.picked"))
        .map(|(_, r)| r.data)
        .collect()
}

fn choices(core: &Core) -> Vec<String> {
    picks(core)
        .iter()
        .map(|p| p["choice"].as_str().unwrap().to_string())
        .collect()
}

/// Two sessions in two places, and two threads, on one project render
/// layers 1 to 3 byte for byte: nothing of a session, a place, a thread or a
/// day is inside them. A session's own standing record goes to its own block.
#[tokio::test]
async fn sessions_threads_and_places_on_one_project_send_layers_one_to_three_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(4), |c| project(dir.path(), c));
    crate::tests_day_ceiling::bind_dm(&core);
    let (a, b) = (session(&core, None), session(&core, Some("dm:5150")));
    let add = |scope: StandingScope, text: &str| {
        core.standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: text.into(),
                scope,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    };
    add(StandingScope::Everywhere, "Quote every level in metres.");
    add(
        StandingScope::Session(a.clone()),
        "Sign each reply as the harbour clerk.",
    );
    say(&core, &a, "the levels today").await;
    say(&core, &b, "the levels today").await;
    let (t1, t2) = (
        thread(&core, "tide-gauges", &[]),
        thread(&core, "moon-tables", &[]),
    );
    say(&core, &t1, "first").await;
    say(&core, &t2, "second").await;
    let r = fake.requests();
    assert_eq!(r.len(), 4);
    assert_eq!(layers(&r[0]), layers(&r[1]), "two sessions, two places");
    assert_eq!(layers(&r[2]), layers(&r[3]), "two threads");
    assert_eq!(layers(&r[0]), layers(&r[2]), "a session and a thread");
    assert!(text(&r[0], 0).contains("Quote every level in metres."));
    assert!(text(&r[0], 2).contains("The harbour log."));
    // The session's own order is in its own block, never in a layer.
    assert!(text(&r[0], 3).contains("Sign each reply as the harbour clerk."));
    for q in &r {
        for i in 0..3 {
            let t = text(q, i);
            for own in [&*a, &*b, &*t1, &*t2, "harbour clerk", "Directory:", "2026-"] {
                assert!(!t.contains(own), "{own} is inside layer {}", i + 1);
            }
        }
    }
    assert!(!text(&r[1], 3).contains("harbour clerk"));
}

/// A second thread on a warm project reuses the project's header verbatim,
/// and its topic's guidance goes after the cached prefix; its appends repeat
/// those bytes; the picks ride their compilations' frames.
#[tokio::test]
async fn a_second_thread_on_the_project_reuses_its_header_and_its_topic_follows_it() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(3), |c| project(dir.path(), c));
    topic(&core, "tides", "Quote every level in metres.");
    let a = thread(&core, "harbour", &[]);
    say(&core, &a, "the harbour").await;
    let b = thread(&core, "gauges", &["topic:tides"]);
    say(&core, &b, "the gauges").await;
    say(&core, &b, "and again").await;
    let r = fake.requests();
    assert_eq!(text(&r[1], 2), text(&r[0], 2), "the header, verbatim");
    assert!(!text(&r[1], 2).contains("metres"));
    assert!(
        text(&r[1], 3).contains("Quote every level in metres."),
        "{}",
        text(&r[1], 3)
    );
    assert_eq!(r[2].system, r[1].system, "the append repeats the bytes");
    assert_eq!(choices(&core), ["mint", "reuse"]);
    let p = &picks(&core)[1];
    assert!(p["fit"].as_f64().unwrap() >= 0.90, "{p}");
    assert_eq!(p["below_act"], false);
    let rec: SessionRecord = core.store.get_session(&b).unwrap().unwrap();
    let c = core
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let used = c.manifest.prefix.unwrap();
    assert_eq!(used.header, p["header"].as_str().unwrap());
    assert_ne!(used.header, used.fresh);
    // A restarted daemon reads the headers back and picks the same bytes.
    let model = Arc::new(FakeProvider::scripted(texts(1)));
    let again = Core::build(crate::rpc::Parts::for_tests(
        (*core.cfg).clone(),
        model.clone(),
        core.store.clone(),
    ))
    .unwrap();
    say(&again, &b, "after a restart").await;
    assert_eq!(model.requests()[0].system, r[1].system);
}

/// A near-miss project (the same layout, another test command) is not
/// reused: each thread's third layer is its own.
#[tokio::test]
async fn a_near_miss_project_is_never_reused() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(2), |c| project(dir.path(), c));
    topic(&core, "gauges", "Run the tests with: cargo test -p gauges");
    topic(&core, "tides", "Run the tests with: cargo test -p tides");
    let a = thread(&core, "gauge-work", &["topic:gauges"]);
    say(&core, &a, "gauges").await;
    let b = thread(&core, "tide-work", &["topic:tides"]);
    say(&core, &b, "tides").await;
    let r = fake.requests();
    assert!(text(&r[1], 2).contains("-p tides"));
    assert!(!text(&r[1], 2).contains("-p gauges"));
    assert!(!text(&r[1], 3).contains("-p gauges"));
    assert_eq!(choices(&core), ["mint", "mint"]);
}

/// A context file edited since: the next compile point mints the new layer
/// under its own digest; the old header no longer fits.
#[tokio::test]
async fn a_changed_context_file_is_a_new_header() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(2), |c| project(dir.path(), c));
    let a = thread(&core, "harbour", &[]);
    say(&core, &a, "first").await;
    std::fs::write(
        dir.path().join("harbour.md"),
        format!("The harbour log, v2. {}", "wave ".repeat(600)),
    )
    .unwrap();
    say(&core, &a, "second").await;
    let r = fake.requests();
    assert!(text(&r[1], 2).contains("v2"));
    let p = picks(&core);
    assert_eq!(choices(&core), ["mint", "mint"]);
    assert_ne!(p[0]["header"], p[1]["header"]);
}

/// Off, every session renders its own layers: the topic is in the thread's
/// third layer, no pick is made, and no manifest names a header.
#[tokio::test]
async fn off_each_session_renders_its_own_layers() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(2), |c| {
        project(dir.path(), c);
        c.prefix.enabled = false;
    });
    topic(&core, "tides", "Quote every level in metres.");
    let a = thread(&core, "harbour", &[]);
    say(&core, &a, "the harbour").await;
    let b = thread(&core, "gauges", &["topic:tides"]);
    say(&core, &b, "the gauges").await;
    let r = fake.requests();
    assert!(text(&r[1], 2).contains("Quote every level in metres."));
    assert!(picks(&core).is_empty());
    let rec: SessionRecord = core.store.get_session(&b).unwrap().unwrap();
    let c = core
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert!(c.manifest.prefix.is_none());
}

/// A thread whose third layer is a reused header, with its topic and a
/// session's own standing record after it, is still kept warm, and the
/// read's bytes are its next turn's first bytes: the keep-warm read compiles
/// the plan and the block as the turn does.
#[tokio::test]
async fn a_thread_on_a_reused_header_is_kept_warm_with_its_next_turns_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(5), |c| {
        // Every profile that says none caches for an hour, as tests_keep_warm's.
        let text = Config::EXAMPLE_TOML
            .replace("live = \"sonnet\"", "live = \"sonnet\"\ncache_ttl = \"1h\"");
        let state = c.server.state_dir.clone();
        *c = Config::parse(&text).unwrap().0;
        (c.server.state_dir, c.tools.roots) = (state, vec![]);
        c.cache.keep_warm_min_tokens = 0;
        project(dir.path(), c);
    });
    topic(&core, "tides", "Quote every level in metres.");
    let a = thread(&core, "harbour", &[]);
    say(&core, &a, "the harbour").await;
    let b = thread(&core, "gauges", &["topic:tides"]);
    core.standing_add(
        &StandingAddParams {
            kind: StandingKind::Order,
            text: "Sign each reply as the gauge clerk.".into(),
            scope: StandingScope::Session(b.clone()),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    // A person's message, as `turn.submit` notes it for keep-warm.
    core.runner.keep_warm.message(&b, true);
    say(&core, &b, "the gauges").await;
    assert_eq!(choices(&core), ["mint", "reuse"]);
    let live = core.live_profile().0;
    let read = core.runner.keep_warm_once(&b, &live).await;
    assert!(
        matches!(read, crate::keep_warm::Outcome::Read { .. }),
        "{read:?}"
    );
    say(&core, &b, "and again").await;
    let r = fake.requests();
    let (kept, next) = (&r[r.len() - 2], &r[r.len() - 1]);
    assert_eq!(kept.max_tokens, crate::keep_warm::MAX_TOKENS);
    assert_eq!(kept.system, next.system, "the read's system is the turn's");
    assert!(text(next, 3).contains("gauge clerk") && text(next, 3).contains("metres"));
    assert_eq!(
        choices(&core),
        ["mint", "reuse"],
        "no pick under a warm session"
    );
}
