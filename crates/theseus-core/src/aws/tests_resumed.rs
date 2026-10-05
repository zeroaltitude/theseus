//! An AWS call that runs after an approval (theseus-0zm4): its requests'
//! spans go under its call's span in the continuation that ran it, and
//! nothing is left in `Aws`'s list for it.

use std::sync::Arc;

use serde_json::json;

use super::tests::{account, board, sts, Fake, Reply, Seen};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::telemetry::tests_resumed::{approved, spans_named};

/// STS, and CloudFormation's `DescribeStacks` with one stack.
fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(super::tests::ACCOUNT, n);
    }
    let id = format!("req-{n}");
    let body = format!(
        "<DescribeStacksResponse><DescribeStacksResult><Stacks><member>\
         <StackName>example-stack</StackName><StackStatus>CREATE_COMPLETE</StackStatus>\
         </member></Stacks></DescribeStacksResult>\
         <ResponseMetadata><RequestId>{id}</RequestId></ResponseMetadata></DescribeStacksResponse>"
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

/// An AWS read under `[policy.aws] read = "approve"`, approved: the
/// continuation's span holds the call's, and its request's span is under it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approved_aws_calls_requests_are_spans_under_its_call_in_the_continuation() {
    let fake = Fake::start(answers);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.aws.insert("read".into(), Posture::Approve);
    cfg.aws = account(&fake.url);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let read = json!({"service": "cloudformation", "operation": "DescribeStacks"});
    let model = Arc::new(FakeProvider::scripted(vec![
        Scripted::tools("", &[("t_read", "aws_call", read)]),
        Scripted::text("There is one stack."),
    ]));
    let core = crate::Core::build(crate::rpc::Parts {
        secrets: board(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();

    let first = super::tests::turn(&core, "what stacks are there?").await;
    assert!(super::tests::ledgered(&core, "aws.called").is_empty());
    let cont = approved(&core, &first).await;
    assert_eq!(cont.output, "There is one stack.");
    let rows = super::tests::ledgered(&core, "aws.called");
    assert_eq!(rows.len(), 1, "{rows:?}");

    let trace = cont.trace.as_ref().unwrap();
    let calls = spans_named(trace, "tool aws_call");
    assert_eq!(calls.len(), 1, "{trace:#?}");
    let (parent, call) = calls[0];
    assert_eq!(parent, "continuation");
    assert_eq!(call.attrs["result"], "ok");
    let requests: Vec<_> = call.children.iter().filter(|c| c.kind == "aws").collect();
    assert_eq!(requests.len(), 1, "{call:#?}");
    let req = requests[0];
    assert_eq!(req.name, "aws cloudformation:DescribeStacks");
    assert_eq!(req.attrs["correlation_id"], rows[0]["correlation_id"]);
    assert!(req.start_us >= call.start_us && req.end_us <= call.end_us);
    let aws = core.tools.aws.as_deref().unwrap();
    assert!(!aws.waits_for_trace("t_read"), "taken by the continuation");
}
