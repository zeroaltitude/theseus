//! `proc.run`'s `steps` (theseus-7gir.3) through the whole core: the batch
//! runs its steps in turn and stops at the first that fails, with one
//! result; the gate judges each step as the call it would be alone and the
//! batch takes the strictest; one approval runs every step; a step past the
//! wait goes to the background and the rest are not run. Jobs run in
//! process (`InlineLauncher`), a real wrapper on a thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig_on(script, tweak, Arc::new(crate::toolrun::InlineLauncher))
}

/// `rig`, its jobs launched by `launcher`.
fn rig_on(
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
    launcher: Arc<dyn crate::toolrun::JobLauncher>,
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = crate::rpc::Parts::for_tests(cfg, fake, store);
    p.launcher = launcher;
    let core = Core::build(p).unwrap();
    Rig {
        core,
        root,
        _dir: dir,
    }
}

/// Keeps the class each job launched in, and runs the job in process
/// without its L1 view, as `tests_sandbox`'s launcher does: the view is the
/// daemon's sandbox, and these are tests of the gate.
#[derive(Default)]
struct Classes(std::sync::Mutex<Vec<&'static str>>);

impl crate::toolrun::JobLauncher for Classes {
    fn launch(
        &self,
        spool: &theseus_kernel::Spool,
        args: &theseus_kernel::job::WrapperArgs,
        done: crate::toolrun::JobDone,
    ) -> anyhow::Result<u32> {
        let class = if args.sandbox.is_some() { "l1" } else { "l0" };
        self.0.lock().unwrap().push(class);
        let mut a = args.clone();
        a.sandbox = None;
        crate::toolrun::InlineLauncher.launch(spool, &a, done)
    }
}

fn run(input: Value) -> Scripted {
    Scripted::tools("", &[("t1", "proc_run", input)])
}

impl Rig {
    async fn turn(&self, input: &str) -> TurnSubmitResult {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
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
            .unwrap()
    }

    fn results(&self, sid: &str) -> Vec<(ResultStatus, String, Value)> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    status,
                    content,
                    meta,
                    ..
                } => Some((*status, content.clone(), meta.clone())),
                _ => None,
            })
            .collect()
    }

    /// The call's gate: its posture, whether the floor asked, and its reason.
    fn gate(&self, sid: &str) -> (String, bool, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    let d = g.decision.clone().unwrap_or_default();
                    Some((d.posture.unwrap_or_default(), d.floor, d.reason))
                }
                _ => None,
            })
            .expect("a tool call with its gate")
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

    /// The file the batches' last step would make.
    fn marker(&self) -> PathBuf {
        self.root.join("never")
    }
}

/// The brief's batch: `printf one`, `false`, `touch never`. The second exits
/// 1, so the third never starts (its marker stays absent), and the one
/// result names each step: the first's exit and output, the second's exit,
/// the third as not run.
#[tokio::test]
async fn a_failed_step_stops_the_batch_and_its_one_result_names_each_step() {
    let batch = json!({"steps": [
        {"argv": ["printf", "one"]},
        {"argv": ["false"]},
        {"argv": ["touch", "never"]},
    ]});
    let r = rig(vec![run(batch), Scripted::text("Done.")], |_| {});
    let marker = r.marker();
    let res = r.turn("run the batch").await;
    assert_eq!(res.tool_calls, 1);
    let results = r.results(&res.session_id);
    assert_eq!(results.len(), 1, "one result: {results:?}");
    let (status, text, meta) = &results[0];
    assert_eq!(*status, ResultStatus::Error, "{text}");
    assert!(!marker.exists(), "the third step ran");
    assert!(text.contains("[step 1 of 3: `printf one`"), "{text}");
    assert!(text.contains("[exit code 0, "), "{text}");
    assert!(text.contains("\none\n"), "{text}");
    assert!(text.contains("[step 2 of 3: `false`"), "{text}");
    assert!(text.contains("[exit code 1 · in "), "{text}");
    assert!(text.contains("[step 3 of 3: `touch "), "{text}");
    assert!(text.trim_end().ends_with(": not run]"), "{text}");
    let steps = meta["steps"].as_array().expect("each step's row");
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["exit_code"], 0);
    assert_eq!(steps[1]["ran"], true);
    assert_eq!(steps[2]["ran"], false);
    // Two jobs started, under the call's one correlation id.
    let started = r.rows("tool.job_started");
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(started[0]["correlation_id"], started[1]["correlation_id"]);
    // The call's one action, settled once, as failed.
    let corr = started[0]["correlation_id"].as_str().unwrap();
    let a = r.core.kernel.action(corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Failed);
}

