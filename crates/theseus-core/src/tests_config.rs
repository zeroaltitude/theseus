//! The config copy and its gate, end to end through the core (theseus-2fo):
//! a start served from the copy of the vault's note answers reads at once,
//! every method that acts waits for the vault, nothing acts on the copy's
//! word, and the vault's answer confirms the copy, holds it, or restarts the
//! daemon onto the vault's version.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_protocol::{error_code, method, ConfigRestart, RpcError, SessionKind};

use crate::approval::{Client, Surface};
use crate::bus::EventSink;
use crate::config_copy;
use crate::config_gate::{self, ConfigGate, Gate, ReadNote};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Parts;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const REF: &str = "op://V/theseus-config/notesPlain";

/// A vault whose note answers each read from a script, in order, once its
/// gate opens; the last answer repeats. It counts its reads.
struct Vault {
    answers: Mutex<VecDeque<Result<String, String>>>,
    last: Mutex<Option<Result<String, String>>>,
    open: tokio::sync::watch::Receiver<bool>,
    reads: AtomicU32,
}

impl Vault {
    /// Answers at once.
    fn new(answers: Vec<Result<String, String>>) -> Arc<Self> {
        let (tx, rx) = tokio::sync::watch::channel(true);
        drop(tx);
        Self::with(answers, rx)
    }
    /// Answers once the sender sends `true`; never, if it never does.
    fn gated(
        answers: Vec<Result<String, String>>,
    ) -> (Arc<Self>, tokio::sync::watch::Sender<bool>) {
        let (tx, rx) = tokio::sync::watch::channel(false);
        (Self::with(answers, rx), tx)
    }
    /// A later answer: the note as the operator changes it.
    fn then(&self, answer: Result<String, String>) {
        self.answers.lock().unwrap().push_back(answer);
    }
    fn with(
        answers: Vec<Result<String, String>>,
        open: tokio::sync::watch::Receiver<bool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            last: Mutex::new(None),
            open,
            reads: AtomicU32::new(0),
        })
    }
}

impl ReadNote for Vault {
    fn read_note<'a>(&'a self, reference: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            assert_eq!(reference, REF);
            self.reads.fetch_add(1, Ordering::SeqCst);
            let mut open = self.open.clone();
            if !*open.borrow() {
                let _ = open.wait_for(|o| *o).await;
            }
            let next = self.answers.lock().unwrap().pop_front();
            match next {
                Some(a) => {
                    *self.last.lock().unwrap() = Some(a.clone());
                    a
                }
                None => self
                    .last
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| Err("nothing scripted".into())),
            }
        })
    }
}

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    root: PathBuf,
    copy: PathBuf,
    store: Store,
    /// The copy's text, as the start read it.
    text: String,
    _dir: tempfile::TempDir,
}

/// The note: the template, working in `root`, with its state in `state`, and
/// every writer waiting for approval; `tweak` changes it.
fn note(root: &Path, state: &Path, tweak: impl FnOnce(&mut Config)) -> String {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.approve_paths = vec![];
    cfg.policy.enforcement = Posture::Approve;
    tweak(&mut cfg);
    let text = toml::to_string_pretty(&cfg).unwrap();
    Config::parse(&text).expect("the test note loads");
    text
}

/// A core started from the copy of the note `tweak` makes, as the daemon
/// starts from one: the gate confirming, with an acting method's wait cut to
/// `wait` and a held read retried after 20 ms.
fn from_copy(script: Vec<Scripted>, wait: Duration, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let text = note(&root, dir.path(), tweak);
    let copy = config_copy::path(Some(dir.path()));
    config_copy::write(&copy, REF, &text).unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let (core, fake) = start(script, wait, &copy, store.clone(), None);
    Rig {
        core,
        fake,
        root,
        copy,
        store,
        text,
        _dir: dir,
    }
}

