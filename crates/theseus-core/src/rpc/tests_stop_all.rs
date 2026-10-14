//! `execution.stop` with `tasks` is an owner's act (theseus-cdcwr): from a shared place it is refused, and writes
//! and cancels nothing, while the plain stop is anyone's as ever; from the CLI and the owner's DM it goes.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{error_code, Message, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::provider::FakeProvider;
use crate::tests_recall::{session, turn, DEN, OWNER, PIER};
use crate::{Config, Core};

fn core() -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{DEN}"),
        name: "#den".into(),
        private: true,
        ..Default::default()
    }]);
    (core, dir)
}

/// One request on a connection of `client`'s surface, its whole response.
async fn call(core: &Arc<Core>, client: Client, method: &str, params: Value) -> Response {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

fn origin(user: u64, channel: u64, guild: bool) -> Value {
    json!({"user_id": user.to_string(), "channel_id": channel.to_string(),
           "guild_id": guild.then_some("7")})
}

fn stop(eid: &str, tasks: bool, discord: Option<Value>) -> Value {
    let mut p = json!({"execution_id": eid, "author": "test"});
    if tasks {
        p["tasks"] = json!(true);
    }
    if let Some(d) = discord {
        p["discord"] = d;
    }
    p
}

fn ledger_kinds(core: &Core, kind: &str) -> usize {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(500)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_of_the_tasks_too_is_refused_from_a_shared_place_and_the_plain_stop_is_not() {
    let (core, _dir) = core();
    let sid = session(&core, None, &["the weir is quiet"]);
    turn(&core, &sid, "hello").await;
    let eid = core
        .kernel
        .open_executions()
        .unwrap()
        .into_iter()
        .find(|e| e.session_id == sid)
        .unwrap()
        .id;
    let discord = || Client::new("discord", Surface::Discord);

    // A stranger in a shared channel: the plain stop goes (anyone's), the stop of its tasks does not.
    let shared = Some(origin(PIER, 4040, true));
    let plain = call(
        &core,
        discord(),
        "execution.stop",
        stop(&eid, false, shared.clone()),
    )
    .await;
    assert!(plain.error.is_none(), "{:?}", plain.error);
    let all = call(&core, discord(), "execution.stop", stop(&eid, true, shared)).await;
    let err = all.error.expect("refused");
    assert_eq!(err.code, error_code::REFUSED);
    assert!(
        err.message.contains("Nothing was stopped"),
        "{}",
        err.message
    );
    assert_eq!(ledger_kinds(&core, "execution.stopped_all"), 0);
    assert_eq!(
        ledger_kinds(&core, "approval.refused"),
        1,
        "the refusal is ledgered"
    );

    // The owner in a guild channel nobody bound private is a shared place too.
    let loud = Some(origin(OWNER, 4041, true));
    let err = call(&core, discord(), "execution.stop", stop(&eid, true, loud))
        .await
        .error
        .expect("refused");
    assert_eq!(err.code, error_code::REFUSED);
    assert!(err.message.contains("shared place"), "{}", err.message);

    // The owner in their DM, in the den, and the CLI: it goes, and the row says who.
    let dm = Some(origin(OWNER, 4042, false));
    let ok = call(&core, discord(), "execution.stop", stop(&eid, true, dm)).await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
    let den = Some(origin(OWNER, DEN, true));
    let ok = call(&core, discord(), "execution.stop", stop(&eid, true, den)).await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
    let cli = Client::new("sock#1", Surface::Cli);
    let ok = call(&core, cli, "execution.stop", stop(&eid, true, None)).await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
    assert_eq!(ledger_kinds(&core, "execution.stopped_all"), 3);
}
