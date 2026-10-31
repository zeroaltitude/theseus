//! A detoured turn (`turn::route_step`'s `compile_detour`): it keeps the project
//! its conversation serves (theseus-v4u66), `context.explain` and `theseus why`
//! say what its model was shown (theseus-r5ydi), and a family its message names
//! is counted on the request the detour sends (theseus-3bs4e). The rig is
//! `tests_route_defer`'s: Haiku 5.5, which has no tool search, is every
//! `anthropic` turn's stand-in, and the fake Jev judges `quick`.

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

/// theseus-v4u66: a quick detour in a conversation attached to a project
/// carries the project's slice, last of the request's tail, as the normal path
/// does: the model sees the goal. A conversation with no project carries none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quick_detour_in_a_project_conversation_carries_the_slice() {
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
    // A plain conversation's detour carries none: its request is as before.
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
/// unchanged. (No plant beyond `why_explains_a_detoured_turn`'s: the detour
/// path is taken only for a turn with no compile.)
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

/// A detoured plain turn writes no more frames than it did: the row rides the
/// frame of its `loop.started`. The frames are counted as
/// `tests_m3::a_plain_turn_stays_within_its_frame_budget` counts them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detoured_plain_turn_keeps_its_frames() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.92);
    let (r, _haiku) = rig_haiku(&jev, 2, |_| {});
    let first = turn(&r.core, None, "warm up", None).await;
    let before = r.core.store.stats().unwrap().frames_appended;
    let res = turn(&r.core, Some(&first.session_id), "hi", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(res.loops, 1);
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(frames <= 4, "a detoured plain turn wrote {frames} frames");
    // The row and `loop.started` are in one frame: adjacent in the log.
    let kinds: Vec<String> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(1000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row.kind)
        .collect();
    let at = kinds.iter().rposition(|k| k == "context.detoured").unwrap();
    assert_eq!(kinds[at + 1], "loop.started", "{kinds:?}");
}

/// The owner's config in shape: its AWS account and its five personas (whose
/// files the full request carries and a detour does not).
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
    // A context file the full request carries (about 2,000 tokens) and a detour leaves out.
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

/// theseus-3bs4e: a detour naming deferred families on the owner's cli sends a
/// first request within the place's budget, counted on the request it sends
/// (its persona files are not in it, so a small family still fits beside the
/// tools): the AWS family it names does not fit and is left out, and the small
/// family it names after it loads, whatever the order of the message.
///
/// theseus-xlri5: this pins the rollback. With `[tools.defer] admit_ceiling = 1.0`
/// the cap is the budget again, byte for byte the rule before admitted families
/// could raise it (`tests_families` holds the raise).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_naming_a_family_stays_within_its_place_budget() {
    let jev = FakeJev::start().unwrap();
    // A quick detour: a trivial one carries `tools_find` alone (theseus-d9je7).
    mode(&jev, "quick", 0.95);
    let dir = tempfile::tempdir().unwrap();
    let (r, haiku) = rig_haiku(&jev, 1, |c| {
        owner_shape(c, dir.path());
        c.tools.defer.admit_ceiling = 1.0;
    });
    let res = turn(
        &r.core,
        None,
        "Call aws_whoami to see which account you are on, then owner_ask me which stack to look at.",
        None,
    )
    .await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    let first = &haiku.requests()[0];
    let names = crate::tests_places::offered(first);
    assert!(names.contains(&"owner_ask".to_string()), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("aws_")), "{names:?}");
    let row = rows(&r.core, "context.detoured");
    let est = row[0]["est_tokens"].as_u64().unwrap();
    let budget = crate::first_request::CLI_TASK_TOKENS;
    assert!(
        est <= budget,
        "the detour's request is {est} tokens, over {budget}: {}",
        row[0]
    );
}

