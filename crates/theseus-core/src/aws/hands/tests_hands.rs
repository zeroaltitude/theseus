//! `aws.hands.run` and the completion poller through the whole core,
//! against a local fake of AWS with a queue (step 40's standing scenarios,
//! offline): a group's frame, its launch on Lambda or Fargate, its hands'
//! envelopes home over the fake queue, each checked; completions dropped,
//! duplicated, and late; a restart with completions waiting; a forged one
//! quarantined, never settled; `first_success` launching nothing after its
//! first success; and a poller that sends nothing while no group is open.
//! Part 2's scenarios are in `tests_part2.rs`, on this rig.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::ActionState;
use theseus_store::Store as _;

use super::envelope::{Envelope, HandSpec, VERSION};
use super::group::GroupRecord;
use super::hand::{self, Creds};
use crate::aws::tests::{board, sts, Fake, Reply, Seen, ACCOUNT};
use crate::config::{AwsAccountConfig, AwsConfig};
use crate::policy::Posture;
use crate::provider::Scripted;

// ------------------------------------------------------------------ the fake

/// AWS with state: the stacks' outputs, a queue, and what was launched.
#[derive(Default)]
pub(super) struct State {
    /// Visible messages: receipt, body.
    pub(super) queue: Mutex<Vec<(String, String)>>,
    /// Each Lambda hand's spec, as invoked.
    pub(super) invoked: Mutex<Vec<HandSpec>>,
    /// Each RunTask's input.
    pub(super) ran: Mutex<Vec<Value>>,
    pub(super) registered: Mutex<Vec<Value>>,
    pub(super) deleted: Mutex<Vec<String>>,
    pub(super) receives: AtomicUsize,
    pub(super) sent: AtomicUsize,
    /// The network stack's NAT.
    pub(super) nat: AtomicBool,
    /// Each StopTask's input (part 2).
    pub(super) stops: Mutex<Vec<Value>>,
    /// A task's last status and stopped reason, by ARN, as DescribeTasks
    /// reads them; a task not here is RUNNING.
    pub(super) tasks: Mutex<BTreeMap<String, (String, Option<String>)>>,
    /// StopTask stops its task at once (STOPPED at the next read).
    pub(super) stop_at_once: AtomicBool,
    pub(super) describes: AtomicUsize,
    /// Fargate's vCPU quota and Lambda's unreserved concurrency, when set;
    /// unset, the reads answer nothing, and no quota caps a group.
    pub(super) quota: Mutex<Option<f64>>,
    pub(super) quota_reads: AtomicUsize,
    /// The network stack is in an existing VPC (theseus-mgw.9), whose
    /// second subnet's route table sends 0.0.0.0/0 nowhere while `unrouted`.
    pub(super) existing: AtomicBool,
    pub(super) unrouted: AtomicBool,
    /// Each DescribeRouteTables' body.
    pub(super) route_reads: Mutex<Vec<String>>,
}

impl State {
    pub(super) fn push(&self, body: String) {
        let mut q = self.queue.lock().unwrap();
        let receipt = format!("rh-{}", self.sent.fetch_add(1, Ordering::SeqCst));
        q.push((receipt, body));
    }

    pub(super) fn invoked(&self) -> Vec<HandSpec> {
        self.invoked.lock().unwrap().clone()
    }
}

pub(super) fn outputs(pairs: &[(&str, &str)], params: &[(&str, &str)]) -> String {
    let o: String = pairs
        .iter()
        .map(|(k, v)| {
            format!("<member><OutputKey>{k}</OutputKey><OutputValue>{v}</OutputValue></member>")
        })
        .collect();
    let p: String = params
        .iter()
        .map(|(k, v)| {
            format!("<member><ParameterKey>{k}</ParameterKey><ParameterValue>{v}</ParameterValue></member>")
        })
        .collect();
    format!(
        "<DescribeStacksResponse><DescribeStacksResult><Stacks><member>\
         <StackName>s</StackName><StackId>arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/s/1</StackId>\
         <CreationTime>2026-10-01T00:00:00Z</CreationTime><StackStatus>CREATE_COMPLETE</StackStatus>\
         <Outputs>{o}</Outputs><Parameters>{p}</Parameters></member></Stacks></DescribeStacksResult>\
         <ResponseMetadata><RequestId>r</RequestId></ResponseMetadata></DescribeStacksResponse>"
    )
}

