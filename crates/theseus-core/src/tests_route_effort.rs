//! route.v3's effort (theseus-qe3v; `routing::effort`, `turn::route_step`):
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
use crate::tests_route::{decided, glm_placements, mode, rig, turn, until_calls, until_late};

/// Jev's effort answer, scripted.
fn effort(jev: &FakeJev, level: &str, confidence: f64) {
    jev.script(
        "reply_effort",
        Jev::Choice {
            option: level.into(),
            confidence,
        },
    );
}

/// The template with `haikuhi`'s effort fixed, under the rig's own sections.
fn fix_haikuhi(c: &mut crate::Config) {
    let text = crate::Config::EXAMPLE_TOML.replace(
        "[profiles.haikuhi]\n",
        "[profiles.haikuhi]\neffort_fixed = true\n",
    );
    let (mut fixed, _) = crate::Config::parse(&text).unwrap();
    assert!(fixed.all_profiles()["haikuhi"].effort_fixed);
    fixed.server = c.server.clone();
    fixed.tools = c.tools.clone();
    fixed.judge = c.judge.clone();
    fixed.routing = c.routing.clone();
    *c = fixed;
}

/// The effort a request carries, if any.
fn sent(req: &crate::provider::ProviderRequest) -> Option<String> {
    req.output_config
        .as_ref()
        .map(|o| o["effort"].as_str().unwrap().to_string())
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

/// One request asks both questions (the route pack's own, beside the batch:
/// two calls for the message, as before), and a sure answer sets the effort
/// of a model that takes effort, over the profile's own: a hard design goes
/// to Opus 5.5 at max, which its profile leaves to the model's default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sure_effort_reaches_the_request_of_a_model_that_takes_it() {
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
    until_calls(&jev, 2).await;
    assert_eq!(jev.connections(), 2, "no call more than route.v2's");
    let route = jev
        .seen()
        .into_iter()
        .find(|s| s.body["questions"].get("route.v3/mode").is_some())
        .expect("the route pack's request");
    assert!(
        route.body["questions"]
            .get("route.v3/reply_effort")
            .is_some(),
        "the effort, in the same request: {}",
        route.body["questions"]
    );
    assert_eq!(res.profile, "opus");
    let req = &r.claude.requests()[0];
    assert_eq!(req.model, "claude-opus-5-5");
    assert_eq!(sent(req).as_deref(), Some("max"));
    let d = &decided(&r.core.store)[0];
    assert_eq!(
        row(d),
        (json!("max"), json!("applied"), json!(true), json!("max"))
    );
    assert_eq!(d["effort_confidence"], 0.95);
    let ro = res.route.unwrap();
    assert_eq!(
        (
            ro.effort.as_deref(),
            ro.effort_reason.as_deref(),
            ro.effort_applied.as_deref()
        ),
        (Some("max"), Some("applied"), Some("max"))
    );
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
    assert_eq!(
        sent(&r.claude.requests()[2]),
        None,
        "the profile's own again"
    );
}

/// A model that takes no effort (GLM 5.3) gets none: the answer is recorded,
/// `no_effort`, and the request carries no `output_config`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_without_effort_ignores_it() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    effort(&jev, "high", 0.95);
    let r = rig(Some(&jev), 1, glm_placements);
    let res = turn(&r.core, None, "Rename these twelve call sites.", None).await;
    assert_eq!(res.profile, "glm53");
    let req = &r.zai.requests()[0];
    assert_eq!((req.model.as_str(), sent(req)), ("glm-5.3", None));
    let d = &decided(&r.core.store)[0];
    assert_eq!(
        row(d),
        (json!("high"), json!("no_effort"), json!(false), Value::Null)
    );
    assert_eq!(res.route.unwrap().effort_applied, None);
}

/// `[routing] effort_bounds` clamps an answer outside them to the nearer
/// bound, and the row says `clamped`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_bounds_clamp_the_effort() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "max", 0.95);
    let bounds = |c: &mut crate::Config| {
        c.routing.effort_bounds = [crate::config::Effort::Medium, crate::config::Effort::High];
    };
    let r = rig(Some(&jev), 2, bounds);
    let one = turn(&r.core, None, "Prove the log's recovery is correct.", None).await;
    effort(&jev, "low", 0.95);
    turn(&r.core, Some(&one.session_id), "ok", None).await;
    let reqs = r.claude.requests();
    assert_eq!(
        (sent(&reqs[0]).as_deref(), sent(&reqs[1]).as_deref()),
        (Some("high"), Some("medium"))
    );
    let d = decided(&r.core.store);
    assert_eq!(
        row(&d[0]),
        (json!("max"), json!("clamped"), json!(true), json!("high"))
    );
    assert_eq!(
        row(&d[1]),
        (json!("low"), json!("clamped"), json!(true), json!("medium"))
    );
    assert_eq!(one.route.unwrap().effort_applied.as_deref(), Some("high"));
}

/// Below the question's confirm bar (0.60), on `unclear`, and for a profile
/// that fixes its effort, the profile's own effort stands: routine
/// programming on `haikuhi` keeps its `high`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn below_the_bar_unclear_or_fixed_the_profiles_own_effort_stands() {
    for (level, confidence, fixed, reason) in [
        ("low", 0.55, false, "unsure"),
        ("unclear", 0.95, false, "unclear"),
        ("low", 0.95, true, "fixed"),
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "routine_coding", 0.95);
        effort(&jev, level, confidence);
        let r = rig(Some(&jev), 1, |c| {
            if fixed {
                fix_haikuhi(c);
            }
        });
        let res = turn(&r.core, None, "Rename these twelve call sites.", None).await;
        assert_eq!(res.profile, "haikuhi", "{reason}");
        let req = &r.claude.requests()[0];
        assert_eq!(sent(req).as_deref(), Some("high"), "{reason}");
        let d = &decided(&r.core.store)[0];
        assert_eq!(
            row(d),
            (json!(level), json!(reason), json!(false), json!("high")),
            "{reason}"
        );
        assert_eq!(res.route.unwrap().effort_applied, None, "{reason}");
    }
}

/// A turn the owner pinned records Jev's effort and applies none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_turn_records_the_effort_and_applies_none() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "max", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "Weigh two designs.", Some("sonnet")).await;
    assert_eq!(res.profile, "sonnet");
    assert_eq!(sent(&r.claude.requests()[0]), None);
    let d = &decided(&r.core.store)[0];
    assert_eq!(
        row(d),
        (json!("max"), json!("recorded"), json!(false), Value::Null)
    );
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
