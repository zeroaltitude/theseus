//! A turn's effort keeps the cache (theseus-o719; `compiler::effort`): on a
//! model that takes per-message effort, a turn route.v3 puts at another level
//! sends the earlier request's bytes unchanged, with the level as an
//! effort-only system message before its own message, and the top-level
//! effort stays the profile's. Before the fix the top-level effort changed,
//! which restarts the provider's cache of the messages: a conversation of
//! 625k tokens on Fable 5.1 wrote 612k of them again for one such turn. The
//! rig is `tests_route_effort`'s.

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::compiler::effort::effective;
use crate::compiler::BETA_TURN_EFFORT;
use crate::config::Effort;
use crate::node::Body;
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_route::{mode, rig, turn};

pub(crate) fn effort(jev: &FakeJev, level: &str, confidence: f64) {
    jev.script(
        "effort",
        Jev::Choice {
            option: level.into(),
            confidence,
        },
    );
}

/// `later` begins with every byte `earlier` sent before its cache breakpoint:
/// the same tools, system and top-level effort, and `earlier`'s messages.
fn keeps(earlier: &ProviderRequest, later: &ProviderRequest, what: &str) {
    assert_eq!(later.model, earlier.model, "{what}: the model");
    assert_eq!(later.tools, earlier.tools, "{what}: the tools");
    assert_eq!(later.system, earlier.system, "{what}: the system");
    assert_eq!(
        later.output_config, earlier.output_config,
        "{what}: the top-level effort"
    );
    assert_eq!(later.thinking, earlier.thinking, "{what}: the thinking");
    let n = earlier.messages.len();
    assert!(later.messages.len() > n, "{what}: nothing appended");
    assert_eq!(
        serde_json::to_string(&later.messages[..n]).unwrap(),
        serde_json::to_string(&earlier.messages).unwrap(),
        "{what}: the messages before the breakpoint changed"
    );
}

/// The effort each answer of the session recorded.
fn recorded(r: &crate::tests_route::Rig, session: &str) -> Vec<Option<Effort>> {
    r.core
        .store
        .transcript(session)
        .unwrap()
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::AssistantMessage { effort, .. } => Some(*effort),
            _ => None,
        })
        .collect()
}

/// A turn's later loops carry its level from its first answer's record: the
/// second loop's request repeats the first's, message included, and places
/// none of its own; only the first answer records it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turns_later_loops_repeat_its_message() {
    let jev = FakeJev::start().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work = work.path().canonicalize().unwrap();
    std::fs::write(work.join("a.txt"), "the heron nests by the weir").unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "low", 0.95);
    let r = rig(Some(&jev), 0, |c| {
        c.model.live = "fable".into();
        c.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    });
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(Scripted::tools(
            "",
            &[("r1", "fs_read", json!({"path": "a.txt"}))],
        ));
        s.push_back(Scripted::text("By the weir."));
    }
    let res = turn(&r.core, None, "Where does the heron nest?", None).await;
    assert_eq!(res.loops, 2);
    let reqs = r.claude.requests();
    keeps(&reqs[0], &reqs[1], "the second loop");
    let systems = |q: &ProviderRequest| q.messages.iter().filter(|m| m["role"] == "system").count();
    assert_eq!((systems(&reqs[0]), systems(&reqs[1])), (1, 1));
    assert_eq!(effective(&reqs[1]), Some(Effort::Low));
    assert_eq!(recorded(&r, &res.session_id), [Some(Effort::Low), None]);
}

/// A model that is named as taking per-message effort and refuses it anyway
/// (Sonnet 5.5 did, 2026-10-11): the provider's 400 that names `output_config`
/// is asked again once in the same turn, without the effort-only messages and
/// with the turn's level at the top; a `provider.effort_refused` row says so,
/// and the model takes none for the next hour (theseus-posww).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_400_on_the_messages_asks_again_without_them_and_remembers() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "low", 0.95);
    let r = rig(Some(&jev), 0, |c| {
        c.model.live = "fable".into();
        c.cache.keep_warm_min_tokens = 30_000;
    });
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
            status: 400,
            message: "invalid_request_error: messages.0.output_config: Extra inputs are not permitted"
                .into(),
        }));
        s.push_back(Scripted::text("By the weir."));
        s.push_back(Scripted::text("Nor in winter."));
    }
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    assert_eq!(one.output, "By the weir.");
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 2, "asked again once");
    let carries = |q: &ProviderRequest| q.messages.iter().any(|m| m["role"] == "system");
    assert!(carries(&reqs[0]) && reqs[0].betas.iter().any(|b| b == BETA_TURN_EFFORT));
    assert!(!carries(&reqs[1]) && !reqs[1].betas.iter().any(|b| b == BETA_TURN_EFFORT));
    assert_eq!(
        effective(&reqs[1]),
        Some(Effort::Low),
        "the level moved to the top"
    );
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = r.core.store.ledger_tail(2000).unwrap();
    let refused = rows
        .iter()
        .filter(|(_, l)| l.kind == "provider.effort_refused")
        .count();
    assert_eq!(refused, 1);
    // The next message, at another level, sends none either.
    effort(&jev, "medium", 0.95);
    turn(&r.core, Some(&one.session_id), "And in winter?", None).await;
    let next = r.claude.requests().pop().unwrap();
    assert!(!carries(&next), "{:?}", next.messages);
    assert_eq!(effective(&next), Some(Effort::Medium));
    assert_eq!(r.claude.requests().len(), 3, "no further failed call");
}
