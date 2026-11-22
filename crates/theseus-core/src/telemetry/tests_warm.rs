//! The warm-up metric's outcomes (theseus-tnky): a first keystroke's warm-up that opens a provider
//! connection is counted `opened`, through a whole core whose telemetry posts to a receiver. (Its
//! refused half went with the restricted-origin rule, 2026-11-21: nothing refuses a warm-up now.)

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::{Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::tests::{attrs_of, flushed, last_metrics, pipeline, points_of, tuning, Receiver};
use crate::approval::{Client, Surface};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::tests_recall::session;
use crate::{Config, Core};

/// One `session.typing` on the CLI's surface, answered.
async fn typed(core: &Arc<Core>, session_id: &str) {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let client = Client::new("sock#1", Surface::Cli);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(
        theseus_protocol::Id::Num(1),
        "session.typing",
        json!({ "session_id": session_id }),
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            assert!(r.result.is_some(), "{:?}", r.error);
            break;
        }
    }
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
}

/// The provider warm-ups by outcome, as the last export counts them.
async fn provider_warms(core: &Core, rx: &Receiver) -> BTreeMap<String, u64> {
    flushed(core.telemetry()).await;
    let metrics = last_metrics(&rx.got());
    points_of(&metrics, "theseus.warm")
        .into_iter()
        .map(|p| (attrs_of(p), p["asInt"].as_str().unwrap().parse().unwrap()))
        .filter(|(a, _)| a["theseus.warm.target"] == "provider")
        .map(|(a, n)| (a["theseus.warm.outcome"].clone(), n))
        .collect()
}

/// A keystroke's warm-up to the session's provider opens its connection, and the metric counts it
/// `opened`.
#[tokio::test(flavor = "multi_thread")]
async fn a_warm_up_that_opens_is_counted_as_opened() {
    let rx = Receiver::start(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let us = Arc::new(FakeProvider::default());
    let core = Core::build(crate::rpc::Parts {
        telemetry: Some(pipeline(&rx.endpoint(), None, tuning())),
        ..crate::rpc::Parts::for_tests(cfg, us.clone(), store)
    })
    .unwrap();

    let open = session(&core, None, &[]);
    typed(&core, &open).await;
    let mut seen = BTreeMap::new();
    for _ in 0..200 {
        seen = provider_warms(&core, &rx).await;
        if seen.values().sum::<u64>() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let want: BTreeMap<String, u64> = [("opened".to_string(), 1)].into();
    assert_eq!(seen, want, "one opened: {seen:?}");
    assert_eq!(us.warms.load(std::sync::atomic::Ordering::SeqCst), 1);
}
