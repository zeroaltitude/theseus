//! `[routing] effort_floor_confidence` (theseus-l7i9k, regress R4): the bar
//! under which an effort answer applies nothing, so the three reasons the
//! 1.0.3 build added (`floored`, `applied_unsure`, `held_high_unsure`) need a
//! confident answer. Off by default: the tests below set it through TOML, the
//! way an operator would, and a test pins that turning nothing on changes
//! nothing. The rig is `tests_route_effort`'s.

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::tests_route::{decided, mode, rig, turn};

fn effort(jev: &FakeJev, level: &str, confidence: f64) {
    jev.script(
        "reply_effort",
        Jev::Choice {
            option: level.into(),
            confidence,
        },
    );
}

/// `[routing]` with `effort_floor_confidence` set, written as TOML so the
/// test goes through the key an operator would write.
fn gate(c: &mut crate::Config, bar: f64) {
    let mut table = toml::Value::try_from(&c.routing).unwrap();
    table
        .as_table_mut()
        .unwrap()
        .insert("effort_floor_confidence".into(), toml::Value::Float(bar));
    c.routing = table.try_into().unwrap();
}

fn sent(req: &crate::provider::ProviderRequest) -> Option<String> {
    crate::compiler::effort::effective(req).map(|e| crate::routing::effort::name(e).to_string())
}

fn row(d: &Value) -> (Value, Value, Value, Value) {
    (
        d["effort"].clone(),
        d["effort_reason"].clone(),
        d["effort_applied"].clone(),
        d["effort_ran"].clone(),
    )
}

/// An `applied_unsure` leg: an unsure `max` at 0.42 runs at `max` today.
/// Under a bar of 0.60 it applies nothing: Opus 5.5 keeps its profile's
/// effort (none), and the row says `gated` with the answer still recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_answer_under_the_bar_applies_nothing() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "max", 0.42);
    let r = rig(Some(&jev), 1, |c| gate(c, 0.60));
    let res = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe log.",
        None,
    )
    .await;
    assert_eq!(res.profile, "opus");
    assert_eq!(sent(&r.claude.requests()[0]), None);
    let d = &decided(&r.core.store)[0];
    assert_eq!(
        row(d),
        (json!("max"), json!("gated"), json!(false), Value::Null)
    );
    assert_eq!(d["effort_confidence"], 0.42);
    let ro = res.route.unwrap();
    assert_eq!(ro.effort_reason.as_deref(), Some("gated"));
    assert_eq!(ro.effort_applied, None);
}

/// At the bar, or over it, nothing changes: the bar is "under", and an answer
/// of exactly 0.45 against a bar of 0.45 is still `applied_unsure`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_at_the_bar_applies_as_it_did() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "max", 0.45);
    let r = rig(Some(&jev), 1, |c| gate(c, 0.45));
    turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe log.",
        None,
    )
    .await;
    assert_eq!(sent(&r.claude.requests()[0]).as_deref(), Some("max"));
    assert_eq!(
        row(&decided(&r.core.store)[0]),
        (
            json!("max"),
            json!("applied_unsure"),
            json!(true),
            json!("max")
        )
    );
}

/// A `held_high_unsure` leg: an unsure `xhigh` runs `high` today, whatever its
/// confidence. Under the bar it applies nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_xhigh_under_the_bar_is_not_held_at_high() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "xhigh", 0.50);
    let r = rig(Some(&jev), 1, |c| gate(c, 0.55));
    turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe log.",
        None,
    )
    .await;
    assert_eq!(sent(&r.claude.requests()[0]), None);
    assert_eq!(
        row(&decided(&r.core.store)[0]),
        (json!("xhigh"), json!("gated"), json!(false), Value::Null)
    );
}

/// The `floored` leg: Jev's sure `medium` on `haikuhi` is lifted to its own
/// `high` today. Under a bar above it the answer applies nothing, and the
/// profile's own effort stands. That is the same `high` (the floor IS the
/// profile's own effort, theseus-53ujg.3), so the level does not move and
/// only the reason does: the test pins that, because it is the finding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_floored_answer_under_the_bar_stands_at_the_profiles_own_effort() {
    for (bar, reason, applied) in [(None, "floored", json!(true)), (Some(0.8), "gated", json!(false))] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "routine_coding", 0.95);
        effort(&jev, "medium", 0.62);
        let r = rig(Some(&jev), 1, |c| {
            if let Some(b) = bar {
                gate(c, b);
            }
        });
        let res = turn(&r.core, None, "Rename these twelve call sites.", None).await;
        assert_eq!(res.profile, "haikuhi", "{reason}");
        assert_eq!(sent(&r.claude.requests()[0]).as_deref(), Some("high"));
        assert_eq!(
            row(&decided(&r.core.store)[0]),
            (json!("medium"), json!(reason), applied, json!("high")),
            "{reason}"
        );
    }
}

/// Off by default: with the key unset an unsure `max` at 0.42 still runs at
/// `max`, and the config does not write the key out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_bar_unset_nothing_is_gated() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    effort(&jev, "max", 0.42);
    let r = rig(Some(&jev), 1, |c| {
        let text = toml::to_string(&c.routing).unwrap();
        assert!(!text.contains("effort_floor_confidence"), "{text}");
    });
    turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe log.",
        None,
    )
    .await;
    assert_eq!(sent(&r.claude.requests()[0]).as_deref(), Some("max"));
    assert_eq!(
        decided(&r.core.store)[0]["effort_reason"],
        json!("applied_unsure")
    );
}
