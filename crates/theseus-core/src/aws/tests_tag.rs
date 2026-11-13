//! The first typed per-service tool-let family (theseus-mgw.15): the three
//! things a tool-let is worth having for, each proved offline.
//!
//! 1. a bad input is **invalid input in `plan`** — before the gate, with no
//!    network — and the message names the field;
//! 2. the tool-let refuses an **operator limit** itself (the account its
//!    ARNs may name), rather than leaning on the gate;
//! 3. each tool-let's **`tool_class` is its own**, so a write is never
//!    mistaken for a read.
//!
//! Proving the family against the real bound account is not a cloud card:
//! `GetCallerIdentity` must name it before anything signs. Everything here
//! runs against the local fake.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_tools::{ToolClass, ToolCtx};

use super::tag::{ARNS_MAX, KEY_CHARS, PAGES_MAX, PAGE_MAX, TAGS_MAX, VALUE_CHARS};
use super::tests::{board, layer, sts, tool, Fake, Reply, Seen, ACCOUNT};

/// A resource of the bound account, and one of another.
const MINE: &str = "arn:aws:s3:::example-bucket";
const OTHERS: &str = "arn:aws:s3:us-west-2:444455556666:accesspoint/far";

/// The tagging API as these tests need it. Its protocol is JSON, so the
/// operation is the `X-Amz-Target` header, not a form `Action`.
fn answers() -> impl Fn(&Seen, usize) -> Reply + Send + Sync {
    move |s, n| {
        let id = format!("req-{n}");
        let json = |body: &str| {
            (
                200,
                vec![
                    ("x-amzn-requestid", id.clone()),
                    ("content-type", "application/x-amz-json-1.1".into()),
                ],
                body.to_string(),
            )
        };
        let target = s
            .header("x-amz-target")
            .and_then(|t| t.rsplit('.').next())
            .map(String::from);
        match (s.action(), target.as_deref()) {
            (Some("GetCallerIdentity"), _) => sts(ACCOUNT, n),
            (_, Some("GetResources")) => json(
                "{\"ResourceTagMappingList\":[\
                 {\"ResourceARN\":\"arn:aws:s3:::example-bucket\",\
                 \"Tags\":[{\"Key\":\"Env\",\"Value\":\"prod\"},{\"Key\":\"Keel\",\"Value\":\"\"}]},\
                 {\"ResourceARN\":\"arn:aws:sqs:us-west-2:111122223333:example-queue\",\
                 \"Tags\":[]}]}",
            ),
            (_, Some("TagResources")) => json("{\"FailedResourcesMap\":{}}"),
            (_, Some("UntagResources")) => json(
                "{\"FailedResourcesMap\":{\"arn:aws:s3:::example-bucket\":\
                 {\"ErrorCode\":\"InternalServiceException\",\"ErrorMessage\":\"try again\"}}}",
            ),
            _ => (
                400,
                vec![("x-amzn-requestid", id)],
                "{\"__type\":\"InvalidAction\",\"message\":\"the fake does not know it\"}".into(),
            ),
        }
    }
}

fn plan_err(aws: &Arc<super::Aws>, name: &str, input: Value) -> String {
    let ctx: ToolCtx = super::tests::plain_ctx();
    match tool(aws, name).plan(&input, &ctx) {
        Ok(p) => panic!("{name} planned {input}: {}", p.summary),
        Err(e) => e,
    }
}

