//! The AWS tools against a local fake endpoint (row 29, C1): what each tool
//! sends and returns, the account's check and its failing closed, the gate's
//! `[policy.aws]`, a write that is invalid input with nothing sent, and AWS
//! calls through the whole core, with their `aws.called` rows and spans.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{AwsPlan, Span};
use theseus_tools::{AsyncResult, AwsBinding, AwsRequest, Plan, Tool, ToolClass, ToolCtx};

use super::{Aws, BIND_WAIT};
use crate::config::{AwsAccountConfig, AwsConfig};
use crate::policy::{Posture, ToolPolicy};
use crate::secrets::{Secret, SecretBoard};

/// The account the tests bind: AWS's documentation's example id.
pub(super) const ACCOUNT: &str = "111122223333";
pub(super) const KEY_ID: &str = "AKIDTESTEXAMPLE0001";
pub(super) const SECRET: &str = "test-secret-not-a-key-0001";

// ------------------------------------------------------------------ the fake

/// A request as the fake saw it.
#[derive(Clone, Debug)]
pub(super) struct Seen {
    pub(super) method: String,
    /// The path and query, as sent.
    pub(super) target: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: String,
}

impl Seen {
    pub(super) fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }

    /// The region it was signed for: its credential scope's.
    pub(super) fn region(&self) -> Option<&str> {
        let auth = self.header("authorization")?;
        let cred = auth.split("Credential=").nth(1)?;
        cred.split('/').nth(2)
    }

    /// A query protocol's action (`GetCallerIdentity`).
    pub(super) fn action(&self) -> Option<&str> {
        self.body
            .split('&')
            .find_map(|kv| kv.strip_prefix("Action="))
    }

    /// The S3 operation a GET names: `ListBuckets`, or `ListObjectsV2`.
    fn s3(&self) -> Option<(&'static str, Option<String>)> {
        if self.method != "GET" {
            return None;
        }
        let (path, query) = self.target.split_once('?').unwrap_or((&self.target, ""));
        let bucket = path.trim_start_matches('/');
        if bucket.is_empty() {
            Some(("ListBuckets", None))
        } else if query.contains("list-type=2") {
            Some(("ListObjectsV2", Some(bucket.split('/').next()?.to_string())))
        } else {
            None
        }
    }
}

/// A reply: a status, its headers, and its body.
pub(super) type Reply = (u16, Vec<(&'static str, String)>, String);

/// A local stand-in for AWS on 127.0.0.1: its listener is bound before any
/// client connects (a connect to a port nothing listens on hangs here), it
/// answers each request with `answer`'s reply and closes, and it keeps every
/// request in arrival order.
pub(super) struct Fake {
    pub(super) url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    pub(super) fn start(answer: impl Fn(&Seen, usize) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let (kept, answer) = (seen.clone(), Arc::new(answer));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (kept, answer) = (kept.clone(), answer.clone());
                std::thread::spawn(move || {
                    let _ = serve(stream, &kept, &*answer);
                });
            }
        });
        Fake { url, seen }
    }

    pub(super) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

