//! A turn routed to Haiku keeps the project its conversation serves (theseus-v4u66), and
//! `context.explain` of a turn that compiled reads as before (theseus-r5ydi). The rig is
//! `tests_route_defer`'s: Haiku 5.5, which has no tool search, is every `anthropic` turn's
//! stand-in, and the fake Jev judges `quick`. (The router's detour is gone: no pick sends a
//! smaller request.)

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::context::{ContextExplainParams, ContextExplainResult};
use theseus_protocol::SessionKind;

use crate::provider::{FakeProvider, ProviderRequest};
use crate::session::SessionRecord;
use crate::tests_judge::texts;
use crate::tests_places::rows;
use crate::tests_route::{mode, rig_parts, turn, Rig};
use crate::Core;

/// A rig whose every `anthropic` turn is a Haiku stand-in that answers `n`
/// texts, with `tweak` applied to the config.
pub(crate) fn rig_haiku(
    jev: &FakeJev,
    n: usize,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, Arc<FakeProvider>) {
    let haiku = Arc::new(FakeProvider::scripted(texts(n)));
    let p = haiku.clone();
    let r = rig_parts(
        Some(jev),
        1,
        |c| {
            c.routing.max_wait_ms = 5_000;
            tweak(c);
        },
        move |parts| {
            parts.cfg.tools.defer.enabled = true;
            parts.providers.insert("anthropic".into(), p);
        },
    );
    (r, haiku)
}

/// A conversation attached to a project whose goal is `GOAL`.
const GOAL: &str = "Chart every mooring in the outer harbour";

fn attached(core: &Arc<Core>) -> String {
    let p = core
        .project_create(
            &serde_json::from_value(json!({
                "title": "Harbour survey", "objective": GOAL, "review": "none",
                "acceptance": ["every mooring has a depth"],
                "milestones": [{"title": "Survey the moorings", "due": "1h"}],
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.project_attach(
        &theseus_protocol::projects::ProjectAttachParams {
            session: rec.session_id.clone(),
            project: Some(p.project.id),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    rec.session_id
}

/// The text of a request's blocks of its last message.
fn tail_blocks(q: &ProviderRequest) -> Vec<String> {
    match &q.messages.last().unwrap()["content"] {
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str().map(str::to_string))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => vec![],
    }
}

/// theseus-v4u66: a turn routed to Haiku in a conversation attached to a project
/// carries the project's slice, last of the request's tail: the model sees the
/// goal. A conversation with no project carries none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turn_in_a_project_conversation_carries_the_slice() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.92);
    let (r, haiku) = rig_haiku(&jev, 2, |_| {});
    let sid = attached(&r.core);
    let res = turn(
        &r.core,
        Some(&sid),
        "What is the goal of the project this conversation serves?",
        None,
    )
    .await;
    let route = res.route.as_ref().unwrap();
    assert_eq!(
        (res.profile.as_str(), route.reason.as_str()),
        ("haiku", "verdict")
    );
    let blocks = tail_blocks(&haiku.requests()[0]);
    let slice = blocks
        .last()
        .filter(|b| b.starts_with(crate::project::slice::HEAD))
        .unwrap_or_else(|| panic!("no slice at the tail: {blocks:?}"));
    assert!(slice.contains(GOAL), "{slice}");
    assert!(slice.contains("Survey the moorings"), "{slice}");
    // A plain conversation's request carries none.
    let plain = turn(&r.core, None, "And what is the tide now?", None).await;
    assert_eq!(plain.route.as_ref().unwrap().reason, "verdict");
    let blocks = tail_blocks(&haiku.requests()[1]);
    assert!(
        !blocks
            .iter()
            .any(|b| b.starts_with(crate::project::slice::HEAD) || b.contains(GOAL)),
        "{blocks:?}"
    );
}

fn explain(core: &Core, sid: &str, turn_id: Option<&str>) -> ContextExplainResult {
    core.context_explain(
        &ContextExplainParams {
            session_id: sid.into(),
            turn_id: turn_id.map(str::to_string),
        },
        true,
    )
    .unwrap()
}

/// A turn that compiled explains as it always did: no `detour` field on the
/// result and no mark on its turns, so every reader of the old shape is
/// unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_compiled_turn_explains_as_before() {
    let r = rig_parts(None, 1, |_| {}, |_| {});
    let res = turn(&r.core, None, "Where is the lamp?", None).await;
    let x = explain(&r.core, &res.session_id, None);
    assert_eq!(x.turn_id.as_deref(), Some(res.turn_id.as_str()));
    assert!(x.compiled.is_some() && x.detour.is_none());
    let v = serde_json::to_value(&x).unwrap();
    assert!(v.get("detour").is_none(), "{v}");
    assert!(v["turns"][0].get("detoured").is_none(), "{}", v["turns"][0]);
    assert!(rows(&r.core, "context.detoured").is_empty());
}

/// The owner's config in shape: its AWS account and its five personas (whose
/// files the full request carries).
pub(crate) fn owner_shape(c: &mut crate::Config, dir: &std::path::Path) {
    let owner = crate::Config::parse(&crate::tests_first_request::with_personas(
        crate::tests_first_request::OWNER_SHAPE,
        dir,
    ))
    .unwrap()
    .0;
    c.aws = owner.aws;
    c.personas = owner.personas;
    c.context = owner.context;
    // A context file the full request carries (about 2,000 tokens).
    let notes = dir.join("harbour-notes.md");
    std::fs::write(
        &notes,
        "The keeper logged the tide and the wind before the lamp was lit.\n".repeat(125),
    )
    .unwrap();
    c.context.files = vec![crate::context_files::ContextEntry::Path(
        notes.to_string_lossy().into_owned(),
    )];
}
