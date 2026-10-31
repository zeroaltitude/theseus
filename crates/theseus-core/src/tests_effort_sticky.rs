//! A change of Jev's effort stops rewriting the conversation's cache
//! (theseus-rwpbg.4). On a model without per-message effort a change of the
//! top-level effort restarts the cache of the messages, so on a long
//! conversation a one-step change is held at the previous request's level
//! (`routing::effort::sticky`, `TurnRunner::stick`); a two-step change, a
//! short conversation and a first request apply Jev's level. Keep-warm
//! replays the effort the last real request carried, and which models take
//! per-message effort is the catalog's. The rig is `tests_route_effort`'s.

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::catalog::CatalogRow;
use crate::compiler::BETA_TURN_EFFORT;
use crate::config::Effort;
use crate::keep_warm::{Outcome, MAX_TOKENS};
use crate::provider::ProviderRequest;
use crate::routing::effort::{name, sticky};
use crate::tests_route::{decided, mode, rig, turn, Rig};
use crate::Config;

fn effort(jev: &FakeJev, level: &str) {
    jev.script(
        "effort",
        Jev::Choice {
            option: level.into(),
            confidence: 0.95,
        },
    );
}

/// A conversation under a floor of one token: every request is long.
fn long(c: &mut Config) {
    c.cache.keep_warm_min_tokens = 1;
}

/// Sonnet 5.5 as a model without per-message effort, the case sticky
/// effort is for (the built-in row takes it since the live probe).
fn no_per_message(c: &mut Config) {
    let row = CatalogRow {
        per_message_effort: Some(false),
        ..Default::default()
    };
    c.catalog.extend([("claude-sonnet-5-5".to_string(), row)]);
}

/// `talk_on` with Sonnet 5.5 taking no per-message effort.
async fn talk(
    levels: &[&str],
    tweak: impl FnOnce(&mut Config),
) -> (
    Rig,
    Vec<ProviderRequest>,
    Vec<Value>,
    theseus_protocol::TurnSubmitResult,
) {
    talk_on(levels, |c| {
        no_per_message(c);
        tweak(c);
    })
    .await
}

/// One conversation on the live profile (Sonnet 5.5), a turn for each level
/// Jev answers; the requests, the `route.decided` rows and the last result.
async fn talk_on(
    levels: &[&str],
    tweak: impl FnOnce(&mut Config),
) -> (
    Rig,
    Vec<ProviderRequest>,
    Vec<Value>,
    theseus_protocol::TurnSubmitResult,
) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), levels.len(), tweak);
    let mut session = None;
    let mut last = None;
    for (i, level) in levels.iter().enumerate() {
        effort(&jev, level);
        let res = turn(&r.core, session.as_deref(), &format!("Question {i}?"), None).await;
        session = Some(res.session_id.clone());
        last = Some(res);
    }
    let reqs = r.claude.requests();
    let rows = decided(&r.core.store);
    (r, reqs, rows, last.unwrap())
}

/// The top-level effort a request carries.
fn top(req: &ProviderRequest) -> Option<String> {
    req.output_config
        .as_ref()
        .and_then(|o| o["effort"].as_str().map(str::to_string))
}

/// `later` begins with every byte `earlier` sent before its breakpoint.
fn keeps(earlier: &ProviderRequest, later: &ProviderRequest) {
    assert_eq!(later.system, earlier.system);
    assert_eq!(later.tools, earlier.tools);
    assert_eq!(
        later.output_config, earlier.output_config,
        "top-level effort"
    );
    let n = earlier.messages.len();
    assert_eq!(
        serde_json::to_string(&later.messages[..n]).unwrap(),
        serde_json::to_string(&earlier.messages).unwrap(),
    );
}

/// A long conversation on a model without per-message effort: Jev asks
/// medium, then high. The second request keeps medium, so the top-level
/// effort, and with it the messages' cache, is the first request's; the row
/// says `sticky` and keeps both levels.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sticky_effort_holds_a_one_level_change_on_a_long_conversation() {
    let (_r, reqs, rows, _) = talk(&["medium", "high"], long).await;
    assert_eq!(reqs[0].model, "claude-sonnet-5-5");
    assert_eq!(
        top(&reqs[0]).as_deref(),
        Some("medium"),
        "no previous level"
    );
    assert_eq!(top(&reqs[1]).as_deref(), Some("medium"), "held");
    keeps(&reqs[0], &reqs[1]);
    let d = &rows[1];
    assert_eq!(
        (&d["effort_reason"], &d["effort_ran"], &d["effort_sent"]),
        (&json!("sticky"), &json!("high"), &json!("medium"))
    );
    assert_eq!(d["effort"], "high", "what Jev answered");
    assert_eq!(rows[0]["effort_reason"], "applied");
    assert!(rows[0].get("effort_sent").is_none());
}

