//! L1 for `proc.run` through the whole core (M4 17b): the class each call
//! is given, L1's posture against L0's, and the class a confirm binds. A
//! test launcher keeps each job's `WrapperArgs` and runs the job at once in
//! process with its L1 view taken off: these tests are of the gate and the
//! dispatch, and the daemon's own tests (`theseusd/tests/sandbox.rs`) run
//! real L1 jobs.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_kernel::job::WrapperArgs;
use theseus_protocol::{ExternalText, SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// Keeps every job's arguments, and runs `true` in their place, in process:
/// what a job does is not these tests' subject, and `sudo` would ask.
#[derive(Default)]
struct Kept(Mutex<Vec<WrapperArgs>>);

impl crate::toolrun::JobLauncher for Kept {
    fn launch(&self, spool: &theseus_kernel::Spool, args: &WrapperArgs) -> anyhow::Result<u32> {
        self.0.lock().unwrap().push(args.clone());
        let mut a = args.clone();
        a.sandbox = None;
        a.argv = vec!["true".into()];
        crate::toolrun::InlineLauncher.launch(spool, &a)
    }
}

struct Rig {
    core: Arc<Core>,
    kept: Arc<Kept>,
    root: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// The template as a deployment that asks before every call that acts,
    /// with `sudo` on the approve list.
    fn new(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config, &Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
        cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        cfg.tools.proc_sync_secs = 10;
        cfg.policy.enforcement = Posture::Approve;
        cfg.policy.tools.remove("proc.run");
        cfg.policy.approve_argv = vec![vec!["sudo".into()]];
        tweak(&mut cfg, &root);
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(script));
        let mut p = crate::rpc::Parts::for_tests(cfg, fake, store);
        let kept = Arc::new(Kept::default());
        p.launcher = kept.clone();
        Self {
            core: Core::build(p).unwrap(),
            kept,
            root,
            _dir: dir,
        }
    }

    /// The class of each job launched so far: `l1` when it had a view.
    fn launched(&self) -> Vec<&'static str> {
        self.kept
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|a| if a.sandbox.is_some() { "l1" } else { "l0" })
            .collect()
    }

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

    /// Each tool call's gate record in the session: its posture, its class,
    /// and the class its proposal names.
    fn gates(&self, sid: &str) -> Vec<(String, Option<String>, Option<String>)> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    let d = g.decision.clone().unwrap_or_default();
                    let bound = g.proposal.policy_context["class"]
                        .as_str()
                        .map(str::to_string);
                    Some((d.posture.unwrap_or_default(), d.class, bound))
                }
                _ => None,
            })
            .collect()
    }

    fn results(&self, sid: &str) -> Vec<(ResultStatus, String)> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    status, content, ..
                } => Some((*status, content.clone())),
                _ => None,
            })
            .collect()
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
}

fn run(input: Value) -> Scripted {
    Scripted::tools("", &[("t1", "proc_run", input)])
}

