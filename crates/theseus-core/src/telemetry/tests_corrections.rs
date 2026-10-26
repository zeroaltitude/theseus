//! The corrections (theseus-yf1): the tool calls' metrics, which model a
//! turn and its provider calls name, and the provider calls' time.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::tests::{
    attr, attrs_of, failed_trace, failed_turn, flushed, last_metrics, pipeline, point_with,
    points_of, result_with, s, sample_trace, spans_of, tool_span, tuning, two_loops_result, u64_of,
    Receiver,
};
use super::{spans, FailedTurn};

/// Every tool call is counted and timed by its tool's name, family, and
/// backend and the call's outcome (one kind of each), with the turn's
/// attributes, in a failed turn too; so the shell-fallback ratio, `proc.run`
/// over every call, is one query (§3.23). A call that waits for the operator
/// or runs on in the background counts at its answer, not here.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn tool_calls_are_counted_and_timed_by_name_family_backend_and_outcome() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let ms = 1000;
    let trace = s(
        "turn",
        "turn",
        0,
        200_000 * ms,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![
            s(
                "loop 0",
                "loop",
                0,
                1_000 * ms,
                json!({"loop": 0}),
                vec![
                    s(
                        "tools",
                        "tools",
                        100,
                        40_100,
                        json!({"calls": 2, "together": true}),
                        vec![
                            tool_span("fs_read", 100, 40_100, ["fs.read", "fs", "inproc", "ok"]),
                            tool_span("fs_read", 200, 2_200, ["fs.read", "fs", "inproc", "error"]),
                        ],
                    ),
                    tool_span(
                        "frob_it",
                        50_000,
                        50_300,
                        ["frob_it", "unknown", "none", "error"],
                    ),
                ],
            ),
            s(
                "loop 1",
                "loop",
                1_000 * ms,
                200_000 * ms,
                json!({"loop": 1}),
                vec![
                    tool_span(
                        "proc_run",
                        1_000 * ms,
                        13_000 * ms,
                        ["proc.run", "proc", "job", "ok"],
                    ),
                    tool_span(
                        "proc_run",
                        13_000 * ms,
                        73_000 * ms,
                        ["proc.run", "proc", "job", "background"],
                    ),
                    tool_span(
                        "proc_run",
                        73_000 * ms,
                        73_001 * ms,
                        ["proc.run", "proc", "job", "awaiting_confirm"],
                    ),
                    tool_span(
                        "proc_run",
                        74_000 * ms,
                        74_002 * ms,
                        ["proc.run", "proc", "job", "cancelled"],
                    ),
                    tool_span(
                        "proc_run",
                        75_000 * ms,
                        75_003 * ms,
                        ["proc.run", "proc", "job", "unknown"],
                    ),
                    tool_span(
                        "http_fetch",
                        76_000 * ms,
                        76_500 * ms,
                        ["http.fetch", "http", "async", "declined"],
                    ),
                ],
            ),
        ],
    );
    tel.record_turn(&result_with(trace));
    // A failed turn's calls are counted too, under its outcome.
    let failed = s(
        "turn",
        "turn",
        0,
        3_000 * ms,
        json!({"origin_unix_ms": 1_790_000_100_000u64, "outcome": "failed", "class": "overloaded"}),
        vec![s(
            "loop 0",
            "loop",
            0,
            3_000 * ms,
            json!({"loop": 0}),
            vec![
                tool_span(
                    "proc_run",
                    10,
                    2_000_010,
                    ["proc.run", "proc", "job", "error"],
                ),
                s(
                    "provider.call",
                    "provider",
                    2_000_100,
                    3_000 * ms,
                    json!({"provider": "zai", "model": "glm-5.1", "error": "overloaded"}),
                    vec![],
                ),
            ],
        )],
    );
    tel.record_failure(&FailedTurn {
        class: "overloaded",
        elapsed_ms: 3000,
        ..failed_turn(&failed)
    });
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let want = [
        (["fs.read", "fs", "inproc", "ok"], "complete", 40.0),
        (["fs.read", "fs", "inproc", "error"], "complete", 2.0),
        (["frob_it", "unknown", "none", "error"], "complete", 0.3),
        (["proc.run", "proc", "job", "ok"], "complete", 12_000.0),
        // A call that waits or runs on counts at its answer (theseus-8pei).
        (["proc.run", "proc", "job", "cancelled"], "complete", 2.0),
        (["proc.run", "proc", "job", "unknown"], "complete", 3.0),
        (
            ["http.fetch", "http", "async", "declined"],
            "complete",
            500.0,
        ),
        (["proc.run", "proc", "job", "error"], "failed", 2_000.0),
    ];
    let calls = points_of(&metrics, "theseus.tool.calls");
    assert_eq!(calls.len(), want.len(), "{calls:#?}");
    assert_eq!(
        points_of(&metrics, "theseus.tool.duration_ms").len(),
        want.len()
    );
    for ([name, family, backend, outcome], turn, took) in want {
        let with = [
            ("theseus.tool.name", name),
            ("theseus.tool.family", family),
            ("theseus.tool.backend", backend),
            ("theseus.tool.outcome", outcome),
            ("theseus.outcome", turn),
        ];
        let c = point_with(&metrics, "theseus.tool.calls", &with);
        assert_eq!(c["asInt"], "1", "{with:?}");
        let a = attrs_of(c);
        assert_eq!(a.len(), 8, "the turn's four, and the tool's four: {a:?}");
        let model = if turn == "failed" {
            "glm-5.1"
        } else {
            "claude-x"
        };
        assert_eq!(a["gen_ai.request.model"], model);
        let d = point_with(&metrics, "theseus.tool.duration_ms", &with);
        assert_eq!(
            (&d["count"], &d["sum"]),
            (&json!("1"), &json!(took)),
            "{with:?}"
        );
        assert_eq!(attrs_of(d), a, "one attribute set for both");
    }
    // The shell-fallback ratio: the calls named proc.run over all of them.
    let count = |p: &&Value| u64_of(&p["asInt"]);
    let all: u64 = calls.iter().map(count).sum();
    let shell: u64 = calls
        .iter()
        .filter(|p| attrs_of(p)["theseus.tool.name"] == "proc.run")
        .map(count)
        .sum();
    assert_eq!((shell, all), (4, 8));
}

