//! The client against a local fake endpoint (AWS design §5, P2): retries by
//! retry class, `outcome_unknown`, pagination, errors and their enforcers,
//! attribution, and the size cap. Then the requests it builds without a
//! network: validation, what it refuses, S3's hosts and checksums, host
//! prefixes, signing, and presigning.

mod fake;

use std::time::{Duration, UNIX_EPOCH};

use base64::Engine as _;
use fake::{Fake, Reply};
use md5::{Digest as _, Md5};
use serde_json::{json, Value};
use theseus_aws::{
    Attribution, Call, CallError, Client, ClientConfig, Credentials, Enforcer, RetryPolicy,
};

fn creds() -> Credentials {
    Credentials::new(
        "AKIDEXAMPLE",
        "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        None,
        None,
    )
}

/// A client of the fake: quick retries, a short attempt timeout.
fn client(fake: &Fake) -> Client {
    let mut c = ClientConfig::new("us-west-2");
    c.endpoint_override = Some(fake.url.clone());
    c.retry = RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(5),
        max_backoff: Duration::from_millis(20),
    };
    c.attempt_timeout = Duration::from_millis(500);
    Client::new(c)
}

fn call<'a>(
    service: &'a str,
    operation: &'a str,
    input: &'a Value,
    a: &'a Attribution,
) -> Call<'a> {
    Call {
        service,
        operation,
        input,
        region: None,
        pages: 1,
        attribution: a,
    }
}

const THROTTLE: &str = r#"{"__type": "ThrottlingException", "message": "Rate exceeded"}"#;
const SENT: &str = r#"{"MD5OfMessageBody": "x", "MessageId": "m-1"}"#;

fn send_message() -> Value {
    json!({"QueueUrl": "https://sqs.us-west-2.amazonaws.com/111122223333/example-done", "MessageBody": "hi"})
}

#[tokio::test]
async fn a_throttle_is_retried_even_when_the_call_is_not_safe_to_repeat() {
    let fake = Fake::start(vec![Reply::json(400, THROTTLE), Reply::json(200, SENT)]).await;
    let a = Attribution::default();
    let input = send_message();
    let out = client(&fake)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap();
    assert_eq!(out.attempts, 2);
    assert_eq!(out.body["MessageId"], "m-1");
    assert_eq!(fake.seen().len(), 2);
}

