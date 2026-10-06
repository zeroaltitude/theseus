//! A voice call's cuts and resumed stops as metrics (theseus-qb8o):
//! `theseus.voice.cuts` and `theseus.voice.resumed`, by why, as the Discord
//! binding records them beside their ledger rows.

use super::tests::{attrs_of, flushed, last_metrics, pipeline, points_of, tuning, Receiver};

/// Each point of `name`, as (why, count), sorted.
fn by_why(metrics: &[serde_json::Value], name: &str) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = points_of(metrics, name)
        .into_iter()
        .map(|p| {
            let n = p["asInt"].as_str().unwrap().parse().unwrap();
            (attrs_of(p)["theseus.voice.why"].clone(), n)
        })
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn a_cut_and_a_resumed_stop_are_counted_by_why() {
    let rx = Receiver::start(vec![]).await;
    let tel = pipeline(&rx.endpoint(), None, tuning());
    tel.record_voice_cut("words");
    tel.record_voice_cut("words");
    tel.record_voice_cut("superseded");
    tel.record_voice_resumed("backchannel");
    flushed(&tel).await;
    let m = last_metrics(&rx.got());
    assert_eq!(
        by_why(&m, "theseus.voice.cuts"),
        [("superseded".to_string(), 1), ("words".to_string(), 2)]
    );
    assert_eq!(
        by_why(&m, "theseus.voice.resumed"),
        [("backchannel".to_string(), 1)]
    );
}