/// What became of each call, as its span's `result` says it: the result
/// node's status, or that it waits for the operator or runs in the
/// background.
#[test]
fn a_calls_result_names_each_outcome() {
    use crate::node::ResultStatus;
    use crate::toolrun::CallOutcome;
    let done = |status| CallOutcome::Done { status };
    for (o, want) in [
        (done(ResultStatus::Ok), "ok"),
        (done(ResultStatus::Error), "error"),
        (done(ResultStatus::Declined), "declined"),
        (done(ResultStatus::Background), "background"),
        (done(ResultStatus::Unknown), "unknown"),
        (done(ResultStatus::Cancelled), "cancelled"),
        (
            CallOutcome::AwaitingConfirm {
                correlation_id: "cor_1".into(),
            },
            "awaiting_confirm",
        ),
        (
            CallOutcome::Background {
                correlation_id: "cor_2".into(),
            },
            "background",
        ),
    ] {
        assert_eq!(crate::turn::call_result(&o), want, "{o:?}");
    }
}

/// `gen_ai.response.model` is the model that answered, as the provider named
/// it, not the one asked for; a call that failed had no answer, so it has
/// none.
#[tokio::test]
async fn the_response_model_is_the_served_model() {
    use spans::semconv::{GEN_AI_REQUEST_MODEL, GEN_AI_RESPONSE_MODEL};
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = s(
        "turn",
        "turn",
        0,
        1_000_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![s(
            "loop 0",
            "loop",
            0,
            1_000_000,
            json!({"loop": 0}),
            vec![s(
                "provider.call",
                "provider",
                10,
                900_000,
                json!({"provider": "zai", "model": "glm-5.3", "served_model": "glm-5.3-0815",
                    "request_id": "req_1", "stop_reason": "end_turn"}),
                vec![],
            )],
        )],
    );
    tel.record_turn(&result_with(trace));
    let failed = failed_trace();
    tel.record_failure(&failed_turn(&failed));
    flushed(&tel).await;
    let spans = spans_of(&rx.got());
    let calls: Vec<&Value> = spans
        .iter()
        .filter(|s| s["name"] == "provider.call")
        .collect();
    assert_eq!(calls.len(), 2);
    let text = |v: &str| Some(json!({ "stringValue": v }));
    let answered = calls.iter().find(|s| s.get("status").is_none()).unwrap();
    assert_eq!(
        attr(answered, GEN_AI_REQUEST_MODEL).cloned(),
        text("glm-5.3")
    );
    assert_eq!(
        attr(answered, GEN_AI_RESPONSE_MODEL).cloned(),
        text("glm-5.3-0815")
    );
    let failed = calls.iter().find(|s| s.get("status").is_some()).unwrap();
    assert_eq!(attr(failed, GEN_AI_REQUEST_MODEL).cloned(), text("glm-5.1"));
    assert_eq!(attr(failed, GEN_AI_RESPONSE_MODEL), None);
}