#[expect(clippy::too_many_lines, reason = "one fake, every operation hands use")]
pub(super) fn answer(state: &State, s: &Seen, n: usize) -> Reply {
    let id = format!("req-{n}");
    let json = |v: Value| -> Reply {
        (
            200,
            vec![
                ("x-amzn-requestid", id.clone()),
                ("content-type", "application/x-amz-json-1.1".into()),
            ],
            v.to_string(),
        )
    };
    let xml = |b: String| -> Reply {
        (
            200,
            vec![
                ("x-amzn-requestid", id.clone()),
                ("content-type", "text/xml".into()),
            ],
            b,
        )
    };
    match s.action() {
        Some("GetCallerIdentity") => return sts(ACCOUNT, n),
        Some("DescribeStacks") => {
            let nat = if state.nat.load(Ordering::SeqCst) {
                "enabled"
            } else {
                "disabled"
            };
            let body = if s.body.contains("theseus-hands-network")
                && state.existing.load(Ordering::SeqCst)
            {
                super::tests_network::existing_outputs()
            } else if s.body.contains("theseus-hands-network") {
                outputs(
                    &[
                        ("PrivateSubnetIds", "subnet-0example0a,subnet-0example0b"),
                        ("HandsSecurityGroupId", "sg-0example"),
                        ("NatGateway", nat),
                    ],
                    &[("NatGateway", nat)],
                )
            } else if s.body.contains("theseus-hands") {
                outputs(
                    &[
                        (
                            "ClusterArn",
                            &format!("arn:aws:ecs:us-west-2:{ACCOUNT}:cluster/theseus-hands"),
                        ),
                        ("HandsLogGroupName", "/theseus/hands"),
                        (
                            "HandBasicRoleArn",
                            &format!("arn:aws:iam::{ACCOUNT}:role/theseus-hand-basic"),
                        ),
                        (
                            "HandReadRoleArn",
                            &format!("arn:aws:iam::{ACCOUNT}:role/theseus-hand-read"),
                        ),
                        (
                            "HandExecutionRoleArn",
                            &format!("arn:aws:iam::{ACCOUNT}:role/theseus-hand-execution"),
                        ),
                        (
                            "LambdaHandArn",
                            &format!(
                                "arn:aws:lambda:us-west-2:{ACCOUNT}:function:theseus-hand-basic"
                            ),
                        ),
                    ],
                    &[
                        ("HandImageUri", "example.invalid/theseus/hand@sha256:00"),
                        ("LambdaHandMemoryMb", "2048"),
                    ],
                )
            } else {
                outputs(
                    &[
                        ("BucketName", "example-bucket"),
                        (
                            "CompletionQueueUrl",
                            &format!(
                                "https://sqs.us-west-2.amazonaws.com/{ACCOUNT}/theseus-completions"
                            ),
                        ),
                    ],
                    &[],
                )
            };
            return xml(body);
        }
        Some("DescribeRouteTables") => {
            state.route_reads.lock().unwrap().push(s.body.clone());
            return xml(super::tests_network::route_tables(
                state.unrouted.load(Ordering::SeqCst),
            ));
        }
        _ => {}
    }
    let target = s.header("x-amz-target").unwrap_or_default().to_string();
    let op = target.rsplit('.').next().unwrap_or_default();
    let body: Value = serde_json::from_str(&s.body).unwrap_or(Value::Null);
    match op {
        "SendMessage" => {
            state.push(body["MessageBody"].as_str().unwrap_or_default().to_string());
            json(json!({"MessageId": format!("m-{n}")}))
        }
        "ReceiveMessage" => {
            state.receives.fetch_add(1, Ordering::SeqCst);
            let got: Vec<(String, String)> = {
                let mut q = state.queue.lock().unwrap();
                let k = q.len().min(10);
                q.drain(..k).collect()
            };
            if got.is_empty() {
                // A short long poll: the test's clock, not AWS's 20 s.
                std::thread::sleep(Duration::from_millis(100));
                return json(json!({}));
            }
            json(json!({"Messages": got.iter().map(|(r, b)| json!({
                "MessageId": r, "ReceiptHandle": r, "Body": b})).collect::<Vec<_>>()}))
        }
        "DeleteMessage" => {
            state
                .deleted
                .lock()
                .unwrap()
                .push(body["ReceiptHandle"].as_str().unwrap_or_default().into());
            json(json!({}))
        }
        "RegisterTaskDefinition" => {
            state.registered.lock().unwrap().push(body.clone());
            json(json!({"taskDefinition": {"taskDefinitionArn":
                format!("arn:aws:ecs:us-west-2:{ACCOUNT}:task-definition/{}:1", body["family"].as_str().unwrap_or_default())}}))
        }
        "RunTask" => {
            let k = {
                let mut r = state.ran.lock().unwrap();
                r.push(body);
                r.len()
            };
            json(
                json!({"tasks": [{"taskArn": format!("arn:aws:ecs:us-west-2:{ACCOUNT}:task/theseus-hands/t{k}")}], "failures": []}),
            )
        }
        "GetServiceQuota" => {
            state.quota_reads.fetch_add(1, Ordering::SeqCst);
            match *state.quota.lock().unwrap() {
                Some(v) => json(json!({"Quota": {"QuotaCode": body["QuotaCode"], "Value": v}})),
                None => json(json!({})),
            }
        }
        "" if s.method == "GET" && s.target.contains("/account-settings") => {
            state.quota_reads.fetch_add(1, Ordering::SeqCst);
            match *state.quota.lock().unwrap() {
                Some(v) => json(json!({"AccountLimit": {"ConcurrentExecutions": 1000,
                    "UnreservedConcurrentExecutions": v}})),
                None => json(json!({})),
            }
        }
        "StopTask" => {
            let arn = body["task"].as_str().unwrap_or_default().to_string();
            state.stops.lock().unwrap().push(body.clone());
            if state.stop_at_once.load(Ordering::SeqCst) {
                state.tasks.lock().unwrap().insert(
                    arn.clone(),
                    ("STOPPED".into(), body["reason"].as_str().map(String::from)),
                );
            }
            json(json!({"task": {"taskArn": arn, "desiredStatus": "STOPPED"}}))
        }
        "DescribeTasks" => {
            state.describes.fetch_add(1, Ordering::SeqCst);
            let known = state.tasks.lock().unwrap().clone();
            let (mut tasks, mut failures) = (Vec::new(), Vec::new());
            for arn in body["tasks"].as_array().into_iter().flatten() {
                let arn = arn.as_str().unwrap_or_default();
                match known.get(arn) {
                    Some((st, _)) if st == "MISSING" => {
                        failures.push(json!({"arn": arn, "reason": "MISSING"}));
                    }
                    Some((st, reason)) => tasks.push(json!({"taskArn": arn, "lastStatus": st,
                        "stoppedReason": reason, "startedAt": 1_790_000_000.0,
                        "stoppedAt": 1_790_000_060.0,
                        "containers": [{"name": "hand", "exitCode": 143}]})),
                    None => tasks.push(json!({"taskArn": arn, "lastStatus": "RUNNING",
                        "startedAt": 1_790_000_000.0, "containers": [{"name": "hand"}]})),
                }
            }
            json(json!({"tasks": tasks, "failures": failures}))
        }
        "" if s.method == "POST" && s.target.contains("/invocations") => {
            let spec: HandSpec = serde_json::from_str(&s.body).expect("a hand's spec");
            assert_eq!(s.header("x-amz-invocation-type"), Some("Event"));
            state.invoked.lock().unwrap().push(spec);
            (
                202,
                vec![("x-amzn-requestid", format!("inv-{n}"))],
                String::new(),
            )
        }
        "" => (
            200,
            vec![("x-amz-request-id", id), ("etag", "\"e\"".into())],
            String::new(),
        ),
        _ => json(json!({})),
    }
}