fn serve(
    stream: TcpStream,
    kept: &Mutex<Vec<Seen>>,
    answer: &(dyn Fn(&Seen, usize) -> Reply + Send + Sync),
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default().to_string(),
        parts.next().unwrap_or_default().to_string(),
    );
    let mut headers = Vec::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let seen = Seen {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let n = {
        let mut k = kept.lock().unwrap();
        k.push(seen.clone());
        k.len()
    };
    let (status, headers, body) = answer(&seen, n);
    let mut out = format!(
        "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(&body);
    let mut w = stream;
    w.write_all(out.as_bytes())?;
    w.flush()
}

/// STS's answer: the key is `account`'s.
pub(super) fn sts(account: &str, n: usize) -> Reply {
    let id = format!("req-{n}");
    let body = format!(
        "<GetCallerIdentityResponse><GetCallerIdentityResult>\
         <Arn>arn:aws:iam::{account}:user/example</Arn><UserId>AIDATESTEXAMPLE</UserId>\
         <Account>{account}</Account></GetCallerIdentityResult>\
         <ResponseMetadata><RequestId>{id}</RequestId></ResponseMetadata></GetCallerIdentityResponse>"
    );
    (
        200,
        vec![
            ("x-amzn-requestid", id),
            ("content-type", "text/xml".into()),
        ],
        body,
    )
}

/// AWS as the tests need it: STS, CloudFormation's `DescribeStacks`, and S3's
/// listings of two buckets in `us-west-2`, one with two objects and a
/// folder under `logs/`.
fn aws_answers(account: &'static str) -> impl Fn(&Seen, usize) -> Reply + Send + Sync {
    move |s, n| {
        let id = format!("req-{n}");
        let xml = |body: String| {
            (
                200,
                vec![
                    ("x-amzn-requestid", id.clone()),
                    ("x-amz-request-id", id.clone()),
                    ("content-type", "text/xml".into()),
                ],
                body,
            )
        };
        match (s.action(), s.s3()) {
            (Some("GetCallerIdentity"), _) => sts(account, n),
            (Some("DescribeStacks"), _) => xml(format!(
                "<DescribeStacksResponse><DescribeStacksResult><Stacks><member>\
                 <StackName>example-stack</StackName>\
                 <StackId>arn:aws:cloudformation:us-west-2:{account}:stack/example-stack/1</StackId>\
                 <CreationTime>2026-09-30T12:00:00Z</CreationTime>\
                 <StackStatus>CREATE_COMPLETE</StackStatus></member></Stacks></DescribeStacksResult>\
                 <ResponseMetadata><RequestId>{id}</RequestId></ResponseMetadata></DescribeStacksResponse>"
            )),
            (_, Some(("ListBuckets", _))) => xml(
                "<ListAllMyBucketsResult><Buckets>\
                 <Bucket><Name>example-bucket</Name><CreationDate>2026-01-02T03:04:05.000Z</CreationDate><BucketRegion>us-west-2</BucketRegion></Bucket>\
                 <Bucket><Name>far-bucket</Name><CreationDate>2026-02-03T04:05:06.000Z</CreationDate><BucketRegion>eu-west-1</BucketRegion></Bucket>\
                 </Buckets><Owner><ID>owner-example</ID></Owner></ListAllMyBucketsResult>"
                    .into(),
            ),
            (_, Some(("ListObjectsV2", Some(b)))) if b == "far-bucket" && s.region() != Some("eu-west-1") => (
                301,
                vec![("x-amz-request-id", id.clone()), ("x-amz-bucket-region", "eu-west-1".into())],
                "<Error><Code>PermanentRedirect</Code><Message>The bucket you are attempting to access must be addressed using the specified endpoint.</Message></Error>".into(),
            ),
            (_, Some(("ListObjectsV2", Some(b)))) => xml(format!(
                "<ListBucketResult><Name>{b}</Name><Prefix>logs/</Prefix><KeyCount>3</KeyCount>\
                 <MaxKeys>200</MaxKeys><Delimiter>/</Delimiter><IsTruncated>false</IsTruncated>\
                 <Contents><Key>logs/a.txt</Key><LastModified>2026-09-30T12:00:00.000Z</LastModified><ETag>\"1\"</ETag><Size>1024</Size><StorageClass>STANDARD</StorageClass></Contents>\
                 <Contents><Key>logs/b.txt</Key><LastModified>2026-10-01T08:00:00.000Z</LastModified><ETag>\"2\"</ETag><Size>2048</Size><StorageClass>STANDARD</StorageClass></Contents>\
                 <CommonPrefixes><Prefix>logs/2026/</Prefix></CommonPrefixes></ListBucketResult>"
            )),
            _ => (
                400,
                vec![("x-amzn-requestid", id)],
                "<ErrorResponse><Error><Type>Sender</Type><Code>InvalidAction</Code><Message>the fake does not know it</Message></Error></ErrorResponse>".into(),
            ),
        }
    }
}

// ------------------------------------------------------------------ the rig

/// A board whose AWS key has resolved.
pub(super) fn board() -> Arc<SecretBoard> {
    let b = SecretBoard::new(
        [
            "aws_access_key_id".to_string(),
            "aws_secret_access_key".to_string(),
        ],
        Instant::now(),
    );
    b.publish(
        BTreeMap::from([
            (
                "aws_access_key_id".to_string(),
                Ok(Secret::new(KEY_ID.into())),
            ),
            (
                "aws_secret_access_key".to_string(),
                Ok(Secret::new(SECRET.into())),
            ),
        ]),
        "test",
    );
    b
}

/// The account, its endpoint the fake, in `us-west-2`, which may name
/// `eu-west-1` too.
pub(super) fn account(endpoint: &str) -> AwsConfig {
    AwsConfig {
        accounts: BTreeMap::from([(
            ACCOUNT.to_string(),
            AwsAccountConfig {
                credentials: Default::default(),
                region: "us-west-2".into(),
                regions: vec!["us-west-2".into(), "eu-west-1".into()],
                endpoint: Some(endpoint.into()),
                owner_role: None,
                deployment: None,
                monthly_budget_usd: None,
                daily_budget_usd: None,
                hourly_alert_usd: crate::config::default_hourly_alert_usd(),
                durability: false,
            },
        )]),
    }
}

pub(super) fn layer(fake: &Fake, board: Arc<SecretBoard>) -> Arc<Aws> {
    Aws::from_config(&account(&fake.url), board).expect("an account")
}

pub(super) fn tool(aws: &Arc<Aws>, name: &str) -> Arc<dyn Tool> {
    aws.tools()
        .into_iter()
        .find(|t| t.name() == name)
        .unwrap_or_else(|| panic!("no tool {name}"))
}

pub(super) fn plain_ctx() -> ToolCtx {
    ToolCtx::for_tests(&std::env::temp_dir())
}

/// One call of an AWS tool, planned then run, as the runtime runs it: with a
/// binding for execution `exe_test` and correlation id `act_test_1`. What
/// it returned, and the requests it made.
pub(super) async fn call(
    aws: &Arc<Aws>,
    name: &str,
    input: Value,
) -> (AsyncResult, Vec<AwsRequest>) {
    let t = tool(aws, name);
    let b = aws.bind("exe_test", "act_test_1", "toolu_test_1");
    let ctx = ToolCtx {
        aws: Some(b.clone()),
        ..plain_ctx()
    };
    t.plan(&input, &ctx).expect("the call plans");
    let r = t.run_async(&input, &ctx).await;
    (r, b.requests())
}

pub(super) fn no_secret_in(v: &str) {
    assert!(!v.contains(SECRET) && !v.contains(KEY_ID), "a key in {v}");
}

// ------------------------------------------------------------------ the tests

/// FAST (§3.10): building the accounts and planning every tool does nothing
/// on the network; `aws.describe` answers from the local catalog; the
/// account is unchecked until a call or the daemon's check.
#[tokio::test]
async fn nothing_reaches_aws_until_a_call() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    for (name, input) in [
        (
            "aws.call",
            json!({"service": "cloudformation", "operation": "DescribeStacks"}),
        ),
        ("aws.s3.list", json!({"path": "s3://example-bucket/logs/"})),
        ("aws.whoami", json!({})),
        ("aws.describe", json!({"service": "s3"})),
    ] {
        tool(&aws, name)
            .plan(&input, &plain_ctx())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let d = tool(&aws, "aws.describe");
    let all = d.run(&json!({}), &plain_ctx()).unwrap().text;
    assert!(
        all.contains("AWS services") && all.contains("cloudformation, "),
        "{all}"
    );
    let s3 = d.run(&json!({"service": "s3"}), &plain_ctx()).unwrap().text;
    assert!(
        s3.contains("read (") && s3.contains("ListObjectsV2"),
        "{s3}"
    );
    let op = d
        .run(
            &json!({"service": "s3", "operation": "list-objects-v2"}),
            &plain_ctx(),
        )
        .unwrap()
        .text;
    assert!(
        op.contains("\"Bucket\"") && op.contains("\"class\": \"read\""),
        "{op}"
    );
    let e = d
        .plan(&json!({"operation": "ListBuckets"}), &plain_ctx())
        .unwrap_err();
    assert!(e.contains("needs its service"), "{e}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        fake.seen().is_empty(),
        "nothing was sent: {:?}",
        fake.seen()
    );
    assert_eq!(aws.status().accounts[0].state, "unchecked");
}

/// `aws.whoami` asks STS now. Before it, the account's check (its own
/// `GetCallerIdentity`) bound the key; the call's user agent names its
/// execution and its correlation id, as CloudTrail keeps it; its row has
/// AWS's request id, and no key.
#[tokio::test]
async fn whoami_asks_aws_now_after_the_check_and_names_its_call() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    let (r, reqs) = call(&aws, "aws.whoami", json!({})).await;
    let (out, external) = r.unwrap_or_else(|f| panic!("{}", f.message));
    assert!(
        external.is_none(),
        "control-plane reads are not external text"
    );
    for want in [
        "AWS account 111122223333",
        "arn:aws:iam::111122223333:user/example",
        "AIDATESTEXAMPLE",
        "the regions a call may name: us-west-2, eu-west-1",
        "signs with: its key (no owner_role yet: theseus aws bootstrap makes it)",
        "budget: not read yet",
        "its check: bound",
        "(request req-2)",
    ] {
        assert!(out.text.contains(want), "{want:?} in {}", out.text);
    }
    let seen = fake.seen();
    assert_eq!(
        seen.iter().map(|s| s.action()).collect::<Vec<_>>(),
        [Some("GetCallerIdentity"), Some("GetCallerIdentity")]
    );
    let ua = |i: usize| seen[i].header("user-agent").unwrap_or_default().to_string();
    assert!(
        ua(0).contains("call/check-111122223333") && !ua(0).contains("exec/"),
        "{}",
        ua(0)
    );
    assert!(
        ua(1).contains("exec/exe_test") && ua(1).contains("call/act_test_1"),
        "{}",
        ua(1)
    );
    assert!(
        ua(1).starts_with(&format!("theseus/{}", crate::VERSION)),
        "{}",
        ua(1)
    );
    assert!(seen.iter().all(|s| s.region() == Some("us-west-2")));
    assert!(seen.iter().all(|s| s
        .header("authorization")
        .is_some_and(|a| a.contains(KEY_ID))));
    assert_eq!(reqs.len(), 1, "the check is the account's, not the call's");
    let row = &reqs[0].row;
    for (k, v) in [
        ("account", "111122223333"),
        ("region", "us-west-2"),
        ("service", "sts"),
        ("operation", "GetCallerIdentity"),
        ("class", "read"),
        ("execution_id", "exe_test"),
        ("correlation_id", "act_test_1"),
        ("request_id", "req-2"),
        ("status", "ok"),
    ] {
        assert_eq!(row[k], v, "{k} in {row}");
    }
    no_secret_in(&row.to_string());
    no_secret_in(&out.text);
    let s = &aws.status().accounts[0];
    assert_eq!((s.state.as_str(), s.calls, s.failed), ("bound", 2, 0));
    assert_eq!(
        s.arn.as_deref(),
        Some("arn:aws:iam::111122223333:user/example")
    );
}

