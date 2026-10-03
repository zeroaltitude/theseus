//! Credential requests under L1 through the whole core (M4 18d; decision
//! 15): the posture rule, each posture's frames and rows, a waiting
//! request's card and its answer (a job's process refused), an input error,
//! a latched session, and the socket's life, served for an L1 job alone and
//! gone once the job settles or is cancelled.
//!
//! No job runs here: a test launcher keeps each job's arguments and leaves
//! it dispatched, as a job that runs on, and a request is made the way the
//! socket makes it (`Core::cred_request`). The daemon's own tests
//! (`theseusd/tests/cred.rs`) run real L1 jobs and the real helper.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::job::WrapperArgs;
use theseus_kernel::{Action, ActionState, Completion, Outcome};
use theseus_protocol::cred::{CredAnswer, CredAsk, CredKind};
use theseus_protocol::{ExternalText, SessionKind, TurnSubmitResult, CRED_TOOL};

use crate::bus::EventSink;
use crate::cred::{judge, Judged};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const GH: &str = "test-cred-gh-0000-planted";
const CRATES: &str = "test-cred-crates-0000-planted";
const OPEN: &str = "test-cred-open-0000-planted";
const PROVIDER: &str = "test-cred-provider-0000-planted";
/// The pid a parked job's launcher reports: no process has it.
const PARKED_PID: u32 = 4_290_000_000;

/// Keeps each job's arguments and leaves it running: nothing is spawned, and
/// no pid is spooled, so a cancel finds nothing to signal.
#[derive(Default)]
struct Parked(Mutex<Vec<WrapperArgs>>);

impl crate::toolrun::JobLauncher for Parked {
    fn launch(&self, _spool: &theseus_kernel::Spool, args: &WrapperArgs) -> anyhow::Result<u32> {
        self.0.lock().unwrap().push(args.clone());
        Ok(PARKED_PID)
    }

    fn exe(&self) -> Option<PathBuf> {
        Some("/usr/bin/true".into())
    }
}

struct Rig {
    core: Arc<Core>,
    parked: Arc<Parked>,
    dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 1;
    // L0 calls run, with a notice; L1's posture is its own (notify).
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    // gh's grant names github_token (notify, by default); crates_io_token
    // and open_token are their own entries; anthropic_api_key is the
    // provider's alone.
    cfg.broker = toml::from_str(
        r#"
        [programs.gh]
        env = { GH_TOKEN = "github_token" }
        [secrets.crates_io_token]
        posture = "approve"
        [secrets.open_token]
        posture = "open"
        "#,
    )
    .unwrap();
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let pairs = [
        ("github_token", GH),
        ("crates_io_token", CRATES),
        ("open_token", OPEN),
        ("anthropic_api_key", PROVIDER),
    ];
    let board =
        crate::secrets::SecretBoard::new(pairs.iter().map(|(n, _)| n.to_string()), Instant::now());
    board.publish(
        pairs
            .iter()
            .map(|(n, v)| {
                (
                    n.to_string(),
                    Ok(crate::secrets::Secret::new(v.to_string())),
                )
            })
            .collect(),
        "test",
    );
    let parked = Arc::new(Parked::default());
    let core = Core::build(crate::rpc::Parts {
        secrets: board.clone(),
        scrubber: Arc::new(crate::scrub::Scrubber::from_board(board)),
        launcher: parked.clone(),
        ..crate::rpc::Parts::for_tests(cfg, fake, store)
    })
    .unwrap();
    Rig { core, parked, dir }
}