// ------------------------------------------------------------------ the rig

pub(super) struct Rig {
    pub(super) core: Arc<crate::Core>,
    pub(super) state: Arc<State>,
    pub(super) fake: Fake,
    pub(super) dir: tempfile::TempDir,
}

pub(super) fn account(endpoint: &str) -> AwsConfig {
    AwsConfig {
        accounts: BTreeMap::from([(
            ACCOUNT.to_string(),
            AwsAccountConfig {
                credentials: Default::default(),
                region: "us-west-2".into(),
                regions: vec![],
                endpoint: Some(endpoint.into()),
                owner_role: None,
                deployment: Some("theseus-example".into()),
                monthly_budget_usd: None,
                daily_budget_usd: None,
                hourly_alert_usd: crate::config::default_hourly_alert_usd(),
                durability: false,
                hands_network: None,
            },
        )]),
    }
}

pub(super) fn config(dir: &Path, endpoint: &str) -> crate::Config {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.policy.enforcement = Posture::Notify;
    cfg.aws = account(endpoint);
    cfg
}

pub(super) fn core_at(dir: &Path, endpoint: &str, script: Vec<Scripted>) -> Arc<crate::Core> {
    let store = crate::store::Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(crate::provider::FakeProvider::scripted(script));
    crate::Core::build(crate::rpc::Parts {
        secrets: board(),
        ..crate::rpc::Parts::for_tests(config(dir, endpoint), model, store)
    })
    .unwrap()
}

pub(super) fn rig(script: Vec<Scripted>) -> Rig {
    let state = Arc::new(State::default());
    let s = state.clone();
    let fake = Fake::start(move |seen, n| answer(&s, seen, n));
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("THESEUS_HAND_DIR", dir.path());
    let core = core_at(dir.path(), &fake.url, script);
    Rig {
        core,
        state,
        fake,
        dir,
    }
}

/// A model that calls `aws_hands_run` with `input`, then says `then` (and
/// `after` in the continuation).
pub(super) fn calls(input: Value) -> Vec<Scripted> {
    vec![
        Scripted::tools("", &[("t_hands", "aws_hands_run", input)]),
        Scripted::text("The hands are running."),
        Scripted::text("The hands are done."),
    ]
}

pub(super) async fn turn(
    core: &Arc<crate::Core>,
    input: &str,
) -> theseus_protocol::TurnSubmitResult {
    try_turn(core, input).await.unwrap()
}