/// A short conversation has little cache to lose: Jev's change applies.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sticky_effort_applies_under_the_prefix_floor() {
    let (_r, reqs, rows, _) = talk(&["medium", "high"], |c| {
        c.cache.keep_warm_min_tokens = 30_000
    })
    .await;
    assert_eq!(top(&reqs[1]).as_deref(), Some("high"));
    assert_eq!(rows[1]["effort_reason"], "applied");
    assert!(rows[1].get("effort_sent").is_none());
}

/// A change of two steps or more applies even on a long conversation, and the
/// bounds and the evidence bar act first (xhigh at 0.95 is above the bar).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sticky_effort_applies_a_two_level_change() {
    let (_r, reqs, rows, _) = talk(&["medium", "xhigh", "low"], long).await;
    assert_eq!(top(&reqs[1]).as_deref(), Some("xhigh"));
    assert_eq!(rows[1]["effort_reason"], "applied");
    assert_eq!(top(&reqs[2]).as_deref(), Some("low"), "xhigh to low");
    assert_eq!(rows[2]["effort_reason"], "applied");
}

/// The route's record, and the turn's result, say what Jev asked and what the
/// request carried.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_route_records_asked_and_sent_effort() {
    let (_r, reqs, rows, res) = talk(&["low", "medium"], long).await;
    assert_eq!(top(&reqs[1]).as_deref(), Some("low"));
    let d = &rows[1];
    assert_eq!(
        (
            d["effort"].clone(),
            d["effort_ran"].clone(),
            d["effort_sent"].clone()
        ),
        (json!("medium"), json!("medium"), json!("low"))
    );
    assert_eq!(
        (d["effort_reason"].clone(), d["effort_applied"].clone()),
        (json!("sticky"), json!(true))
    );
    let ro = res.route.unwrap();
    assert_eq!(
        (
            ro.effort.as_deref(),
            ro.effort_reason.as_deref(),
            ro.effort_applied.as_deref()
        ),
        (Some("medium"), Some("sticky"), Some("low")),
        "the result's applied level is the one sent"
    );
}

/// The rule, alone: it never moves more than one step from the previous level.
#[test]
fn sticky_is_one_step_at_most() {
    use Effort::*;
    let all = [Low, Medium, High, Xhigh, Max];
    for p in all {
        for a in all {
            let held = sticky(a, Some(p), 50_000, 20_000);
            let far = (p as i8 - a as i8).abs() >= 2;
            assert_eq!(
                held,
                (p != a && !far).then_some(p),
                "{} to {}",
                name(p),
                name(a)
            );
            assert_eq!(sticky(a, Some(p), 100, 20_000), None, "under the floor");
        }
        assert_eq!(sticky(p, None, 50_000, 20_000), None, "no previous level");
    }
}

/// Keep-warm sends the effort the last real request carried (Jev's), not the
/// profile's, so its read repeats that request's prefix.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_warm_replays_the_last_effort() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "low");
    let r = rig(Some(&jev), 1, |c| {
        // The template with a 1-hour `[model]`, under the rig's own sections.
        let mut hour = crate::tests_keep_warm::one_hour(24);
        hour.server = c.server.clone();
        hour.tools = c.tools.clone();
        hour.judge = c.judge.clone();
        hour.routing = c.routing.clone();
        hour.memory = c.memory.clone();
        *c = hour;
        no_per_message(c);
    });
    let res = turn(&r.core, None, "Where do the herons nest?", None).await;
    let sid = res.session_id;
    let own = r.core.runner.cfg.all_profiles()["sonnet"].effort;
    assert_ne!(
        own,
        Some(Effort::Low),
        "the profile's own differs from Jev's"
    );
    r.core.runner.keep_warm.message(&sid, true);
    let (live, _) = r.core.live_profile();
    let out = r.core.runner.keep_warm_once(&sid, &live).await;
    assert!(matches!(out, Outcome::Read { .. }), "{out:?}");
    let reqs = r.claude.requests();
    let (turned, read) = (&reqs[0], reqs.last().unwrap());
    assert_eq!(read.max_tokens, MAX_TOKENS);
    assert_eq!(top(turned).as_deref(), Some("low"));
    keeps(turned, read);
}