impl Rig {
    async fn turn(&self, session: Option<SessionRecord>, input: &str) -> TurnSubmitResult {
        let rec = session.unwrap_or_else(|| SessionRecord::new(SessionKind::Conversation, None));
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), &rec.session_id, None);
        self.core
            .runner
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
                from_discord: false,
            })
            .await
            .unwrap()
    }

    /// The last job launched, and its arguments.
    fn job(&self) -> WrapperArgs {
        self.parked
            .0
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a job was launched")
    }

    async fn ask(&self, job: &str, name: &str) -> CredAnswer {
        let ask = CredAsk {
            kind: CredKind::Secret,
            name: name.into(),
        };
        self.core.cred_request(job, &ask).await
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(2000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    fn frames(&self) -> u64 {
        self.core.store.stats().unwrap().frames_appended
    }

    /// The job's credential requests, by their parent.
    fn requests(&self, job: &str) -> Vec<Action> {
        self.core
            .kernel
            .actions()
            .unwrap()
            .into_iter()
            .filter(|a| a.tool == CRED_TOOL && a.parent.as_deref() == Some(job))
            .collect()
    }

    /// Wait, bounded, for the job's one request to wait for the operator.
    async fn waiting(&self, job: &str) -> Action {
        let t0 = Instant::now();
        loop {
            if let Some(q) = self.requests(job).into_iter().find(Action::awaits_confirm) {
                return q;
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "no request waits");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Every file under the rig's directory that holds `needle`'s bytes.
    fn files_holding(&self, needle: &str) -> Vec<PathBuf> {
        fn walk(p: &Path, needle: &[u8], out: &mut Vec<PathBuf>) {
            let Ok(rd) = std::fs::read_dir(p) else { return };
            for e in rd.flatten() {
                let path = e.path();
                match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&path, needle, out),
                    Ok(t) if t.is_file() => {
                        let held = std::fs::read(&path)
                            .is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle));
                        if held {
                            out.push(path);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(self.dir.path(), needle.as_bytes(), &mut out);
        out
    }

    fn socket_dir(&self, job: &str) -> PathBuf {
        self.core.spool.dir().join("broker").join(job)
    }

    /// Wait, bounded, until the job's socket directory is gone.
    async fn socket_gone(&self, job: &str) {
        let t0 = Instant::now();
        while self.socket_dir(job).exists() {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "{} is still served",
                self.socket_dir(job).display()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

fn l1(argv: &[&str]) -> Scripted {
    Scripted::tools(
        "",
        &[("t1", "proc_run", json!({"argv": argv, "sandbox": true}))],
    )
}

/// A rig whose first turn starts a parked L1 job: the rig, and the job.
async fn with_l1_job(tweak: impl FnOnce(&mut Config)) -> (Rig, String, TurnSubmitResult) {
    let r = rig(
        vec![l1(&["cargo", "publish"]), Scripted::text("Started.")],
        tweak,
    );
    let res = r.turn(None, "publish in L1").await;
    assert_eq!(res.stop_reason, "no_tool_calls", "{res:?}");
    let job = r.job().correlation_id;
    (r, job, res)
}

/// Decision 15: the posture is the stricter of the one the job's call ran
/// at and the secret's own. An approve secret waits even under an open
/// call; a notify one is granted under an open call at notify, with a
/// notice; under a call that ran at approve (a latched session's), every
/// request waits. A name the broker may not hand out is an input error, and
/// so is the AWS seam's kind until it is built.
#[test]
fn decision_15_takes_the_stricter_of_the_call_and_the_secret() {
    let r = rig(vec![], |_| {});
    let b = &r.core.tools.broker;
    let ask = |name: &str| CredAsk {
        kind: CredKind::Secret,
        name: name.into(),
    };
    let ran = |p: Posture| format!("proc.run ran at {}", p.as_str());
    let approve_line = "[broker.secrets.crates_io_token] posture = approve".to_string();
    let notify_line = "the broker's posture for github_token, notify by default".to_string();
    for (ran_at, name, judged) in [
        (
            Posture::Open,
            "open_token",
            Judged::Grant(Posture::Open, ran(Posture::Open)),
        ),
        (
            Posture::Open,
            "github_token",
            Judged::Grant(Posture::Notify, notify_line),
        ),
        (
            Posture::Open,
            "crates_io_token",
            Judged::Ask(approve_line.clone()),
        ),
        (
            Posture::Notify,
            "open_token",
            Judged::Grant(Posture::Notify, ran(Posture::Notify)),
        ),
        (
            Posture::Notify,
            "github_token",
            Judged::Grant(Posture::Notify, ran(Posture::Notify)),
        ),
        (
            Posture::Notify,
            "crates_io_token",
            Judged::Ask(approve_line),
        ),
        (
            Posture::Approve,
            "open_token",
            Judged::Ask(ran(Posture::Approve)),
        ),
        (
            Posture::Approve,
            "github_token",
            Judged::Ask(ran(Posture::Approve)),
        ),
    ] {
        assert_eq!(
            judge(b, ran_at, &ask(name)),
            judged,
            "{name} under {ran_at:?}"
        );
    }
    let Judged::Refused(why) = judge(b, Posture::Open, &ask("anthropic_api_key")) else {
        panic!("a provider's key is no job's to ask for")
    };
    assert!(why.contains("not a secret a job may ask for"), "{why}");
    let aws = CredAsk {
        kind: CredKind::Aws,
        name: "github_token".into(),
    };
    let Judged::Refused(why) = judge(b, Posture::Open, &aws) else {
        panic!("the AWS seam is not built")
    };
    assert!(why.contains("not built yet"), "{why}");
}

/// Notify grants, with a notice, in one frame: the request's action (its
/// parent the job's call), `secret.requested` and `secret.granted { via:
/// request }`; the value goes to the asker, and no file under the state dir
/// holds it (B1's check).
#[tokio::test]
async fn a_notify_request_is_granted_in_one_frame_with_its_notice() {
    let (r, job, res) = with_l1_job(|_| {}).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    r.core.bus.watch(&res.session_id, "test", tx);
    let before = r.frames();
    let answer = r.ask(&job, "github_token").await;
    assert_eq!(answer.value.as_deref(), Some(GH), "{answer:?}");
    assert_eq!(
        r.frames() - before,
        1,
        "decided, recorded, and granted in one frame"
    );
    let reqs = r.requests(&job);
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].state, ActionState::Succeeded);
    assert_eq!(reqs[0].resolution.as_deref(), Some("granted at notify"));
    assert_eq!(reqs[0].execution_id, res.execution_id.clone().unwrap());
    let asked = r.rows("secret.requested");
    assert_eq!(
        (
            &asked[0]["secret"],
            &asked[0]["posture"],
            &asked[0]["outcome"]
        ),
        (&json!("github_token"), &json!("notify"), &json!("granted"))
    );
    assert_eq!(asked[0]["setting"], "proc.run ran at notify");
    assert_eq!(asked[0]["command"], "cargo publish");
    let granted: Vec<Value> = r
        .rows("secret.granted")
        .into_iter()
        .filter(|g| g["via"] == "request")
        .collect();
    assert_eq!(granted.len(), 1, "{granted:?}");
    assert_eq!(granted[0]["job"], json!(job));
    // The notice, to the session's clients.
    let mut notices = Vec::new();
    while let Ok(m) = rx.try_recv() {
        let m = serde_json::to_value(&m).unwrap();
        if m["method"] == "secret.requested" {
            notices.push(m["params"].clone());
        }
    }
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        (
            &notices[0]["outcome"],
            &notices[0]["posture"],
            &notices[0]["short"]
        ),
        (
            &json!("granted"),
            &json!("notify"),
            &json!(crate::task::short(&job))
        )
    );
    assert!(r.files_holding(GH).is_empty(), "{:?}", r.files_holding(GH));
    // Open is quiet, and as one frame: the job's call ran at notify, so an
    // open secret is given at notify (decision 15), with its notice too.
    let answer = r.ask(&job, "open_token").await;
    assert_eq!(answer.value.as_deref(), Some(OPEN));
}

/// Approve waits on a card (one frame: the request and its row; this
/// session posts nowhere, so the card is `theseus confirm`'s and the web
/// UI's), until the operator answers it through `action.confirm`. A job's
/// process cannot answer it (J1), and the request keeps waiting; the
/// operator's approval grants it; a decline returns an error to the asker.
#[tokio::test]
async fn an_approve_request_waits_on_its_card_and_its_answer_decides() {
    let (r, job, res) = with_l1_job(|_| {}).await;
    let core = r.core.clone();
    let j = job.clone();
    let before = r.frames();
    let asked = tokio::spawn(async move {
        let ask = CredAsk {
            kind: CredKind::Secret,
            name: "crates_io_token".into(),
        };
        core.cred_request(&j, &ask).await
    });
    let q = r.waiting(&job).await;
    assert_eq!(r.frames() - before, 1, "the request and its row, one frame");
    let listed = r.core.confirm_list().unwrap();
    let card = listed
        .iter()
        .find(|c| c.correlation_id == q.correlation_id)
        .expect("listed though no turn waits on it");
    assert_eq!(card.tool, CRED_TOOL);
    assert_eq!(
        card.reason,
        format!(
            "Job {} (`cargo publish`, L1) asks for `crates_io_token` ([broker.secrets.crates_io_token] posture = approve)",
            crate::task::short(&job)
        )
    );
    assert_eq!(
        card.expires_at_ms, q.deadline_at_ms,
        "it waits until the job's deadline"
    );
    assert_eq!(r.rows("secret.requested")[0]["outcome"], "waiting");

    // J1: a job's process is refused, and nothing changes.
    let standin = crate::peer::Standin::start("act_cardanswerer");
    let from_job = crate::approval::Answerer {
        label: "sock#7".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        peer: crate::peer::Peer::process(standin.child),
    };
    let refused = r
        .core
        .confirm_action(&q.correlation_id, true, None, from_job)
        .expect_err("refused")
        .to_string();
    assert!(
        refused.contains("from a Theseus job's process (job act_cardanswerer"),
        "{refused}"
    );
    assert!(r
        .core
        .kernel
        .action(&q.correlation_id)
        .unwrap()
        .unwrap()
        .awaits_confirm());

    // The operator's approval grants it.
    let answered = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(!answered.resumes, "no turn waits on a request");
    let answer = tokio::time::timeout(Duration::from_secs(10), asked)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(answer.value.as_deref(), Some(CRATES), "{answer:?}");
    let settled = r.core.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(settled.state, ActionState::Succeeded);
    let granted: Vec<Value> = r
        .rows("secret.granted")
        .into_iter()
        .filter(|g| g["via"] == "request")
        .collect();
    assert_eq!(granted[0]["by"], "approved by cli");

    // A decline is an error the helper prints.
    let core = r.core.clone();
    let j = job.clone();
    let asked = tokio::spawn(async move {
        let ask = CredAsk {
            kind: CredKind::Secret,
            name: "crates_io_token".into(),
        };
        core.cred_request(&j, &ask).await
    });
    let q = r.waiting(&job).await;
    r.core
        .confirm_action(&q.correlation_id, false, Some("not today"), "cli")
        .unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(10), asked)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(answer.value, None);
    assert_eq!(answer.error.as_deref(), Some("declined: not today"));
    let declined = r.rows("secret.declined");
    assert_eq!(
        (&declined[0]["by"], &declined[0]["why"]),
        (&json!("cli"), &json!("not today"))
    );
    assert_eq!(res.stop_reason, "no_tool_calls");
    assert!(r.files_holding(CRATES).is_empty());
}

/// A name the broker may not hand out is an input error: the asker hears
/// why; `secret.requested` and `secret.declined` say so; no action is made.
#[tokio::test]
async fn an_unknown_name_is_an_input_error_and_makes_no_action() {
    let (r, job, _) = with_l1_job(|_| {}).await;
    for name in ["anthropic_api_key", "no_such_secret"] {
        let answer = r.ask(&job, name).await;
        assert_eq!(answer.value, None);
        let why = answer.error.unwrap();
        assert!(why.contains("is not a secret a job may ask for"), "{why}");
    }
    assert!(r.requests(&job).is_empty());
    let declined = r.rows("secret.declined");
    assert_eq!(declined.len(), 2);
    assert_eq!(declined[0]["by"], "harness");
    assert_eq!(r.rows("secret.requested")[0]["outcome"], "declined");
    assert!(r.files_holding(PROVIDER).is_empty());
}

/// In a session that holds external text the L1 call waited, and the
/// operator approved it, so it ran at approve: by decision 15 its requests
/// wait too, a notify secret's included.
#[tokio::test]
async fn in_a_latched_session_a_request_waits() {
    let r = rig(
        vec![l1(&["cargo", "publish"]), Scripted::text("Started.")],
        |_| {},
    );
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.external = Some(ExternalText {
        since_ms: 1,
        tool: "http.fetch".into(),
        url: "page".into(),
        node_id: "nod_page".into(),
        from_session: None,
        via: None,
        query: None,
    });
    let res = r.turn(Some(rec), "publish in L1").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let call = r.core.pending_confirms(&res.session_id).unwrap()[0]
        .correlation_id
        .clone();
    r.core.confirm_action(&call, true, None, "cli").unwrap();
    let exec = res.execution_id.clone().unwrap();
    r.core.continue_execution(&exec).await.unwrap();
    let job = r.job().correlation_id;
    assert_eq!(job, call);
    let core = r.core.clone();
    let j = job.clone();
    let asked = tokio::spawn(async move {
        let ask = CredAsk {
            kind: CredKind::Secret,
            name: "github_token".into(),
        };
        core.cred_request(&j, &ask).await
    });
    let q = r.waiting(&job).await;
    let args = &q.proposal.as_ref().unwrap().args;
    assert_eq!(
        (&args["posture"], &args["setting"]),
        (&json!("approve"), &json!("proc.run ran at approve"))
    );
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
    let answer = asked.await.unwrap();
    assert_eq!(
        answer.error.as_deref(),
        Some("declined: the operator declined")
    );
}

/// The socket is an L1 job's alone: an L0 job gets none. An L1 job's view
/// binds its directory at `/run/theseus/broker` and the helper at
/// `/run/theseus/bin/theseus-cred`, on its PATH. It is served until the
/// job's call settles, then removed; a cancelled job's too, and what its
/// helper waited for is closed.
#[tokio::test]
async fn the_socket_is_an_l1_jobs_alone_and_goes_once_the_job_settles_or_is_cancelled() {
    let r = rig(
        vec![
            Scripted::tools("", &[("t0", "proc_run", json!({"argv": ["true"]}))]),
            Scripted::text("Ran."),
            l1(&["cargo", "publish"]),
            Scripted::text("Started."),
            l1(&["cargo", "build"]),
            Scripted::text("Started."),
        ],
        |_| {},
    );
    r.turn(None, "at L0").await;
    let l0 = r.job();
    assert!(l0.sandbox.is_none());
    assert!(
        !r.socket_dir(&l0.correlation_id).exists(),
        "an L0 job has no socket"
    );
    assert!(r.core.tools.creds.served(&l0.correlation_id).is_none());

    let res = r.turn(None, "in L1").await;
    let a = r.job();
    let job = a.correlation_id.clone();
    let dir = r.socket_dir(&job);
    assert!(
        dir.join(crate::cred::SOCKET).exists(),
        "served before the launch"
    );
    let view = a.sandbox.clone().unwrap();
    assert_eq!(
        view.binds,
        [
            (dir.clone(), PathBuf::from("/run/theseus/broker")),
            (
                PathBuf::from("/usr/bin/true"),
                PathBuf::from("/run/theseus/bin/theseus-cred")
            ),
        ]
    );
    let path = a.env.iter().find(|(k, _)| k == "PATH").unwrap();
    assert!(path.1.ends_with(":/run/theseus/bin"), "{}", path.1);
    // The job settles: its completion is drained, and its socket goes.
    let now = theseus_protocol::now_unix_ms();
    r.core
        .spool
        .write(&Completion {
            correlation_id: job.clone(),
            outcome: Outcome::Succeeded,
            result_ref: None,
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: "wrapper".into(),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"exit_code": 0})),
        })
        .unwrap();
    r.core.drain_spool();
    r.socket_gone(&job).await;
    assert!(r.core.tools.creds.served(&job).is_none());
    assert_eq!(res.stop_reason, "no_tool_calls");

    // A cancelled job's socket goes too, and its waiting request is closed.
    let res = r.turn(None, "in L1 again").await;
    let job = r.job().correlation_id;
    assert!(r.socket_dir(&job).join(crate::cred::SOCKET).exists());
    let core = r.core.clone();
    let j = job.clone();
    let asked = tokio::spawn(async move {
        let ask = CredAsk {
            kind: CredKind::Secret,
            name: "crates_io_token".into(),
        };
        core.cred_request(&j, &ask).await
    });
    r.waiting(&job).await;
    let exec = res.execution_id.clone().unwrap();
    r.core.cancel_execution(&exec, "operator").await.unwrap();
    r.socket_gone(&job).await;
    let answer = tokio::time::timeout(Duration::from_secs(10), asked)
        .await
        .unwrap()
        .unwrap();
    assert!(answer.value.is_none(), "{answer:?}");
    assert!(r.requests(&job).iter().all(|q| q.state.is_settled()));
}