/// Every step that exits 0 runs, and the batch succeeds with each step's
/// output in its block.
#[tokio::test]
async fn a_batch_whose_steps_all_pass_runs_every_one() {
    let r = rig(
        vec![
            run(json!({"steps": [
                {"argv": ["printf", "one"]},
                {"argv": ["printf", "two"]},
                {"argv": ["touch", "made"]},
            ]})),
            Scripted::text("Done."),
        ],
        |_| {},
    );
    let res = r.turn("run them").await;
    let results = r.results(&res.session_id);
    let (status, text, _) = &results[0];
    assert_eq!(*status, ResultStatus::Ok, "{text}");
    assert!(r.root.join("made").exists(), "the last step ran");
    assert!(text.contains("one") && text.contains("two"), "{text}");
    assert!(!text.contains("not run"), "{text}");
    assert_eq!(r.rows("tool.job_started").len(), 3);
}

/// Allow-listed steps run open; a batch with one step that is not runs at
/// the tool's posture, its reason naming that step.
#[tokio::test]
async fn a_batch_takes_its_strictest_steps_posture() {
    let listed = json!({"steps": [{"argv": ["true"]}, {"argv": ["printf", "x"]}]});
    let mixed = json!({"steps": [{"argv": ["true"]}, {"argv": ["ls"]}]});
    for (batch, want) in [(listed, "open"), (mixed, "notify")] {
        let r = rig(vec![run(batch), Scripted::text("Done.")], |c| {
            c.policy.allow_argv = vec![vec!["true".into()], vec!["printf".into()]];
        });
        let res = r.turn("run").await;
        let (posture, floor, reason) = r.gate(&res.session_id);
        assert_eq!(posture, want, "{reason}");
        assert!(!floor);
        if want == "notify" {
            assert!(reason.starts_with("step 2 of 2 (`ls`): "), "{reason}");
        }
        assert_eq!(r.results(&res.session_id)[0].0, ResultStatus::Ok);
    }
}

/// A step on the floor makes the whole batch wait as the floor does, its
/// reason naming the step; nothing runs before the answer.
#[tokio::test]
async fn a_step_on_the_floor_makes_the_batch_wait_as_the_floor() {
    let r = rig(
        vec![
            run(json!({"steps": [{"argv": ["touch", "first"]}, {"argv": ["op", "whoami"]}]})),
            Scripted::text("Done."),
        ],
        |c| c.policy.enforcement = Posture::Open,
    );
    let res = r.turn("run").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let (posture, floor, reason) = r.gate(&res.session_id);
    assert_eq!(posture, "approve");
    assert!(floor, "{reason}");
    assert!(
        reason.starts_with("step 2 of 2 (`op whoami`): "),
        "{reason}"
    );
    assert!(
        !r.root.join("first").exists(),
        "a step ran before the answer"
    );
}

