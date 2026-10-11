//! A turn's effort keeps the cache (theseus-o719; `compiler::effort`): on a
//! model that takes per-message effort, a turn route.v3 puts at another level
//! sends the earlier request's bytes unchanged, with the level as an
//! effort-only system message before its own message, and the top-level
//! effort stays the profile's. Before the fix the top-level effort changed,
//! which restarts the provider's cache of the messages: a conversation of
//! 625k tokens on Fable 5.1 wrote 612k of them again for one such turn. The
//! rig is `tests_route_effort`'s.

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::compiler::effort::effective;
use crate::compiler::BETA_TURN_EFFORT;
use crate::config::Effort;
use crate::node::Body;
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_route::{mode, rig, turn};

fn effort(jev: &FakeJev, level: &str, confidence: f64) {
    jev.script(
        "reply_effort",
        Jev::Choice {
            option: level.into(),
            confidence,
        },
    );
}

/// The effort-only system message of `level`.
fn says(level: &str) -> Value {
    json!({"role": "system", "content": [], "output_config": {"effort": level}})
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

/// The owner's case on Sonnet 5.5 (whose profile leaves effort to the model):
/// a turn at the model's own level, one Jev puts at `low`, and one at the
/// model's own again. Each request repeats the one before it whole; the
/// `low` turn's message stays where it went, and the next turn's message
/// puts the level back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_at_jevs_effort_repeats_the_cached_prefix() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "unclear", 0.95);
    let r = rig(Some(&jev), 3, |_| {});
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    assert_eq!(one.profile, "sonnet");
    effort(&jev, "low", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "And the egrets?", None).await;
    assert_eq!(two.route.unwrap().effort_applied.as_deref(), Some("low"));
    effort(&jev, "unclear", 0.95);
    turn(&r.core, Some(&one.session_id), "Thanks.", None).await;
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 3);
    let (r1, r2, r3) = (&reqs[0], &reqs[1], &reqs[2]);
    assert_eq!(
        r1.output_config, None,
        "the profile's own: the model's default"
    );
    assert!(r1.messages.iter().all(|m| m["role"] != "system"));
    keeps(r1, r2, "the low turn");
    let n = r1.messages.len();
    let roles: Vec<&Value> = r2.messages[n..].iter().map(|m| &m["role"]).collect();
    assert_eq!(roles, ["assistant", "system", "user"], "{:?}", r2.messages);
    assert_eq!(r2.messages[n + 1], says("low"));
    assert_eq!(effective(r2), Some(Effort::Low));
    assert!(r2.betas.iter().any(|b| b == BETA_TURN_EFFORT));
    keeps(r2, r3, "the turn after it");
    assert_eq!(r3.messages[r2.messages.len() + 1], says("high"));
    assert_eq!(effective(r3), Some(Effort::High), "Sonnet 5.5's default");
    assert_eq!(
        recorded(&r, &one.session_id),
        [None, Some(Effort::Low), Some(Effort::High)]
    );
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

/// The history's messages come from its answers' records, whatever the
/// request's top-level effort: on Opus 5.5 (the owner's choice) the `low`
/// turn's message is still just before its user message; Haiku 5.5, which
/// takes no per-message effort, gets none and its profile's own `low` at the
/// top level.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_historys_messages_follow_the_records() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "low", 0.95);
    let r = rig(Some(&jev), 3, |_| {});
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    effort(&jev, "unclear", 0.95);
    let sid = one.session_id.clone();
    assert_eq!(
        turn(&r.core, Some(&sid), "And now?", Some("opus"))
            .await
            .profile,
        "opus"
    );
    assert_eq!(
        turn(&r.core, Some(&sid), "Thanks.", Some("haiku"))
            .await
            .profile,
        "haiku"
    );
    let reqs = r.claude.requests();
    let (opus, haiku) = (&reqs[1], &reqs[2]);
    assert_eq!(opus.model, "claude-opus-5-5");
    assert_eq!(
        (&opus.messages[0], &opus.messages[1]),
        (&says("low"), &reqs[0].messages[1])
    );
    assert_eq!(
        opus.messages[3],
        says("medium"),
        "Opus 5.5's default puts it back"
    );
    assert_eq!(haiku.model, "claude-haiku-5-5");
    assert!(
        haiku.messages.iter().all(|m| m["role"] != "system"),
        "{:?}",
        haiku.messages
    );
    assert!(!haiku.betas.iter().any(|b| b == BETA_TURN_EFFORT));
    assert_eq!(effective(haiku), Some(Effort::Low));
}