/// One start from the copy at `copy`, on `store`: what a restart does, with
/// `restarted` its marker.
fn start(
    script: Vec<Scripted>,
    wait: Duration,
    copy: &Path,
    store: Store,
    restarted: Option<ConfigRestart>,
) -> (Arc<Core>, Arc<FakeProvider>) {
    let c = config_copy::read(copy, REF).unwrap().expect("a copy");
    let gate = ConfigGate::from_copy(REF, copy.to_path_buf(), c.text, Instant::now())
        .with_timings(wait, Duration::from_millis(20));
    if let Some(r) = restarted {
        gate.began_as_restart(r);
    }
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(Parts {
        config_gate: gate,
        ..Parts::for_tests(c.config, fake.clone(), store)
    })
    .unwrap();
    (core, fake)
}

/// One request over a real protocol connection, accepted as `client`: the
/// result, or the error.
async fn rpc_as(
    core: &Arc<Core>,
    client: Client,
    m: &str,
    params: Value,
) -> Result<Value, RpcError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), m, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => Ok(v),
                (None, e) => Err(e.unwrap()),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

fn cli() -> Client {
    Client::new("sock#1", Surface::Cli)
}

/// A turn run through the runner itself, as a test sets one up: the gate
/// is the dispatcher's, and this goes around it.
async fn turn(core: &Arc<Core>, input: &str) -> theseus_protocol::TurnSubmitResult {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
}

fn write_script() -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "fs_write",
                json!({"path": "out.txt", "content": "approved\n"}),
            )],
        ),
        Scripted::text("Written."),
    ]
}