/// Bar (a) and (b): every field is checked in `plan`, the message names the
/// field, and **nothing is sent** — the fake saw no request at all, not even
/// the account's check.
#[tokio::test]
async fn a_bad_input_is_invalid_input_in_plan_and_names_its_field() {
    let fake = Fake::start(answers());
    let aws = layer(&fake, board());
    let long_key = "k".repeat(KEY_CHARS + 1);
    let long_value = "v".repeat(VALUE_CHARS + 1);
    let many: Vec<String> = (0..=ARNS_MAX)
        .map(|i| format!("arn:aws:sqs:us-west-2:{ACCOUNT}:q{i}"))
        .collect();
    let keys: Vec<String> = (0..=TAGS_MAX).map(|i| format!("k{i}")).collect();
    for (name, input, want) in [
        (
            "aws.tag.set",
            json!({"arns": [], "tags": {"a": "b"}}),
            "\"arns\"",
        ),
        (
            "aws.tag.set",
            json!({"arns": many, "tags": {"a": "b"}}),
            "over the 20 one call takes",
        ),
        (
            "aws.tag.set",
            json!({"arns": ["not-an-arn"], "tags": {"a": "b"}}),
            "which is no ARN",
        ),
        (
            "aws.tag.set",
            json!({"arns": [MINE], "tags": {"aws:cloudformation:stack-name": "x"}}),
            "reserves the aws: prefix",
        ),
        (
            "aws.tag.set",
            json!({"arns": [MINE], "tags": {long_key: "x"}}),
            "a key is 1 to 128",
        ),
        (
            "aws.tag.set",
            json!({"arns": [MINE], "tags": {"Env": long_value}}),
            "a value is at most 256",
        ),
        (
            "aws.tag.set",
            json!({"arns": [MINE], "tags": {"Env": 7}}),
            "a tag's value is a string",
        ),
        (
            "aws.tag.set",
            json!({"arns": [MINE], "tags": {}}),
            "\"tags\" must name 1 to 50 tag keys",
        ),
        (
            "aws.tag.clear",
            json!({"arns": [MINE], "keys": keys}),
            "over the 50 one call takes",
        ),
        (
            "aws.tag.clear",
            json!({"arns": [MINE], "keys": []}),
            "\"keys\" must name 1 to 50 tag keys",
        ),
        (
            "aws.tag.list",
            json!({"limit": 0}),
            "\"limit\" must be 1 to 100",
        ),
        (
            "aws.tag.list",
            json!({"limit": PAGE_MAX + 1}),
            "\"limit\" must be 1 to 100",
        ),
        (
            "aws.tag.list",
            json!({"pages": PAGES_MAX + 1}),
            "\"pages\" must be 1 to 10",
        ),
        (
            "aws.tag.list",
            json!({"tags": {"Env": ["prod"]}}),
            "a filter's value is a string",
        ),
        // An unknown field is refused by the schema itself.
        (
            "aws.tag.list",
            json!({"bucket": "example-bucket"}),
            "bucket",
        ),
    ] {
        let e = plan_err(&aws, name, input.clone());
        assert!(e.contains(want), "{name} {input}: {e:?} lacks {want:?}");
    }
    assert!(
        fake.seen().is_empty(),
        "a refused plan reached AWS: {:?}",
        fake.seen()
    );
}

/// Bar (b), on its own: the account is an **operator limit**, and the
/// tool-let refuses it. The gate is never asked — there is no plan for it to
/// judge — and nothing is sent. The same ARN with this account's id plans.
#[tokio::test]
async fn the_account_is_an_operator_limit_the_tool_let_refuses_itself() {
    let fake = Fake::start(answers());
    let aws = layer(&fake, board());
    for name in ["aws.tag.set", "aws.tag.clear", "aws.tag.list"] {
        let input = match name {
            "aws.tag.set" => json!({"arns": [OTHERS], "tags": {"Env": "prod"}}),
            "aws.tag.clear" => json!({"arns": [OTHERS], "keys": ["Env"]}),
            _ => json!({"arns": [OTHERS]}),
        };
        let e = plan_err(&aws, name, input);
        assert!(e.contains("444455556666"), "{name}: {e}");
        assert!(e.contains(ACCOUNT), "{name}: {e}");
        assert!(e.contains("operator limit"), "{name}: {e}");
        assert!(e.contains("nothing was sent"), "{name}: {e}");
    }
    // The bound account's own resource plans, so the refusal is the account
    // and not the ARN's shape.
    let ours = format!("arn:aws:sqs:us-west-2:{ACCOUNT}:example-queue");
    let p = tool(&aws, "aws.tag.set")
        .plan(
            &json!({"arns": [ours], "tags": {"Env": "prod"}}),
            &super::tests::plain_ctx(),
        )
        .expect("the bound account's resource plans");
    assert_eq!(
        p.summary,
        "set Env on arn:aws:sqs:us-west-2:111122223333:example-queue in us-west-2"
    );
    assert!(fake.seen().is_empty(), "{:?}", fake.seen());
}

