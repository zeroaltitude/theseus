//! The calls' metrics: a failed provider call's time apart from the answered
//! ones (`error.type`, theseus-lmhp), and the AWS calls (theseus-ku5f).

use std::collections::BTreeMap;

use serde_json::{json, Value};
use theseus_protocol::Span;

use super::tests::{
    attrs_of, flushed, last_metrics, pipeline, point_with, points_of, result_with, s, tuning,
    Receiver,
};
use super::FailedTurn;
use theseus_protocol::Usage;

/// A provider call's span, as the turn records it.
fn call(start: u64, end: u64, attrs: Value, children: Vec<Span>) -> Span {
    let mut a = json!({"provider": "zai", "model": "glm-5.1"});
    a.as_object_mut()
        .unwrap()
        .extend(attrs.as_object().unwrap().clone());
    s("provider zai", "provider", start, end, a, children)
}

fn turn_of(calls: Vec<Span>) -> Span {
    s(
        "turn",
        "turn",
        0,
        100_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        calls,
    )
}

/// The provider spans of the first trace request.
fn provider_spans(rx: &Receiver) -> Vec<Value> {
    rx.got()
        .iter()
        .filter(|g| g.path == "/v1/traces")
        .flat_map(|g| {
            g.body["resourceSpans"][0]["scopeSpans"][0]["spans"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|sp| {
            sp["attributes"]
                .as_array()
                .is_some_and(|a| a.iter().any(|kv| kv["key"] == "gen_ai.operation.name"))
        })
        .collect()
}

const CALL: &str = "theseus.provider.call.duration_ms";
const FIRST: &str = "theseus.provider.first_token_ms";

/// Two answered calls and one that was rate limited: the model has two
/// series, the failure's own with `error.type`, so its 150 ms never lands in
/// the answered calls' percentiles (theseus-lmhp).
#[tokio::test]
async fn a_failed_calls_time_is_its_own_series_beside_the_answered_ones() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = turn_of(vec![
        call(0, 900_000, json!({"stop_reason": "end_turn"}), vec![]),
        call(
            1_000_000,
            1_150_000,
            json!({"error": "rate_limited", "message": "429"}),
            vec![],
        ),
        call(
            2_000_000,
            2_800_000,
            json!({"stop_reason": "end_turn"}),
            vec![],
        ),
    ]);
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    assert_eq!(points_of(&metrics, CALL).len(), 2, "answered, and failed");
    let failed = point_with(&metrics, CALL, &[("error.type", "rate_limited")]);
    assert_eq!(
        attrs_of(failed),
        BTreeMap::from([
            ("error.type".to_string(), "rate_limited".to_string()),
            ("gen_ai.provider.name".to_string(), "zai".to_string()),
            ("gen_ai.request.model".to_string(), "glm-5.1".to_string()),
        ])
    );
    assert_eq!(
        (&failed["count"], &failed["min"], &failed["max"]),
        (&json!("1"), &json!(150.0), &json!(150.0))
    );
    let answered: Vec<&Value> = points_of(&metrics, CALL)
        .into_iter()
        .filter(|p| !attrs_of(p).contains_key("error.type"))
        .collect();
    assert_eq!(answered.len(), 1);
    assert_eq!(
        (
            &answered[0]["count"],
            &answered[0]["min"],
            &answered[0]["max"]
        ),
        (&json!("2"), &json!(800.0), &json!(900.0))
    );
}

/// A refusal is an answer (`stop_reason` `refusal`), and so is the call the
/// fallback makes on its own model: neither carries `error.type`.
#[tokio::test]
async fn a_refused_call_and_its_fallback_are_answers_and_carry_no_error_type() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let fallback = s(
        "provider anthropic",
        "provider",
        1_000_000,
        1_600_000,
        json!({"provider": "anthropic", "model": "sonnet-5", "stop_reason": "end_turn"}),
        vec![],
    );
    let trace = turn_of(vec![
        call(0, 400_000, json!({"stop_reason": "refusal"}), vec![]),
        fallback,
    ]);
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    assert_eq!(points_of(&metrics, CALL).len(), 2, "one series a model");
    for p in points_of(&metrics, CALL) {
        assert!(!attrs_of(p).contains_key("error.type"), "{p:#}");
    }
    for sp in provider_spans(&rx) {
        let keys: Vec<&str> = sp["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kv| kv["key"].as_str().unwrap())
            .collect();
        assert!(!keys.contains(&"error.type"), "{sp:#}");
        assert!(sp.get("status").is_none_or(|st| st["code"] != 2));
    }
}

/// A call a `/stop` cut never answered, so it has its own `error.type`,
/// `stopped`: its time is a cut one, and left among the answers it would
/// pull their percentiles down. (A cancelled tool call is the operator's
/// choice and counts as no failure; a call is timed, and its time says
/// nothing about the model.)
#[tokio::test]
async fn a_call_a_stop_cut_is_timed_as_stopped() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = turn_of(vec![
        call(0, 700_000, json!({"stop_reason": "end_turn"}), vec![]),
        call(
            1_000_000,
            1_050_000,
            json!({"error": "stopped", "message": "cut by a stop from cli"}),
            vec![],
        ),
    ]);
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let cut = point_with(&metrics, CALL, &[("error.type", "stopped")]);
    assert_eq!((&cut["count"], &cut["max"]), (&json!("1"), &json!(50.0)));
    let ok: Vec<&Value> = points_of(&metrics, CALL)
        .into_iter()
        .filter(|p| !attrs_of(p).contains_key("error.type"))
        .collect();
    assert_eq!(
        (&ok[0]["count"], &ok[0]["max"]),
        (&json!("1"), &json!(700.0))
    );
}

