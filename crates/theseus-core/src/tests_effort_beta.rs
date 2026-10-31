//! The beta rides every request that carries an effort-only message
//! (theseus-blzt6; `compiler::effort::place`). Before the fix only a request
//! whose last message opens a turn got it, so a request after a tool call,
//! whose history held the turn's own message, went out with the message and
//! without the beta, and the provider answered `messages.N.output_config:
//! Extra inputs are not permitted`. The rig is `tests_route`'s.

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::compiler::BETA_TURN_EFFORT;
use crate::provider::{ProviderRequest, Scripted};
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

fn carries_message(q: &ProviderRequest) -> bool {
    q.messages.iter().any(|m| m["role"] == "system")
}

fn carries_beta(q: &ProviderRequest) -> bool {
    q.betas.iter().any(|b| b == BETA_TURN_EFFORT)
}

/// A first turn at the profile's level, then one Jev puts at `low` that reads
/// a file and answers: every request carries the beta exactly when it carries
/// a message, the one after the tool call included. Sonnet 5.5's profile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_loop_request_with_an_effort_message_carries_the_beta() {
    let jev = FakeJev::start().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work = work.path().canonicalize().unwrap();
    std::fs::write(work.join("a.txt"), "the heron nests by the weir").unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "unclear");
    let r = rig(Some(&jev), 0, |c| {
        c.model.live = "sonnet".into();
        c.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    });
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(Scripted::text("Herons."));
        s.push_back(Scripted::tools(
            "",
            &[("r1", "fs_read", json!({"path": "a.txt"}))],
        ));
        s.push_back(Scripted::text("By the weir."));
    }
    let one = turn(&r.core, None, "Which birds are here?", None).await;
    effort(&jev, "low");
    let two = turn(&r.core, Some(&one.session_id), "Where do they nest?", None).await;
    assert_eq!(two.loops, 2);
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 3);
    for (i, q) in reqs.iter().enumerate() {
        assert_eq!(q.model, "claude-sonnet-5-5");
        assert_eq!(
            carries_beta(q),
            carries_message(q),
            "request {i}: a message and the beta go together: {:?} {:?}",
            q.messages,
            q.betas
        );
    }
    assert!(
        carries_message(&reqs[2]),
        "the loop-1 request holds the turn's message"
    );
}

/// An answer recorded at `low` under Sonnet 5.5 is no message for Haiku 5.5,
/// which takes none: no request of the Haiku turn carries one or the beta.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_without_per_message_effort_gets_no_message_from_another_models_record() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "unclear");
    let r = rig(Some(&jev), 3, |c| c.model.live = "sonnet".into());
    let one = turn(&r.core, None, "Which birds are here?", None).await;
    let sid = one.session_id.clone();
    effort(&jev, "low");
    turn(&r.core, Some(&sid), "Where do they nest?", None).await;
    let first = r.claude.requests().pop().unwrap();
    assert!(
        carries_message(&first) && carries_beta(&first),
        "the record's source"
    );
    turn(&r.core, Some(&sid), "And the egrets?", Some("haiku")).await;
    let reqs = r.claude.requests();
    let haiku = reqs.last().unwrap();
    assert_eq!(haiku.model, "claude-haiku-5-5");
    assert!(!carries_message(haiku), "{:?}", haiku.messages);
    assert!(!carries_beta(haiku), "{:?}", haiku.betas);
}
