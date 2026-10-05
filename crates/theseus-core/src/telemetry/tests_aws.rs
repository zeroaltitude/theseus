//! The AWS calls' metrics (theseus-ku5f): each `aws` span of the trace,
//! counted in `theseus.aws.calls` and timed in `theseus.aws.duration_ms`, by
//! service, operation and outcome.

use serde_json::{json, Value};
use theseus_protocol::{Span, Usage};

use super::tests::{
    attrs_of, flushed, last_metrics, pipeline, point_with, points_of, result_with, s, tuning,
    Receiver,
};
use super::FailedTurn;

const CALLS: &str = "theseus.aws.calls";
const TIME: &str = "theseus.aws.duration_ms";

/// An AWS request's span, as `aws::span` makes it.
fn request(
    start: u64,
    end: u64,
    service: &str,
    op: &str,
    status: &str,
    code: Option<&str>,
) -> Span {
    let mut a = json!({
        "rpc.system": "aws-api",
        "rpc.service": service,
        "rpc.method": op,
        "cloud.region": "us-east-2",
        "class": "read",
        "status": status,
    });
    if let Some(c) = code {
        a["error"] = json!(c);
        a["message"] = json!("denied");
    }
    s(&format!("aws {service}:{op}"), "aws", start, end, a, vec![])
}

fn tool(start: u64, end: u64, children: Vec<Span>) -> Span {
    s(
        "tool aws_call",
        "tool",
        start,
        end,
        json!({"tool": "aws.call", "family": "aws", "backend": "async", "result": "ok"}),
        children,
    )
}

/// A turn whose second loop holds the tool calls: the requests are below a
/// loop and a tool, not at the top.
fn turn_of(tools: Vec<Span>) -> Span {
    s(
        "turn",
        "turn",
        0,
        100_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![s("loop 0", "loop", 0, 90_000, json!({}), tools)],
    )
}

fn three_requests() -> Span {
    turn_of(vec![
        tool(
            1_000,
            90_000,
            vec![
                request(
                    1_000,
                    41_000,
                    "cloudformation",
                    "DescribeStacks",
                    "ok",
                    None,
                ),
                request(
                    50_000,
                    60_000,
                    "cloudformation",
                    "DescribeStacks",
                    "error",
                    Some("AccessDenied"),
                ),
            ],
        ),
        tool(
            70_000,
            71_000,
            vec![request(
                70_000,
                70_500,
                "sts",
                "GetCallerIdentity",
                "unbound",
                None,
            )],
        ),
    ])
}

fn check(metrics: &[Value]) {
    assert_eq!(points_of(metrics, CALLS).len(), 3, "a series each");
    assert_eq!(points_of(metrics, TIME).len(), 3);
    let want = [
        ("cloudformation", "DescribeStacks", "ok", 40.0),
        ("cloudformation", "DescribeStacks", "error", 10.0),
        ("sts", "GetCallerIdentity", "unbound", 0.5),
    ];
    for (service, op, outcome, ms) in want {
        let with = [
            ("rpc.service", service),
            ("rpc.method", op),
            ("theseus.outcome", outcome),
        ];
        let n = point_with(metrics, CALLS, &with);
        assert_eq!(n["asInt"], "1", "{outcome}");
        assert_eq!(
            attrs_of(n).keys().map(String::as_str).collect::<Vec<_>>(),
            ["rpc.method", "rpc.service", "theseus.outcome"],
            "no error.type: the code is the service's own text"
        );
        let t = point_with(metrics, TIME, &with);
        assert_eq!(
            (&t["count"], &t["sum"]),
            (&json!("1"), &json!(ms)),
            "{outcome}"
        );
    }
}

/// Two DescribeStacks (one AccessDenied) and one unbound GetCallerIdentity
/// give three series, each counted and timed, wherever the spans sit.
#[tokio::test]
async fn a_turns_aws_requests_are_counted_and_timed_by_service_operation_and_outcome() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_turn(&result_with(three_requests()));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    check(&metrics);
}

/// A failed turn's AWS calls count too.
#[tokio::test]
async fn a_failed_turns_aws_requests_count_too() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = three_requests();
    tel.record_failure(&FailedTurn {
        profile: "glm",
        provider: "zai",
        model: "glm-5.1",
        class: "overloaded",
        transient: true,
        elapsed_ms: 100,
        trace: Some(&trace),
        usage: &Usage::default(),
        cost_usd: None,
    });
    flushed(&tel).await;
    check(&last_metrics(&rx.got()));
}

/// A request is counted wherever its span sits: under a continuation's span
/// beside the turn's loops, not only below a tool call.
#[tokio::test]
async fn an_aws_request_deep_in_the_trace_is_counted() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let deep = s(
        "continuation",
        "continuation",
        0,
        50_000,
        json!({}),
        vec![tool(
            0,
            50_000,
            vec![request(0, 20_000, "sts", "GetCallerIdentity", "ok", None)],
        )],
    );
    let trace = s(
        "turn",
        "turn",
        0,
        100_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![deep],
    );
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let n = point_with(
        &metrics,
        CALLS,
        &[("rpc.service", "sts"), ("theseus.outcome", "ok")],
    );
    assert_eq!(n["asInt"], "1");
}
