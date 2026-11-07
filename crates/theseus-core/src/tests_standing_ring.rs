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

/// The window is measured, never guessed (theseus-ataas, 2026-11-07). It used
/// to be a constant: 48,000, then 48,250, then 49,000, re-tuned each time the
/// system prompt grew (`task_fan_out` joining the catalog last), and by
/// 2026-11-07 the band's near edge had crept past 48,500 again with nobody
/// noticing. A window under that edge does not fail loudly: the first request
/// never fits, so NOTHING compacts and the test reads "three compactions
/// happened, left: 0" — which says nothing about the standing block this suite
/// is actually about.
///
/// So the suite measures one request of its own on a window nothing can reach,
/// and gives the real window that much plus [`HEADROOM`]. Three or four of
/// these turns then fill the ring whatever the prompt weighs, and growing the
/// prompt moves the window with it.
const HEADROOM: u64 = 20_000;

/// The tokens each turn's message is built to cost.
const TURN_TOKENS: usize = 6_000;

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

/// A core on `dir`, with `window` as the session model's context window: its
/// fake Claude, and the fake GLM that summarizes.
fn core_with(
    dir: &std::path::Path,
    window: u64,
) -> (Arc<Core>, Arc<FakeProvider>, Arc<FakeProvider>) {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.summary_profile = Some("glm".into());
    // glm only summarizes here: an unrestricted model (theseus-a1wvq.3).
    cfg.serve_glm_unrestricted();
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(window),
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
    let store = Store::open(&dir.join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    (Core::build(parts).unwrap(), model, glm)
}

/// One turn of this suite's size, on `sid`.
async fn turn(core: &Core, sid: &str, tag: &str) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(words(tag, TURN_TOKENS)),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// A fresh session's first request of this suite's size, in tokens, on a
/// window nothing can reach: what the prompt weighs today, plus one turn.
async fn one_request_tokens() -> u64 {
    let dir = tempfile::tempdir().unwrap();
    let (core, _model, _glm) = core_with(dir.path(), 1_000_000);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    turn(&core, &rec.session_id, "measure").await;
    let used = core
        .store
        .session_compilations(&rec.session_id)
        .unwrap()
        .first()
        .and_then(|c| c.budget.as_ref())
        .map(|b| b.used_tokens)
        .expect("the first compilation's budget");
    assert!(used > 0, "an empty measurement");
    used
}

#[tokio::test]
async fn the_block_survives_three_compactions() {
    let window = one_request_tokens().await + HEADROOM;
    let dir = tempfile::tempdir().unwrap();
    let (core, model, glm) = core_with(dir.path(), window);

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
    let mut turns = 0;
    for k in 0..40 {
        turns = k + 1;
        turn(&core, &sid, &format!("turn{k}")).await;
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
    assert_eq!(
        compactions, 3,
        "three compactions happened (window {window}, after {turns} of 40 turns)"
    );
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
