//! A stopping daemon begins no turn's model call (theseus-jtrc), through the
//! whole core with a scripted stand-in provider, each by order: the stop
//! begins, then the driver or a turn asks.
//! - The driver's retry of a turn that failed with a class that passes with
//!   time is not begun once the stop has: the execution stays queued for the
//!   next start, and the provider is not asked again.
//! - A turn that reaches its call after the stop began settles the call
//!   failed without sending it, as one whose connection the network refused,
//!   makes no in-turn retry, and leaves its execution woken for the next
//!   start's driver, as before.
//! - That failure has its own class, `stopping` (theseus-36re): a session
//!   bound to a place gets no notice of it, and health counts no provider
//!   error.

use std::path::Path;
use std::sync::Arc;

use theseus_kernel::ExecState;

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderError, Scripted, TimeoutPhase};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::{TurnError, TurnRequest};
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg
}

fn rig(script: Vec<Scripted>, transient: u32) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = config(&root.canonicalize().unwrap(), dir.path());
    cfg.model.retries = crate::config::Retries {
        transient,
        backoff_ms: 5,
        backoff_max_ms: 5,
        ..Default::default()
    };
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

fn session(core: &Core) -> String {
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<()> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .map(|_| ())
}

fn first_byte() -> Scripted {
    Scripted::Fail(ProviderError::Timeout {
        phase: TimeoutPhase::FirstByte,
        elapsed_ms: 1_000,
    })
}

fn execution(core: &Core, sid: &str) -> theseus_kernel::Execution {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    core.kernel.execution(&exec).unwrap().unwrap()
}

/// The ledger rows of `kind`, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<serde_json::Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(500).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

fn thens(core: &Core) -> Vec<String> {
    rows(core, "turn.next")
        .iter()
        .map(|r| r["then"].as_str().unwrap().to_string())
        .collect()
}

/// A conversation bound to a place, so that its notices are outbox posts.
fn bound_session(core: &Core) -> String {
    let sid = session(core);
    core.outbox.bind_place("dm:7", &sid).unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: "discord:dm:7".into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    sid
}

/// The failed-turn notices the session's place has been sent.
fn notices(core: &Core) -> Vec<serde_json::Value> {
    core.outbox
        .open_for("discord:dm:7")
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "failed")
        .map(|a| crate::outbox::body_of(a).clone())
        .collect()
}

/// `turn.submit` in `sid` over a connection, as a client sends it: its
/// answer.
async fn submit(core: &Arc<Core>, sid: &str, input: &str) -> theseus_protocol::Response {
    use theseus_protocol::{method, Id, Message, Request, TurnSubmitParams};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let params = TurnSubmitParams {
        carried: false,
        prompt: None,
        session_id: Some(sid.into()),
        input: input.into(),
        profile: None,
        provider: None,
        model: None,
        author: None,
        attachments: vec![],
        reply_to: None,
        opened_from: None,
    };
    let mut line =
        serde_json::to_string(&Request::new(Id::Num(1), method::TURN_SUBMIT, params)).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let answer = loop {
        let line = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&line).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop((cw, lines));
    let _ = srv.await;
    answer
}

/// A first-byte timeout with no in-turn retry, as `transient = 0` makes it:
/// the turn fails and its execution is queued for the driver's retry
/// (theseus-ljr). The stop begins, and then the driver's continuation is not
/// begun: the provider saw the one call, and the execution waits, queued,
/// for the next start. Before, the retry began, and its call went out.
#[tokio::test]
async fn the_driver_begins_no_retry_once_the_stop_has_begun() {
    let r = rig(vec![first_byte(), Scripted::text("Hello.")], 0);
    let sid = session(&r.core);
    turn(&r.core, &sid, "say hello")
        .await
        .expect_err("the first byte never came");
    let e = execution(&r.core, &sid);
    assert_eq!(
        (e.state, e.resume_pending),
        (ExecState::Queued, true),
        "the driver's retry is queued"
    );
    r.core.stopping_on("SIGTERM");
    let resumed = r.core.continue_execution(&e.id).await.unwrap();
    assert!(resumed.is_none(), "a continuation began during the stop");
    assert_eq!(r.fake.requests().len(), 1, "the provider was asked again");
    let e = execution(&r.core, &sid);
    assert_eq!(
        (e.state, e.resume_pending),
        (ExecState::Queued, true),
        "the next start's driver takes it"
    );
}

/// A turn that reaches its call after the stop began, with in-turn retries
/// on (the bench profile's): nothing is sent, the call is settled failed as
/// a refused connection, no retry is made inside the turn, and the run says
/// backoff with the execution woken, so the next start retries it.
#[tokio::test]
async fn a_call_asked_after_the_stop_began_is_not_sent() {
    let r = rig(vec![Scripted::text("Hello.")], 4);
    let sid = session(&r.core);
    r.core.stopping_on("SIGTERM");
    let err = turn(&r.core, &sid, "say hello")
        .await
        .expect_err("a call during the stop");
    let te = err.downcast_ref::<TurnError>().expect("a turn's failure");
    assert_eq!((te.class.as_str(), te.transient), ("stopping", true));
    assert!(format!("{err:#}").contains("not sent"), "{err:#}");
    assert!(r.fake.requests().is_empty(), "a request went out");
    assert_eq!(
        rows(&r.core, "provider.error").len(),
        1,
        "a retry inside the turn"
    );
    assert_eq!(thens(&r.core), ["backoff"]);
    let e = execution(&r.core, &sid);
    assert_eq!((e.state, e.resume_pending), (ExecState::Queued, true));
}

/// A client's turn in a session bound to a place reaches its call after the
/// stop began (theseus-36re). The call is not sent, and its failure's class
/// is `stopping`: the run keeps its backoff for the next start, the place is
/// sent no "Turn failed" notice, and health counts no provider error. As a
/// refused connection (`network`), the notice was written during the stop,
/// posted at the next start before the retry's answer, and counted.
#[tokio::test]
async fn a_turn_the_stop_kept_back_posts_no_notice_and_counts_no_provider_error() {
    let r = rig(vec![Scripted::text("Hello.")], 0);
    let sid = bound_session(&r.core);
    r.core.stopping_on("SIGTERM");
    let answer = submit(&r.core, &sid, "say hello").await;
    let error = answer.error.expect("a call during the stop");
    assert_eq!(
        (
            error.data["class"].as_str(),
            error.data["transient"].as_bool()
        ),
        (Some("stopping"), Some(true)),
        "{error:?}"
    );
    assert!(r.fake.requests().is_empty(), "a request went out");
    assert_eq!(rows(&r.core, "provider.error")[0]["class"], "stopping");
    let next = rows(&r.core, "turn.next");
    assert_eq!(next.len(), 1, "{next:?}");
    assert_eq!(
        (&next[0]["then"], &next[0]["class"], &next[0]["notice"]),
        (
            &serde_json::json!("backoff"),
            &serde_json::json!("stopping"),
            &serde_json::json!(false)
        )
    );
    assert_eq!(notices(&r.core), Vec::<serde_json::Value>::new());
    assert_eq!(r.core.health().provider_errors, 0, "a provider's error");
    let e = execution(&r.core, &sid);
    assert_eq!(
        (e.state, e.resume_pending),
        (ExecState::Queued, true),
        "the next start's driver retries it"
    );
}