/// The floor outranks an `approve_argv` entry at the same posture
/// (theseus-grms): a batch whose first step is on the approve list and whose
/// second is on the floor waits as the floor does, naming step 2, though
/// both ask at `approve`; a tie keeps the earlier step, so posture alone
/// would have named step 1.
#[tokio::test]
async fn a_floor_step_outranks_an_earlier_step_on_the_approve_list() {
    let r = rig(
        vec![
            run(json!({"steps": [{"argv": ["touch", "first"]}, {"argv": ["op", "whoami"]}]})),
            Scripted::text("Done."),
        ],
        |c| {
            c.policy.enforcement = Posture::Open;
            c.policy.approve_argv = vec![vec!["touch".into()]];
        },
    );
    let res = r.turn("run").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let (posture, floor, reason) = r.gate(&res.session_id);
    assert_eq!(posture, "approve");
    assert!(floor, "the floor step sets the batch's gate: {reason}");
    assert!(
        reason.starts_with("step 2 of 2 (`op whoami`): "),
        "{reason}"
    );
    assert!(
        !r.root.join("first").exists(),
        "a step ran before the answer"
    );
}

/// One approval runs every step; a batch that differs in one step is a new
/// proposal, and asks again.
#[tokio::test]
async fn one_approval_runs_every_step_and_a_changed_batch_asks_again() {
    let batch =
        |last: &str| json!({"steps": [{"argv": ["touch", "a"]}, {"argv": ["touch", last]}]});
    let r = rig(
        vec![
            run(batch("b")),
            Scripted::text("Ran."),
            run(batch("c")),
            Scripted::text("Ran."),
        ],
        |c| c.policy.enforcement = Posture::Approve,
    );
    let res = r.turn("run").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending.len(), 1, "one card for the batch");
    let asked = r.rows("tool.confirm_requested");
    let card = asked[0].to_string();
    assert!(
        card.contains(r#"["touch","a"]"#) && card.contains(r#"["touch","b"]"#),
        "{card}"
    );
    let corr = pending[0].correlation_id.clone();
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let exec = res.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Ran.");
    assert!(r.root.join("a").exists() && r.root.join("b").exists());
    assert_eq!(r.rows("tool.job_started").len(), 2);
    // The same session's next batch differs in its last step: it asks again.
    let res2 = r.turn("run another").await;
    assert_eq!(res2.stop_reason, "awaiting_confirm", "{res2:?}");
    assert!(!r.root.join("c").exists());
}

/// A step still running at the batch's wait goes on in the background: the
/// answer lists the steps done, that step's background id, and the rest as
/// not run, which never start.
#[tokio::test]
async fn a_step_past_the_wait_goes_to_the_background_and_the_rest_are_not_run() {
    let r = rig(
        vec![
            run(json!({"steps": [
                {"argv": ["printf", "one"]},
                {"argv": ["sleep", "3"]},
                {"argv": ["touch", "never"]},
            ]})),
            Scripted::text("Waiting."),
        ],
        |c| c.tools.proc_sync_secs = 1,
    );
    let marker = r.marker();
    let res = r.turn("run").await;
    let results = r.results(&res.session_id);
    let (status, text, _) = &results[0];
    assert_eq!(*status, ResultStatus::Background, "{text}");
    assert!(text.contains("[step 1 of 3: `printf one`"), "{text}");
    assert!(text.contains("[step 2 of 3: `sleep 3`"), "{text}");
    assert!(
        text.contains("Still running as background job act_"),
        "{text}"
    );
    assert!(text.contains("[step 3 of 3: `touch "), "{text}");
    assert!(text.trim_end().ends_with(": not run]"), "{text}");
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!marker.exists(), "a step after the background one ran");
}

