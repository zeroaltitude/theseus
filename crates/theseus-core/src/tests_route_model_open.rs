//! A routed turn's metrics and rows, through route.v10 (theseus-h66v4.11). route.v8's tests
//! reached a switch and a detour through the class walk (`mode(&jev, "sophisticated" |
//! "trivial")`); the code they tested did not go: `route_to` moves the trace's root with the turn
//! (theseus-490i), so the root's `profile`, `provider` and `model`, and every point of the turn's
//! metrics, name the model the call went to, and a turn records one `context.compiled` and one
//! `loop.started` a loop (theseus-d13v), whether routing kept the first compile or switched. Here
//! Jev picks a model (`model`, route.v10's shape acting: `tests_route_open`'s rig).
//!
//! - a switch (a hard question to Opus) and a pick of Flash, each from Sonnet: the trace's root and
//!   the turn's `theseus.turns` point name the model the call went to;
//! - a switch and a pick that keeps the routed model each record one row of each kind, and each
//!   row's compilation reads back.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::TurnSubmitResult;

use crate::provider::FakeProvider;
use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};
use crate::tests_act_open::act_open;
use crate::tests_judge::{kinds, texts};
use crate::tests_route::{call, rig_parts, turn, until_route_rows, Rig};
use crate::tests_route_open::{open, says};

/// `theseus.turns`'s points in the receiver's last metrics, each by its attributes (values as
/// text).
fn turns(rx: &Receiver) -> Vec<BTreeMap<String, String>> {
    let got = rx.at("/v1/metrics");
    let last = got.last().expect("a metrics export");
    let metrics = last.body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    metrics
        .iter()
        .filter(|m| m["name"] == "theseus.turns")
        .flat_map(|m| {
            m["sum"]["dataPoints"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|p| {
            let attrs = p["attributes"].as_array().cloned().unwrap_or_default();
            attrs
                .iter()
                .map(|kv| {
                    let v = kv["value"]["stringValue"].as_str().unwrap_or_default();
                    (kv["key"].as_str().unwrap().to_string(), v.to_string())
                })
                .collect()
        })
        .collect()
}

/// The trace root's `profile`, `provider` and `model`.
fn root(res: &TurnSubmitResult) -> (String, String, String) {
    let attrs = &res.trace.as_ref().expect("a trace").attrs;
    let s = |k: &str| attrs[k].as_str().unwrap_or_default().to_string();
    (s("profile"), s("provider"), s("model"))
}

/// A switch (a hard question to Opus) and a pick of Flash (thanks, to `dsflash` on the `deepseek`
/// fake), each from Sonnet: the trace's root and the turn's `theseus.turns` point name the model
/// the call went to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turns_metrics_name_the_model_it_ran_on() {
    let rx = Receiver::start(vec![]).await;
    let jev = FakeJev::start().unwrap();
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let deepseek = std::sync::Arc::new(FakeProvider::scripted(texts(1)));
    let ds = deepseek.clone();
    let r = rig_parts(
        Some(&jev),
        2,
        |c| c.serve_deepseek_flash(),
        move |p| {
            p.providers.insert("deepseek".into(), ds);
            p.telemetry = Some(Telemetry::from_config(&tel, None).unwrap())
        },
    );
    act_open(&r);
    let submit = |input: &str| {
        let core = r.core.clone();
        let params = json!({"input": input});
        async move {
            let v: Value = call(&core, "turn.submit", params).await;
            serde_json::from_value::<TurnSubmitResult>(v).unwrap()
        }
    };
    says(&jev, "opus", "high", 0.95);
    let hard = submit("Weigh two designs for a crash-safe write-ahead log.").await;
    assert_eq!(hard.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(r.claude.requests()[0].model, "claude-opus-5-5");
    assert_eq!(
        root(&hard),
        ("opus".into(), "anthropic".into(), "claude-opus-5-5".into())
    );
    says(&jev, "deepseek_flash", "low", 0.95);
    let thanks = submit("thank you!").await;
    assert_eq!(thanks.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(deepseek.requests()[0].model, "deepseek-flash");
    assert_eq!(
        root(&thanks),
        ("dsflash".into(), "deepseek".into(), "deepseek-flash".into())
    );
    until_route_rows(&r.core.store, 2).await;
    assert!(r.core.telemetry().flush(Duration::from_secs(10)).await);
    let points = turns(&rx);
    let named = |profile: &str| {
        let p: Vec<_> = points
            .iter()
            .filter(|a| a.get("theseus.profile").map(String::as_str) == Some(profile))
            .collect();
        assert_eq!(p.len(), 1, "{profile}: {points:#?}");
        (
            p[0]["gen_ai.provider.name"].clone(),
            p[0]["gen_ai.request.model"].clone(),
        )
    };
    assert_eq!(
        named("opus"),
        ("anthropic".into(), "claude-opus-5-5".into())
    );
    assert_eq!(
        named("dsflash"),
        ("deepseek".into(), "deepseek-flash".into())
    );
    assert_eq!(points.len(), 2, "{points:#?}");
}

/// The turn's rows of `kind`, by their loop index.
fn loops(r: &Rig, res: &TurnSubmitResult, kind: &str) -> Vec<u64> {
    kinds(&r.core.store, kind)
        .into_iter()
        .filter(|row| row.turn_id.as_deref() == Some(res.turn_id.as_str()))
        .map(|row| row.data["loop"].as_u64().unwrap())
        .collect()
}

/// One `context.compiled` and one `loop.started` for loop 0, and the compilation the row names is
/// stored.
fn one_of_each(r: &Rig, res: &TurnSubmitResult, what: &str) {
    assert_eq!(loops(r, res, "loop.started"), [0], "{what}: loop.started");
    assert_eq!(
        loops(r, res, "context.compiled"),
        [0],
        "{what}: context.compiled"
    );
    for row in kinds(&r.core.store, "context.compiled") {
        let id = row.data["compilation_id"].as_str().unwrap();
        assert!(
            r.core.store.get_compilation(id).unwrap().is_some(),
            "{what}: {id} does not read back"
        );
    }
}

/// A switch (a hard question to Opus) and a pick that keeps the first compile (Opus again, on the
/// routed profile) each record one row of each kind, and each row's compilation reads back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_switched_turn_records_one_compile_and_one_loop() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.95);
    // `open` names the providers' keys, which a pick needs to run (a placeholder is a fallback).
    let r = open(&jev, 3, |c| c.routing.max_wait_ms = 5_000).r;
    let one = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    assert_eq!(one.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(one.profile, "opus");
    one_of_each(&r, &one, "the switch");
    let two = turn(&r.core, Some(&one.session_id), "What is a frame?", None).await;
    assert_eq!(two.profile, "opus");
    one_of_each(&r, &two, "the first compile kept");
}