fn ledgered(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

/// Until the gate leaves `Confirming`, at most 5 s.
async fn settled(core: &Core) -> Gate {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let g = core.config_gate.state();
        if g != Gate::Confirming || Instant::now() > deadline {
            return g;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// What a test sends each method: enough to reach the method.
fn params(m: &str) -> Value {
    match m {
        method::SESSION_HISTORY | method::SESSION_WATCH | method::SESSION_UNWATCH => {
            json!({"session_id": "ses_none"})
        }
        method::TURN_SUBMIT => json!({"input": "hi", "attachments": []}),
        method::NODE_REACH => json!({"node_id": "msg_none"}),
        _ => json!({}),
    }
}

// ---------------------------------------------------------------- the gate

/// With a vault that never answers: `health` answers inside the cold-start
/// budget and says the config is confirming; every method that only reads
/// answers at once; every method that acts waits, then fails with
/// `config_unconfirmed`, naming why; and `shutdown` works.
#[tokio::test]
async fn a_start_from_the_copy_answers_reads_at_once_and_every_acting_method_waits() {
    let wait = Duration::from_millis(1500);
    let r = from_copy(vec![], wait, |_| {});
    let (vault, _never) = Vault::gated(vec![]);
    tokio::spawn(config_gate::confirm(
        r.core.clone(),
        vault,
        None,
        Instant::now(),
    ));

    // Every method is a read or an act, and these are the reads.
    let reads: Vec<&str> = method::ALL
        .iter()
        .copied()
        .filter(|m| !crate::rpc::ACTS.contains(m))
        .collect();
    assert_eq!(reads.len() + crate::rpc::ACTS.len(), method::ALL.len());
    assert!(crate::rpc::ACTS.iter().all(|m| method::ALL.contains(m)));
    assert_eq!(
        reads,
        [
            "health",
            "session.list",
            "ledger.tail",
            "profile.list",
            "execution.list",
            "action.list",
            "confirm.list",
            "session.history",
            "session.watch",
            "session.unwatch",
            "catalog.list",
            "compilation.list",
            "node.list",
            "node.reach",
            "tool.list",
            "shutdown",
            "narrative.watch",
            "narrative.unwatch",
            "task.list",
            "wake.list",
            "executions.watch",
            "executions.unwatch",
            "session.wait",
            "index.status",
            "index.query",
        ]
    );

    let t0 = Instant::now();
    let h = rpc_as(&r.core, cli(), method::HEALTH, Value::Null)
        .await
        .unwrap();
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(50), "health took {took:?}");
    assert_eq!(h["config"]["state"], "confirming", "{}", h["config"]);
    assert_eq!(h["config"]["started_from"], "copy");
    assert_eq!(h["config"]["source"], "vault");
    assert_eq!(h["config"]["reference"], REF);
    assert_eq!(h["config"]["copy"].as_str(), Some(r.copy.to_str().unwrap()));

    for m in reads.iter().filter(|m| **m != method::SHUTDOWN) {
        let t0 = Instant::now();
        let got = rpc_as(&r.core, cli(), m, params(m)).await;
        assert!(t0.elapsed() < wait / 2, "{m} waited {:?}", t0.elapsed());
        if let Err(e) = got {
            assert_ne!(e.code, error_code::CONFIG_UNCONFIRMED, "{m}: {e:?}");
        }
    }

    // Every acting method at once: each waits out the bound, then fails.
    let t0 = Instant::now();
    let acting = crate::rpc::ACTS.iter().map(|m| {
        let core = r.core.clone();
        async move { (*m, rpc_as(&core, cli(), m, params(m)).await) }
    });
    for (m, got) in futures_util::future::join_all(acting).await {
        let e = got.expect_err(m);
        assert_eq!(e.code, error_code::CONFIG_UNCONFIRMED, "{m}: {e:?}");
        assert_eq!(e.data["class"], "config_unconfirmed", "{m}");
        assert_eq!(e.data["state"], "confirming", "{m}");
        assert!(
            e.message
                .contains("the vault has not confirmed the config this daemon started from")
                && e.message.contains(REF)
                && e.message.contains("nothing acts until it does"),
            "{m}: {}",
            e.message
        );
    }
    assert!(t0.elapsed() >= wait, "they waited {:?}", t0.elapsed());
    assert!(
        r.fake.requests.lock().unwrap().is_empty(),
        "no provider call"
    );
    assert!(
        ledgered(&r.core, "session.opened").is_empty(),
        "nothing written"
    );

    let ok = rpc_as(&r.core, cli(), method::SHUTDOWN, Value::Null)
        .await
        .unwrap();
    assert_eq!(ok["ok"], true);
    assert_eq!(ledgered(&r.core, "server.stopping").len(), 1);
}

/// With the vault slow, a turn sent at once waits at the gate, then runs
/// once the vault confirms the copy; its trace says how long it waited.
#[tokio::test]
async fn a_turn_sent_from_the_copy_waits_for_the_vault_then_runs() {
    let r = from_copy(
        vec![Scripted::text("after the vault")],
        Duration::from_secs(10),
        |_| {},
    );
    let (vault, open) = Vault::gated(vec![Ok(r.text.clone())]);
    tokio::spawn(config_gate::confirm(
        r.core.clone(),
        vault,
        None,
        Instant::now(),
    ));
    let core = r.core.clone();
    let sent = tokio::spawn(async move {
        rpc_as(
            &core,
            cli(),
            method::TURN_SUBMIT,
            params(method::TURN_SUBMIT),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!sent.is_finished(), "the turn waits for the vault");
    open.send(true).unwrap();
    let res = tokio::time::timeout(Duration::from_secs(10), sent)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(res["output"], "after the vault");
    let trace = serde_json::to_string(&res["trace"]).unwrap();
    assert!(trace.contains("config.wait"), "{trace}");
    let h = r.core.health();
    assert_eq!(h.config.state, "confirmed");
    assert_eq!(
        h.config.detail.as_deref(),
        Some("the same text as the copy")
    );
}

/// A config that may act already (a file, as every older test has) waits for
/// nothing, and a turn's trace has no `config.wait` span.
#[tokio::test]
async fn a_config_that_may_act_waits_for_nothing() {
    assert_eq!(ConfigGate::file("x").wait().await, Ok(Duration::ZERO));
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text("plain")]));
    let core = Core::build(Parts::for_tests(cfg, fake, store)).unwrap();
    let res = rpc_as(
        &core,
        cli(),
        method::TURN_SUBMIT,
        params(method::TURN_SUBMIT),
    )
    .await
    .unwrap();
    assert_eq!(res["output"], "plain");
    let trace = serde_json::to_string(&res["trace"]).unwrap();
    assert!(!trace.contains("config.wait"), "{trace}");
    assert_eq!(core.health().config.state, "confirmed");
}

/// Nothing acts on the copy's word: a continuation queued behind an answered
/// question waits while the vault is silent, and the driver runs it once the
/// vault answers the copy's own text. The startup phase `config.vault` and a
/// `config.confirmed` row record the read.
#[tokio::test]
async fn the_driver_runs_no_continuation_until_the_vault_confirms_the_copy() {
    let r = from_copy(write_script(), Duration::from_secs(10), |_| {});
    let res = turn(&r.core, "write out.txt").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let exec = res.execution_id.clone().unwrap();
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let queued = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(queued.state.as_str(), "queued");
    let (vault, open) = Vault::gated(vec![Ok(r.text.clone())]);
    tokio::spawn(crate::harness::drive(r.core.clone()));
    tokio::spawn(crate::harness::run(r.core.clone()));
    tokio::spawn(config_gate::confirm(
        r.core.clone(),
        vault,
        None,
        Instant::now(),
    ));
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        !r.root.join("out.txt").exists(),
        "nothing acted on the copy"
    );
    assert_eq!(
        r.fake.requests.lock().unwrap().len(),
        1,
        "only the first turn's call"
    );
    assert!(ledgered(&r.core, "driver.started").is_empty());
    let phase = r.core.startup_log.snapshot();
    let vault_phase = phase.iter().find(|p| p.name == "config.vault").unwrap();
    assert!(
        vault_phase.background && vault_phase.end_us.is_none(),
        "{vault_phase:?}"
    );

    open.send(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !r.root.join("out.txt").exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "approved\n"
    );
    assert_eq!(r.core.config_gate.state(), Gate::Open);
    let confirmed = ledgered(&r.core, "config.confirmed");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0]["how"], "the same text as the copy");
    assert_eq!(confirmed[0]["reference"], REF);
    let phase = r.core.startup_log.snapshot();
    let vault_phase = phase.iter().find(|p| p.name == "config.vault").unwrap();
    assert_eq!(vault_phase.detail["outcome"], "confirmed");
    assert!(ledgered(&r.core, "driver.started").len() == 1);
}

