//! A routed turn sends its first step again (issue 20, the review's B1): routing changes the
//! turn's first request after it is compiled (Jev's effort puts a message before the turn's own,
//! or takes the one the compile placed away, and a persona line is appended), so what the first
//! step sent after the conversation must be what routing left, and the second step must find the
//! turn's message where it moved to. The rig is `tests_route`'s.

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_binding::assert_same_prefix;
use crate::tests_route::{mode, rig, turn};

fn effort(jev: &FakeJev, level: &str) {
    jev.script(
        "effort",
        Jev::Choice {
            option: level.into(),
            confidence: 0.95,
        },
    );
}

/// The second turn of a session whose Jev says `first`, then `second`: it reads a file between its
/// two requests. Returns the second turn's two requests.
async fn routed_turn(first: &str, second: &str) -> Vec<ProviderRequest> {
    let jev = FakeJev::start().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work = work.path().canonicalize().unwrap();
    std::fs::write(work.join("a.txt"), "the heron nests by the weir").unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, first);
    let r = rig(Some(&jev), 0, |c| {
        c.model.live = "sonnet".into();
        c.project.tail_facts = Some(true);
        c.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    });
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(Scripted::text("Herons."));
        s.push_back(Scripted::Blocks {
            blocks: vec![
                json!({"type": "thinking", "thinking": "read it", "signature": "sig"}),
                json!({"type": "tool_use", "id": "r1", "name": "fs_read", "input": {"path": "a.txt"}}),
            ],
            stop_reason: "tool_use".into(),
        });
        s.push_back(Scripted::text("By the weir."));
    }
    let one = turn(&r.core, None, "Which birds are here?", None).await;
    effort(&jev, second);
    let two = turn(&r.core, Some(&one.session_id), "Where do they nest?", None).await;
    assert_eq!(two.loops, 2);
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 3);
    reqs[1..].to_vec()
}

/// Jev's level is the one in force: the compile placed an effort message and routing took it
/// away, so the turn's message moved up by one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turn_whose_effort_message_was_taken_away_sends_its_first_step_again() {
    let reqs = routed_turn("low", "low").await;
    assert_same_prefix(&reqs[0], &reqs[1]);
}

/// Jev's level changes: routing replaces the message in place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turn_whose_effort_message_was_replaced_sends_its_first_step_again() {
    let reqs = routed_turn("unclear", "low").await;
    assert_same_prefix(&reqs[0], &reqs[1]);
}
