//! `turn.submit`'s `deadline_secs` (theseus-yct32) through the protocol: the
//! deadline a client gives reaches the turn, so the model's first request
//! holds the time line; a submit with none has no line (theseus-rmjqr).

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::Message;
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::tests::submit;
use super::{Core, Parts};
use crate::approval::{Client, Surface};
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::Config;

/// One `turn.submit` with `deadline_secs`, and the messages of the model's
/// first request as text.
async fn first_request(deadline_secs: Option<u64>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.judge.enabled = false;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text("Done.")]));
    let core = Core::build(Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(
        sr,
        sw,
        Client::new("conn", Surface::Cli),
    ));
    let (cr, mut cw) = tokio::io::split(client);
    let mut lines = BufReader::new(cr).lines();
    let mut request = serde_json::to_value(submit(1, "Write the tide tables.")).unwrap();
    request["params"]["deadline_secs"] = serde_json::json!(deadline_secs);
    let mut line = serde_json::to_string(&request).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
            assert!(r.error.is_none(), "{r:?}");
            break;
        }
    }
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    let first = fake.requests().remove(0);
    serde_json::to_string::<Value>(&serde_json::to_value(&first.messages).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_submit_with_deadline_secs_tells_the_model_its_time() {
    let with = first_request(Some(600)).await;
    assert!(with.contains("minutes left of this turn's 10"), "{with}");
    let without = first_request(None).await;
    assert!(!without.contains("left of this turn"), "{without}");
}