/// Which models take per-message effort is catalog data: one row's field
/// takes Sonnet 5.5's effort-only message away (the top-level effort then
/// carries the level), and takes it from Fable 5.1, with no other change.
/// Built in, Fable 5.1 and Sonnet 5.5 take it and Opus 5.5 does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn per_message_effort_is_catalog_data() {
    let flip = |model: &str, on: bool| {
        let row = CatalogRow {
            per_message_effort: Some(on),
            ..Default::default()
        };
        (model.to_string(), row)
    };
    let carries = |q: &ProviderRequest| q.messages.iter().any(|m| m["role"] == "system");
    let built = crate::catalog::Catalog::builtin();
    let takes = |id: &str| built.get(id).unwrap().per_message_effort;
    assert!(takes("claude-fable-5-1") && takes("claude-sonnet-5-5"));
    assert!(!takes("claude-opus-5-5"), "Opus 5.5's effort stays sticky");
    // Sonnet 5.5 as the built-in has it: an effort-only message.
    let (_r, reqs, _, _) = talk_on(&["low"], |_| {}).await;
    assert!(carries(&reqs[0]) && reqs[0].betas.iter().any(|b| b == BETA_TURN_EFFORT));
    assert_ne!(
        top(&reqs[0]).as_deref(),
        Some("low"),
        "the profile's own stays on top"
    );
    // The same turn with the row turned off: the level is the top-level one.
    let (_r, reqs, _, _) = talk_on(&["low"], |c| {
        c.catalog.extend([flip("claude-sonnet-5-5", false)]);
    })
    .await;
    assert!(!carries(&reqs[0]) && !reqs[0].betas.iter().any(|b| b == BETA_TURN_EFFORT));
    assert_eq!(top(&reqs[0]).as_deref(), Some("low"));
    // Fable 5.1 with it turned off.
    let (_r, reqs, _, _) = talk(&["low"], |c| {
        c.model.live = "fable".into();
        c.catalog.extend([flip("claude-fable-5-1", false)]);
    })
    .await;
    assert!(!carries(&reqs[0]));
    assert_eq!(top(&reqs[0]).as_deref(), Some("low"));
}

/// A long conversation on a model without per-message effort, a turn for
/// each (level, confidence) Jev answers; the requests and the route rows.
async fn talk_at(answers: &[(&str, f64)]) -> (Rig, Vec<ProviderRequest>, Vec<Value>) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), answers.len(), |c| {
        no_per_message(c);
        long(c);
    });
    let mut session = None;
    for (i, (level, confidence)) in answers.iter().enumerate() {
        jev.script(
            "effort",
            Jev::Choice {
                option: (*level).into(),
                confidence: *confidence,
            },
        );
        let res = turn(&r.core, session.as_deref(), &format!("Question {i}?"), None).await;
        session = Some(res.session_id);
    }
    let reqs = r.claude.requests();
    let rows = decided(&r.core.store);
    (r, reqs, rows)
}

fn reasons(rows: &[Value]) -> Vec<&str> {
    rows.iter()
        .map(|d| d["effort_reason"].as_str().unwrap_or(""))
        .collect()
}

/// A `sticky` hold ends when a later sure answer still disagrees with it
/// (theseus-u5a37): turn 2 is held at high, turn 3 asks medium again, sure,
/// and the request carries medium; the reason is `sticky_reverted`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_sure_answer_ends_a_sticky_hold() {
    let (_r, reqs, rows) = talk_at(&[("high", 0.95), ("medium", 0.95), ("medium", 0.95)]).await;
    assert_eq!(reasons(&rows), ["applied", "sticky", "sticky_reverted"]);
    assert_eq!(top(&reqs[1]).as_deref(), Some("high"), "held");
    assert_eq!(top(&reqs[2]).as_deref(), Some("medium"), "reverted");
    let d = &rows[2];
    assert_eq!(
        (&d["effort_ran"], d.get("effort_sent")),
        (&json!("medium"), None)
    );
    assert_eq!(d["effort_applied"], true);
}

/// The turn that opens a hold never reverts it on its own answer, and an
/// answer in no window is held as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turn_that_opens_a_hold_does_not_revert_it() {
    let (_r, _reqs, rows) = talk_at(&[("high", 0.95), ("medium", 0.95)]).await;
    assert_eq!(reasons(&rows), ["applied", "sticky"]);
}