/// `aws.call` makes any read: CloudFormation's `DescribeStacks` comes back as
/// its output's JSON, with AWS's request id. Its plan is a read, with the
/// account, region, and operation the gate and the surfaces read.
#[tokio::test]
async fn aws_call_reads_any_service_and_returns_its_output() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    let input = json!({"service": "cloudformation", "operation": "DescribeStacks"});
    let plan = tool(&aws, "aws.call").plan(&input, &plain_ctx()).unwrap();
    assert_eq!(plan.class, Some(ToolClass::Read));
    assert_eq!(
        plan.aws,
        Some(AwsPlan {
            account: ACCOUNT.into(),
            region: "us-west-2".into(),
            service: "cloudformation".into(),
            operation: "DescribeStacks".into(),
            ..Default::default()
        })
    );
    assert_eq!(
        plan.summary,
        "read cloudformation:DescribeStacks in us-west-2"
    );
    let (r, reqs) = call(&aws, "aws.call", input).await;
    let (out, _) = r.unwrap_or_else(|f| panic!("{}", f.message));
    assert!(
        out.text.starts_with(
            "cloudformation:DescribeStacks in us-west-2, account 111122223333: HTTP 200 · request req-2 · 1 page\n"
        ),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("\"StackName\": \"example-stack\""),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("\"CreationTime\": \"2026-09-30T12:00:00Z\""),
        "{}",
        out.text
    );
    assert_eq!(out.meta["request_id"], "req-2");
    assert_eq!(out.meta["operation"], "cloudformation:DescribeStacks");
    assert_eq!(fake.seen()[1].action(), Some("DescribeStacks"));
    assert_eq!(reqs[0].row["request_id"], "req-2");
    // The service's other names, and the operation's, find the same call.
    let alias =
        json!({"service": "CloudFormation", "operation": "describe-stacks", "region": "eu-west-1"});
    let p = tool(&aws, "aws.call").plan(&alias, &plain_ctx()).unwrap();
    let a = p.aws.unwrap();
    assert_eq!(
        (a.service.as_str(), a.operation.as_str(), a.region.as_str()),
        ("cloudformation", "DescribeStacks", "eu-west-1")
    );
}

