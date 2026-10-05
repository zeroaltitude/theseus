//! The files lane's metric (theseus-c9l6): each file read for a model, its
//! `file.read` span, timed by how it came, its type, and its outcome.

use std::collections::BTreeMap;

use serde_json::json;

use super::tests::{
    attrs_of, flushed, last_metrics, pipeline, point_with, points_of, result_with, s, tuning,
    Receiver,
};

/// Each file read for a model (theseus-c9l6), its `file.read` span, is
/// timed in `theseus.file.read.duration`, by how it came, its type, and its
/// outcome.
#[tokio::test]
async fn a_turns_file_reads_are_timed_by_how_they_came_and_their_outcome() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    let read = |start: u64, end: u64, via: &str, outcome: &str| {
        s(
            "file.read",
            "file",
            start,
            end,
            json!({"via": via, "media_type": "application/pdf", "outcome": outcome}),
            vec![],
        )
    };
    let trace = s(
        "turn",
        "turn",
        0,
        10_000,
        json!({"origin_unix_ms": 1_790_000_000_000u64}),
        vec![
            read(100, 2_100, "attachment", "read"),
            read(3_000, 4_000, "attachment", "read"),
            read(5_000, 5_500, "fs.read", "unread"),
        ],
    );
    tel.record_turn(&result_with(trace));
    flushed(&tel).await;
    let metrics = last_metrics(&rx.got());
    let name = "theseus.file.read.duration";
    assert_eq!(
        points_of(&metrics, name).len(),
        2,
        "one series a way in and outcome"
    );
    let att = point_with(&metrics, name, &[("theseus.file.via", "attachment")]);
    assert_eq!(
        attrs_of(att),
        BTreeMap::from([
            (
                "theseus.file.type".to_string(),
                "application/pdf".to_string()
            ),
            ("theseus.file.via".to_string(), "attachment".to_string()),
            ("theseus.outcome".to_string(), "read".to_string()),
        ])
    );
    assert_eq!((&att["count"], &att["sum"]), (&json!("2"), &json!(3.0)));
}
