//! 18e (row 34): the `aws` grant under L1. An L1 `proc.run` of a program
//! granted `[broker.programs.<p>] aws_account` takes its job session at its
//! launch, as any broker grant is taken (theseus-w5op; AWS design, "Steps
//! 17–18, L1"), through the whole core: the gate's record names the session
//! and the class, the job's environment holds the session (named by the job's
//! correlation id, under the guards) and no key, its wrapper is told to
//! withhold the session from its output, its view hides `~/.aws` whatever the
//! approve list says, and its egress list is the one its proposal binds, as
//! for any host. A call that names its own AWS endpoint waits, and nothing is
//! minted before the answer. A test launcher keeps each job's arguments and
//! runs `true` in their place; the daemon's `tests/sandbox.rs` runs a real L1
//! job given a session.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_kernel::job::WrapperArgs;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use super::session::{policy_arns, Kind};
use super::tests::{board, sts, Fake, ACCOUNT, KEY_ID, SECRET};
use super::tests_c2::{assumed, first_mint, form, minted_arns, SESSION_KEY, SESSION_SECRET};
use crate::bus::EventSink;
use crate::config::{AwsAccountConfig, ProgramGrant};
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The session token `assumed` mints.
const TOKEN: &str = "test-session-token";
/// AWS's regional STS endpoint, as the CLI asks for it in us-west-2.
const STS: &str = "sts.us-west-2.amazonaws.com:443";

/// Keeps every job's arguments, and runs `true` in their place, in process.
#[derive(Default)]
struct Kept(Mutex<Vec<WrapperArgs>>);

impl crate::toolrun::JobLauncher for Kept {
    fn launch(
        &self,
        spool: &theseus_kernel::Spool,
        args: &WrapperArgs,
        done: crate::toolrun::JobDone,
    ) -> anyhow::Result<u32> {
        self.0.lock().unwrap().push(args.clone());
        let mut a = args.clone();
        a.sandbox = None;
        a.argv = vec!["true".into()];
        crate::toolrun::InlineLauncher.launch(spool, &a, done)
    }
}

struct Rig {
    core: Arc<Core>,
    kept: Arc<Kept>,
    fake: Fake,
    dir: tempfile::TempDir,
}

/// A deployment whose `true` is granted a job session of the account, which
/// the fake stands in for (its owner role made), with `egress` as
/// `[sandbox] egress`. The approve list leaves out `~/.aws`, so only L1's
/// own rule can hide it.
fn rig(script: Vec<Scripted>, egress: &[&str]) -> Rig {
    let fake = Fake::start(|s, n| {
        let f = form(&s.body);
        match f.get("Action").map(String::as_str) {
            Some("AssumeRole") => assumed(n, f.get("RoleSessionName").map_or("?", |s| s)),
            _ => sts(ACCOUNT, n),
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.tools.approve_paths.retain(|p| p != "~/.aws");
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    cfg.sandbox.egress = egress.iter().map(|e| e.to_string()).collect();
    cfg.aws.accounts = BTreeMap::from([(
        ACCOUNT.to_string(),
        AwsAccountConfig {
            credentials: Default::default(),
            region: "us-west-2".into(),
            regions: vec![],
            endpoint: Some(fake.url.clone()),
            owner_role: Some("theseus-owner".into()),
            deployment: None,
            monthly_budget_usd: None,
            daily_budget_usd: None,
            hourly_alert_usd: crate::config::default_hourly_alert_usd(),
            durability: false,
            hands_network: None,
        },
    )]);
    cfg.broker.programs = BTreeMap::from([(
        "true".to_string(),
        ProgramGrant {
            env: BTreeMap::new(),
            aws_account: Some(ACCOUNT.into()),
        },
    )]);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let board = board();
    let kept = Arc::new(Kept::default());
    let core = Core::build(crate::rpc::Parts {
        secrets: board.clone(),
        scrubber: Arc::new(crate::scrub::Scrubber::from_board(board)),
        launcher: kept.clone(),
        ..crate::rpc::Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(script)), store)
    })
    .unwrap();
    Rig {
        core,
        kept,
        fake,
        dir,
    }
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

    fn result(&self, sid: &str) -> ResultStatus {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolResult { status, .. } => Some(*status),
                _ => None,
            })
            .expect("a tool result")
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

    /// The `AssumeRole` requests the fake answered.
    fn mints(&self) -> usize {
        self.fake
            .seen()
            .iter()
            .filter(|s| form(&s.body).get("Action").map(String::as_str) == Some("AssumeRole"))
            .count()
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

/// `true sts get-caller-identity`, as an L1 call, its input `sandbox` as given.
fn call(sandbox: Value) -> Scripted {
    Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            json!({"argv": ["true", "sts", "get-caller-identity"], "sandbox": sandbox}),
        )],
    )
}