pub(super) async fn try_turn(
    core: &Arc<crate::Core>,
    input: &str,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    use crate::session::SessionRecord;
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(crate::turn::TurnRequest {
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
}

pub(super) fn rows(core: &crate::Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// The group's id: the `aws.hands.run` call's correlation id.
pub(super) fn group_of(core: &crate::Core) -> String {
    rows(core, "aws.hands.launched")
        .first()
        .and_then(|r| r["group"].as_str().map(String::from))
        .expect("a launched group")
}

pub(super) fn record(core: &crate::Core, group: &str) -> GroupRecord {
    GroupRecord::load(&core.store, group)
        .unwrap()
        .expect("its record")
}

pub(super) fn state_of(core: &crate::Core, corr: &str) -> ActionState {
    core.kernel.action(corr).unwrap().expect("an action").state
}

/// Wait, on the real clock, until `f` holds (at most 20 s).
pub(super) async fn until(what: &str, f: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "waited 20 s for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The hand role, run here against the fake, as the Lambda would run it:
/// its envelope lands on the fake queue.
pub(super) async fn play(spec: &HandSpec) -> Envelope {
    hand::run(
        spec,
        Some("inv-played".into()),
        &Creds::fixed(theseus_aws::Credentials::new(
            "AKIDHANDEXAMPLE",
            "hand-secret",
            None,
            None,
        )),
    )
    .await
    .expect("the hand ran and sent its envelope")
}

/// An envelope as a hand would sign it, with its outcome.
pub(super) fn signed(spec: &HandSpec, outcome: &str, exit: i32) -> String {
    let mut e = Envelope {
        v: VERSION,
        correlation_id: spec.correlation_id.clone(),
        group: spec.group.clone(),
        index: spec.index,
        outcome: outcome.into(),
        exit_code: Some(exit),
        timed_out: false,
        result_ref: Some(format!(
            "s3://example-bucket/hands/{}/",
            spec.correlation_id
        )),
        external_op_id: Some("inv-signed".into()),
        started_at_ms: 1_000,
        finished_at_ms: 3_000,
        producer: "hand:lambda".into(),
        tail: format!("hand {} says {outcome}\n", spec.index),
        note: None,
        signature: String::new(),
    };
    e.sign(&spec.key_bytes().unwrap());
    serde_json::to_string(&e).unwrap()
}

/// The late result the continuation wrote for the group's call.
pub(super) fn late_result(
    core: &crate::Core,
    session: &str,
) -> (String, crate::node::ResultStatus) {
    core.store
        .session_nodes(session)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult {
                tool,
                content,
                status,
                late: true,
                ..
            } if tool == "aws.hands.run" => Some((content.clone(), *status)),
            _ => None,
        })
        .expect("the group's late result")
}

// ------------------------------------------------------------------ the scenarios

/// A group of two on Lambda: the call writes its record and both hands'
/// actions, dispatched, before it launches; each hand is an asynchronous
/// Invoke of the hand function with its own spec and key; the call answers
/// `background`. The real hand role runs each spec against the fake, its
/// envelope comes home over the queue, the poller checks and settles each,
/// deletes its message, and settles the group, whose continuation reads the
/// aggregate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[expect(clippy::too_many_lines, reason = "one scenario, start to end")]
async fn a_lambda_group_launches_then_settles_from_its_hands_envelopes() {
    let r = rig(calls(json!({
        "argv": ["sh", "-c", "echo hand {index} of {input}"],
        "inputs": ["red", "blue"],
    })));
    let res = turn(&r.core, "run two hands").await;
    let exec = res.execution_id.clone().unwrap();
    let g = group_of(&r.core);
    let rec = record(&r.core, &g);
    assert_eq!(rec.backend, super::launch::Backend::Lambda);
    assert_eq!(
        (rec.hands.len(), rec.execution_id.as_str()),
        (2, exec.as_str())
    );
    assert_eq!(
        rec.env.queue_url,
        format!("https://sqs.us-west-2.amazonaws.com/{ACCOUNT}/theseus-completions")
    );
    for h in &rec.hands {
        let a = r.core.kernel.action(&h.correlation_id).unwrap().unwrap();
        assert_eq!(
            (a.tool.as_str(), a.state),
            ("aws.hand", ActionState::Dispatched)
        );
        assert_eq!(
            a.resource.as_deref(),
            Some(format!("aws.hands.group.{g}").as_str())
        );
        assert!(
            h.external_op_id
                .as_deref()
                .is_some_and(|x| x.starts_with("inv-")),
            "{h:?}"
        );
    }
    assert_eq!(state_of(&r.core, &g), ActionState::Dispatched);
    // The call's own answer: background.
    let first = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult {
                tool,
                content,
                status,
                ..
            } if tool == "aws.hands.run" => Some((content.clone(), *status)),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        first.1,
        crate::node::ResultStatus::Background,
        "{}",
        first.0
    );
    assert!(
        first.0.contains("group of 2 hands on lambda"),
        "{}",
        first.0
    );
    // Each Invoke: the hand's own spec, key, and index; its row.
    // Launched at once, so in either order.
    let mut specs = r.state.invoked();
    specs.sort_by_key(|s| s.index);
    assert_eq!(specs.len(), 2);
    let mut keys: Vec<&str> = specs.iter().map(|s| s.key.as_str()).collect();
    keys.dedup();
    assert_eq!(keys.len(), 2, "a key per hand");
    assert_eq!(
        specs
            .iter()
            .map(|s| (s.index, s.input.clone()))
            .collect::<Vec<_>>(),
        [(0, json!("red")), (1, json!("blue"))]
    );
    assert!(specs.iter().all(|s| s.group == g && s.deadline_secs == 570));
    let called = rows(&r.core, "aws.called");
    let invokes = called.iter().filter(|c| c["operation"] == "Invoke").count();
    assert_eq!(invokes, 2, "{called:?}");
    // No key reaches the ledger.
    for row in rows(&r.core, "aws.hands.launched") {
        for s in &specs {
            assert!(!row.to_string().contains(&s.key));
        }
    }
    let launched = rows(&r.core, "aws.hands.launched");
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0]["hands"].as_array().unwrap().len(), 2);

    // The hands run, and their envelopes come home.
    for s in &specs {
        play(s).await;
    }
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    for h in &rec.hands {
        assert_eq!(state_of(&r.core, &h.correlation_id), ActionState::Succeeded);
    }
    until("both messages deleted", || {
        r.state.deleted.lock().unwrap().len() == 2
    })
    .await;
    let settled = rows(&r.core, "aws.hands.settled");
    assert_eq!(settled.len(), 1);
    assert_eq!(
        (
            settled[0]["met"].as_bool(),
            settled[0]["succeeded"].as_u64()
        ),
        (Some(true), Some(2))
    );
    // The continuation reads the aggregate.
    r.core.continue_execution(&exec).await.unwrap().unwrap();
    let (text, status) = late_result(&r.core, &res.session_id);
    assert_eq!(status, crate::node::ResultStatus::Ok);
    assert!(text.contains("2 of 2 succeeded"), "{text}");
    assert!(
        text.contains("hand 0 of red") && text.contains("hand 1 of blue"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "s3://example-bucket/hands/{}/",
            rec.hands[0].correlation_id
        )),
        "{text}"
    );
    drop(r.dir);
    drop(r.fake);
}