/// A finished turn's metrics name the model asked for in
/// `gen_ai.request.model`, as its trace's root recorded it, and not the one
/// that answered, which the result names: so a turn on an alias lands in the
/// series its failure would, and agrees with its provider calls' time.
#[tokio::test]
async fn a_turns_metrics_name_the_model_asked_for() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = s(
        "turn",
        "turn",
        0,
        1_000_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64, "provider": "anthropic",
            "model": "claude-haiku-4-5"}),
        vec![s(
            "provider.call",
            "provider",
            10,
            900_000,
            json!({"provider": "anthropic", "model": "claude-haiku-4-5",
                "served_model": "claude-haiku-4-5-20251001"}),
            vec![],
        )],
    );
    let mut r = result_with(trace);
    r.model = "claude-haiku-4-5-20251001".into();
    tel.record_turn(&r);
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    for name in [
        "theseus.turns",
        "theseus.tokens",
        "theseus.turn.duration_ms",
        "theseus.provider.call.duration_ms",
    ] {
        let points = points_of(&metrics, name);
        assert!(!points.is_empty(), "{name}");
        for p in points {
            assert_eq!(
                attrs_of(p)["gen_ai.request.model"],
                "claude-haiku-4-5",
                "{name}"
            );
        }
    }
}

/// Each provider call's time carries the provider and model its span
/// recorded, one series for each pair. It had no attributes, so every
/// model's calls were one series.
#[tokio::test]
async fn a_provider_calls_time_carries_its_provider_and_model() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&two_loops_result());
    tel.record_turn(&result_with(sample_trace()));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.provider.call.duration_ms";
    assert_eq!(points_of(&metrics, name).len(), 2);
    let zai = point_with(&metrics, name, &[("gen_ai.provider.name", "zai")]);
    assert_eq!(
        attrs_of(zai),
        BTreeMap::from([
            ("gen_ai.provider.name".to_string(), "zai".to_string()),
            ("gen_ai.request.model".to_string(), "glm-5.1".to_string()),
        ])
    );
    assert_eq!(
        (&zai["count"], &zai["min"], &zai["max"]),
        (&json!("2"), &json!(890.0), &json!(891.9))
    );
    let anthropic = point_with(&metrics, name, &[("gen_ai.provider.name", "anthropic")]);
    assert_eq!(attrs_of(anthropic)["gen_ai.request.model"], "claude-x");
    assert_eq!(anthropic["count"], "1");
}
