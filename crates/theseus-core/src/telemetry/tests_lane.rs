//! The lane's wait (theseus-klo2's review, finding 7): an ordered request on a
//! protocol connection records `theseus.rpc.lane.wait` by its method, from its
//! line's arrival to its lane clearing; a read never enters the lane and
//! records nothing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::tests::{flushed, last_metrics, point_with, points_of, Receiver};
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, config, texts};

/// One request on `w`, and its answer read from `lines`.
async fn ask<W, R>(
    w: &mut W,
    lines: &mut tokio::io::Lines<R>,
    id: u64,
    method: &str,
    params: Value,
) -> Value
where
    W: AsyncWriteExt + Unpin,
    R: tokio::io::AsyncBufRead + Unpin,
{
    let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    w.write_all(format!("{req}\n").as_bytes()).await.unwrap();
    loop {
        let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .expect("an answer in time")
            .unwrap()
            .expect("the connection is open");
        let m: Value = serde_json::from_str(&line).unwrap();
        if m["id"] == json!(id) {
            assert!(m.get("error").is_none(), "{m}");
            return m["result"].clone();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ordered_request_records_its_lane_wait_by_method() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), None);
    cfg.telemetry.otlp_endpoint = Some(rx.endpoint());
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::scripted(texts(1)));
    let mut p = Parts::for_tests(cfg, fake, store);
    p.secrets = board();
    p.telemetry = None;
    let core: Arc<Core> = Core::build(p).unwrap();
    core.clone().install_telemetry().await;
    assert!(core.telemetry().enabled(), "the exporter is built");

    let (ours, theirs) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(theirs);
    let client = crate::approval::Client::new("one", crate::approval::Surface::Cli);
    tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (r, mut w) = tokio::io::split(ours);
    let mut lines = BufReader::new(r).lines();
    let sid = ask(&mut w, &mut lines, 1, "session.open", json!({})).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    ask(&mut w, &mut lines, 2, "health", json!({})).await;
    ask(
        &mut w,
        &mut lines,
        3,
        "turn.submit",
        json!({"session_id": sid, "input": "Say done."}),
    )
    .await;

    let t0 = Instant::now();
    let m = loop {
        flushed(core.telemetry()).await;
        let m = last_metrics(&rx.got());
        if !points_of(&m, "theseus.rpc.lane.wait").is_empty() {
            break m;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "no lane wait: {m:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let points = points_of(&m, "theseus.rpc.lane.wait");
    assert_eq!(points.len(), 1, "one method, the turn's: {points:#?}");
    let p = point_with(
        &m,
        "theseus.rpc.lane.wait",
        &[("theseus.rpc.method", "turn.submit")],
    );
    assert_eq!(p["count"], "1", "{p}");
}