/// A failed turn's calls split the same way, and a failed call that had
/// streamed a token keeps its first token in the model's one series.
#[tokio::test]
async fn a_failed_turns_calls_split_by_error_type_too() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = turn_of(vec![
        call(0, 500_000, json!({"stop_reason": "tool_use"}), vec![]),
        call(
            1_000_000,
            1_900_000,
            json!({"error": "stream", "message": "broke off"}),
            vec![s(
                "first_token",
                "mark",
                1_300_000,
                1_300_000,
                json!({}),
                vec![],
            )],
        ),
        call(
            2_000_000,
            2_010_000,
            json!({"error": "overloaded", "message": "529"}),
            vec![],
        ),
    ]);
    tel.record_failure(&FailedTurn {
        profile: "glm",
        provider: "zai",
        model: "glm-5.1",
        class: "overloaded",
        transient: true,
        elapsed_ms: 2_010,
        trace: Some(&trace),
        usage: &Usage::default(),
        cost_usd: None,
    });
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    assert_eq!(points_of(&metrics, CALL).len(), 3);
    for class in ["stream", "overloaded"] {
        let p = point_with(&metrics, CALL, &[("error.type", class)]);
        assert_eq!(p["count"], "1", "{class}");
    }
    let first = points_of(&metrics, FIRST);
    assert_eq!(first.len(), 1, "a first token's series has no error.type");
    assert!(!attrs_of(first[0]).contains_key("error.type"));
    assert_eq!(first[0]["min"], json!(300.0));
}

/// The span carries the class too, as the GenAI conventions put it, beside
/// the `error` attribute and the ERROR status it always had.
#[tokio::test]
async fn a_failed_calls_span_carries_error_type() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let trace = turn_of(vec![call(
        0,
        150_000,
        json!({"error": "timeout", "message": "600 s"}),
        vec![],
    )]);
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let spans = provider_spans(&rx);
    assert_eq!(spans.len(), 1);
    let a = attrs_of(&spans[0]);
    assert_eq!(a["error.type"], "timeout");
    assert_eq!(a["error"], "timeout", "the flattened attribute stays");
    assert_eq!(spans[0]["status"]["code"], 2);
}

/// A turn whose call the daemon's stop kept from being sent (`stopping`,
/// theseus-36re) is a failed turn, and no provider's error: the provider
/// errors hold the refused connection beside it, and only that.
#[tokio::test]
async fn a_call_the_stop_kept_back_is_no_provider_error() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let usage = Usage::default();
    for class in ["stopping", "network"] {
        tel.record_failure(&FailedTurn {
            profile: "glm",
            provider: "zai",
            model: "glm-5.1",
            class,
            transient: true,
            elapsed_ms: 3,
            trace: None,
            usage: &usage,
            cost_usd: None,
        });
    }
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let errors = points_of(&metrics, "theseus.provider.errors");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(attrs_of(errors[0])["theseus.error.class"], "network");
    let failed = point_with(&metrics, "theseus.turns", &[("theseus.outcome", "failed")]);
    assert_eq!(failed["asInt"], "2", "both turns failed");
}