/// Decision 1 (Eddie, 2026-10-02): an L1 job runs at notify, though the
/// approve list, a path outside the roots, and the deployment's own posture
/// would each make it wait at L0; and L0 keeps its postures. Before 17b the
/// `sandbox` input was refused as unknown, and every call here waited.
#[tokio::test]
async fn l1_runs_at_notify_where_l0_waits() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["sudo", "true"], "sandbox": true})),
            Scripted::text("Done."),
            run(json!({"argv": ["true"], "cwd": "/", "sandbox": true})),
            Scripted::text("Done."),
            run(json!({"argv": ["sudo", "true"]})),
        ],
        |_, _| {},
    );
    let listed = r.turn(None, "sudo in L1").await;
    assert_eq!(listed.stop_reason, "no_tool_calls", "{listed:?}");
    let outside = r.turn(None, "outside the roots in L1").await;
    assert_eq!(outside.stop_reason, "no_tool_calls", "{outside:?}");
    let l0 = r.turn(None, "sudo at L0").await;
    assert_eq!(l0.stop_reason, "awaiting_confirm", "{l0:?}");
    // Both L1 calls ran, with their views; the L0 one waits, unlaunched.
    assert_eq!(r.launched(), ["l1", "l1"]);
    for res in [&listed, &outside] {
        let gates = r.gates(&res.session_id);
        assert_eq!(
            gates,
            [("notify".into(), Some("l1".into()), Some("l1".into()))],
            "{gates:?}"
        );
        assert_eq!(r.results(&res.session_id)[0].0, ResultStatus::Ok);
    }
    assert_eq!(
        r.gates(&l0.session_id),
        [("approve".into(), None, None)],
        "L0 keeps its posture, and its proposal names no class"
    );
    // Each L1 call's notice says what chose L1, and its job's rows say L1.
    let notified = r.rows("tool.notified");
    assert_eq!(notified.len(), 2, "{notified:?}");
    assert!(
        notified[0]["setting"]
            .as_str()
            .unwrap()
            .contains("sandbox: true"),
        "{notified:?}"
    );
    let started = r.rows("sandbox.started");
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(started[0]["class"], "l1");
    assert_eq!(started[0]["limits"]["pids"], 512);
    let jobs = r.rows("tool.job_started");
    assert!(jobs.iter().all(|j| j["class"] == "l1"), "{jobs:?}");
    // The floor and the approve list are hidden in every view, and the
    // workspace is the job's, read-only under scratch.
    let view = r.kept.0.lock().unwrap()[0].sandbox.clone().unwrap();
    assert_eq!(view.workspace, std::slice::from_ref(&r.root));
    assert!(
        view.hidden.iter().any(|p| p.ends_with("store")),
        "{:?}",
        view.hidden
    );
}

/// The operator's own word about `proc.run` reaches L1 (Eddie, 2026-10-02,
/// theseus-jfs6): a `[policy.tools]` line that asks, or a "should have
/// asked" tightening, makes an L1 call wait as it would at L0, so the model
/// cannot step around it with `sandbox: true`. The call still names L1, so
/// it runs there once approved. The inherited `[policy].enforcement` is not
/// that word (the rig's `approve` never makes L1 wait, as the test above
/// shows), and a looser line never makes L1 quieter than notify (the test
/// below).
#[tokio::test]
async fn the_operators_own_word_about_proc_run_reaches_l1() {
    let reasons = |r: &Rig, sid: &str| -> Vec<String> {
        r.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    g.decision.as_ref().map(|d| d.reason.clone())
                }
                _ => None,
            })
            .collect()
    };
    let line = Rig::new(
        vec![run(json!({"argv": ["true"], "sandbox": true}))],
        |cfg, _| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
        },
    );
    let res = line.turn(None, "L1 under an explicit approve").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    assert_eq!(
        line.gates(&res.session_id),
        [("approve".into(), Some("l1".into()), Some("l1".into()))]
    );
    let why = reasons(&line, &res.session_id).remove(0);
    assert!(
        why.contains("L1: the call asked for it")
            && why.contains("[policy.tools] \"proc.run\" = approve"),
        "{why}"
    );
    assert!(
        line.launched().is_empty(),
        "nothing runs before the approval"
    );

    let pressed = Rig::new(
        vec![run(json!({"argv": ["true"], "sandbox": true}))],
        |cfg, _| cfg.policy.enforcement = Posture::Notify,
    );
    pressed.core.tighten("proc.run", None, "test").unwrap();
    let res = pressed.turn(None, "L1 after should have asked").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let (posture, class, bound) = pressed.gates(&res.session_id).remove(0);
    assert_eq!(
        (posture.as_str(), class.as_deref(), bound.as_deref()),
        ("approve", Some("l1"), Some("l1"))
    );
    let why = reasons(&pressed, &res.session_id).remove(0);
    assert!(why.contains("tightened by"), "{why}");
    assert!(pressed.launched().is_empty());
}