/// Until 14b, `aws.call` makes reads only: a write, a call that runs code,
/// and a secret-bearing read are invalid input that names the step that
/// brings them, and nothing is sent. So are an unknown operation, a bad
/// input, and a region the account does not allow; an event stream says
/// the CLI can make it. Since C2 (14b) a write and a run plan; durable
/// infrastructure and a stack's own writes are what stay invalid input.
#[tokio::test]
async fn iac_and_a_stack_write_are_invalid_input_and_nothing_is_sent() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    let t = tool(&aws, "aws.call");
    for (input, says) in [
        (
            json!({"service": "ec2", "operation": "RunInstances", "input": {"ImageId": "ami-1", "MinCount": 1, "MaxCount": 1}}),
            "ec2:RunInstances makes or changes durable infrastructure; use aws.stack.plan",
        ),
        (
            json!({"service": "cloudformation", "operation": "DeleteStack", "input": {"StackName": "example-stack"}}),
            "cloudformation:DeleteStack writes a stack outside the review; use aws.stack.plan",
        ),
        (
            json!({"service": "ec2", "operation": "DescribeNothing"}),
            "ec2 has no operation \"DescribeNothing\"",
        ),
        (
            json!({"service": "nosuchservice", "operation": "ListThings"}),
            "no AWS service is named \"nosuchservice\"",
        ),
        (
            json!({"service": "s3", "operation": "ListObjectsV2", "input": {}}),
            "Bucket",
        ),
        (
            json!({"service": "s3", "operation": "ListObjectsV2", "input": {"Bucket": "b", "Nope": 1}}),
            "Nope",
        ),
        (
            json!({"service": "sts", "operation": "GetCallerIdentity", "region": "ap-south-1"}),
            "ap-south-1 is not one of account 111122223333's regions (us-west-2, eu-west-1)",
        ),
        (
            json!({"service": "sts", "operation": "GetCallerIdentity", "input": []}),
            "input must be an object",
        ),
        (
            json!({"service": "sts", "operation": "GetCallerIdentity", "pages": 11}),
            "pages must be 1 to 10",
        ),
        (
            json!({"service": "sts", "operation": "GetCallerIdentity", "account": "444455556666"}),
            "no AWS account \"444455556666\" is bound",
        ),
        (
            json!({"service": "logs", "operation": "StartLiveTail", "input": {"logGroupIdentifiers": ["g"]}}),
            "the aws CLI can, through proc_run",
        ),
    ] {
        let e = t.plan(&input, &plain_ctx()).unwrap_err();
        assert!(e.contains(says), "{input}: {e}");
    }
    // A secret-bearing read plans as a read (14c holds its value as a handle).
    for ok in [
        json!({"service": "ssm", "operation": "GetParameter", "input": {"Name": "p"}}),
        json!({"service": "secretsmanager", "operation": "GetSecretValue", "input": {"SecretId": "s"}}),
    ] {
        t.plan(&ok, &plain_ctx()).unwrap();
    }
    // A run that skipped the plan would still check it again, and send nothing.
    let f = t
        .run_async(
            &json!({"service": "s3", "operation": "CreateBucket", "input": {"Bucket": "b-1"}}),
            &plain_ctx(),
        )
        .await
        .unwrap_err();
    assert!(f.message.contains("use aws.stack.plan"), "{}", f.message);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        fake.seen().is_empty(),
        "nothing was sent: {:?}",
        fake.seen()
    );
}