/// The job's AWS variables, by name.
fn aws_env(job: &WrapperArgs) -> BTreeMap<&str, &str> {
    job.env
        .iter()
        .filter(|(k, _)| k.starts_with("AWS_"))
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

/// The brief's points, for an L1 job of a program granted a session, with
/// AWS's STS endpoint on `[sandbox] egress`: the gate's record names the
/// class and the session (`aws_session_label`) at notify; the job is launched
/// in L1 with the session's three variables, its region, and no profile
/// file, never the key, and nothing else of the daemon's but `proc_env`; its
/// wrapper withholds the three from its output; the session is minted once,
/// named by the job's correlation id, under the job kind's guards, for the
/// job's deadline; the view hides `~/.aws` though the approve list leaves it
/// out; the job's list is the operator's, as its `sandbox.started` row says;
/// and no file holds a secret.
#[tokio::test]
async fn an_l1_job_granted_aws_takes_its_session_at_launch_and_no_other_credential() {
    let r = rig(vec![call(json!(true)), Scripted::text("Done.")], &[STS]);
    let res = r.turn("whoami, in L1").await;
    assert_eq!(res.stop_reason, "no_tool_calls", "{res:?}");
    let label = crate::broker::aws_session_label(ACCOUNT);
    let (posture, class, granted, reason) = r.gate(&res.session_id);
    assert_eq!(
        (posture.as_str(), class.as_deref(), granted.as_deref()),
        (
            "notify",
            Some("l1"),
            Some(format!("true got {label}").as_str())
        ),
        "{reason}"
    );
    let jobs = r.launched();
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    let view = job.sandbox.as_ref().expect("launched in L1");
    let env = aws_env(job);
    assert_eq!(
        env,
        BTreeMap::from([
            ("AWS_ACCESS_KEY_ID", SESSION_KEY),
            ("AWS_SECRET_ACCESS_KEY", SESSION_SECRET),
            ("AWS_SESSION_TOKEN", TOKEN),
            ("AWS_REGION", "us-west-2"),
            ("AWS_DEFAULT_REGION", "us-west-2"),
            ("AWS_CONFIG_FILE", "/dev/null"),
            ("AWS_SHARED_CREDENTIALS_FILE", "/dev/null"),
        ])
    );
    assert!(
        !job.env.iter().any(|(_, v)| v == KEY_ID || v == SECRET),
        "the key reached the job"
    );
    let cfg = &r.core.cfg;
    for (k, _) in &job.env {
        assert!(
            env.contains_key(k.as_str())
                || cfg.tools.proc_env.contains(k)
                || k == theseus_protocol::JOB_SESSION_ENV,
            "{k} is not the session's, nor proc_env's"
        );
    }
    let session = [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
    ];
    assert_eq!(
        job.redact,
        session.map(|v| (v.to_string(), label.clone())),
        "the wrapper withholds the session from the job's output"
    );
    // One session, named by the job, under the job's guards, for its deadline.
    assert_eq!(r.mints(), 1);
    let mint = first_mint(&r.fake.seen());
    assert_eq!(mint["RoleSessionName"], job.correlation_id);
    assert_eq!(minted_arns(&mint), policy_arns(ACCOUNT, Kind::Job));
    assert_eq!(
        mint["DurationSeconds"], "900",
        "600 s and a minute, raised to STS's shortest"
    );
    // The view: no ~/.aws, whatever the approve list says; the list its
    // proposal binds, the operator's.
    let aws_dir = theseus_tools::paths::canonical_best_effort(&crate::config::expand("~/.aws"));
    assert!(view.hidden.contains(&aws_dir), "{:?}", view.hidden);
    assert_eq!(view.egress, [STS]);
    let started = r.rows("sandbox.started");
    assert_eq!(started[0]["egress"], json!([STS]));
    let job_started = r.rows("tool.job_started");
    assert_eq!(job_started[0]["class"], "l1", "{job_started:?}");
    let given = r.rows("secret.granted");
    assert_eq!(given.len(), 3, "{given:?}");
    for (row, var) in given.iter().zip(session) {
        assert_eq!(
            (&row["program"], &row["variable"], &row["secret"]),
            (&json!("true"), &json!(var), &json!(label))
        );
        assert_eq!(row["correlation_id"], job.correlation_id.as_str());
    }
    assert!(r.rows("secret.withheld").is_empty());
    assert_eq!(r.result(&res.session_id), ResultStatus::Ok);
    for value in [SESSION_SECRET, TOKEN, SECRET] {
        let held = r.files_holding(value);
        assert!(held.is_empty(), "a secret is in {held:?}");
    }
}

/// The other way a job's list gets an AWS endpoint (18c, as for any host):
/// its call names it beyond `[sandbox] egress`. The call waits, its reason
/// names the host, and nothing is minted before the answer; once approved,
/// the job runs in L1 with the list its proposal bound, and its session.
#[tokio::test]
async fn a_call_that_names_its_aws_endpoint_waits_and_mints_nothing_before_the_answer() {
    let r = rig(
        vec![call(json!({"egress": [STS]})), Scripted::text("Answered.")],
        &[],
    );
    let res = r.turn("whoami, naming STS").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let (posture, class, granted, reason) = r.gate(&res.session_id);
    assert_eq!(
        (posture.as_str(), class.as_deref()),
        ("approve", Some("l1"))
    );
    assert_eq!(
        granted,
        Some(format!(
            "true got {}",
            crate::broker::aws_session_label(ACCOUNT)
        ))
    );
    assert!(
        reason.contains(&format!("beyond [sandbox] egress: {STS}")),
        "{reason}"
    );
    assert!(r.launched().is_empty());
    assert_eq!(r.mints(), 0, "a session was minted before the answer");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    r.core
        .confirm_action(&pending[0].correlation_id, true, None, "test")
        .unwrap();
    let exec = res.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Answered.");
    let jobs = r.launched();
    assert_eq!(jobs.len(), 1);
    let view = jobs[0]
        .sandbox
        .as_ref()
        .expect("it runs in L1, as its proposal names");
    assert_eq!(view.egress, [STS], "the list its proposal bound");
    assert_eq!(aws_env(&jobs[0])["AWS_SESSION_TOKEN"], TOKEN);
    assert_eq!(r.mints(), 1);
    assert_eq!(
        first_mint(&r.fake.seen())["RoleSessionName"],
        jobs[0].correlation_id
    );
}