/// Bar (c): each tool-let's class is its own, in the tool and in its plan,
/// and `[policy.aws]` reads `write` for the two writes whatever `aws.call`'s
/// own line says.
#[tokio::test]
async fn each_tool_lets_class_is_its_own_so_a_write_is_no_read() {
    let fake = Fake::start(answers());
    let aws = layer(&fake, board());
    for (name, class, line) in [
        ("aws.tag.list", ToolClass::Read, "read"),
        ("aws.tag.set", ToolClass::Write, "write"),
        ("aws.tag.clear", ToolClass::Write, "write"),
    ] {
        assert_eq!(tool(&aws, name).class(), class, "{name}");
        assert_eq!(super::tool_class(name), Some(line), "{name}");
        let input = match name {
            "aws.tag.set" => json!({"arns": [MINE], "tags": {"Env": "prod"}}),
            "aws.tag.clear" => json!({"arns": [MINE], "keys": ["Env"]}),
            _ => json!({}),
        };
        let p = tool(&aws, name)
            .plan(&input, &super::tests::plain_ctx())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.class, Some(class), "{name}");
        let a = p.aws.expect("an AwsPlan");
        assert_eq!(a.service, "resourcegroupstaggingapi", "{name}");
        assert_eq!(a.account, ACCOUNT, "{name}");
    }
}

/// The family through the one caller: a listing reads its rows, a set
/// writes, and a write AWS partly refused says so rather than reading as a
/// success.
#[tokio::test]
async fn the_family_lists_and_writes_through_the_accounts_one_caller() {
    let fake = Fake::start(answers());
    let aws = layer(&fake, board());
    let (r, reqs) = super::tests::call(&aws, "aws.tag.list", json!({"limit": 2})).await;
    let (out, _) = r.unwrap_or_else(|f| panic!("{}", f.message));
    assert!(out.text.contains("2 resources"), "{}", out.text);
    assert!(out.text.contains("Env=prod"), "{}", out.text);
    assert!(out.text.contains("(no tags)"), "{}", out.text);
    assert_eq!(
        out.meta["operation"],
        "resourcegroupstaggingapi:GetResources"
    );
    assert_eq!(reqs.len(), 1, "{reqs:?}");

    let (r, _) = super::tests::call(
        &aws,
        "aws.tag.set",
        json!({"arns": [MINE], "tags": {"Env": "prod"}}),
    )
    .await;
    let (out, _) = r.unwrap_or_else(|f| panic!("{}", f.message));
    assert!(
        out.text.starts_with("set Env on 1 of 1 resource"),
        "{}",
        out.text
    );
    assert_eq!(out.meta["tag_keys"], json!(["Env"]));

    // UntagResources, which the fake refuses for this ARN: the result says
    // 0 of 1 and names AWS's reason.
    let (r, _) = super::tests::call(
        &aws,
        "aws.tag.clear",
        json!({"arns": [MINE], "keys": ["Env"]}),
    )
    .await;
    let (out, _) = r.unwrap_or_else(|f| panic!("{}", f.message));
    assert!(
        out.text.starts_with("removed Env on 0 of 1 resource"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("refused arn:aws:s3:::example-bucket: InternalServiceException"),
        "{}",
        out.text
    );
}