/// `aws.s3.list`: the buckets with their regions; the folders and objects
/// under a prefix, with a summary; every key with `recursive`; and a bucket
/// in another region the account allows, found through its listing of its
/// buckets and listed there.
#[tokio::test]
async fn s3_list_names_buckets_and_what_is_under_a_prefix() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    let (r, _) = call(&aws, "aws.s3.list", json!({})).await;
    let text = r.unwrap_or_else(|f| panic!("{}", f.message)).0.text;
    assert!(
        text.starts_with("2 S3 buckets in account 111122223333 (request req-2):\n"),
        "{text}"
    );
    assert!(
        text.contains("  example-bucket · us-west-2 · made 2026-01-02T03:04:05Z\n"),
        "{text}"
    );
    assert!(text.contains("  far-bucket · eu-west-1 · made"), "{text}");

    let (r, reqs) = call(
        &aws,
        "aws.s3.list",
        json!({"path": "s3://example-bucket/logs/"}),
    )
    .await;
    let text = r.unwrap_or_else(|f| panic!("{}", f.message)).0.text;
    assert!(
        text.starts_with(
            "s3://example-bucket/logs/ in us-west-2 (request req-3): 1 folder and 2 objects (3,072 bytes), the newest 2026-10-01T08:00:00Z\n"
        ),
        "{text}"
    );
    assert!(text.contains("folders:\n  logs/2026/\n"), "{text}");
    assert!(
        text.contains("  logs/b.txt · 2,048 · 2026-10-01T08:00:00Z\n"),
        "{text}"
    );
    let get = fake.seen()[2].clone();
    assert!(get.target.starts_with("/example-bucket?"), "{}", get.target);
    for q in [
        "list-type=2",
        "prefix=logs%2F",
        "delimiter=%2F",
        "max-keys=200",
    ] {
        assert!(get.target.contains(q), "{q} in {}", get.target);
    }
    assert_eq!(reqs[0].row["operation"], "ListObjectsV2");

    let (r, _) = call(
        &aws,
        "aws.s3.list",
        json!({"path": "example-bucket", "recursive": true, "max": 3000}),
    )
    .await;
    r.unwrap_or_else(|f| panic!("{}", f.message));
    let all = fake.seen()[3].clone();
    assert!(
        !all.target.contains("delimiter") && all.target.contains("max-keys=1000"),
        "{}",
        all.target
    );

    // far-bucket lives in eu-west-1: S3 answers 301 in us-west-2, the
    // account's listing names its region, and the listing goes there.
    let before = fake.seen().len();
    let (r, reqs) = call(
        &aws,
        "aws.s3.list",
        json!({"path": "s3://far-bucket/logs/"}),
    )
    .await;
    let text = r.unwrap_or_else(|f| panic!("{}", f.message)).0.text;
    assert!(
        text.starts_with("s3://far-bucket/logs/ in eu-west-1"),
        "{text}"
    );
    let regions: Vec<_> = fake.seen()[before..]
        .iter()
        .map(|s| s.region().map(String::from))
        .collect();
    assert_eq!(
        regions,
        [
            Some("us-west-2".into()),
            Some("us-west-2".into()),
            Some("eu-west-1".into())
        ]
    );
    let statuses: Vec<_> = reqs.iter().map(|r| r.row["status"].clone()).collect();
    assert_eq!(statuses, [json!("error"), json!("ok"), json!("ok")]);
    assert_eq!(reqs[0].row["error_code"], "PermanentRedirect");

    for (input, says) in [
        (
            json!({"path": "s3://Bad_Bucket/x"}),
            "is not an S3 bucket's name",
        ),
        (json!({"max": 0}), "max must be 1 to 5000"),
        (
            json!({"path": "x", "region": "ap-south-1"}),
            "not one of account",
        ),
    ] {
        let e = tool(&aws, "aws.s3.list")
            .plan(&input, &plain_ctx())
            .unwrap_err();
        assert!(e.contains(says), "{input}: {e}");
    }
}