/// A forged envelope (signed with another key) is quarantined: a
/// `completion.quarantined` row saying why, a quarantined completion health
/// counts, its message deleted, and its hand never settled; the hand's own
/// envelope then settles it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_signature_is_quarantined_and_never_settled() {
    let r = rig(calls(json!({"argv": ["true"]})));
    turn(&r.core, "run a hand").await;
    let g = group_of(&r.core);
    let spec = r.state.invoked().pop().unwrap();
    let mut forged = spec.clone();
    forged.key = "11".repeat(32);
    r.state.push(signed(&forged, "succeeded", 0));
    r.core.poll_hands_after_serving();
    until("the forgery's quarantine", || {
        !rows(&r.core, "completion.quarantined").is_empty()
    })
    .await;
    let q = rows(&r.core, "completion.quarantined");
    assert_eq!(q[0]["correlation_id"], spec.correlation_id);
    assert!(q[0]["why"].as_str().unwrap().contains("signature"), "{q:?}");
    assert_eq!(r.core.kernel.quarantined().unwrap().len(), 1);
    until("its message deleted", || {
        r.state.deleted.lock().unwrap().len() == 1
    })
    .await;
    assert_eq!(
        state_of(&r.core, &spec.correlation_id),
        ActionState::Dispatched
    );
    assert_eq!(state_of(&r.core, &g), ActionState::Dispatched);
    assert!(rows(&r.core, "action.succeeded")
        .iter()
        .all(|a| a["correlation_id"] != spec.correlation_id));
    // The real one settles it.
    r.state.push(signed(&spec, "succeeded", 0));
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    assert_eq!(rows(&r.core, "completion.quarantined").len(), 1);
}

/// Duplicates and late arrivals settle once: a hand's envelope delivered
/// twice is one settle and one `completion.duplicate` row; the group
/// settles once, and stops its other hand (part 2: Lambda's cancel is
/// `unsupported`); that hand's envelope after its group is done is
/// recorded as late, and settles nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_duplicate_settles_once_and_a_late_one_settles_only_its_hand() {
    let r = rig(calls(
        json!({"argv": ["true"], "count": 2, "until": "first_success"}),
    ));
    turn(&r.core, "run two hands").await;
    let g = group_of(&r.core);
    let specs = r.state.invoked();
    let first = signed(&specs[0], "succeeded", 0);
    r.state.push(first.clone());
    r.state.push(first);
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    until("the duplicate's row", || {
        !rows(&r.core, "completion.duplicate").is_empty()
    })
    .await;
    let ok: Vec<Value> = rows(&r.core, "action.succeeded")
        .into_iter()
        .filter(|a| a["correlation_id"] == specs[0].correlation_id)
        .collect();
    assert_eq!(ok.len(), 1, "settled once: {ok:?}");
    assert_eq!(rows(&r.core, "completion.duplicate").len(), 1);
    // The second hand comes home after its group is done: late.
    assert_eq!(
        state_of(&r.core, &specs[1].correlation_id),
        ActionState::Cancelled
    );
    r.state.push(signed(&specs[1], "failed", 1));
    until("the late hand", || {
        !rows(&r.core, "completion.late_after_cancel").is_empty()
    })
    .await;
    assert_eq!(
        state_of(&r.core, &specs[1].correlation_id),
        ActionState::Cancelled
    );
    until("every message deleted", || {
        r.state.deleted.lock().unwrap().len() == 3
    })
    .await;
    assert_eq!(
        rows(&r.core, "aws.hands.settled").len(),
        1,
        "the group settled once"
    );
    assert_eq!(state_of(&r.core, &g), ActionState::Succeeded);
}

