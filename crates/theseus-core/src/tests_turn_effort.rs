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

pub(crate) fn effort(jev: &FakeJev, level: &str, confidence: f64) {
    jev.script(
        "reply_effort",
        Jev::Choice {
            option: level.into(),
            confidence,
        },
    );
}

/// The effort-only system message of `level`.
pub(crate) fn says(level: &str) -> Value {
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

/// The owner's case on Fable 5.1, the one model that takes per-message effort
/// (whose profile leaves effort to the model; theseus-posww):
/// a turn at the model's own level, one Jev puts at `low`, and one at the
/// model's own again. Each request repeats the one before it whole; the
/// `low` turn's message stays where it went, and the next turn's message
/// puts the level back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_at_jevs_effort_repeats_the_cached_prefix() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "leave", 0.95);
    let r = rig(Some(&jev), 3, |c| c.model.live = "fable".into());
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    assert_eq!(one.profile, "fable");
    effort(&jev, "low", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "And the egrets?", None).await;
    assert_eq!(two.route.unwrap().effort_applied.as_deref(), Some("low"));
    effort(&jev, "leave", 0.95);
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
    assert_eq!(effective(r3), Some(Effort::High), "Fable 5.1's default");
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

/// The history's messages come from its answers' records, whatever the
/// request's top-level effort: on Fable 5.1 the `low` turn's message is still
/// just before its user message. Opus 5.5 and Haiku 5.5, which are not known
/// to take per-message effort (theseus-posww), get none, and the beta rides
/// no request of theirs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_historys_messages_follow_the_records() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "low", 0.95);
    let r = rig(Some(&jev), 4, |c| c.model.live = "fable".into());
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    effort(&jev, "leave", 0.95);
    let sid = one.session_id.clone();
    assert_eq!(
        turn(&r.core, Some(&sid), "And now?", None).await.profile,
        "fable"
    );
    for (name, model) in [("opus", "claude-opus-5-5"), ("haiku", "claude-haiku-5-5")] {
        let res = turn(&r.core, Some(&sid), "Thanks.", Some(name)).await;
        assert_eq!(res.profile, name);
        let req = r.claude.requests().pop().unwrap();
        assert_eq!(req.model, model);
        assert!(
            req.messages.iter().all(|m| m["role"] != "system"),
            "{name}: {:?}",
            req.messages
        );
        assert!(!req.betas.iter().any(|b| b == BETA_TURN_EFFORT), "{name}");
    }
    let reqs = r.claude.requests();
    let haiku = reqs.last().unwrap();
    assert_eq!(effective(haiku), Some(Effort::Low), "Haiku's profile's own");
    let fable = &reqs[1];
    assert_eq!(fable.model, "claude-fable-5-1");
    assert_eq!(
        (&fable.messages[0], &fable.messages[1]),
        (&says("low"), &reqs[0].messages[1])
    );
    assert_eq!(
        fable.messages[3],
        says("high"),
        "Fable 5.1's default puts it back"
    );
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

/// The owner's call of 2026-10-23 (theseus-goq94): `xhigh` costs about twice a
/// `high`, so it applies only on a sure answer. Jev's `xhigh` at 0.45 is above
/// the 0.40 bar but under the effort question's confirm bar, and the request
/// carries `high`; the same answer on `max` keeps the bar alone and runs at
/// `max` (`an_unsure_max_at_0_42_runs_at_max_on_the_effort_bar`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_xhigh_above_the_bar_runs_high() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "xhigh", 0.45);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe log.",
        None,
    )
    .await;
    assert_eq!(effective(&r.claude.requests()[0]), Some(Effort::High));
    let route = res.route.unwrap();
    assert_eq!(
        (
            route.effort.as_deref(),
            route.effort_reason.as_deref(),
            route.effort_applied.as_deref()
        ),
        (Some("xhigh"), Some("held_high_unsure"), Some("high"))
    );
}