/// Fail closed (§3.5): a key that STS says is another account's binds
/// nothing. The call is not sent, its row says so, and health says why.
#[tokio::test]
async fn a_key_of_another_account_fails_closed_and_health_says_why() {
    let fake = Fake::start(|s, n| match s.action() {
        Some("GetCallerIdentity") => sts("444455556666", n),
        _ => (500, vec![], String::new()),
    });
    let aws = layer(&fake, board());
    let (r, reqs) = call(
        &aws,
        "aws.call",
        json!({"service": "cloudformation", "operation": "DescribeStacks"}),
    )
    .await;
    let f = r.unwrap_err();
    assert!(
        f.message.contains(
            "cloudformation:DescribeStacks in us-west-2 was not sent: AWS account 111122223333 is not bound: its key is account 444455556666's, not 111122223333"
        ),
        "{}",
        f.message
    );
    assert_eq!(fake.seen().len(), 1, "the check alone");
    assert_eq!(reqs[0].row["status"], "unbound");
    assert_eq!(reqs[0].row["sent"], false);
    let s = &aws.status().accounts[0];
    assert_eq!(s.state, "failed");
    assert!(
        s.error
            .as_deref()
            .unwrap_or_default()
            .contains("is account 444455556666's"),
        "{s:?}"
    );
    // A second call does not check again: the key will not change.
    let (r, _) = call(&aws, "aws.whoami", json!({})).await;
    assert!(r.is_err());
    assert_eq!(fake.seen().len(), 1);
}

/// A call before the key resolves waits for it at most `BIND_WAIT`, then
/// fails closed, saying the check waits for the key and why; nothing is
/// sent (§3.10). On tokio's paused clock, so the wait costs nothing.
#[tokio::test(start_paused = true)]
async fn a_call_waits_for_the_key_then_fails_closed() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let board = SecretBoard::new(
        [
            "aws_access_key_id".to_string(),
            "aws_secret_access_key".to_string(),
        ],
        Instant::now(),
    );
    board.publish(
        BTreeMap::from([
            (
                "aws_access_key_id".to_string(),
                Err("the vault said no".to_string()),
            ),
            (
                "aws_secret_access_key".to_string(),
                Err("the vault said no".to_string()),
            ),
        ]),
        "test",
    );
    let aws = layer(&fake, board);
    let t0 = tokio::time::Instant::now();
    let (r, reqs) = call(&aws, "aws.whoami", json!({})).await;
    assert!(t0.elapsed() >= BIND_WAIT, "it waited {:?}", t0.elapsed());
    let f = r.unwrap_err();
    assert!(
        f.message.contains("is not bound yet: its check is waiting after 30 s (its secret aws_access_key_id did not resolve (the vault said no); the vault is asked again)"),
        "{}",
        f.message
    );
    assert_eq!(reqs[0].row["status"], "unbound");
    assert!(fake.seen().is_empty());
    let s = &aws.status().accounts[0];
    assert_eq!(s.state, "waiting");
}