/// `until: first_success`, one at a time: the first hand fails, so the
/// second launches; it succeeds, so the group is done, and the other two
/// never launch (cancelled, verified: nothing ran). Two Invokes in all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_success_stops_launching_after_the_first_success() {
    let r = rig(calls(
        json!({"argv": ["true"], "count": 4, "concurrency": 1, "until": "first_success"}),
    ));
    let res = turn(&r.core, "find one that works").await;
    let g = group_of(&r.core);
    assert_eq!(r.state.invoked().len(), 1, "one at a time");
    let rec = record(&r.core, &g);
    assert_eq!(
        state_of(&r.core, &rec.hands[1].correlation_id),
        ActionState::Authorized
    );
    r.state.push(signed(&r.state.invoked()[0], "failed", 2));
    r.core.poll_hands_after_serving();
    until("the second launch", || r.state.invoked().len() == 2).await;
    r.state.push(signed(&r.state.invoked()[1], "succeeded", 0));
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        r.state.invoked().len(),
        2,
        "nothing launched after the first success"
    );
    for h in &rec.hands[2..] {
        let a = r.core.kernel.action(&h.correlation_id).unwrap().unwrap();
        assert_eq!(a.state, ActionState::Cancelled);
        assert!(a.dispatched_at_ms.is_none(), "never dispatched");
    }
    let s = &rows(&r.core, "aws.hands.settled")[0];
    assert_eq!(
        (
            s["succeeded"].as_u64(),
            s["failed"].as_u64(),
            s["not_launched"].as_u64()
        ),
        (Some(1), Some(1), Some(2))
    );
    r.core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    let (text, _) = late_result(&r.core, &res.session_id);
    assert!(
        text.contains("1 of 4 succeeded, 1 failed, 2 not launched"),
        "{text}"
    );
}

/// The poller sends nothing while no group is open, and polls once one is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_poller_is_idle_with_nothing_outstanding() {
    let r = rig(calls(json!({"argv": ["true"]})));
    r.core.poll_hands_after_serving();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(r.state.receives.load(Ordering::SeqCst), 0);
    assert!(r.fake.seen().is_empty(), "nothing reached AWS");
    turn(&r.core, "run a hand").await;
    until("a poll", || r.state.receives.load(Ordering::SeqCst) > 0).await;
    let g = group_of(&r.core);
    r.state.push(signed(&r.state.invoked()[0], "succeeded", 0));
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    // Done: it goes quiet again.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let n = r.state.receives.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        r.state.receives.load(Ordering::SeqCst),
        n,
        "no poll with nothing open"
    );
}

/// A restart with completions waiting: the daemon that launched the group
/// is gone before its hands report; the next one's poller finds the open
/// group in the store, takes the waiting envelopes (one twice), and settles
/// each hand once and the group once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_with_completions_waiting_settles_each_once() {
    let state = Arc::new(State::default());
    let s = state.clone();
    let fake = Fake::start(move |seen, n| answer(&s, seen, n));
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("THESEUS_HAND_DIR", dir.path());
    let (g, exec) = {
        let core = core_at(
            dir.path(),
            &fake.url,
            calls(json!({"argv": ["true"], "count": 2})),
        );
        let res = turn(&core, "run two hands").await;
        (group_of(&core), res.execution_id.unwrap())
    };
    let specs = state.invoked();
    state.push(signed(&specs[0], "succeeded", 0));
    state.push(signed(&specs[1], "succeeded", 0));
    state.push(signed(&specs[1], "succeeded", 0));
    let core = core_at(
        dir.path(),
        &fake.url,
        vec![Scripted::text("The hands are done.")],
    );
    core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&core, &g) == ActionState::Succeeded
    })
    .await;
    until("every message deleted", || {
        state.deleted.lock().unwrap().len() == 3
    })
    .await;
    for s in &specs {
        let n = rows(&core, "action.succeeded")
            .iter()
            .filter(|a| a["correlation_id"] == s.correlation_id)
            .count();
        assert_eq!(n, 1, "{} settled once", s.correlation_id);
    }
    assert_eq!(rows(&core, "aws.hands.settled").len(), 1);
    assert_eq!(rows(&core, "completion.duplicate").len(), 1);
    core.continue_execution(&exec).await.unwrap().unwrap();
}

