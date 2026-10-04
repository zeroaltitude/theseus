//! The config copy end to end through the core (theseus-2fo, theseus-zmgb):
//! a start served from the copy the daemon wrote acts on it at once, and the
//! vault's one read after serving confirms the copy, rewrites it and its
//! digest, restarts the daemon onto a changed note, or says why it could not
//! and keeps serving the copy.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_protocol::{method, ConfigRestart, RpcError, SessionKind};

use crate::approval::{Client, Surface};
use crate::bus::EventSink;
use crate::config_copy;
use crate::config_gate::{self, ConfigGate, ReadNote, State};
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

/// A core started from the copy of the note `tweak` makes, kept as the
/// daemon keeps one (its digest in the store), as the daemon starts from it.
fn from_copy(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let text = note(&root, dir.path(), tweak);
    let copy = config_copy::path(Some(dir.path()));
    let store = Store::open(&dir.path().join("store")).unwrap();
    config_copy::keep(&store, &copy, REF, &text).unwrap();
    let (core, fake) = start(script, &copy, store.clone(), None);
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
/// `restarted` its marker. The copy must be the one the daemon wrote, as
/// `theseusd` requires before it serves from one.
fn start(
    script: Vec<Scripted>,
    copy: &Path,
    store: Store,
    restarted: Option<ConfigRestart>,
) -> (Arc<Core>, Arc<FakeProvider>) {
    let c = config_copy::read(copy, REF).unwrap().expect("a copy");
    assert_eq!(config_copy::written_by_daemon(&store, &c.text), Ok(()));
    let gate = ConfigGate::from_copy(REF, copy.to_path_buf(), c.text, Instant::now());
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

/// A turn run through the runner itself, as a test sets one up.
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
            reply_to: None,
        })
        .await
        .unwrap()
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

