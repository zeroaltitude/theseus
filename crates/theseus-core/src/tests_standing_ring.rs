//! The standing block outside the ring (theseus-a7vi): it sits in the
//! system, so a compaction, which replaces the leading turns with a summary,
//! never drops it. Three compactions in a row, and every request after each
//! still carries every live record, the manifest listing them, and the
//! situation check passing.

use std::sync::Arc;

use theseus_protocol::{SessionKind, StandingAddParams, StandingKind, StandingScope, Usage};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// And 48,000 until `task_fan_out` joined the catalog (theseus-z9mze.3): its definition grows the tools
/// block by about 500 tokens. 48,250 and up hold (to 52,000, the sweep's end) for this suite, and 48,500 and up for the
/// others in this family of tests; 49,000 is the middle of the near end of that band.
const WINDOW: u64 = 49_000;

fn summary(text: &str) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: 9_000,
            output_tokens: 60,
            ..Default::default()
        },
        then: Box::new(Scripted::text(text)),
    }
}

fn words(tag: &str, tokens: usize) -> String {
    let mut s = format!("{tag} ");
    while s.len() < tokens * 4 {
        s.push_str("the keeper logged the tide and the wind before the lamp was lit. ");
    }
    s
}

fn system_text(q: &ProviderRequest) -> String {
    q.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn the_block_survives_three_compactions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.summary_profile = Some("glm".into());
    // glm only summarizes here: an unrestricted model (theseus-a1wvq.3).
    cfg.serve_glm_unrestricted();
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(WINDOW),
            max_output_tokens: Some(2_000),
            ..Default::default()
        },
    );
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let glm = Arc::new(FakeProvider::scripted(
        (1..=8)
            .map(|n| summary(&format!("SUMMARY {n}: the tides so far.")))
            .collect(),
    ));
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    let core = Core::build(parts).unwrap();

    let ids: Vec<String> = [
        (StandingKind::Order, "never push to main"),
        (StandingKind::Preference, "answer in two sentences"),
        (StandingKind::Fact, "keeps a vegetable garden"),
    ]
    .into_iter()
    .map(|(kind, text)| {
        core.standing_add(
            &StandingAddParams {
                kind,
                text: text.into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap()
        .record
        .id
    })
    .collect();

    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;
    let mut compactions = 0;
    for k in 0..40 {
        let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
        let (live, _) = core.live_profile();
        let target = core.runner.resolve_target(&live, None, None, None).unwrap();
        core.runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some(words(&format!("turn{k}"), 6_000)),
                target,
                sink: EventSink::new(core.bus.clone(), &sid, None),
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap();
        let q = model.requests().pop().unwrap();
        let system = system_text(&q);
        for id in &ids {
            assert!(
                system.contains(&format!("- {id}: ")),
                "turn {k}: {id} missing: {system}"
            );
        }
        compactions = glm.requests().len();
        if compactions >= 3 {
            break;
        }
    }
    assert_eq!(compactions, 3, "three compactions happened");
    let comps = core.store.session_compilations(&sid).unwrap();
    let compacted: Vec<_> = comps
        .iter()
        .filter(|c| c.strategy == "compaction")
        .collect();
    assert!(compacted.len() >= 3, "{} compactions", compacted.len());
    for c in compacted {
        let listed: Vec<&str> = c.manifest.standing.iter().map(|u| u.id.as_str()).collect();
        for id in &ids {
            assert!(listed.contains(&id.as_str()), "{listed:?}");
        }
    }
}

/// A trivial turn routed to a cheaper model, a detour, carries the standing
/// block too, though it carries no context file: orders hold in every prompt
/// of their scope, and the check refuses a detour that dropped one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_carries_the_standing_block() {
    use crate::tests_route::{glm_placements, mode, rig, turn};
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, glm_placements);
    let id = r
        .core
        .standing_add(
            &StandingAddParams {
                text: "never push to main".into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap()
        .record
        .id;
    let first = turn(&r.core, None, "What does the store's manifest hold?", None).await;
    let sid = first.session_id.clone();
    mode(&jev, "trivial", 0.95);
    let thanks = turn(&r.core, Some(&sid), "thank you!", None).await;
    assert_eq!(thanks.route.as_ref().unwrap().reason, "detour");
    let detour = &r.zai.requests()[0];
    assert!(
        system_text(detour).contains(&format!("- {id}: never push to main")),
        "{}",
        system_text(detour)
    );
    assert!(
        system_text(&r.claude.requests()[0]).contains(&format!("- {id}: ")),
        "the session's own request carries it too"
    );
}