/// The gate (§3.9): an AWS call with no `[policy.tools]` line takes its
/// operation's `[policy.aws]` line, then its service's, then `read`'s, then
/// `enforcement`; a `[policy.tools]` line wins over all of them. The tools'
/// postures, as the system note and the tool list show them, are `read`'s.
#[test]
fn policy_aws_gives_each_call_its_posture() {
    let policy = |aws: &[(&str, Posture)], tools: &[(&str, Posture)]| ToolPolicy {
        roots: vec![],
        approve_paths: vec![],
        allow_argv: vec![],
        approve_argv: vec![],
        enforcement: Posture::Notify,
        tools: tools.iter().map(|(k, p)| (k.to_string(), *p)).collect(),
        mcp: BTreeMap::new(),
        aws: aws.iter().map(|(k, p)| (k.to_string(), *p)).collect(),
        confirmer: "operator".into(),
        floor_paths: vec![],
        floor_argv: crate::policy::floor_argv(),
    };
    let plan = |service: &str, operation: &str| Plan {
        summary: format!("read {service}:{operation}"),
        class: Some(ToolClass::Read),
        aws: Some(AwsPlan {
            account: ACCOUNT.into(),
            region: "us-west-2".into(),
            service: service.into(),
            operation: operation.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    let call = tool(&aws, "aws.call");
    let p = policy(
        &[
            ("read", Posture::Open),
            ("s3", Posture::Notify),
            ("s3:ListBuckets", Posture::Approve),
        ],
        &[],
    );
    for ((service, operation), posture, setting) in [
        (
            ("cloudformation", "DescribeStacks"),
            Posture::Open,
            "[policy.aws] read = open",
        ),
        (
            ("s3", "ListObjectsV2"),
            Posture::Notify,
            "[policy.aws] \"s3\" = notify",
        ),
        (
            ("s3", "ListBuckets"),
            Posture::Approve,
            "[policy.aws] \"s3:ListBuckets\" = approve",
        ),
    ] {
        let d = p.decide(call.as_ref(), &plan(service, operation));
        assert_eq!(d.posture, posture, "{service}:{operation}");
        assert!(d.reason.contains(setting), "{}", d.reason);
    }
    // A [policy.tools] line wins.
    let p = policy(
        &[("s3:ListBuckets", Posture::Open)],
        &[("aws.call", Posture::Approve)],
    );
    assert_eq!(
        p.decide(call.as_ref(), &plan("s3", "ListBuckets")).posture,
        Posture::Approve
    );
    // No line at all: enforcement.
    let p = policy(&[], &[]);
    let d = p.decide(call.as_ref(), &plan("s3", "ListBuckets"));
    assert_eq!(
        (d.posture, d.reason.contains("enforcement = notify")),
        (Posture::Notify, true)
    );
    // The tools' postures: read's, but aws.describe's is its own.
    let p = policy(&[("read", Posture::Open)], &[]);
    assert_eq!(
        p.posture("aws.whoami"),
        (Posture::Open, "[policy.aws] read = open".into())
    );
    assert_eq!(p.posture("aws.describe").0, Posture::Notify);
}

// ------------------------------------------------------------------ through the core

/// A core whose config binds the account at the fake, and whose model calls
/// what `script` says; its board holds the key.
pub(super) fn core(
    fake: &Fake,
    dir: &std::path::Path,
    script: Vec<crate::provider::Scripted>,
) -> Arc<crate::Core> {
    let root: PathBuf = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.policy.enforcement = Posture::Notify;
    cfg.aws = account(&fake.url);
    let store = crate::store::Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(crate::provider::FakeProvider::scripted(script));
    crate::Core::build(crate::rpc::Parts {
        secrets: board(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap()
}

pub(super) async fn turn(
    core: &Arc<crate::Core>,
    input: &str,
) -> theseus_protocol::TurnSubmitResult {
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
        .unwrap()
}

pub(super) fn ledgered(core: &crate::Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

fn find<'a>(s: &'a Span, pred: &dyn Fn(&Span) -> bool) -> Option<&'a Span> {
    if pred(s) {
        return Some(s);
    }
    s.children.iter().find_map(|c| find(c, pred))
}

/// Through the whole core: the model's `aws_call` runs open (`[policy.aws]
/// read`, the template's), its result is the output, its `aws.called` row
/// names the call's correlation id and AWS's request id, its span sits under
/// the call's own in the turn's trace, and health counts it. A write in the
/// next turn is invalid input: a `tool.invalid_input` row, and nothing sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn an_aws_call_through_the_core_is_a_row_a_span_and_a_result() {
    use crate::provider::Scripted;
    let fake = Fake::start(aws_answers(ACCOUNT));
    let dir = tempfile::tempdir().unwrap();
    let read = json!({"service": "cloudformation", "operation": "DescribeStacks"});
    let write = json!({"service": "s3", "operation": "CreateBucket", "input": {"Bucket": "theseus-scratch"}});
    let core = core(
        &fake,
        dir.path(),
        vec![
            Scripted::tools("", &[("t_read", "aws_call", read)]),
            Scripted::text("There is one stack."),
            Scripted::tools("", &[("t_write", "aws_call", write)]),
            Scripted::text("That is a write; it waits for 14b."),
        ],
    );
    let wire: Vec<String> = core
        .tools
        .definitions()
        .iter()
        .filter_map(|d| d["name"].as_str().map(String::from))
        .filter(|n| n.starts_with("aws_"))
        .collect();
    assert_eq!(
        wire,
        [
            "aws_call",
            "aws_cost",
            "aws_describe",
            "aws_hands_run",
            "aws_inventory",
            "aws_logs_query",
            "aws_logs_tail",
            "aws_s3_get",
            "aws_s3_list",
            "aws_s3_put",
            "aws_stack_apply",
            "aws_stack_delete",
            "aws_stack_plan",
            "aws_stack_status",
            "aws_trail",
            "aws_whoami"
        ]
    );
    let r = turn(&core, "what stacks are there?").await;
    assert_eq!(r.tool_calls, 1);
    let rows = ledgered(&core, "aws.called");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(
        (row["operation"].as_str(), row["status"].as_str()),
        (Some("DescribeStacks"), Some("ok"))
    );
    assert_eq!(row["execution_id"].as_str(), r.execution_id.as_deref());
    let called = ledgered(&core, "tool.notified");
    assert!(called.is_empty(), "an AWS read is open: {called:?}");
    // The call's correlation id is its tool-call node's.
    let corr = core
        .store
        .session_nodes(&r.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolCall {
                tool,
                correlation_id,
                ..
            } if tool == "aws.call" => correlation_id.clone(),
            _ => None,
        })
        .expect("a tool-call node");
    assert_eq!(row["correlation_id"], corr);
    let ua = fake.seen()[1]
        .header("user-agent")
        .unwrap_or_default()
        .to_string();
    assert!(ua.contains(&format!("call/{corr}")), "{ua}");
    // The span: under the call's, in OpenTelemetry's AWS names.
    let trace = r.trace.as_ref().expect("a trace");
    let call_span = find(trace, &|s| s.name == "tool aws_call").expect("the call's span");
    let aws_span = find(call_span, &|s| s.kind == "aws").expect("the request's span");
    assert_eq!(aws_span.name, "aws cloudformation:DescribeStacks");
    for (k, v) in [
        ("rpc.system", "aws-api"),
        ("rpc.service", "cloudformation"),
        ("rpc.method", "DescribeStacks"),
        ("aws.request_id", "req-2"),
        ("cloud.account.id", ACCOUNT),
        ("cloud.region", "us-west-2"),
        ("correlation_id", corr.as_str()),
    ] {
        assert_eq!(aws_span.attrs[k], v, "{k} in {}", aws_span.attrs);
    }
    assert!(aws_span.start_us >= call_span.start_us && aws_span.end_us <= call_span.end_us);
    let s = &core.health().aws.expect("health's aws").accounts[0];
    assert_eq!((s.state.as_str(), s.calls), ("bound", 2));
    // The result the model read.
    let result = core
        .store
        .session_nodes(&r.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult { tool, content, .. } if tool == "aws.call" => {
                Some(content.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(result.contains("example-stack"), "{result}");
    no_secret_in(&result);

    // Durable infrastructure: invalid input (C2's IaC-only), and nothing more is sent.
    let sent = fake.seen().len();
    let w = turn(&core, "make a bucket").await;
    let invalid = ledgered(&core, "tool.invalid_input");
    assert_eq!(invalid.len(), 1, "{invalid:?}");
    assert!(
        invalid[0]["reason"].as_str().unwrap_or_default().contains(
            "s3:CreateBucket makes or changes durable infrastructure; use aws.stack.plan"
        ),
        "{}",
        invalid[0]
    );
    assert_eq!(fake.seen().len(), sent);
    assert_eq!(ledgered(&core, "aws.called").len(), 1);
    assert_eq!(w.tool_calls, 1);
}

/// A binding's requests go to its call's trace once.
#[test]
fn a_calls_spans_are_taken_once() {
    let fake = Fake::start(aws_answers(ACCOUNT));
    let aws = layer(&fake, board());
    let b: Arc<AwsBinding> = aws.bind("exe_1", "act_1", "toolu_1");
    let t = Instant::now();
    b.record(AwsRequest {
        started: t,
        ended: t,
        row: json!({"service": "s3", "operation": "ListBuckets", "status": "ok", "request_id": "r1"}),
    });
    let spans = aws.spans("toolu_1", |_| 7);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].name, "aws s3:ListBuckets");
    assert_eq!((spans[0].start_us, spans[0].end_us), (7, Some(7)));
    assert!(aws.spans("toolu_1", |_| 7).is_empty());
    assert!(aws.spans("toolu_other", |_| 7).is_empty());
}