/// Until the vault's read leaves `Confirming`, at most 5 s.
async fn settled(core: &Core) -> State {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let g = core.config_gate.state();
        if g != State::Confirming || Instant::now() > deadline {
            return g;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// An acting method answers while the vault is silent: the copy acts.
async fn opens_a_session(core: &Arc<Core>) {
    let opened = tokio::time::timeout(
        Duration::from_secs(10),
        rpc_as(core, cli(), method::SESSION_OPEN, json!({})),
    )
    .await
    .expect("no wait for the vault")
    .expect("session.open acts on the copy");
    assert!(opened["session_id"].is_string(), "{opened}");
}

// ---------------------------------------------------------------- acting on the copy

/// With a vault that never answers, a start from the copy the daemon wrote
/// acts at once (theseus-zmgb): health says it is confirming, a turn runs
/// and its trace has no wait for the config, a session opens, and `shutdown`
/// works.
#[tokio::test]
async fn a_start_from_the_copy_acts_at_once_while_the_vault_is_read() {
    let r = from_copy(vec![Scripted::text("from the copy")], |_| {});
    let (vault, _never) = Vault::gated(vec![]);
    tokio::spawn(config_gate::check(
        r.core.clone(),
        vault,
        None,
        Instant::now(),
    ));
    let h = rpc_as(&r.core, cli(), method::HEALTH, Value::Null)
        .await
        .unwrap();
    assert_eq!(h["config"]["state"], "confirming", "{}", h["config"]);
    assert_eq!(h["config"]["started_from"], "copy");
    assert_eq!(h["config"]["source"], "vault");
    assert_eq!(h["config"]["reference"], REF);
    assert_eq!(h["config"]["copy"].as_str(), Some(r.copy.to_str().unwrap()));
    assert!(
        h["config"]["detail"]
            .as_str()
            .is_some_and(|d| d.starts_with("acting on the copy")),
        "{}",
        h["config"]
    );

    let res = tokio::time::timeout(
        Duration::from_secs(10),
        rpc_as(
            &r.core,
            cli(),
            method::TURN_SUBMIT,
            json!({"input": "hi", "attachments": []}),
        ),
    )
    .await
    .expect("the turn waits for nothing")
    .unwrap();
    assert_eq!(res["output"], "from the copy");
    let trace = serde_json::to_string(&res["trace"]).unwrap();
    assert!(!trace.contains("config.wait"), "{trace}");
    assert_eq!(r.fake.requests.lock().unwrap().len(), 1);
    opens_a_session(&r.core).await;
    assert_eq!(r.core.config_gate.state(), State::Confirming);

    let ok = rpc_as(&r.core, cli(), method::SHUTDOWN, Value::Null)
        .await
        .unwrap();
    assert_eq!(ok["ok"], true);
    assert_eq!(ledgered(&r.core, "server.stopping").len(), 1);
}

// ---------------------------------------------------------------- the vault's answer

/// The same text: confirmed, with nothing written; health says so, and the
/// `config.vault` startup phase records the read.
#[tokio::test]
async fn the_same_text_confirms_and_writes_nothing() {
    let r = from_copy(vec![], |_| {});
    let at = r.store.stats().unwrap().last_position;
    config_gate::check(
        r.core.clone(),
        Vault::new(vec![Ok(r.text.clone())]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(r.core.config_gate.state(), State::Confirmed);
    let h = r.core.health().config;
    assert_eq!(
        (h.state.as_str(), h.detail.as_deref(), h.reads),
        ("confirmed", Some("the same text as the copy"), 1)
    );
    assert_eq!(
        r.store.stats().unwrap().last_position,
        at,
        "nothing written"
    );
    let phases = r.core.startup_log.snapshot();
    let vault = phases.iter().find(|p| p.name == "config.vault").unwrap();
    assert!(vault.background, "{vault:?}");
    assert_eq!(vault.detail["outcome"], "confirmed");
}

/// Only comments or formatting differ: confirmed with no restart, and the
/// copy and its digest are rewritten to the vault's text.
#[tokio::test]
async fn only_comments_differ_so_it_confirms_and_rewrites_the_copy() {
    let r = from_copy(vec![], |_| {});
    let commented = format!("# pasted on 2026-09-29\n{}\n\n", r.text.replace(" = ", "="));
    config_gate::check(
        r.core.clone(),
        Vault::new(vec![Ok(commented.clone())]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(r.core.config_gate.state(), State::Confirmed);
    assert!(r.core.restart_requested().is_none());
    assert_eq!(
        r.core.health().config.detail.as_deref(),
        Some("only comments or formatting differed, and the copy was rewritten")
    );
    assert_eq!(
        config_copy::read(&r.copy, REF).unwrap().unwrap().text,
        commented
    );
    assert_eq!(config_copy::written_by_daemon(&r.store, &commented), Ok(()));
    assert!(ledgered(&r.core, "config.changed").is_empty());
}

/// A different note that loads: `config.changed` names the tables and both
/// digests, the copy and its digest are rewritten, and the clean shutdown
/// path runs. The restarted daemon starts from the rewritten copy, and the
/// vault's version governs at once: `fs.read` asks first, as the vault says.
#[tokio::test]
async fn a_changed_note_restarts_onto_the_vault_and_the_vault_governs() {
    let r = from_copy(vec![], |_| {});
    assert_eq!(r.core.tools.posture_now("fs.read").posture, Posture::Open);
    let dir = r._dir.path().to_path_buf();
    let vault_text = note(&r.root, &dir, |c| {
        c.policy.tools.insert("fs.read".into(), Posture::Approve);
    });
    config_gate::check(
        r.core.clone(),
        Vault::new(vec![Ok(vault_text.clone())]),
        None,
        Instant::now(),
    )
    .await;

    let changed = ledgered(&r.core, "config.changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["tables"], json!(["policy.tools"]));
    assert_eq!(changed[0]["reference"], REF);
    assert_eq!(changed[0]["copy_sha256"], config_copy::sha256(&r.text));
    assert_eq!(changed[0]["vault_sha256"], config_copy::sha256(&vault_text));
    let restart = r.core.restart_requested().expect("a restart");
    assert_eq!(restart.tables, ["policy.tools"]);
    assert_eq!(r.core.config_gate.state(), State::Restarting);
    assert_eq!(r.core.health().config.state, "restarting");
    assert_eq!(
        ledgered(&r.core, "server.stopping").len(),
        1,
        "the clean shutdown path"
    );
    assert_eq!(
        config_copy::read(&r.copy, REF).unwrap().unwrap().text,
        vault_text
    );
    assert_eq!(
        config_copy::written_by_daemon(&r.store, &vault_text),
        Ok(())
    );

    // The restart: the same state dir, from the rewritten copy, which acts
    // before the vault answers.
    drop(r.core);
    let (core, _) = start(vec![], &r.copy, r.store.clone(), Some(restart.clone()));
    assert_eq!(core.tools.posture_now("fs.read").posture, Posture::Approve);
    config_gate::check(
        core.clone(),
        Vault::new(vec![Ok(vault_text.clone())]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(core.config_gate.state(), State::Confirmed);
    assert!(core.restart_requested().is_none());
    let h = core.health();
    assert_eq!(h.config.restarted.as_ref(), Some(&restart));
    assert_eq!(h.config.state, "confirmed");
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
/// different again says so and keeps serving, and acting on, the copy; it
/// restarts nothing and reads the vault once.
#[tokio::test]
async fn a_restarted_daemon_that_finds_the_note_changed_again_keeps_serving() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let text = note(&root, &root, |_| {});
    let again = note(&root, &root, |c| c.kernel.spend_limit_usd = 7.5);
    let copy = config_copy::path(Some(&root));
    let store = Store::open(&root.join("store")).unwrap();
    config_copy::keep(&store, &copy, REF, &text).unwrap();
    let marker = ConfigRestart {
        reference: REF.into(),
        at_unix_ms: 1_790_000_000_000,
        tables: vec!["kernel".into()],
        copy_sha256: "a".into(),
        vault_sha256: "b".into(),
    };
    let (core, _) = start(vec![], &copy, store, Some(marker));
    let vault = Vault::new(vec![Ok(again)]);
    tokio::spawn(config_gate::check(
        core.clone(),
        vault.clone(),
        None,
        Instant::now(),
    ));
    let held = settled(&core).await;
    let why = "the vault's note changed again since the restart; restart to apply";
    assert_eq!(held, State::Held(why.into()));
    let h = core.health().config;
    assert_eq!(
        (h.state.as_str(), h.detail.as_deref(), h.reads),
        ("held", Some(why), 1)
    );
    assert!(core.restart_requested().is_none(), "no second restart");
    assert!(ledgered(&core, "config.changed").is_empty());
    assert_eq!(
        ledgered(&core, "config.held")[0]["change"]["tables"],
        json!(["kernel"])
    );
    assert_eq!(config_copy::read(&copy, REF).unwrap().unwrap().text, text);
    opens_a_session(&core).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(vault.reads.load(Ordering::SeqCst), 1, "read once");
}

/// A note that does not load, and a vault that does not answer: each says
/// why in health, the ledger, and the narrative, once; the copy is left as
/// it was; and the daemon keeps serving, and acting on, the copy.
#[tokio::test]
async fn an_invalid_note_and_an_unreachable_vault_each_say_so_and_keep_serving() {
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
        let r = from_copy(vec![], |_| {});
        let vault = Vault::new(vec![bad, Ok(r.text.clone())]);
        config_gate::check(r.core.clone(), vault.clone(), None, Instant::now()).await;
        let State::Held(why) = r.core.config_gate.state() else {
            panic!("{says}: not held");
        };
        assert!(why.starts_with(says), "{why}");
        let h = r.core.health().config;
        assert_eq!((h.state.as_str(), h.reads), ("held", 1));
        assert!(h.detail.unwrap().starts_with(says));
        let rows = ledgered(&r.core, row);
        assert_eq!(rows.len(), 1, "{row}: said once");
        assert_eq!(rows[0]["reference"], REF);
        let lines: Vec<String> = r.core.narrator.tail().into_iter().map(|l| l.text).collect();
        assert!(
            lines
                .iter()
                .any(|l| l.contains(says) && l.contains("keeps serving the copy")),
            "{lines:?}"
        );
        assert_eq!(
            config_copy::read(&r.copy, REF).unwrap().unwrap().text,
            r.text
        );
        opens_a_session(&r.core).await;
        assert_eq!(vault.reads.load(Ordering::SeqCst), 1, "{row}: read once");
    }
}

// ---------------------------------------------------------------- the floor

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
        json!({"path": copy.to_string_lossy(), "content": "[policy]\nenforcement = \"open\"\n"});
    let fake = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t1", "fs_write", widen)]),
        Scripted::text("Waiting on you."),
    ]));
    let core = Core::build(Parts::for_tests(cfg, fake, store)).unwrap();
    let res = turn(&core, "widen the policy").await;
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

// ---------------------------------------------------------------- the spend limit (theseus-3pj)

/// "I raised the limit in the vault and restarted": the daemon starts from
/// the old copy, the vault's changed note restarts it onto the new one, and
/// the restarted start's kernel gives the session waiting at its old limit
/// the new limit, so it continues. Its question is withdrawn with the reason.
/// Neither the spend nor the lifetime cost went down.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn a_limit_raised_in_the_vault_lets_a_session_waiting_at_its_old_limit_continue() {
    use theseus_kernel::{micros_to_usd, ActionState, ExecState};
    let r = from_copy(
        vec![Scripted::tools(
            &"word ".repeat(30_000),
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        )],
        |c| c.kernel.spend_limit_usd = 1.40,
    );
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
    let (core, _) = start(vec![], &r.copy, r.store.clone(), None);
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state, e.budget.limit_micros),
        (ExecState::Waiting, 1_400_000)
    );
    config_gate::check(
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

    // The restarted daemon, from the rewritten copy: its kernel's startup
    // gives the waiting session the new limit.
    drop(core);
    let (core, fake) = start(
        vec![Scripted::text("The diff is one line.")],
        &r.copy,
        r.store.clone(),
        Some(restart),
    );
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
    let lines: Vec<String> = core.narrator.tail().into_iter().map(|l| l.text).collect();
    assert!(
        lines.iter().any(|l| l.contains("follows the config's spend limit: $1.40 before, $3 now; the call that waited at the old limit proceeds")),
        "{lines:?}"
    );
    assert_eq!(lifetime(&core), before, "a new limit lowers nothing");
    config_gate::check(
        core.clone(),
        Vault::new(vec![Ok(raised)]),
        None,
        Instant::now(),
    )
    .await;
    assert_eq!(core.config_gate.state(), State::Confirmed);

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
