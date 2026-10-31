//! route.v4's effort (theseus-qe3v; `routing::effort`, `turn::route_step`):
//! the reply's effort is asked in the route pack's own request beside the
//! mode, and the turn's requests carry it, over the profile's own, only for a
//! model that takes effort, within `[routing] effort_bounds`, unless the
//! profile fixes its own; below the bar, on `unclear`, on a pinned turn and
//! for a late verdict the profile's own stands. `route.decided` and the
//! turn's result say which. The rig is `tests_route`'s: `anthropic` (Sonnet
//! 5.5, Opus 5.5, Haiku 5.5) and `zai` (GLM) fakes, so each request shows its
//! model and its `output_config`.

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};

use crate::provider::Scripted;
use crate::tests_route::{decided, mode, rig, turn, until_late};

/// Jev's effort answer, scripted.
fn effort(jev: &FakeJev, level: &str, confidence: f64) {
    jev.script(
        "effort",
        Jev::Choice {
            option: level.into(),
            confidence,
        },
    );
}

/// The effort a request carries, if any: its turn's effort message's on a
/// model that takes per-message effort (theseus-o719), else its top-level one.
fn sent(req: &crate::provider::ProviderRequest) -> Option<String> {
    crate::compiler::effort::effective(req).map(|e| crate::routing::effort::name(e).to_string())
}

/// The row's effort fields.
fn row(d: &Value) -> (Value, Value, Value, Value) {
    (
        d["effort"].clone(),
        d["effort_reason"].clone(),
        d["effort_applied"].clone(),
        d["effort_ran"].clone(),
    )
}

/// Every loop of the turn carries Jev's effort: the second loop's request
/// (after a tool call) as the first's; the next message's verdict decides
/// its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_loop_of_the_turn_carries_the_effort() {
    let jev = FakeJev::start().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work = work.path().canonicalize().unwrap();
    std::fs::write(work.join("a.txt"), "the heron nests by the weir").unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "medium", 0.95);
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
        s.push_back(Scripted::text("Done."));
    }
    let res = turn(&r.core, None, "Where does the heron nest?", None).await;
    assert_eq!((res.loops, res.profile.as_str()), (2, "sonnet"));
    let reqs = r.claude.requests();
    assert_eq!(
        reqs.iter().map(sent).collect::<Vec<_>>(),
        [Some("medium".to_string()), Some("medium".to_string())],
        "both loops"
    );
    effort(&jev, "unclear", 0.95);
    turn(&r.core, Some(&res.session_id), "And in winter?", None).await;
    // The profile leaves effort to the model, and Sonnet 5.5 takes
    // per-message effort (the 2026-10-17 probe, theseus-rwpbg.4): the turn
    // says the per-message default, high, as Fable 5.1's does.
    assert_eq!(
        sent(&r.claude.requests()[2]).as_deref(),
        Some("high"),
        "the per-message default again"
    );
}

/// The digest the turn records of each request (the assistant node's
/// `request_digest`, the call's span) is the digest of the request sent, the
/// first loop's effort included: the first request is patched after its
/// compile, so its digest is taken again (the review's join fix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_recorded_digest_is_the_request_sent_with_its_effort() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "max", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe log.",
        None,
    )
    .await;
    assert_eq!(res.profile, "opus");
    let sent_digests: Vec<String> = r.claude.requests().iter().map(|q| q.digest()).collect();
    let recorded: Vec<String> = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            crate::node::Body::AssistantMessage { request_digest, .. } => request_digest,
            _ => None,
        })
        .collect();
    assert_eq!(sent(&r.claude.requests()[0]).as_deref(), Some("max"));
    assert_eq!(recorded, sent_digests);
}

/// A late verdict carries its mode to the next message, never its effort:
/// that message's turn moves to Opus 5.5 and runs at the profile's own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_carries_its_mode_and_never_its_effort() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "max", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig(Some(&jev), 2, |c| {
        c.routing.max_wait_ms = 200;
        c.judge.total_secs = 30;
    });
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(one.route.as_ref().unwrap().reason, "late");
    jev.release();
    until_late(&r.core, &one.session_id).await;
    // The next message's own verdict is held too: it reads the late one.
    let next = turn(&r.core, Some(&one.session_id), "And the index?", None).await;
    jev.release();
    assert_eq!(next.profile, "opus", "the late verdict's mode");
    let req = &r.claude.requests()[1];
    assert_eq!((req.model.as_str(), sent(req)), ("claude-opus-5-5", None));
    let d = &decided(&r.core.store)[1];
    assert_eq!(
        row(d),
        (json!("max"), json!("carried"), json!(false), Value::Null)
    );
}

/// A detour runs at Jev's effort too: a quick question on `haiku` (whose
/// profile says `low`) at medium, for that turn alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_runs_at_jevs_effort() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.95);
    effort(&jev, "medium", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "which port is the cockpit on?", None).await;
    assert_eq!(res.profile, "haiku");
    let req = &r.claude.requests()[0];
    assert_eq!(
        (req.model.as_str(), sent(req).as_deref()),
        ("claude-haiku-5-5", Some("medium"))
    );
}
