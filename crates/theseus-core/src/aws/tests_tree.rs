//! The tree over `aws.call` (theseus-0nnk, row B6): a model given only the
//! root reaches an operation no `NAMES` entry wraps, and the breadth that
//! costs no new entry.
//!
//! The rule under it is `super::tree`: a discovered operation defaults to
//! the WRITE posture unless the catalog proves it a read (the owner's
//! cautious decision for this row).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_aws::catalog::{Catalog, Class, ClassReason};

use super::tests::{board, core, ledgered, sts, turn, Fake, Reply, Seen, ACCOUNT};
use super::tree::{self, Breadth};

/// The operation the walk reaches: a read of a service no AWS tool of
/// `super::NAMES` names, so only the tree can find it.
const SERVICE: &str = "sns";
const OPERATION: &str = "ListTopics";

/// AWS as this walk needs it: the account's check, and the one operation.
fn answers(account: &'static str) -> impl Fn(&Seen, usize) -> Reply + Send + Sync {
    move |s, n| {
        let id = format!("req-{n}");
        match s.action() {
            Some("GetCallerIdentity") => sts(account, n),
            Some("ListTopics") => (
                200,
                vec![
                    ("x-amzn-requestid", id.clone()),
                    ("content-type", "text/xml".into()),
                ],
                format!(
                    "<ListTopicsResponse><ListTopicsResult><Topics><member>\
                     <TopicArn>arn:aws:sns:us-west-2:{account}:example-topic</TopicArn>\
                     </member></Topics></ListTopicsResult>\
                     <ResponseMetadata><RequestId>{id}</RequestId></ResponseMetadata>\
                     </ListTopicsResponse>"
                ),
            ),
            _ => (
                400,
                vec![("x-amzn-requestid", id)],
                "<ErrorResponse><Error><Type>Sender</Type><Code>InvalidAction</Code>\
                 <Message>the fake does not know it</Message></Error></ErrorResponse>"
                    .into(),
            ),
        }
    }
}

/// One `aws.describe` answer, as the model reads it.
fn described(aws: &Arc<super::Aws>, input: Value) -> String {
    let t = super::tests::tool(aws, "aws.describe");
    let ctx = super::tests::plain_ctx();
    t.plan(&input, &ctx).expect("the describe plans");
    t.run(&input, &ctx)
        .unwrap_or_else(|f| panic!("{}", f.message))
        .text
}

/// The bar (theseus-0nnk): from the root alone — no tool name, no service
/// name given — the model reads the services, then one service's
/// operations, then the operation's input schema, and issues it with
/// `aws.call`; the `aws.called` row names it. No `NAMES` entry wraps it,
/// and the walk's four steps are the model's own, each planned and run by
/// the core.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_given_only_the_trees_root_reaches_an_operation_no_tool_names() {
    use crate::provider::Scripted;
    // Nothing in the hand-written tool list names this service.
    assert!(
        !super::NAMES.iter().any(|n| n.contains(SERVICE)),
        "{SERVICE} is wrapped by a tool: pick an operation the tree alone reaches"
    );
    let fake = Fake::start(answers(ACCOUNT));
    let aws = super::tests::layer(&fake, board());

    // Step 1, the root: every service, and how to go on.
    let root = described(&aws, json!({}));
    assert!(root.contains("AWS services"), "{root}");
    assert!(
        root.contains(SERVICE),
        "the root lists no {SERVICE}: {root}"
    );
    // Step 2, the service: its operations by class.
    let svc = described(&aws, json!({"service": SERVICE}));
    assert!(
        svc.contains(OPERATION),
        "the service lists no {OPERATION}: {svc}"
    );
    assert!(svc.contains("read ("), "no read class: {svc}");
    // And the service's own words tell the model the call it can make.
    // Until this row they said "writes and runs arrive with step 14b",
    // which has been false since 14b landed (`aws.call` plans a write).
    assert!(
        svc.contains("aws.call makes any of them"),
        "the service does not say what aws.call can do: {svc}"
    );
    // Step 3, the operation: its input, as JSON schema.
    let op = described(&aws, json!({"service": SERVICE, "operation": OPERATION}));
    let d: Value = serde_json::from_str(&op).expect("the operation describes as JSON");
    assert_eq!(d["class"], "read", "{op}");
    assert!(d["input"].is_object(), "no input schema: {op}");

    // Step 4, the call, through the whole core: the model walks the same
    // four steps and the last one is the operation itself.
    let dir = tempfile::tempdir().unwrap();
    let core = core(
        &fake,
        dir.path(),
        vec![
            Scripted::tools("", &[("t_root", "aws_describe", json!({}))]),
            Scripted::tools(
                "",
                &[("t_svc", "aws_describe", json!({"service": SERVICE}))],
            ),
            Scripted::tools(
                "",
                &[(
                    "t_op",
                    "aws_describe",
                    json!({"service": SERVICE, "operation": OPERATION}),
                )],
            ),
            Scripted::tools(
                "",
                &[(
                    "t_call",
                    "aws_call",
                    json!({"service": SERVICE, "operation": OPERATION}),
                )],
            ),
            Scripted::text("There is one topic."),
        ],
    );
    let r = turn(&core, "what SNS topics are there?").await;
    assert_eq!(r.tool_calls, 4, "the four steps of the walk");
    assert!(
        ledgered(&core, "tool.invalid_input").is_empty(),
        "a step of the walk was invalid input"
    );
    let rows = ledgered(&core, "aws.called");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (
            rows[0]["operation"].as_str(),
            rows[0]["service"].as_str(),
            rows[0]["status"].as_str()
        ),
        (Some(OPERATION), Some(SERVICE), Some("ok")),
        "{:?}",
        rows[0]
    );
}