/// A dropped completion: a hand whose envelope never came, but whose Lambda
/// invocation failed into the queue's failure destination, settles failed
/// from that record, which counts only with the hand's own key; a forged
/// record is quarantined.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lambda_failure_record_settles_its_hand_and_a_forged_one_is_quarantined() {
    let r = rig(calls(json!({"argv": ["true"], "count": 2})));
    turn(&r.core, "run two hands").await;
    let g = group_of(&r.core);
    let specs = r.state.invoked();
    let record = |spec: &HandSpec| {
        json!({
            "version": "1.0",
            "requestContext": {"requestId": "inv-failed", "condition": "RetriesExhausted",
                "functionArn": format!("arn:aws:lambda:us-west-2:{ACCOUNT}:function:theseus-hand-basic")},
            "requestPayload": spec,
            "responseContext": {"statusCode": 200, "functionError": "Unhandled"},
            "responsePayload": {"errorMessage": "the envelope was not sent", "errorType": "HandFailed"},
        })
        .to_string()
    };
    let mut forged = specs[1].clone();
    forged.key = "22".repeat(32);
    r.state.push(record(&forged));
    r.state.push(record(&specs[0]));
    r.core.poll_hands_after_serving();
    until("the failed hand", || {
        state_of(&r.core, &specs[0].correlation_id) == ActionState::Failed
    })
    .await;
    until("the forgery's quarantine", || {
        !rows(&r.core, "completion.quarantined").is_empty()
    })
    .await;
    assert_eq!(
        state_of(&r.core, &specs[1].correlation_id),
        ActionState::Dispatched
    );
    r.state.push(signed(&specs[1], "succeeded", 0));
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Failed
    })
    .await;
    let s = &rows(&r.core, "aws.hands.settled")[0];
    assert_eq!(
        (s["met"].as_bool(), s["failed"].as_u64()),
        (Some(false), Some(1))
    );
}