/// A `/stop` during the second step starts no third.
#[tokio::test]
async fn a_stop_during_a_step_starts_no_more() {
    let r = Arc::new(rig(
        vec![
            run(json!({"steps": [
                {"argv": ["touch", "first"]},
                {"argv": ["sleep", "2"]},
                {"argv": ["touch", "never"]},
            ]})),
            Scripted::text("Stopped."),
        ],
        |_| {},
    ));
    let marker = r.marker();
    let turn = {
        let r = r.clone();
        tokio::spawn(async move { r.turn("run").await })
    };
    // The first step has run: the second is starting or running. Stop the
    // execution then.
    while !r.root.join("first").exists() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let open = r.core.kernel.open_executions().unwrap();
    let exec = open
        .iter()
        .find(|e| e.kind == theseus_kernel::types::SessionKind::Conversation)
        .expect("the batch's execution")
        .id
        .clone();
    r.core.stop_execution(&exec, "test").await.unwrap();
    let res = turn.await.unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!marker.exists(), "a step after the stop ran");
    assert_eq!(r.rows("tool.job_started").len(), 2);
    let results = r.results(&res.session_id);
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].0, ResultStatus::Cancelled, "{results:?}");
    assert!(
        results[0].1.contains("[step 1 of 3: `touch first`"),
        "{results:?}"
    );
}

/// A batch runs in one class (theseus-nrvq, option 1). An `l1_argv` step
/// and an `approve_argv` step differ, so the batch is invalid input: no
/// card, nothing run, and the result names each step's class. Before, the
/// approve step's L0 bound both, and a yes ran the L1 program at L0. A batch
/// whose steps share L1 runs as before, every step in L1.
#[tokio::test]
async fn a_batch_whose_steps_differ_in_class_is_refused_and_one_class_runs() {
    let mixed = json!({"steps": [{"argv": ["true"]}, {"argv": ["printf", "two"]}]});
    let l1 = json!({"steps": [{"argv": ["true"]}, {"argv": ["true", "again"]}]});
    let classes = Arc::new(Classes::default());
    let r = rig_on(
        vec![
            run(mixed),
            Scripted::text("Split."),
            run(l1),
            Scripted::text("Done."),
        ],
        |c| {
            c.sandbox.l1_argv = vec![vec!["true".into()]];
            c.policy.approve_argv = vec![vec!["printf".into(), "two".into()]];
        },
        classes.clone(),
    );
    let res = r.turn("run the mixed batch").await;
    assert_eq!(res.stop_reason, "no_tool_calls", "{res:?}");
    let results = r.results(&res.session_id);
    assert_eq!(results.len(), 1, "{results:?}");
    let (status, text, _) = &results[0];
    assert_eq!(*status, ResultStatus::Error, "{text}");
    assert_eq!(
        text,
        "Invalid input: step 1 of 2 (`true`) runs in L1 ([sandbox] l1_argv names `true`) and \
         step 2 of 2 (`printf two`) at L0: a batch runs in one class; split it"
    );
    assert!(r.core.pending_confirms(&res.session_id).unwrap().is_empty());
    assert!(r.rows("tool.confirm_requested").is_empty());
    assert!(r.rows("tool.job_started").is_empty(), "a step ran");
    assert!(classes.0.lock().unwrap().is_empty(), "a step ran");

    let res = r.turn("run the L1 batch").await;
    let results = r.results(&res.session_id);
    assert_eq!(results[0].0, ResultStatus::Ok, "{results:?}");
    assert_eq!(*classes.0.lock().unwrap(), ["l1", "l1"]);
}

/// The kernel's deadline covers every step's timeout.
#[test]
fn a_batchs_deadline_covers_every_steps_timeout() {
    let r = rig(vec![], |_| {});
    let tool = r.core.tools.registry.get("proc.run").unwrap().clone();
    let one = r
        .core
        .tools
        .deadline_ms(tool.as_ref(), &json!({"argv": ["true"], "timeout_secs": 7}));
    let batch = json!({"timeout_secs": 7, "steps": [{"argv": ["true"]}, {"argv": ["true"], "timeout_secs": 20}]});
    let both = r.core.tools.deadline_ms(tool.as_ref(), &batch);
    assert_eq!(one, (7 + 30) * 1000);
    assert_eq!(both, (7 + 20 + 30) * 1000);
}