/// What the tree reached when it was measured, by the catalog snapshot it
/// was measured over.
///
/// The figures are a property of the catalog, not of this branch: a refresh
/// of `data/aws-catalog.bin` moves every one of them. So each row names its
/// snapshot, a row is added and never edited when the catalog is refreshed,
/// and the test below pins the row for the snapshot in the tree while the
/// floors it also asserts hold for any catalog. A figure that moves *under
/// its own snapshot* is a change to the classification and fails here.
const MEASURED: &[(&str, Breadth)] = &[
    (
        "aws-cli/2.34.15",
        Breadth {
            services: 416,
            operations: 17_928,
            proved_reads: 7_663,
            writes: 10_207,
            unproved: 10_145,
            runs: 58,
            iac_only: 1_758,
        },
    ),
    // theseus-mgw.14's refresh. By the branch of the catalog's rule:
    // Default 11,040 (every one a write), NamePrefix 3,981, HttpMethod
    // 2,161, ReadonlyTrait 2,239, Override 116 (49 reads), RunList 58.
    // Callable without one new `NAMES` entry: 17,793.
    (
        "aws-cli/2.40.2",
        Breadth {
            services: 436,
            operations: 19_595,
            proved_reads: 8_430,
            writes: 11_107,
            unproved: 11_040,
            runs: 58,
            iac_only: 1_802,
        },
    ),
];

/// The measure this row cares about, and the rule under it, in one walk of
/// the catalog (it decodes every service, so it is the operator's read, not
/// a turn's).
///
/// **The rule**: no operation is called a read for a reason that cannot
/// prove one ([`tree::may_prove_a_read`]), and the fall-through — nothing
/// marked it a read — is always a write. That is the owner's cautious
/// decision for this row, and both halves are checked over every operation,
/// so a change to the catalog's classification or to the rule fails here.
///
/// **The breadth** is [`MEASURED`]: what the tree reaches without one new
/// `NAMES` entry.
#[test]
fn the_trees_breadth_is_measured_and_no_unproved_operation_is_a_read() {
    let c = Catalog::embedded().unwrap();
    let mut bad: Vec<String> = Vec::new();
    let mut by_reason: std::collections::BTreeMap<String, usize> = Default::default();
    for entry in c.services() {
        let svc = c.service(&entry.name).expect("a listed service decodes");
        for op in svc.operations() {
            let k = op.classify();
            *by_reason.entry(format!("{:?}", k.reason)).or_default() += 1;
            if k.class == Class::Read && !tree::may_prove_a_read(k.reason) {
                bad.push(format!(
                    "{}:{} is a read by {:?}, which cannot prove one",
                    entry.name,
                    op.name(),
                    k.reason
                ));
            }
            if k.reason == ClassReason::Default && k.class != Class::Write {
                bad.push(format!(
                    "{}:{} is unproved and {:?}, not a write",
                    entry.name,
                    op.name(),
                    k.class
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{} bad:\n{}", bad.len(), bad.join("\n"));
    let b = tree::breadth(c);
    println!(
        "snapshot {} breadth {b:?} callable {} by_reason {by_reason:?}",
        c.snapshot(),
        b.callable()
    );
    // A zero of any of these would pass an empty catalog: they are the
    // positive counts the measure rests on, and they hold for any snapshot.
    assert!(b.services > 300, "{b:?}");
    assert!(b.operations > 15_000 && b.callable() > 14_000, "{b:?}");
    assert!(b.proved_reads > 5_000, "{b:?}");
    assert!(b.unproved > 5_000 && b.unproved <= b.writes, "{b:?}");
    assert!(b.runs > 20 && b.iac_only > 1_000, "{b:?}");
    // Against sixteen hand-written tool names: the breadth that costs none.
    assert!(b.callable() > 500 * super::NAMES.len(), "{b:?}");
    let Some((_, want)) = MEASURED.iter().find(|(s, _)| *s == c.snapshot()) else {
        panic!(
            "the catalog is {}, which MEASURED does not name: add its row with the figures \
             printed above, in the commit that refreshed it",
            c.snapshot()
        );
    };
    assert_eq!(&b, want, "the classification moved under {}", c.snapshot());
}

/// The hand-written list and the registry are the same set, and every name
/// in it has a `[policy.aws]` class.
///
/// This is the sharp problem the row names, from the other side: a tool the
/// runtime offers that [`super::NAMES`] does not hold takes
/// `[policy].enforcement` in silence (`super::tool_class` answers `None`
/// for a name it does not know) instead of its class's line, and the config
/// template's `[policy.tools]` census
/// (`crate::config::tests::example_template_lists_every_tool_under_policy_tools`)
/// never sees it. The tree needs no entry at all, which is the point of the
/// row; a *tool* still does, and this says so.
#[test]
fn every_aws_tool_the_runtime_offers_is_in_names_with_a_class() {
    use std::collections::BTreeSet;
    let fake = Fake::start(answers(ACCOUNT));
    let aws = super::tests::layer(&fake, board());
    let tools = aws.tools();
    let offered: BTreeSet<&str> = tools.iter().map(|t| t.name()).collect();
    let listed: BTreeSet<&str> = super::NAMES.iter().copied().collect();
    assert_eq!(
        offered, listed,
        "the registry and NAMES must name the same tools"
    );
    assert!(!offered.is_empty(), "no AWS tool was offered at all");
    for name in &offered {
        let class = super::tool_class(name);
        if *name == "aws.describe" {
            // It calls nothing, so it has no AWS posture of its own.
            assert_eq!(class, None, "{name}");
        } else {
            assert!(
                matches!(class, Some("read" | "write" | "run")),
                "{name} has no [policy.aws] class: {class:?}"
            );
        }
    }
    assert_eq!(super::tool_class("aws.nothing.offers.this"), None);
}