/// Fargate needs the NAT: with it off, the call fails saying so and what
/// turns it on, and nothing is launched. With it on, the group's task
/// definition and each RunTask carry the tags; the task runs in the private
/// subnets with no public IP, `startedBy` its group, its spec in its
/// environment. A task that stops with its hand failing makes the hand
/// unknown, and its envelope, when it comes, resolves it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[expect(clippy::too_many_lines, reason = "one scenario, start to end")]
async fn fargate_waits_for_the_nat_and_runs_tagged_in_private_subnets() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "aws_hands_run",
                json!({"argv": ["true"], "backend": "fargate"}),
            )],
        ),
        Scripted::text("No NAT."),
    ]);
    let res = turn(&r.core, "run on fargate").await;
    let text = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult { tool, content, .. } if tool == "aws.hands.run" => {
                Some(content.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(
        text.contains("NAT, which is off") && text.contains("NatGateway=enabled"),
        "{text}"
    );
    assert!(
        r.state.ran.lock().unwrap().is_empty() && r.state.registered.lock().unwrap().is_empty()
    );

    // With the NAT on (a new daemon reads the stacks again).
    r.state.nat.store(true, Ordering::SeqCst);
    let dir2 = tempfile::tempdir().unwrap();
    let core = core_at(
        dir2.path(),
        &r.fake.url,
        vec![
            Scripted::tools(
                "",
                &[(
                    "t2",
                    "aws_hands_run",
                    json!({"argv": ["true"], "image": "example.invalid/tools:1", "count": 2, "ttl_secs": 3600}),
                )],
            ),
            Scripted::text("Running."),
        ],
    );
    turn(&core, "run on fargate").await;
    let g = group_of(&core);
    let td = r.state.registered.lock().unwrap().clone();
    assert_eq!(td.len(), 1, "one task definition a group");
    let defs = td[0]["containerDefinitions"].as_array().unwrap();
    assert_eq!(defs.len(), 2, "the init container, then the image");
    assert_eq!(defs[1]["image"], "example.invalid/tools:1");
    assert_eq!(defs[1]["entryPoint"], json!(["/hand/theseusd"]));
    assert_eq!(defs[1]["dependsOn"][0]["condition"], "SUCCESS");
    assert_eq!(
        td[0]["taskRoleArn"],
        format!("arn:aws:iam::{ACCOUNT}:role/theseus-hand-basic")
    );
    let ran = r.state.ran.lock().unwrap().clone();
    assert_eq!(ran.len(), 2);
    let rec = record(&core, &g);
    for (i, t) in ran.iter().enumerate() {
        assert_eq!(t["launchType"], "FARGATE");
        assert_eq!(t["startedBy"], g);
        let net = &t["networkConfiguration"]["awsvpcConfiguration"];
        assert_eq!(net["assignPublicIp"], "DISABLED");
        assert_eq!(
            net["subnets"],
            json!(["subnet-0example0a", "subnet-0example0b"])
        );
        let tags: BTreeMap<String, String> = t["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                (
                    t["key"].as_str().unwrap().into(),
                    t["value"].as_str().unwrap().into(),
                )
            })
            .collect();
        assert_eq!(tags["theseus:owner"], "theseus");
        assert_eq!(tags["theseus:group"], g);
        assert_eq!(tags["theseus:deployment"], "theseus-example");
        assert_eq!(tags["theseus:execution"], rec.execution_id);
        assert_eq!(tags["theseus:session"], rec.session_id);
        assert!(
            tags["theseus:ttl"].ends_with('Z'),
            "{}",
            tags["theseus:ttl"]
        );
        let corr = &tags["theseus:correlation"];
        assert!(rec.hands.iter().any(|h| &h.correlation_id == corr));
        let env = &t["overrides"]["containerOverrides"][0]["environment"][0];
        assert_eq!(env["name"], "THESEUS_HAND");
        let spec: HandSpec = serde_json::from_str(env["value"].as_str().unwrap()).unwrap();
        assert_eq!(
            (spec.backend.as_str(), spec.deadline_secs),
            ("fargate", 3570)
        );
        assert_eq!(&spec.correlation_id, corr);
        let h = rec
            .hands
            .iter()
            .find(|h| &h.correlation_id == corr)
            .unwrap();
        let arn = format!(
            "arn:aws:ecs:us-west-2:{ACCOUNT}:task/theseus-hands/t{}",
            i + 1
        );
        assert_eq!(h.external_op_id.as_deref(), Some(arn.as_str()));
    }
    // Its task stops with the hand failing: unknown, then resolved.
    let stopped = |task: &str, exit: i64| {
        json!({"source": "aws.ecs", "detail-type": "ECS Task State Change",
            "detail": {"lastStatus": "STOPPED", "startedBy": g, "taskArn": task,
                "stoppedReason": "Essential container in task exited",
                "containers": [{"name": "hand", "exitCode": exit}]}})
        .to_string()
    };
    let ran0: HandSpec = serde_json::from_str(
        ran[0]["overrides"]["containerOverrides"][0]["environment"][0]["value"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let h0 = rec
        .hands
        .iter()
        .find(|h| h.correlation_id == ran0.correlation_id)
        .unwrap()
        .clone();
    core.poll_hands_after_serving();
    r.state
        .push(stopped(h0.external_op_id.as_deref().unwrap(), 137));
    until("the hand unknown", || {
        state_of(&core, &h0.correlation_id) == ActionState::OutcomeUnknown
    })
    .await;
    r.state.push(signed(&ran0, "succeeded", 0));
    until("the hand resolved", || {
        state_of(&core, &h0.correlation_id) == ActionState::Succeeded
    })
    .await;
    assert_eq!(rows(&core, "action.resolved").len(), 1);
}

/// Whether a frame holds the group's record.
fn holds_group(records: &[theseus_store::NewRecord]) -> bool {
    records.iter().any(|x| {
        x.kind == theseus_store::kinds::META
            && x.key
                .as_deref()
                .is_some_and(|k| k.starts_with(super::group::PREFIX))
    })
}

/// One call is one frame for its group: the record, and every hand's
/// action, before anything launches. Since Tier 7.3 (theseus-kpfv) a frame
/// keeps each action's last copy, so each hand is there once: the first
/// wave dispatched, the rest authorized. A failed frame writes none of it,
/// and launches nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_groups_record_and_its_hands_are_one_frame() {
    let r = rig(calls(
        json!({"argv": ["true"], "count": 3, "concurrency": 2}),
    ));
    // A spy: the frame that writes the group's record, by its records.
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let spy = seen.clone();
    r.core.store.fail_turn_frame(move |records| {
        if holds_group(records) {
            for x in records
                .iter()
                .filter(|x| x.kind == theseus_store::kinds::ACTION)
            {
                let a: Value = serde_json::from_slice(&x.payload).unwrap();
                spy.lock().unwrap().push((
                    a["tool"].as_str().unwrap_or_default().into(),
                    a["state"].as_str().unwrap_or_default().into(),
                ));
            }
        }
        false
    });
    turn(&r.core, "run three hands").await;
    let mut states = seen.lock().unwrap().clone();
    states.sort();
    states.dedup();
    // Each hand's last state, all in the one frame: the first two
    // dispatched, the third authorized (a frame keeps each record's last copy).
    assert_eq!(
        states,
        [
            ("aws.hand".to_string(), "authorized".to_string()),
            ("aws.hand".to_string(), "dispatched".to_string()),
        ]
    );
    let n = |s: &str| {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(_, st)| st == s)
            .count()
    };
    assert_eq!((n("planned"), n("authorized"), n("dispatched")), (0, 1, 2));

    // That frame fails: nothing of the group is written, nothing launched.
    let r = rig(calls(json!({"argv": ["true"], "count": 3})));
    r.core.store.fail_turn_frame(holds_group);
    let failed = try_turn(&r.core, "run three hands").await;
    assert!(failed.is_err(), "the turn whose frame failed fails");
    let hands = r
        .core
        .kernel
        .actions_by(&[theseus_kernel::terms::prefix("s:")])
        .unwrap();
    assert!(hands.iter().all(|a| a.tool != "aws.hand"), "{hands:?}");
    assert!(r.state.invoked().is_empty());
    assert!(rows(&r.core, "aws.hands.launched").is_empty());
    let metas = r
        .core
        .store
        .inner()
        .latest_with_prefix(theseus_store::kinds::META, super::group::PREFIX)
        .unwrap();
    assert!(metas.is_empty());
}
