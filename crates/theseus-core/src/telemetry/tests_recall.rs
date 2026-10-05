//! Recall's metrics (M6 32b; design §2.13, "Telemetry"): each spread's
//! `recall.activate` span, inside its `recall` span, is timed by outcome in
//! `theseus.recall.activate_ms`, and the nodes it added, and those
//! admitted, are counted in `theseus.recall.activated`.

use serde_json::json;

use super::tests::{
    flushed, last_metrics, pipeline, point_with, points_of, result_with, s, tuning, Receiver,
};

#[tokio::test]
async fn a_spread_is_timed_by_outcome_and_its_additions_counted() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let activate = |start: u64, end: u64, outcome: &str, added: u64, admitted: u64| {
        s(
            "recall.activate",
            "recall",
            start,
            end,
            json!({"outcome": outcome, "added": added, "admitted_added": admitted}),
            vec![],
        )
    };
    let recall = |children| s("recall", "recall", 0, 9_000, json!({}), children);
    let trace = s(
        "turn",
        "turn",
        0,
        10_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![
            recall(vec![activate(1_000, 3_500, "ran", 3, 1)]),
            recall(vec![activate(4_000, 4_500, "ran", 2, 2)]),
            recall(vec![activate(5_000, 5_000, "building", 0, 0)]),
        ],
    );
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.recall.activate_ms";
    assert_eq!(points_of(&metrics, name).len(), 2, "one series an outcome");
    let ran = point_with(&metrics, name, &[("theseus.outcome", "ran")]);
    assert_eq!((&ran["count"], &ran["sum"]), (&json!("2"), &json!(3.0)));
    let building = point_with(&metrics, name, &[("theseus.outcome", "building")]);
    assert_eq!(building["count"], json!("1"));
    let name = "theseus.recall.activated";
    let added = point_with(&metrics, name, &[("theseus.recall.stage", "added")]);
    let admitted = point_with(&metrics, name, &[("theseus.recall.stage", "admitted")]);
    assert_eq!(
        (&added["asInt"], &admitted["asInt"]),
        (&json!("5"), &json!("3"))
    );
}
