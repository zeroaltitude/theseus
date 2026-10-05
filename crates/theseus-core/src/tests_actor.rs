//! Every operator act's `by` (V8, theseus-cny7): an act names the person or
//! the surface through `Conn::actor` (`the CLI`, `the web UI`), never the
//! connection's label (`sock#7`, `web#35`), which names no one a reader
//! knows; on a surface no listener named (a test's), the label, as `actor`
//! says.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{method, SessionKind};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::session::SessionRecord;
use crate::Core;

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for Rig {
    type Target = Arc<Core>;
    fn deref(&self) -> &Arc<Core> {
        &self.core
    }
}

fn core() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(crate::provider::FakeProvider::default()),
        store,
    ))
    .unwrap();
    Rig { core, _dir: dir }
}

/// One request on a connection of `client`'s surface, and its answer.
async fn rpc_as(core: &Arc<Core>, client: Client, method: &str, params: Value) -> Value {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r
                .result
                .unwrap_or_else(|| panic!("{method}: {:?}", r.error));
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// The newest row of `kind`'s data.
fn newest(core: &Core, kind: &str) -> Value {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(1000)
        .unwrap()
        .into_iter()
        .rev()
        .find(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .unwrap_or_else(|| panic!("no {kind} row"))
}

/// The three connections each act comes from: the CLI's socket, the web
/// UI's WebSocket, and a test's unnamed one.
fn clients() -> [(Client, &'static str); 3] {
    [
        (Client::new("sock#7", Surface::Cli), "the CLI"),
        (Client::new("web#35", Surface::Web), "the web UI"),
        ("tide#3".to_string().into(), "tide#3"),
    ]
}

/// `profile.use`: its `profile.changed` row and its answer name the surface.
#[tokio::test]
async fn a_profile_use_names_its_surface_not_its_label() {
    let core = core();
    for (client, by) in clients() {
        let changed = rpc_as(&core, client, method::PROFILE_USE, json!({"name": "glm"})).await;
        assert_eq!(changed["by"], by);
        assert_eq!(newest(&core, "profile.changed")["by"], by);
    }
}

/// `session.recompile`: its `context.recompile_requested` row names the
/// surface.
#[tokio::test]
async fn a_recompile_names_its_surface_not_its_label() {
    let core = core();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    for (client, by) in clients() {
        rpc_as(
            &core,
            client,
            method::SESSION_RECOMPILE,
            json!({"session_id": rec.session_id, "strategy": "transcript"}),
        )
        .await;
        let row = newest(&core, "context.recompile_requested");
        assert_eq!(
            (row["by"].as_str(), row["strategy"].as_str()),
            (Some(by), Some("transcript"))
        );
    }
}