/// Decision 2: `l1_argv` routes a call to L1, and `sandbox: false` undoes
/// neither it nor `default = "l1"`.
#[tokio::test]
async fn l1_argv_and_the_default_route_to_l1_whatever_the_call_says() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true", "x"], "sandbox": false})),
            Scripted::text("Done."),
            run(json!({"argv": ["ls"], "sandbox": false})),
        ],
        |cfg, _| {
            cfg.sandbox.l1_argv = vec![vec!["true".into()]];
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
        },
    );
    let listed = r.turn(None, "listed").await;
    assert_eq!(listed.stop_reason, "no_tool_calls");
    let l0 = r.turn(None, "not listed").await;
    assert_eq!(l0.stop_reason, "no_tool_calls");
    assert_eq!(r.launched(), ["l1", "l0"]);
    let (posture, class, _) = r.gates(&listed.session_id).remove(0);
    assert_eq!((posture.as_str(), class.as_deref()), ("notify", Some("l1")));
    assert_eq!(r.gates(&l0.session_id)[0].0, "open", "L0 keeps open");

    let all = Rig::new(
        vec![run(json!({"argv": ["ls"], "sandbox": false}))],
        |cfg, _| cfg.sandbox.default = crate::sandbox::Class::L1,
    );
    all.turn(None, "every job in L1").await;
    assert_eq!(all.launched(), ["l1"]);
}

/// The class is bound (design §2.2): a session that holds external text
/// still holds an L1 call (decision 4: 20a lifts it), so it waits; its
/// proposal names L1, so a proposal without the class (an L0 dispatch of
/// it) is refused by the digest, and once approved it runs in L1, the class
/// its proposal names.
#[tokio::test]
async fn an_approved_l1_call_runs_in_l1_and_never_as_l0() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Ran."),
        ],
        |_, _| {},
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
    let res = r.turn(Some(rec), "run it in L1").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    assert_eq!(
        r.gates(&res.session_id),
        [("approve".into(), Some("l1".into()), Some("l1".into()))]
    );
    assert!(r.launched().is_empty());
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    let corr = pending[0].correlation_id.clone();
    // The same call with its class taken off is another proposal.
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let mut l0 = a.proposal.clone().unwrap();
    l0.policy_context
        .as_object_mut()
        .unwrap()
        .remove("class")
        .expect("the proposal names its class");
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let refused = r
        .core
        .kernel
        .authorize(&corr, &l0, Some(crate::turn::OPERATOR))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("digest"), "{refused}");
    // Approved, it runs in L1.
    let exec = res.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Ran.");
    assert_eq!(r.launched(), ["l1"]);
    assert_eq!(r.results(&res.session_id)[0].0, ResultStatus::Ok);
}

/// An L1 job's result says where it ran and what it wrote to scratch, and
/// one that could not start says why and that it never ran at L0.
#[test]
fn an_l1_results_head_says_where_it_ran() {
    let ran = json!({"sandbox": {"class": "l1", "pids_refused": 3},
        "scratch": {"summary": "wrote 1 file, 1 KB, to scratch: out.txt; discarded"}});
    let lines = crate::sandbox::result_lines(&ran);
    assert!(
        lines.starts_with(
            "[ran in L1, the sandbox: no network, no secret at its start; wrote 1 file"
        ),
        "{lines}"
    );
    assert!(lines.contains("3 of its forks were refused"), "{lines}");
    let failed = json!({"sandbox": {"class": "l1",
        "error": {"stage": "entering the cwd", "error": "No such file or directory"}}});
    let lines = crate::sandbox::result_lines(&failed);
    assert!(lines.contains("could not start in L1"), "{lines}");
    assert!(lines.contains("It did not run, in L1 or at L0"), "{lines}");
    assert_eq!(crate::sandbox::result_lines(&json!({"exit_code": 0})), "");
}
