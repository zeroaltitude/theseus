//! An L1 job takes its grants at launch, as an L0 job does (theseus-w5op;
//! decision 15), through the whole core: the broker's grant to the job's
//! program by its argv, the gate's stricter of the call's posture and the
//! secret's, so any approval comes before the launch, the value in the job's
//! environment with its wrapper told to withhold it from the job's output,
//! and no harness-only key handed out. A test launcher keeps each job's
//! arguments and runs `true` in their place, in process; the daemon's own
//! test (`theseusd/tests/sandbox.rs`) runs a real L1 job given its grant.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use theseus_kernel::job::WrapperArgs;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const GH: &str = "test-grant-gh-0000-planted";
const PROVIDER: &str = "test-grant-provider-0000-planted";

/// Keeps every job's arguments, and runs `true` in their place, in process,
/// with the environment it was given.
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
    dir: tempfile::TempDir,
}

/// A deployment whose `true` is granted github_token as `GH_TOKEN`, at
/// `posture`, with the provider's key on the board beside it. L0 calls run
/// with a notice; L1's posture is its own.
fn rig(script: Vec<Scripted>, posture: Posture, tweak: impl FnOnce(&mut Config)) -> Rig {
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
    cfg.broker = toml::from_str(&format!(
        r#"
        [programs.true]
        env = {{ GH_TOKEN = "github_token" }}
        [secrets.github_token]
        posture = "{}"
        "#,
        posture.as_str()
    ))
    .unwrap();
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let pairs = [("github_token", GH), ("anthropic_api_key", PROVIDER)];
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
    let kept = Arc::new(Kept::default());
    let core = Core::build(crate::rpc::Parts {
        secrets: board.clone(),
        scrubber: Arc::new(crate::scrub::Scrubber::from_board(board)),
        launcher: kept.clone(),
        ..crate::rpc::Parts::for_tests(cfg, fake, store)
    })
    .unwrap();
    Rig { core, kept, dir }
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

    fn launched(&self) -> Vec<WrapperArgs> {
        self.kept.0.lock().unwrap().clone()
    }

    /// The session's one tool call's gate: its posture, its class, what it
    /// is given, and its reason.
    fn gate(&self, sid: &str) -> (String, Option<String>, Option<String>, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    let d = g.decision.clone().unwrap_or_default();
                    Some((d.posture.unwrap_or_default(), d.class, d.granted, d.reason))
                }
                _ => None,
            })
            .expect("a tool call with its gate")
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
}

fn run(input: Value) -> Scripted {
    Scripted::tools("", &[("t1", "proc_run", input)])
}

/// The value a job was given in `var`, if any.
fn env_of<'a>(a: &'a WrapperArgs, var: &str) -> Option<&'a str> {
    a.env
        .iter()
        .find(|(k, _)| k == var)
        .map(|(_, v)| v.as_str())
}

/// The brief's test: an L1 job is given its program's grant at its launch,
/// as an L0 job is, where 17b withheld it and 18d made the job ask at run
/// time. Its environment holds the value, its wrapper is told to withhold
/// the value from its output, the gate and the job's rows name the grant,
/// and `secret.granted` records it; nothing is withheld. The provider's key,
/// on the same board, reaches neither job, and no file holds a value.
#[tokio::test]
async fn an_l1_job_takes_its_programs_grant_at_launch_as_l0_does() {
    let r = rig(
        vec![
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Done."),
            run(json!({"argv": ["true"]})),
            Scripted::text("Done."),
        ],
        Posture::Notify,
        |_| {},
    );
    let l1 = r.turn("true in L1").await;
    assert_eq!(l1.stop_reason, "no_tool_calls", "{l1:?}");
    let (posture, class, granted, _) = r.gate(&l1.session_id);
    assert_eq!(
        (posture.as_str(), class.as_deref(), granted.as_deref()),
        ("notify", Some("l1"), Some("true got GH_TOKEN"))
    );
    let l0 = r.turn("true at L0").await;
    assert_eq!(l0.stop_reason, "no_tool_calls", "{l0:?}");
    let jobs = r.launched();
    assert_eq!(jobs.len(), 2);
    assert!(jobs[0].sandbox.is_some() && jobs[1].sandbox.is_none());
    for job in &jobs {
        assert_eq!(env_of(job, "GH_TOKEN"), Some(GH), "{job:?}");
        assert_eq!(job.redact, [("GH_TOKEN".into(), "github_token".into())]);
        assert!(
            !job.env.iter().any(|(_, v)| v == PROVIDER),
            "a harness-only key reached a job: {job:?}"
        );
    }
    assert_eq!(r.results(&l1.session_id)[0].0, ResultStatus::Ok);
    let given = r.rows("secret.granted");
    assert_eq!(given.len(), 2, "{given:?}");
    assert_eq!(
        (
            &given[0]["program"],
            &given[0]["variable"],
            &given[0]["secret"]
        ),
        (&json!("true"), &json!("GH_TOKEN"), &json!("github_token"))
    );
    assert!(r.rows("secret.withheld").is_empty());
    let started = r.rows("tool.job_started");
    assert_eq!(started[0]["class"], "l1", "{started:?}");
    assert_eq!(given[0]["correlation_id"], started[0]["correlation_id"]);
    assert!(!r.core.tools.broker.may_hand_out("anthropic_api_key"));
    for value in [GH, PROVIDER] {
        let held = r.files_holding(value);
        assert!(held.is_empty(), "a value is in {held:?}");
    }
}