// ---------------------------------------------------------------- the vault's answer

/// Only comments or formatting differ: confirmed with no restart, and the
/// copy is rewritten to the vault's text.
#[tokio::test]
async fn only_comments_differ_so_it_confirms_and_rewrites_the_copy() {
    let r = from_copy(vec![], Duration::from_secs(10), |_| {});
    let commented = format!("# pasted on 2026-09-29\n{}\n\n", r.text.replace(" = ", "="));
    config_gate::confirm(
        r.core.clone(),
        Vault::new(vec![Ok(commented.clone())]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(r.core.config_gate.state(), Gate::Open);
    assert!(r.core.restart_requested().is_none());
    assert_eq!(
        r.core.health().config.detail.as_deref(),
        Some("only comments or formatting differed, and the copy was rewritten")
    );
    assert_eq!(
        config_copy::read(&r.copy, REF).unwrap().unwrap().text,
        commented
    );
    assert!(ledgered(&r.core, "config.changed").is_empty());
}

/// A different note that loads: `config.changed` names the tables and both
/// digests, the copy is rewritten, a request waiting at the gate is told the
/// daemon restarts, and the clean shutdown path runs. The restarted daemon
/// starts from the rewritten copy, the vault confirms it, and the vault's
/// version governs: `fs.read` asks first, as the vault says.
#[tokio::test]
async fn a_changed_note_restarts_onto_the_vault_and_the_vault_governs() {
    let r = from_copy(vec![], Duration::from_secs(10), |_| {});
    assert_eq!(r.core.tools.posture_now("fs.read").posture, Posture::Open);
    let dir = r._dir.path().to_path_buf();
    let vault_text = note(&r.root, &dir, |c| {
        c.policy.tools.insert("fs.read".into(), Posture::Approve);
    });
    let (vault, open) = Vault::gated(vec![Ok(vault_text.clone())]);
    let core = r.core.clone();
    let waiting =
        tokio::spawn(async move { rpc_as(&core, cli(), method::SESSION_OPEN, json!({})).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    open.send(true).unwrap();
    config_gate::confirm(r.core.clone(), vault, None, Instant::now()).await;

    let e = waiting.await.unwrap().expect_err("told to send it again");
    assert_eq!(e.code, error_code::CONFIG_UNCONFIRMED);
    assert_eq!(e.data["state"], "restarting");
    assert!(
        e.message
            .contains("restarting onto the vault's version: send this again"),
        "{}",
        e.message
    );
    let changed = ledgered(&r.core, "config.changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["tables"], json!(["policy.tools"]));
    assert_eq!(changed[0]["reference"], REF);
    assert_eq!(changed[0]["copy_sha256"], config_copy::sha256(&r.text));
    assert_eq!(changed[0]["vault_sha256"], config_copy::sha256(&vault_text));
    let restart = r.core.restart_requested().expect("a restart");
    assert_eq!(restart.tables, ["policy.tools"]);
    assert_eq!(r.core.config_gate.state(), Gate::Restarting);
    assert_eq!(r.core.health().config.state, "restarting");
    assert_eq!(
        ledgered(&r.core, "server.stopping").len(),
        1,
        "the clean shutdown path"
    );
    assert!(ledgered(&r.core, "session.opened").is_empty());
    assert_eq!(
        config_copy::read(&r.copy, REF).unwrap().unwrap().text,
        vault_text
    );

    // The restart: the same state dir, from the rewritten copy.
    drop(r.core);
    let (core, _) = start(
        vec![],
        Duration::from_secs(10),
        &r.copy,
        r.store.clone(),
        Some(restart.clone()),
    );
    config_gate::confirm(
        core.clone(),
        Vault::new(vec![Ok(vault_text.clone())]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(core.config_gate.state(), Gate::Open);
    assert!(core.restart_requested().is_none());
    assert_eq!(core.tools.posture_now("fs.read").posture, Posture::Approve);
    let h = core.health();
    assert_eq!(h.config.restarted.as_ref(), Some(&restart));
    assert_eq!(h.config.state, "confirmed");
    let confirmed = ledgered(&core, "config.confirmed");
    assert_eq!(
        confirmed.last().unwrap()["restarted"]["tables"],
        json!(["policy.tools"])
    );
    let lines: Vec<String> = core.narrator.tail().into_iter().map(|l| l.text).collect();
    assert!(
        lines.iter().any(
            |l| l.contains("began as a restart onto the vault's config note")
                && l.contains("policy.tools")
        ),
        "{lines:?}"
    );
}

/// No loop: a process that began as a restart and finds the vault's note
/// different again holds, answering reads only, and says so; it restarts
/// nothing. It confirms when a later read finds the copy's text.
#[tokio::test]
async fn a_restarted_daemon_that_finds_the_note_changed_again_holds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let text = note(&root, &root, |_| {});
    let again = note(&root, &root, |c| c.kernel.spend_limit_usd = 7.5);
    let copy = config_copy::path(Some(&root));
    config_copy::write(&copy, REF, &text).unwrap();
    let store = Store::open(&root.join("store")).unwrap();
    let marker = ConfigRestart {
        reference: REF.into(),
        at_unix_ms: 1_790_000_000_000,
        tables: vec!["kernel".into()],
        copy_sha256: "a".into(),
        vault_sha256: "b".into(),
    };
    let (core, _) = start(
        vec![],
        Duration::from_millis(200),
        &copy,
        store,
        Some(marker),
    );
    let vault = Vault::new(vec![Ok(again)]);
    tokio::spawn(config_gate::confirm(
        core.clone(),
        vault.clone(),
        None,
        Instant::now(),
    ));
    let held = settled(&core).await;
    assert_eq!(
        held,
        Gate::Held("the vault's note changed again since the restart; restart to apply".into())
    );
    let h = core.health().config;
    assert_eq!(
        (h.state.as_str(), h.detail.as_deref()),
        (
            "held",
            Some("the vault's note changed again since the restart; restart to apply")
        )
    );
    assert!(core.restart_requested().is_none(), "no second restart");
    assert!(ledgered(&core, "config.changed").is_empty());
    assert_eq!(
        ledgered(&core, "config.held")[0]["change"]["tables"],
        json!(["kernel"])
    );
    assert_eq!(config_copy::read(&copy, REF).unwrap().unwrap().text, text);
    let e = rpc_as(&core, cli(), method::PROFILE_USE, json!({"name": "glm"}))
        .await
        .unwrap_err();
    assert_eq!(e.data["state"], "held");
    assert!(
        e.message
            .contains("the config is held: the vault's note changed again"),
        "{}",
        e.message
    );
    // The note is put back: the next read finds the copy's text.
    vault.then(Ok(text.clone()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while core.config_gate.state() != Gate::Open && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(core.config_gate.state(), Gate::Open);
    assert!(
        vault.reads.load(Ordering::SeqCst) >= 2,
        "read again after 20 ms"
    );
}

/// A note that does not load, and a vault that does not answer: each holds,
/// health says why, the ledger says so once, the copy is left as it was, and
/// each confirms when a later read succeeds.
#[tokio::test]
async fn an_invalid_note_and_an_unreachable_vault_each_hold_and_recover() {
    for (bad, says, row) in [
        (
            Ok("[kernel]\nspend_limit_usd = -1.0\n".to_string()),
            "the vault's note does not load (",
            "config.invalid",
        ),
        (
            Err("network down".to_string()),
            "the vault did not answer: network down",
            "config.unreachable",
        ),
    ] {
        let r = from_copy(vec![], Duration::from_millis(200), |_| {});
        let vault = Vault::new(vec![bad.clone(), bad, Ok(r.text.clone())]);
        tokio::spawn(config_gate::confirm(
            r.core.clone(),
            vault,
            None,
            Instant::now(),
        ));
        let Gate::Held(why) = settled(&r.core).await else {
            panic!("{says}: not held");
        };
        assert!(why.starts_with(says), "{why}");
        let h = r.core.health().config;
        assert_eq!(h.state, "held");
        assert!(h.detail.unwrap().starts_with(says));
        assert!(h.retry_in_ms.is_some());
        let rows = ledgered(&r.core, row);
        assert_eq!(rows.len(), 1, "{row}: said once");
        assert_eq!(rows[0]["reference"], REF);
        assert_eq!(
            config_copy::read(&r.copy, REF).unwrap().unwrap().text,
            r.text
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while r.core.config_gate.state() != Gate::Open && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(r.core.config_gate.state(), Gate::Open, "{row}: recovers");
        assert_eq!(r.core.health().config.reads, 3);
    }
}

// ---------------------------------------------------------------- security

/// The copy is on the tool floor, as the token file is (theseus-2fo): a tool
/// that would write it waits for approval, even under `open` and even inside
/// the roots, and the copy is left as it was.
#[tokio::test]
async fn the_copy_is_on_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    // Inside the roots on purpose: only the floor stops the write.
    let copy = root.join(config_copy::FILE);
    let text = note(&root, dir.path(), |c| c.policy.enforcement = Posture::Open);
    config_copy::write(&copy, REF, &text).unwrap();
    let mut cfg = Config::parse(&text).unwrap().0;
    cfg.config_copy = Some(copy.clone());
    let store = Store::open(&dir.path().join("store")).unwrap();
    let widen =
        json!({"path": copy.to_string_lossy(), "content": "[approval]\nchannels = [\"cli\"]\n"});
    let fake = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t1", "fs_write", widen)]),
        Scripted::text("Waiting on you."),
    ]));
    let core = Core::build(Parts::for_tests(cfg, fake, store)).unwrap();
    let res = turn(&core, "widen the approval").await;
    res.awaiting_confirm
        .clone()
        .expect("the floor waits, even under open");
    let pending = core.pending_confirms(&res.session_id).unwrap();
    assert!(pending[0].floor, "{}", pending[0].reason);
    assert_eq!(
        config_copy::read(&copy, REF).unwrap().unwrap().text,
        text,
        "the copy is untouched"
    );
}

const EDDIE: &str = "271828182845904523";

/// A copy edited to widen `[approval]` (the CLI added to its channels) never
/// judges an approval: the CLI's answer waits at the gate, the vault's note
/// differs, and the daemon restarts; after the restart the vault's version
/// judges the same answer, and refuses it. Nothing was written.
#[tokio::test]
async fn a_copy_that_widens_approval_never_judges_an_approval() {
    let approval = |channels: &[&str]| crate::config::ApprovalConfig {
        trusted_users: vec![format!("discord:{EDDIE}")],
        channels: channels.iter().map(|c| c.to_string()).collect(),
    };
    let r = from_copy(write_script(), Duration::from_secs(10), |c| {
        c.approval = Some(approval(&["cli", "discord:dm"]))
    });
    let dir = r._dir.path().to_path_buf();
    let vault_text = note(&r.root, &dir, |c| {
        c.approval = Some(approval(&["discord:dm"]))
    });
    let res = turn(&r.core, "write out.txt").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let answer = json!({"correlation_id": corr, "approve": true});

    let (vault, open) = Vault::gated(vec![Ok(vault_text.clone())]);
    tokio::spawn(config_gate::confirm(
        r.core.clone(),
        vault,
        None,
        Instant::now(),
    ));
    let core = r.core.clone();
    let a = answer.clone();
    let waiting =
        tokio::spawn(async move { rpc_as(&core, cli(), method::ACTION_CONFIRM, a).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!waiting.is_finished(), "the answer waits for the vault");
    open.send(true).unwrap();
    let e = waiting
        .await
        .unwrap()
        .expect_err("never judged by the copy");
    assert_eq!(e.data["state"], "restarting", "{e:?}");
    assert_eq!(
        ledgered(&r.core, "config.changed")[0]["tables"],
        json!(["approval"])
    );
    assert!(ledgered(&r.core, "action.confirm_answered").is_empty());
    assert!(ledgered(&r.core, "approval.refused").is_empty());
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Planned);
    tokio::time::timeout(Duration::from_secs(5), r.core.restart_asked())
        .await
        .unwrap();
    let restart = r.core.restart_requested().unwrap();

    // After the restart, the vault's version judges the same answer.
    drop(r.core);
    let (core, _) = start(
        write_script(),
        Duration::from_secs(10),
        &r.copy,
        r.store.clone(),
        Some(restart),
    );
    config_gate::confirm(
        core.clone(),
        Vault::new(vec![Ok(vault_text)]),
        None,
        Instant::now(),
    )
    .await;
    let e = rpc_as(&core, cli(), method::ACTION_CONFIRM, answer)
        .await
        .expect_err("refused");
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(
        e.message
            .contains("the CLI is not a trusted channel ([approval] channels = [\"discord:dm\"])"),
        "{}",
        e.message
    );
    assert!(!r.root.join("out.txt").exists());
    assert_eq!(
        core.kernel.action(&corr).unwrap().unwrap().state,
        theseus_kernel::ActionState::Planned
    );
}

// ---------------------------------------------------------------- the spend limit (theseus-3pj)

/// "I raised the limit in the vault and restarted": the daemon starts from
/// the old copy, the vault's changed note restarts it onto the new one, and
/// on the vault's word the session waiting at its old limit takes the new
/// limit and continues. Its question is withdrawn with the reason, before
/// anything may act. Nothing written under either copy changed a limit, and
/// neither the spend nor the lifetime cost went down.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn a_limit_raised_in_the_vault_lets_a_session_waiting_at_its_old_limit_continue() {
    use theseus_kernel::{micros_to_usd, ActionState, ExecState};
    let r = from_copy(
        vec![Scripted::tools(
            &"word ".repeat(30_000),
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        )],
        Duration::from_secs(10),
        |c| c.kernel.spend_limit_usd = 1.40,
    );
    config_gate::confirm(
        r.core.clone(),
        Vault::new(vec![Ok(r.text.clone())]),
        None,
        Instant::now(),
    )
    .await;
    let res = turn(&r.core, "diff these").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    let (sid, exec) = (res.session_id.clone(), res.execution_id.clone().unwrap());
    let q = res.awaiting_confirm.clone().expect("it asks");
    let spent = r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .budget
        .spent_micros;
    let lifetime = |core: &Core| {
        core.store
            .get_session::<SessionRecord>(&sid)
            .unwrap()
            .unwrap()
            .cost_usd
    };
    let before = lifetime(&r.core);
    let dir = r._dir.path().to_path_buf();
    let raised = note(&r.root, &dir, |c| c.kernel.spend_limit_usd = 3.0);

    // The start after the edit serves from the old copy; the vault's note
    // differs, so it restarts onto it.
    drop(r.core);
    let (core, _) = start(
        vec![],
        Duration::from_secs(10),
        &r.copy,
        r.store.clone(),
        None,
    );
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.budget.limit_micros),
        (ExecState::Waiting, 1_400_000)
    );
    config_gate::confirm(
        core.clone(),
        Vault::new(vec![Ok(raised.clone())]),
        None,
        Instant::now(),
    )
    .await;
    let restart = core.restart_requested().expect("a restart onto the note");
    assert_eq!(restart.tables, ["kernel"]);
    assert_eq!(
        core.kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .budget
            .limit_micros,
        1_400_000,
        "a restart onto the note changes nothing itself"
    );

    // The restarted daemon, from the rewritten copy: unconfirmed, it still
    // writes no limit; on the vault's word it does, before the gate opens.
    drop(core);
    let (core, fake) = start(
        vec![Scripted::text("The diff is one line.")],
        Duration::from_secs(10),
        &r.copy,
        r.store.clone(),
        Some(restart),
    );
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.budget.limit_micros, e.budget.question.as_deref()),
        (1_400_000, Some(q.as_str()))
    );
    assert!(ledgered(&core, "budget.limit_changed").is_empty());
    config_gate::confirm(
        core.clone(),
        Vault::new(vec![Ok(raised)]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(core.config_gate.state(), Gate::Open);
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (
            e.state,
            e.budget.limit_micros,
            e.budget.spent_micros,
            e.budget.resets
        ),
        (ExecState::Queued, 3_000_000, spent, 0)
    );
    assert!(e.resume_pending && e.budget.question.is_none());
    let withdrawn = core.kernel.action(&q).unwrap().unwrap();
    assert_eq!(withdrawn.state, ActionState::Cancelled);
    assert_eq!(
        withdrawn.resolution.as_deref(),
        Some("withdrawn: the spend limit was raised from $1.40 to $3")
    );
    assert!(core.pending_confirms(&sid).unwrap().is_empty());
    let changed = ledgered(&core, "budget.limit_changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(
        (
            &changed[0]["from_usd"],
            &changed[0]["to_usd"],
            &changed[0]["spent_usd"]
        ),
        (&json!(1.4), &json!(3.0), &json!(micros_to_usd(spent)))
    );
    // The limit was the vault's before anything could act on the old one.
    let rows = core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap();
    let at = |kind: &str| rows.iter().rev().find(|(_, r)| r.kind == kind).unwrap().0;
    assert!(at("budget.limit_changed") < at("config.confirmed"));
    let lines: Vec<String> = core.narrator.tail().into_iter().map(|l| l.text).collect();
    assert!(
        lines.iter().any(|l| l.contains("follows the config's spend limit: $1.40 before, $3 now; the call that waited at the old limit proceeds")),
        "{lines:?}"
    );
    assert_eq!(lifetime(&core), before, "a new limit lowers nothing");

    // The driver's continuation makes the call that waited, under the new limit.
    let cont = core.continue_execution(&exec).await.unwrap().unwrap();
    assert!(cont.continuation);
    assert_eq!(cont.output, "The diff is one line.");
    assert_eq!(fake.requests().len(), 1, "the waiting call ran");
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert!(e.budget.spent_micros > spent, "no reset: the spend goes on");
    let now = before + cont.cost_usd.unwrap();
    assert!((lifetime(&core) - now).abs() < 1e-9);
}