#[tokio::test]
async fn a_timeout_after_sending_a_non_repeatable_call_is_outcome_unknown() {
    let fake = Fake::start(vec![Reply::Hang(Duration::from_secs(5))]).await;
    let a = Attribution::default();
    let input = send_message();
    let err = client(&fake)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            CallError::OutcomeUnknown {
                attempts: 1,
                error: None,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(err.may_have_run());
    // Never sent twice.
    assert_eq!(fake.seen().len(), 1);
}

#[tokio::test]
async fn a_timeout_on_a_read_is_retried() {
    let fake = Fake::start(vec![
        Reply::Hang(Duration::from_secs(5)),
        Reply::json(200, r#"{"QueueUrl": "https://sqs.example/q"}"#),
    ])
    .await;
    let a = Attribution::default();
    let input = json!({"QueueName": "example-done"});
    let out = client(&fake)
        .call(&call("sqs", "GetQueueUrl", &input, &a), &creds())
        .await
        .unwrap();
    assert_eq!(out.attempts, 2);
    assert_eq!(out.body["QueueUrl"], "https://sqs.example/q");
}

#[tokio::test]
async fn a_server_error_is_retried_only_when_safe() {
    let boom = r#"{"__type": "InternalFailure", "message": "boom"}"#;
    // A write that is not safe to repeat: its outcome is unknown, at once.
    let fake = Fake::start(vec![Reply::json(500, boom), Reply::json(200, SENT)]).await;
    let a = Attribution::default();
    let input = send_message();
    let err = client(&fake)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap_err();
    match &err {
        CallError::OutcomeUnknown {
            attempts, error, ..
        } => {
            assert_eq!(*attempts, 1);
            assert_eq!(error.as_ref().unwrap().code, "InternalFailure");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.seen().len(), 1);

    // A read is retried.
    let fake = Fake::start(vec![
        Reply::json(500, boom),
        Reply::json(200, r#"{"QueueUrl": "q"}"#),
    ])
    .await;
    let input = json!({"QueueName": "example-done"});
    let out = client(&fake)
        .call(&call("sqs", "GetQueueUrl", &input, &a), &creds())
        .await
        .unwrap();
    assert_eq!(out.attempts, 2);
}

#[tokio::test]
async fn a_dropped_connection_after_sending_is_outcome_unknown() {
    let fake = Fake::start(vec![Reply::Drop]).await;
    let a = Attribution::default();
    let input = send_message();
    let err = client(&fake)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::OutcomeUnknown { .. }), "{err:?}");
    assert_eq!(fake.seen().len(), 1);
}

#[tokio::test]
async fn retries_stop_at_the_policy() {
    let fake = Fake::start(vec![Reply::json(400, THROTTLE)]).await;
    let a = Attribution::default();
    let input = send_message();
    let err = client(&fake)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap_err();
    assert_eq!(err.aws().unwrap().code, "ThrottlingException");
    assert_eq!(fake.seen().len(), 3);
}

#[tokio::test]
async fn an_idempotent_call_keeps_its_token_across_attempts() {
    let fake = Fake::start(vec![
        Reply::new(503, "<Response><Errors><Error><Code>RequestLimitExceeded</Code><Message>slow down</Message></Error></Errors><RequestID>r1</RequestID></Response>"),
        Reply::new(200, "<RunInstancesResponse><requestId>r2</requestId><reservationId>r-1</reservationId><instancesSet><item><instanceId>i-1</instanceId></item></instancesSet></RunInstancesResponse>"),
    ])
    .await;
    let a = Attribution {
        execution: Some("exe_test".into()),
        call: Some("call_1".into()),
    };
    let input = json!({"ImageId": "ami-0example", "MinCount": 1, "MaxCount": 1});
    let out = client(&fake)
        .call(&call("ec2", "RunInstances", &input, &a), &creds())
        .await
        .unwrap();
    assert_eq!(out.attempts, 2);
    assert_eq!(
        out.idempotency_token,
        Some(("ClientToken".into(), "call_1".into()))
    );
    assert_eq!(out.request_id.as_deref(), Some("r2"));
    assert_eq!(out.body["Instances"][0]["InstanceId"], "i-1");
    let seen = fake.seen();
    assert_eq!(seen.len(), 2);
    for s in &seen {
        assert!(
            s.body_text().contains("ClientToken=call_1"),
            "{}",
            s.body_text()
        );
        // The attribution user agent, on every attempt (AWS design §3.5).
        assert_eq!(
            s.header("user-agent"),
            Some(
                format!(
                    "theseus/{} exec/exe_test call/call_1",
                    env!("CARGO_PKG_VERSION")
                )
                .as_str()
            )
        );
    }
}

fn endpoints_page(name: &str, next: Option<&str>) -> Reply {
    let mut page = json!({"Endpoints": [{"EndpointName": name, "EndpointArn": format!("arn:aws:sagemaker:us-west-2:111122223333:endpoint/{name}"),
        "CreationTime": 1790000000, "LastModifiedTime": 1790000000, "EndpointStatus": "InService"}]});
    if let Some(n) = next {
        page["NextToken"] = json!(n);
    }
    Reply::json(200, &page.to_string())
}

#[tokio::test]
async fn pages_are_followed_and_joined() {
    let pages = vec![
        endpoints_page("a", Some("p2")),
        endpoints_page("b", Some("p3")),
        endpoints_page("c", None),
    ];
    let a = Attribution::default();
    let input = json!({"MaxResults": 1});

    // All of them.
    let fake = Fake::start(pages.clone()).await;
    let mut c = call("sagemaker", "ListEndpoints", &input, &a);
    c.pages = 10;
    let out = client(&fake).call(&c, &creds()).await.unwrap();
    assert_eq!(out.pages, 3);
    let names: Vec<&str> = out.body["Endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["EndpointName"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert_eq!(out.next, None);
    assert!(out.body.get("NextToken").is_none());
    let seen = fake.seen();
    assert!(!seen[0].body_text().contains("NextToken"));
    assert!(seen[1].body_text().contains(r#""NextToken":"p2""#));
    assert!(seen[2].body_text().contains(r#""NextToken":"p3""#));

    // Two of them: the body and `next` say where the third starts.
    let fake = Fake::start(pages).await;
    c.pages = 2;
    let out = client(&fake).call(&c, &creds()).await.unwrap();
    assert_eq!(out.pages, 2);
    assert_eq!(out.body["Endpoints"].as_array().unwrap().len(), 2);
    assert_eq!(out.body["NextToken"], "p3");
    assert_eq!(
        out.next,
        Some(json!({"NextToken": "p3"}).as_object().unwrap().clone())
    );
    assert_eq!(out.request_ids.len(), 0, "the fake sends no request ids");
}

#[tokio::test]
async fn a_paginator_that_says_when_it_is_done() {
    let page = |items: &str, truncated: bool, marker: &str| {
        Reply::new(
            200,
            &format!(
                r#"<ListHostedZonesResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><HostedZones>{items}</HostedZones><IsTruncated>{truncated}</IsTruncated>{marker}<MaxItems>1</MaxItems></ListHostedZonesResponse>"#
            ),
        )
    };
    let zone = |id: &str| {
        format!("<HostedZone><Id>/hostedzone/{id}</Id><Name>{id}.example.test.</Name><CallerReference>r</CallerReference></HostedZone>")
    };
    let fake = Fake::start(vec![
        page(&zone("Z1"), true, "<NextMarker>Z2</NextMarker>"),
        // A marker on the last page, ignored: IsTruncated says it is done.
        page(&zone("Z2"), false, "<NextMarker>Z3</NextMarker>"),
    ])
    .await;
    let a = Attribution::default();
    let input = json!({});
    let mut c = call("route53", "ListHostedZones", &input, &a);
    c.pages = 5;
    let out = client(&fake).call(&c, &creds()).await.unwrap();
    assert_eq!(out.pages, 2);
    assert_eq!(out.body["HostedZones"].as_array().unwrap().len(), 2);
    assert_eq!(out.body["IsTruncated"], false);
    assert_eq!(out.next, None);
    assert!(
        fake.seen()[1].target.contains("marker=Z2"),
        "{}",
        fake.seen()[1].target
    );
}

#[tokio::test]
async fn a_denial_names_its_enforcer() {
    let body = json!({"__type": "AccessDeniedException", "message":
        "User: arn:aws:sts::111122223333:assumed-role/example-owner/exe_test is not authorized to perform: sqs:SendMessage on resource: arn:aws:sqs:us-west-2:111122223333:example-done with an explicit deny in a session policy"});
    let fake = Fake::start(vec![
        Reply::json(400, &body.to_string()).header("x-amzn-RequestId", "req-9")
    ])
    .await;
    let a = Attribution::default();
    let input = send_message();
    let err = client(&fake)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap_err();
    let aws = err.aws().unwrap();
    assert_eq!(aws.code, "AccessDeniedException");
    assert_eq!(aws.request_id.as_deref(), Some("req-9"));
    let d = aws.denial.as_ref().unwrap();
    assert_eq!((d.enforcer, d.explicit), (Enforcer::Guard, true));
    assert!(err.to_string().contains("[refused by guard]"), "{err}");
    // Not retried: a denial is an answer.
    assert_eq!(fake.seen().len(), 1);
}

#[tokio::test]
async fn an_answer_past_the_cap_says_the_call_ran() {
    let fake = Fake::start(vec![Reply::json(
        200,
        &format!(r#"{{"MessageId": "{}"}}"#, "x".repeat(4096)),
    )
    .header("x-amzn-RequestId", "req-big")])
    .await;
    let mut c = ClientConfig::new("us-west-2");
    c.endpoint_override = Some(fake.url.clone());
    c.max_response_bytes = 1024;
    let a = Attribution::default();
    let input = send_message();
    let err = Client::new(c)
        .call(&call("sqs", "SendMessage", &input, &a), &creds())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        CallError::TooLarge {
            limit: 1024,
            request_id: Some("req-big".into())
        }
    );
    assert!(err.may_have_run());
}

#[tokio::test]
async fn an_s3_error_inside_a_200_is_an_error() {
    let fake = Fake::start(vec![
        Reply::new(200, "<?xml version=\"1.0\"?>\n<Error><Code>InternalError</Code><Message>We encountered an internal error.</Message></Error>"),
        Reply::new(200, "<CopyObjectResult><ETag>\"e\"</ETag><LastModified>2026-09-30T23:45:00.000Z</LastModified></CopyObjectResult>"),
    ])
    .await;
    let a = Attribution::default();
    let input = json!({"Bucket": "example-theseus-bucket", "Key": "b.txt", "CopySource": "example-theseus-bucket/a.txt"});
    let out = client(&fake)
        .call(&call("s3", "CopyObject", &input, &a), &creds())
        .await
        .unwrap();
    // CopyObject is safe to repeat (it writes the same bytes), so it was retried.
    assert_eq!(out.attempts, 2);
    assert_eq!(out.body["CopyObjectResult"]["ETag"], "\"e\"");
    assert_eq!(
        out.body["CopyObjectResult"]["LastModified"],
        "2026-09-30T23:45:00Z"
    );
}

#[tokio::test]
async fn a_redirect_is_an_error_not_a_second_request() {
    let fake = Fake::start(vec![Reply::new(
        301,
        "<Error><Code>PermanentRedirect</Code><Message>The bucket you are attempting to access must be addressed using the specified endpoint.</Message><Endpoint>example-theseus-bucket.s3.us-east-1.amazonaws.com</Endpoint></Error>",
    )
    .header("Location", "http://127.0.0.1:9/elsewhere")
    .header("x-amz-bucket-region", "us-east-1")])
    .await;
    let a = Attribution::default();
    let input = json!({"Bucket": "example-theseus-bucket"});
    let err = client(&fake)
        .call(&call("s3", "ListObjectsV2", &input, &a), &creds())
        .await
        .unwrap_err();
    let aws = err.aws().unwrap();
    assert_eq!(aws.code, "PermanentRedirect");
    assert!(
        aws.message.ends_with("(the bucket is in us-east-1)"),
        "{}",
        aws.message
    );
    assert_eq!(fake.seen().len(), 1);
}

// ---- without a network --------------------------------------------------

fn offline() -> Client {
    Client::new(ClientConfig::new("us-west-2"))
}

fn prepare(
    service: &str,
    operation: &str,
    input: Value,
) -> Result<theseus_aws::Prepared, CallError> {
    let a = Attribution {
        execution: None,
        call: Some("call_1".into()),
    };
    offline().prepare(
        &call(service, operation, &input, &a),
        &creds(),
        UNIX_EPOCH + Duration::from_secs(1_790_000_000),
    )
}

fn invalid(r: Result<theseus_aws::Prepared, CallError>) -> String {
    match r {
        Err(CallError::InvalidInput(m)) => m,
        other => panic!("not invalid input: {other:?}"),
    }
}

#[test]
fn input_that_does_not_match_its_shape_is_invalid() {
    let m = invalid(prepare(
        "sqs",
        "SendMessage",
        json!({"QueueUrl": "q", "MessageBody": "b", "Bogus": 1}),
    ));
    assert!(
        m.contains("unknown member \"Bogus\"") && m.contains("MessageBody"),
        "{m}"
    );
    let m = invalid(prepare("sqs", "SendMessage", json!({"QueueUrl": "q"})));
    assert!(m.contains("missing required member \"MessageBody\""), "{m}");
    let m = invalid(prepare(
        "sqs",
        "SendMessage",
        json!({"QueueUrl": "q", "MessageBody": "b", "DelaySeconds": "soon"}),
    ));
    assert!(m.starts_with("DelaySeconds: expected an integer"), "{m}");
    let m = invalid(prepare(
        "ec2",
        "DescribeInstances",
        json!({"Filters": [{"Name": "x", "Values": "not-a-list"}]}),
    ));
    assert!(m.starts_with("Filters[0].Values: expected an array"), "{m}");
    let m = invalid(prepare("sqs", "SendMesage", json!({})));
    assert!(m.contains("has no operation"), "{m}");
    assert!(invalid(prepare("nope", "X", json!({}))).contains("no AWS service"));
    let a = Attribution::default();
    let input = json!({});
    let mut c = call("sqs", "ListQueues", &input, &a);
    c.region = Some("us-west-2.evil.example");
    let m = match offline().prepare(&c, &creds(), UNIX_EPOCH) {
        Err(CallError::InvalidInput(m)) => m,
        other => panic!("{other:?}"),
    };
    assert!(m.contains("is not a region"), "{m}");
    // Numbers and booleans may be strings; the token may be left out.
    assert!(prepare(
        "sqs",
        "SendMessage",
        json!({"QueueUrl": "q", "MessageBody": "b", "DelaySeconds": "5"})
    )
    .is_ok());
    assert!(prepare(
        "ec2",
        "RunInstances",
        json!({"ImageId": "ami-1", "MinCount": 1, "MaxCount": 1})
    )
    .is_ok());
}

#[test]
fn what_the_client_cannot_do_it_says() {
    for (svc, op, input) in [
        ("sdb", "ListDomains", json!({})),
        ("bedrock-runtime", "ConverseStream", json!({"modelId": "m"})),
        (
            "timestream-query",
            "Query",
            json!({"QueryString": "SELECT 1"}),
        ),
        (
            "s3",
            "ListObjectsV2",
            json!({"Bucket": "example--usw2-az1--x-s3"}),
        ),
    ] {
        match prepare(svc, op, input) {
            Err(CallError::Unsupported(m)) => assert!(m.contains("CLI"), "{svc}:{op}: {m}"),
            other => panic!("{svc}:{op}: {other:?}"),
        }
    }
}

fn header<'a>(p: &'a theseus_aws::Prepared, name: &str) -> Option<&'a str> {
    p.request.header(name)
}

/// S3's bucket-configuration calls carry `UseS3ExpressControlEndpoint`,
/// which the rule set applies only to directory buckets: a regular bucket's
/// calls stay on S3's own (virtual) host, signed for `s3`.
#[test]
fn a_regular_buckets_configuration_calls_stay_on_s3() {
    let bucket = "example-theseus-bucket";
    for (op, input) in [
        ("GetBucketLocation", json!({"Bucket": bucket})),
        ("GetBucketPolicy", json!({"Bucket": bucket})),
        (
            "PutBucketVersioning",
            json!({"Bucket": bucket, "VersioningConfiguration": {"Status": "Enabled"}}),
        ),
        ("DeleteBucketCors", json!({"Bucket": bucket})),
        (
            "CreateBucket",
            json!({"Bucket": bucket, "CreateBucketConfiguration": {"LocationConstraint": "us-west-2"}}),
        ),
    ] {
        let p = prepare("s3", op, input).unwrap_or_else(|e| panic!("{op}: {e}"));
        assert_eq!(
            p.request.origin, "https://example-theseus-bucket.s3.us-west-2.amazonaws.com",
            "{op}"
        );
        assert!(
            header(&p, "authorization")
                .unwrap()
                .contains("/us-west-2/s3/aws4_request"),
            "{op}"
        );
    }
}

#[test]
fn requests_are_signed_for_their_scope() {
    let p = prepare("sqs", "ListQueues", json!({})).unwrap();
    let auth = header(&p, "authorization").unwrap();
    assert!(
        auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260921/us-west-2/sqs/aws4_request, SignedHeaders=content-type;host;x-amz-date;x-amz-target;x-amzn-query-mode, Signature="),
        "{auth}"
    );
    assert_eq!(header(&p, "x-amz-date"), Some("20260921T141320Z"));
    // IAM is global: one host, signed for us-east-1.
    let p = prepare("iam", "ListRoles", json!({})).unwrap();
    assert_eq!(p.request.origin, "https://iam.amazonaws.com");
    assert!(header(&p, "authorization")
        .unwrap()
        .contains("/us-east-1/iam/aws4_request"));
    // S3 carries its payload's hash.
    let p = prepare(
        "s3",
        "PutObject",
        json!({"Bucket": "example-theseus-bucket", "Key": "k", "Body": "hello"}),
    )
    .unwrap();
    assert_eq!(
        header(&p, "x-amz-content-sha256"),
        Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
    // A session's token rides along.
    let a = Attribution::default();
    let input = json!({});
    let session = Credentials::new("ASIAEXAMPLE", "secret", Some("token-example".into()), None);
    let p = offline()
        .prepare(
            &call("sts", "GetCallerIdentity", &input, &a),
            &session,
            UNIX_EPOCH,
        )
        .unwrap();
    assert_eq!(header(&p, "x-amz-security-token"), Some("token-example"));
    // An unsigned operation goes unsigned.
    let p = prepare(
        "cognito-identity",
        "GetId",
        json!({"IdentityPoolId": "us-west-2:x"}),
    )
    .unwrap();
    assert_eq!(header(&p, "authorization"), None);
}

#[test]
fn s3_hosts_and_checksums() {
    // A dotted bucket goes path-style.
    let p = prepare(
        "s3",
        "GetObject",
        json!({"Bucket": "example.dotted.bucket", "Key": "a/b c"}),
    )
    .unwrap();
    assert_eq!(p.request.origin, "https://s3.us-west-2.amazonaws.com");
    assert_eq!(p.request.path, "/example.dotted.bucket/a/b%20c");
    // An operation that requires a checksum gets Content-MD5.
    let p = prepare(
        "s3",
        "DeleteObjects",
        json!({"Bucket": "example-theseus-bucket", "Delete": {"Objects": [{"Key": "a"}]}}),
    )
    .unwrap();
    let md5 = base64::engine::general_purpose::STANDARD.encode(Md5::digest(&p.request.body));
    assert_eq!(header(&p, "content-md5"), Some(md5.as_str()));
    // A chosen flexible checksum is computed.
    let p = prepare(
        "s3",
        "PutObject",
        json!({"Bucket": "example-theseus-bucket", "Key": "k", "Body": "hello", "ChecksumAlgorithm": "CRC32"}),
    )
    .unwrap();
    assert_eq!(header(&p, "x-amz-sdk-checksum-algorithm"), Some("CRC32"));
    let crc =
        base64::engine::general_purpose::STANDARD.encode(crc32fast::hash(b"hello").to_be_bytes());
    assert_eq!(header(&p, "x-amz-checksum-crc32"), Some(crc.as_str()));
    // The directory-bucket control endpoint is only for a call with no bucket.
    let p = prepare("s3", "ListDirectoryBuckets", json!({})).unwrap();
    assert_eq!(
        p.request.origin,
        "https://s3express-control.us-west-2.amazonaws.com"
    );
    assert!(header(&p, "authorization")
        .unwrap()
        .contains("/s3express/aws4_request"));
}

#[test]
fn host_prefixes_take_their_labels() {
    let p = prepare(
        "s3control",
        "ListAccessPoints",
        json!({"AccountId": "111122223333"}),
    )
    .unwrap();
    assert_eq!(
        p.request.origin,
        "https://111122223333.s3-control.us-west-2.amazonaws.com"
    );
    let m = invalid(prepare(
        "s3control",
        "ListAccessPoints",
        json!({"AccountId": "evil.example/x"}),
    ));
    assert!(m.contains("one DNS label"), "{m}");
    let p = prepare(
        "neptune-graph",
        "ExecuteQuery",
        json!({"graphIdentifier": "g-0example", "queryString": "MATCH (n) RETURN n LIMIT 1", "language": "OPEN_CYPHER"}),
    )
    .unwrap();
    assert_eq!(
        p.request.origin,
        "https://g-0example.us-west-2.neptune-graph.amazonaws.com"
    );
}

#[test]
fn an_idempotency_token_that_does_not_fit_is_replaced() {
    let a = Attribution {
        execution: None,
        call: Some("x".repeat(100)),
    };
    let input = json!({"ImageId": "ami-1", "MinCount": 1, "MaxCount": 1});
    let p = offline()
        .prepare(
            &call("ec2", "RunInstances", &input, &a),
            &creds(),
            UNIX_EPOCH,
        )
        .unwrap();
    let (member, token) = p.idempotency_token.unwrap();
    assert_eq!(member, "ClientToken");
    assert!(token.len() <= 64 && token != "x".repeat(100), "{token}");
}

#[test]
fn presigned_urls() {
    let a = Attribution::default();
    let input = json!({"Bucket": "example-theseus-bucket", "Key": "out/a b.txt"});
    let now = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
    let url = offline()
        .presign(
            &call("s3", "GetObject", &input, &a),
            &creds(),
            now,
            Duration::from_secs(3600),
        )
        .unwrap();
    assert!(
        url.starts_with("https://example-theseus-bucket.s3.us-west-2.amazonaws.com/out/a%20b.txt?"),
        "{url}"
    );
    for part in [
        "X-Amz-Algorithm=AWS4-HMAC-SHA256",
        "X-Amz-Credential=AKIDEXAMPLE%2F20260921%2Fus-west-2%2Fs3%2Faws4_request",
        "X-Amz-Date=20260921T141320Z",
        "X-Amz-Expires=3600",
        "X-Amz-SignedHeaders=host",
        "X-Amz-Signature=",
    ] {
        assert!(url.contains(part), "{part} in {url}");
    }
    assert!(!url.contains("x-amz-content-sha256"), "{url}");
    assert!(offline()
        .presign(
            &call("s3", "GetObject", &input, &a),
            &creds(),
            now,
            Duration::from_secs(8 * 86400)
        )
        .is_err());
}