/// Decision 15 at the gate: an L1 call given a secret runs at the stricter
/// of its own posture (notify, or approve where the operator's own word
/// about proc.run asks) and the secret's, so an approve anywhere waits
/// before the job starts, and nothing is launched until it is answered.
#[tokio::test]
async fn decision_15_takes_the_stricter_of_the_call_and_the_secret_before_the_launch() {
    use Posture::{Approve, Notify, Open};
    for (secret, call, expect) in [
        (Open, Notify, Notify),
        (Notify, Notify, Notify),
        (Approve, Notify, Approve),
        (Open, Approve, Approve),
        (Notify, Approve, Approve),
        (Approve, Approve, Approve),
    ] {
        let r = rig(
            vec![
                run(json!({"argv": ["true"], "sandbox": true})),
                Scripted::text("Done."),
            ],
            secret,
            |cfg| {
                if call == Approve {
                    cfg.policy.tools.insert("proc.run".into(), Approve);
                }
            },
        );
        let res = r.turn("true in L1").await;
        let (posture, class, granted, reason) = r.gate(&res.session_id);
        let case = format!("secret {secret:?}, call {call:?}: {reason}");
        assert_eq!(posture, expect.as_str(), "{case}");
        assert_eq!(class.as_deref(), Some("l1"), "{case}");
        assert_eq!(granted.as_deref(), Some("true got GH_TOKEN"), "{case}");
        match expect {
            Approve => {
                assert_eq!(res.stop_reason, "awaiting_confirm", "{case}");
                assert!(r.launched().is_empty(), "launched before an answer: {case}");
            }
            _ => {
                assert_eq!(res.stop_reason, "no_tool_calls", "{case}");
                assert_eq!(r.launched().len(), 1, "{case}");
                assert_eq!(env_of(&r.launched()[0], "GH_TOKEN"), Some(GH), "{case}");
            }
        }
        if (secret, call) == (Approve, Notify) {
            assert!(
                reason.contains("[broker.secrets.github_token] posture = approve"),
                "{case}"
            );
        }
    }
}

/// An approve secret makes an L1 call wait before its launch, and the
/// operator's answer decides: a decline runs nothing and hands nothing out;
/// an approval launches the job in L1 with its grant.
#[tokio::test]
async fn an_approve_secret_waits_before_the_launch_and_the_answer_decides() {
    let script = || {
        vec![
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Answered."),
        ]
    };
    for approve in [false, true] {
        let r = rig(script(), Posture::Approve, |_| {});
        let res = r.turn("true in L1").await;
        assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
        assert!(r.launched().is_empty());
        let pending = r.core.pending_confirms(&res.session_id).unwrap();
        let corr = pending[0].correlation_id.clone();
        r.core
            .confirm_action(&corr, approve, Some("not today"), "test")
            .unwrap();
        let exec = res.execution_id.clone().unwrap();
        let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
        assert_eq!(cont.output, "Answered.");
        let given = r.rows("secret.granted");
        if approve {
            let jobs = r.launched();
            assert_eq!(jobs.len(), 1);
            assert!(
                jobs[0].sandbox.is_some(),
                "it runs in L1, as its proposal names"
            );
            assert_eq!(env_of(&jobs[0], "GH_TOKEN"), Some(GH));
            assert_eq!(given.len(), 1, "{given:?}");
            assert_eq!(r.results(&res.session_id)[0].0, ResultStatus::Ok);
        } else {
            assert!(r.launched().is_empty(), "a declined call ran");
            assert!(given.is_empty(), "{given:?}");
            assert_eq!(r.results(&res.session_id)[0].0, ResultStatus::Declined);
        }
    }
}
