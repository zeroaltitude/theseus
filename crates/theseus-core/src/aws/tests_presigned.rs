//! The shape rule (theseus-3i6d1): a presigned URL is a credential, and no
//! name-based rule can see it.
//!
//! A presigned URL carries its own authorisation — whoever holds the string
//! can perform the operation it was signed for — and it arrives in members
//! called `Location`, `Url`, `presignedUrl`, `PreSignedLogUrl`: names an
//! ordinary URL wears too. `super::presigned` therefore reads the **value**,
//! not the member's name, the shape it sits in, or AWS's `sensitive`
//! marking, and runs on every call rather than only on a secret-bearing one.
//!
//! The tests here measure it in both directions, and in both the easy case
//! and the hard one.
//!
//! * **The easy case** is `gamelift:GetGameSessionLogUrl`, whose member is
//!   literally called `PreSignedUrl`. A name list would have caught it, had
//!   anyone written the row.
//! * **The hard case** is `lambda:GetFunction`, whose `Code.Location` is a
//!   presigned S3 URL with a name that says nothing at all. No list of
//!   member names would ever have been written for it, which is the whole
//!   argument for keying on the value.
//! * **The control is the same member with a different value**: the same
//!   operation, the same code path, an ordinary console URL in `Code.Location`.
//!   It comes through whole. A rule that held every URL would pass the two
//!   cases above and fail this one.
//! * **The breadth** is measured over the embedded catalog: a presigned URL
//!   planted in every string of every operation's output, and an ordinary
//!   URL planted the same way. The first must be held in all of them, the
//!   second in none, and both counts are asserted positive so an empty walk
//!   cannot pass.
//!
//! Every value here is invented: the hosts, the key ids and the signatures.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use theseus_aws::catalog::{Catalog, Kind, SecretBearing, ShapeId, ShapeRef};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};
use super::tests_c3::{json_reply, target};

/// A presigned S3 URL, SigV4: invented bucket, invented key id, invented
/// signature.
const PRESIGNED_S3: &str = "https://example-bucket.s3.us-west-2.amazonaws.com/code.zip\
?X-Amz-Algorithm=AWS4-HMAC-SHA256\
&X-Amz-Credential=EXAMPLEKEYID0001%2F20260101%2Fus-west-2%2Fs3%2Faws4_request\
&X-Amz-Date=20260101T000000Z&X-Amz-Expires=600&X-Amz-SignedHeaders=host\
&X-Amz-Signature=1111111111111111111111111111111111111111111111111111111111111111";

/// A presigned URL for a game session's log, on another invented host.
const PRESIGNED_LOG: &str = "https://example-logs.s3.us-east-1.amazonaws.com/session.log\
?X-Amz-Algorithm=AWS4-HMAC-SHA256\
&X-Amz-Credential=EXAMPLEKEYID0002%2F20260101%2Fus-east-1%2Fs3%2Faws4_request\
&X-Amz-Date=20260101T000000Z&X-Amz-Expires=900&X-Amz-SignedHeaders=host\
&X-Amz-Signature=2222222222222222222222222222222222222222222222222222222222222222";

/// An ordinary URL with a query and a fragment, in the very same member: it
/// authorises nothing and must reach the model whole.
const PLAIN_URL: &str =
    "https://console.example-cloud.test/lambda/home?region=us-west-2#/functions/example-plain";

const FUNCTION: &str = "example-function";
const PLAIN_FUNCTION: &str = "example-plain";
const FUNCTION_ARN: &str = "arn:aws:lambda:us-west-2:111122223333:function:example-function";

fn rest(n: usize, body: Value) -> Reply {
    (
        200,
        vec![
            ("x-amzn-requestid", format!("req-{n}")),
            ("content-type", "application/json".into()),
        ],
        body.to_string(),
    )
}

/// The fake: STS, Lambda's `GetFunction` over rest-json (two functions, one
/// with a presigned URL in `Code.Location` and one with a console link), and
/// GameLift's `GetGameSessionLogUrl` over JSON 1.1.
fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(ACCOUNT, n);
    }
    if target(s) == Some("GetGameSessionLogUrl") {
        return json_reply(n, json!({"PreSignedUrl": PRESIGNED_LOG}));
    }
    if s.target.contains(PLAIN_FUNCTION) {
        return rest(
            n,
            json!({
                "Configuration": {"FunctionName": PLAIN_FUNCTION, "Runtime": "python3.12"},
                "Code": {"RepositoryType": "S3", "Location": PLAIN_URL},
            }),
        );
    }
    if s.target.contains(FUNCTION) {
        return rest(
            n,
            json!({
                "Configuration": {"FunctionName": FUNCTION, "FunctionArn": FUNCTION_ARN,
                                  "Runtime": "python3.12"},
                "Code": {"RepositoryType": "S3", "Location": PRESIGNED_S3},
            }),
        );
    }
    (400, vec![], "{}".into())
}

fn call(service: &str, op: &str, input: Value) -> Value {
    json!({"service": service, "operation": op, "input": input})
}

/// One `aws.call` through the real tool, against the fake.
async fn run(aws: &std::sync::Arc<super::Aws>, input: Value) -> theseus_tools::ToolOutput {
    let t = tool(aws, "aws.call");
    let binding = aws.bind("exe_test", "act_test_1", "toolu_test_1");
    let ctx = theseus_tools::ToolCtx {
        aws: Some(binding),
        ..plain_ctx()
    };
    let what = format!("{}:{}", input["service"], input["operation"]);
    t.plan(&input, &ctx)
        .unwrap_or_else(|e| panic!("{what}: {e}"));
    let (out, _) = t
        .run_async(&input, &ctx)
        .await
        .unwrap_or_else(|f| panic!("{what}: {}", f.message));
    out
}

/// The hard case and the easy one: neither operation is secret-bearing, so
/// before the shape rule `aws.call` printed the whole URL into the model's
/// context. Each must answer with a handle, the board must hold the value,
/// and the rest of the answer must come through.
#[tokio::test]
async fn a_presigned_url_is_held_whatever_its_member_is_called() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let mut bad: Vec<String> = Vec::new();

    for (what, input, path, value, shown) in [
        (
            // The name says nothing: `Code.Location`.
            "lambda:GetFunction",
            call("lambda", "GetFunction", json!({"FunctionName": FUNCTION})),
            "Code.Location",
            PRESIGNED_S3,
            FUNCTION_ARN,
        ),
        (
            // The name says everything, and nothing held it.
            "gamelift:GetGameSessionLogUrl",
            call(
                "gamelift",
                "GetGameSessionLogUrl",
                json!({"GameSessionId": "arn:aws:gamelift:us-east-1::gamesession/example"}),
            ),
            "PreSignedUrl",
            PRESIGNED_LOG,
            "request req-",
        ),
    ] {
        let out = run(&aws, input).await;
        let said = format!("{} {}", out.text, out.meta);
        if said.contains(value) {
            bad.push(format!(
                "MISSED {what}: {path} reaches the model:\n{}",
                out.text
            ));
            continue;
        }
        // The signature alone is enough to look for: a mask that kept it
        // would have kept the credential.
        if said.contains("X-Amz-Signature=") {
            bad.push(format!(
                "{what}: the signature is still shown:\n{}",
                out.text
            ));
        }
        let handles: Vec<String> = out.meta["secrets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| h.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let want = format!("#{path}");
        match handles.iter().find(|h| h.ends_with(&want)) {
            None => bad.push(format!("{what}: no handle for {path} in {handles:?}")),
            Some(h) => {
                match b.get(h) {
                    Some(kept) if kept.expose() == value => {}
                    Some(_) => bad.push(format!("{what}: the board's {path} is not the URL")),
                    None => bad.push(format!("{what}: {h} is not on the board")),
                }
                if !out.text.contains(h) {
                    bad.push(format!("{what}: the handle is not shown:\n{}", out.text));
                }
            }
        }
        // The call did not fail closed: the rest of the answer is there,
        // and the model is told where the URL pointed.
        if !out.text.contains(shown) {
            bad.push(format!(
                "{what}: the rest of the result is gone:\n{}",
                out.text
            ));
        }
        if !out.text.contains("a presigned URL") {
            bad.push(format!(
                "{what}: the mask does not say what it is:\n{}",
                out.text
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} findings:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The control: the same operation and the same member, an ordinary URL.
///
/// It must come through character for character, with nothing masked and
/// nothing held — and the positive count is asserted, so a result that lost
/// everything cannot pass this as "no credential found".
#[tokio::test]
async fn an_ordinary_url_in_the_same_member_still_reaches_the_model() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let out = run(
        &aws,
        call(
            "lambda",
            "GetFunction",
            json!({"FunctionName": PLAIN_FUNCTION}),
        ),
    )
    .await;
    for kept in [
        PLAIN_URL,
        PLAIN_FUNCTION,
        "python3.12",
        "\"RepositoryType\": \"S3\"",
    ] {
        assert!(out.text.contains(kept), "{kept} is gone:\n{}", out.text);
    }
    assert!(
        !out.text.contains("<masked"),
        "something is masked:\n{}",
        out.text
    );
    assert!(
        out.meta.get("secrets").is_none(),
        "nothing should be held: {}",
        out.meta
    );
}

/// The rule reads the value: a table of what is a presigned URL and what
/// merely looks like a URL. Both sides are asserted, so a rule that answered
/// one way for everything fails here.
#[test]
fn the_rule_reads_the_value_and_not_the_name() {
    let yes = [
        PRESIGNED_S3,
        PRESIGNED_LOG,
        // Any case of the scheme.
        "HTTPS://example-bucket.s3.amazonaws.com/k?X-Amz-Credential=A%2Fb&X-Amz-Signature=ab12",
        // A presigned websocket.
        "wss://example-chat.connect.test/?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Signature=cd34",
        // CloudFront, canned policy.
        "https://example.cloudfront.test/a.mp4?Expires=1790000000&Signature=ef56&Key-Pair-Id=\
         APKAEXAMPLEKEYID01",
        // The older query signing.
        "https://s3.example.test/bucket/key?AWSAccessKeyId=EXAMPLEKEYID03&Expires=1790000000\
         &Signature=gh78",
        // A fragment after the query ends the query, not the rule.
        "https://example-bucket.s3.amazonaws.com/k?X-Amz-Credential=A%2Fb&X-Amz-Signature=ij90#p",
    ];
    let no = [
        "",
        PLAIN_URL,
        // An endpoint, no query at all.
        "https://example-bucket.s3.us-west-2.amazonaws.com/code.zip",
        // Not a URL: an ARN, a repository URI, a host.
        FUNCTION_ARN,
        "111122223333.dkr.ecr.us-west-2.amazonaws.com/example:latest",
        // A signature parameter with no value signs nothing.
        "https://example.test/a?X-Amz-Credential=A%2Fb&X-Amz-Signature=",
        // The key without the signature: a canonical request's template.
        "https://example.test/a?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=A%2Fb",
        // The signature without a key: an ordinary field called `signature`.
        "https://example.test/webhook?signature=abc123&event=created",
        // Free text that names one: the rule holds whole values only.
        "the log says GET /k?X-Amz-Signature=ab12&X-Amz-Credential=A%2Fb returned 200",
    ];
    let (mut wrong, mut n) = (Vec::new(), 0usize);
    for s in yes {
        n += 1;
        if !super::presigned::is_presigned(s) {
            wrong.push(format!("not found: {s}"));
        }
    }
    for s in no {
        n += 1;
        if super::presigned::is_presigned(s) {
            wrong.push(format!("false positive: {s}"));
        }
    }
    assert_eq!(n, yes.len() + no.len());
    assert!(n >= 16, "the table must hold both sides: {n} cases");
    assert!(
        wrong.is_empty(),
        "{} findings:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

// ------------------------------------------------- the breadth, over the catalog

/// Synthesize a body for `shape`, every string `plant`.
fn synth(shape: ShapeRef<'_>, plant: &str, seen: &mut HashSet<ShapeId>, depth: usize) -> Value {
    if depth > 6 {
        return Value::Null;
    }
    match shape.kind() {
        Kind::Structure => {
            if !seen.insert(shape.id()) {
                return Value::Null;
            }
            let mut o = Map::new();
            for m in shape.members() {
                let v = synth(m.shape(), plant, seen, depth + 1);
                if !v.is_null() {
                    o.insert(m.name().to_string(), v);
                }
            }
            seen.remove(&shape.id());
            if o.is_empty() {
                Value::Null
            } else {
                Value::Object(o)
            }
        }
        Kind::List => shape.list_member().map_or(Value::Null, |m| {
            let v = synth(m.shape(), plant, seen, depth + 1);
            if v.is_null() {
                Value::Null
            } else {
                json!([v])
            }
        }),
        Kind::Map => shape.map_value().map_or(Value::Null, |m| {
            let v = synth(m.shape(), plant, seen, depth + 1);
            if v.is_null() {
                Value::Null
            } else {
                json!({ "k": v })
            }
        }),
        Kind::String => json!(plant),
        Kind::Blob => json!({"base64": "ZXhhbXBsZQ=="}),
        Kind::Boolean => json!(true),
        Kind::Integer | Kind::Long => json!(1),
        Kind::Float | Kind::Double => json!(1.0),
        Kind::Timestamp => json!(1_791_028_800.0),
    }
}

fn carries(v: &Value, plant: &str) -> bool {
    serde_json::to_string(v).unwrap_or_default().contains(plant)
}

/// Over the whole embedded catalog, as `tools.rs` runs it: the real
/// classification, then the name walk for a secret-bearing call, then the
/// shape rule for every call.
///
/// A presigned URL planted anywhere in any operation's output is held, and
/// an ordinary URL planted in exactly the same places is held in none of
/// them. Both counts are asserted positive: "zero leaks" over an empty set
/// of operations would otherwise pass.
#[test]
fn a_presigned_url_is_held_in_every_operations_output_and_an_ordinary_one_in_none() {
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let b = board();
    // A marker the planted URL ends with, so a partial mask is caught too.
    let signature =
        "X-Amz-Signature=1111111111111111111111111111111111111111111111111111111111111111";
    let (mut planted, mut leaked, mut plain_planted, mut plain_held) =
        (0usize, 0usize, 0usize, 0usize);
    let mut first: Vec<String> = Vec::new();
    for e in c.services() {
        let svc = c.service(&e.name).expect("the service decodes");
        for op in svc.operations() {
            let Some(output) = op.output() else { continue };
            let cl = op.classify();
            let secret = cl.secret != SecretBearing::No && cl.secret.for_input(&json!({}));

            let mut body = synth(output, PRESIGNED_S3, &mut HashSet::new(), 0);
            if !carries(&body, signature) {
                continue;
            }
            planted += 1;
            if secret {
                let _ = super::secret::hold(&b, &e.name, op.name(), &json!({}), &mut body);
            }
            let _ = super::presigned::hold(&b, op.name(), &mut body);
            if carries(&body, signature) {
                leaked += 1;
                if first.len() < 10 {
                    first.push(format!("{}:{}", e.name, op.name()));
                }
            }

            // The same places, an ordinary URL: the shape rule must hold
            // none of them. (The name walk may still hold some by name,
            // which is not this rule's doing, so it is not run here.)
            let mut plain = synth(output, PLAIN_URL, &mut HashSet::new(), 0);
            if !carries(&plain, PLAIN_URL) {
                continue;
            }
            plain_planted += 1;
            if !super::presigned::hold(&b, op.name(), &mut plain).is_empty() {
                plain_held += 1;
                if first.len() < 20 {
                    first.push(format!("false positive {}:{}", e.name, op.name()));
                }
            }
        }
    }
    assert!(
        planted > 10_000,
        "the walk found only {planted} operations to plant a URL in: it is not measuring the \
         catalog"
    );
    assert_eq!(
        plain_planted, planted,
        "the two plants must cover the same operations"
    );
    assert_eq!(
        (leaked, plain_held),
        (0, 0),
        "of {planted} operations, a presigned URL still reaches the model in {leaked} and an \
         ordinary URL is wrongly held in {plain_held}: {first:?}"
    );
}

/// The query parameters a presigned URL carries that are **not** signature
/// material: the time it was made, how long it lives, which headers the
/// signature covers, and the session token of temporary credentials (a
/// parameter that is part of the canonical query but is not signature
/// material, `x-amz-security-token`).
/// Everything else the real signer writes must be on
/// `presigned`'s vocabulary.
const NOT_VOCABULARY: &[&str] = &[
    "x-amz-date",
    "x-amz-expires",
    "x-amz-signedheaders",
    "x-amz-security-token",
];

/// **Completeness for `presigned`'s vocabulary** (theseus-3i6d1), in the shape
/// of `crates/complete.rs`: the world is the URL the repository's own SigV4
/// signer writes (`theseus_aws::sign::presign_request`), the list is
/// `SIGNATURE` plus `KEYED_BY`. A new query parameter the signer starts to
/// write fails here, naming itself, instead of leaving a presigned URL the
/// rule no longer recognises. It reaches only the SigV4 spelling: the older
/// query signing and CloudFront's are AWS's, not the signer's, and are
/// listed by hand (`stale_fails` is off for that reason).
#[test]
fn the_presigned_vocabulary_holds_every_parameter_the_real_signer_writes() {
    use std::time::{Duration, UNIX_EPOCH};
    use theseus_aws::sign::{presign_request, SigningScope};
    use theseus_aws::{Credentials, HttpRequest};

    let req = HttpRequest {
        method: "GET",
        origin: "https://example-bucket.s3.us-west-2.amazonaws.com".into(),
        path: "/code.zip".into(),
        query: vec![],
        headers: vec![],
        body: vec![],
    };
    let scope = SigningScope {
        region: "us-west-2".into(),
        service: "s3".into(),
        s3: true,
        unsigned_payload: true,
    };
    // A session token too, so `X-Amz-Security-Token` is in the walk.
    let creds = Credentials::new(
        "EXAMPLEKEYID0001",
        "example-secret-0001",
        Some("example-session-token-0001".into()),
        None,
    );
    let url = presign_request(
        &req,
        &creds,
        &scope,
        UNIX_EPOCH + Duration::from_secs(1_790_000_000),
        Duration::from_secs(600),
    )
    .expect("the signer presigns");
    assert!(
        super::presigned::is_presigned(&url),
        "the real signer's own URL is not recognised as presigned: {url}"
    );
    let query = url.split_once('?').expect("a query").1;
    let found: Vec<(String, String)> = query
        .split('&')
        .filter_map(|p| p.split('=').next())
        .map(str::to_ascii_lowercase)
        .filter(|n| !NOT_VOCABULARY.contains(&n.as_str()))
        .map(|n| (n, "written by sign::presign_request".to_string()))
        .collect();
    let listed = super::presigned::SIGNATURE
        .iter()
        .chain(super::presigned::KEYED_BY)
        .map(|n| (n.to_string(), String::new()));
    let spec = crate::complete::MustBeComplete {
        item: "a query parameter of a presigned URL that carries signature material",
        list: "presigned.rs's SIGNATURE and KEYED_BY",
        world: "the URL theseus_aws::sign::presign_request writes",
        add: "add it to SIGNATURE (it is the signature) or KEYED_BY (it names the key), or, if it \
              carries neither, to NOT_VOCABULARY in tests_presigned.rs with a reason",
        drop: "",
        least: 3,
        stale_fails: false,
    };
    crate::complete::check(&spec, listed, found);
}

/// **What can reach `out.next`** (theseus-3i6d1): the continuation `call_text`
/// prints beside the body, unmasked. Measured over the embedded catalog:
/// every paginator's output token. None is a signature, a key, or a URL: they
/// are opaque cursors (`NextToken`, `Marker`), resource ids (`Jobs[-1].JobId`,
/// `ShardId`) and one data key (`Contents[-1].Key`, an object's name). So
/// `out.next` is not masked: a handle in a token's place would break the
/// "call again with these in `input`" the text offers, for a leak nobody can
/// point to. This census pins the measurement: a token expression the catalog
/// gains fails, naming itself, to be judged the same way.
#[test]
fn no_paginator_token_names_a_credential_or_a_url() {
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let mut found: std::collections::BTreeMap<String, String> = Default::default();
    for e in c.services() {
        let svc = c.service(&e.name).expect("the service decodes");
        for op in svc.operations() {
            if let Some(p) = op.paginator() {
                for t in p.output_tokens() {
                    found
                        .entry(t.to_string())
                        .or_insert(format!("{}:{}", e.name, op.name()));
                }
            }
        }
    }
    for t in found.keys() {
        let last = t.rsplit('.').next().unwrap_or(t).to_ascii_lowercase();
        for bad in [
            "signature",
            "credential",
            "securitytoken",
            "secret",
            "password",
            "url",
            "uri",
        ] {
            assert!(
                !last.contains(bad),
                "paginator token {t} names {bad}: judge `out.next`"
            );
        }
    }
    let listed = MEASURED_TOKENS
        .iter()
        .map(|t| (t.to_string(), String::new()));
    let spec = crate::complete::MustBeComplete {
        item: "a paginator's output-token expression",
        list: "MEASURED_TOKENS in tests_presigned.rs",
        world: "every paginator in the embedded catalog",
        add: "read what the token carries; if it is an opaque cursor or an id, add it; if it can \
              carry a signed or keyed value, mask `out.next` in tools.rs `call_text`",
        drop: "",
        least: 30,
        stale_fails: false,
    };
    crate::complete::check(&spec, listed, found);
}

/// The 37 distinct output-token expressions of the catalog's paginators,
/// measured 2026-11-09, each judged: a cursor, an id, or one data key.
const MEASURED_TOKENS: &[&str] = &[
    "CloudFrontOriginAccessIdentityList.NextMarker",
    "ContinuationToken",
    "DefaultClusterParameters.Marker",
    "DistributionList.NextMarker",
    "EngineDefaults.Marker",
    "InvalidationList.NextMarker",
    "Jobs[-1].JobId",
    "KeyValueStoreList.NextMarker",
    "LastEvaluatedBackupArn",
    "LastEvaluatedKey",
    "LastEvaluatedTableName",
    "Marker",
    "NextContinuationToken",
    "NextKeyMarker",
    "NextMarker",
    "NextMarker || Contents[-1].Key",
    "NextPageMarker",
    "NextPageToken",
    "NextPartNumberMarker",
    "NextRecordIdentifier",
    "NextRecordName",
    "NextRecordType",
    "NextToken",
    "NextUploadIdMarker",
    "NextVersionIdMarker",
    "OriginAccessControlList.NextMarker",
    "PageToken",
    "PaginationToken",
    "PublicKeyList.NextMarker",
    "StreamDescription.Shards[-1].ShardId",
    "StreamingDistributionList.NextMarker",
    "continuationToken",
    "marker",
    "nextMarker",
    "nextPageToken",
    "nextToken",
    "position",
];

/// The five operations the second review named by hand (theseus-3i6d1): the
/// mailmanager address-list import jobs' `PreSignedUrl` and security-ir's
/// `attachmentPresignedUrl`. The catalog-wide test above uses the same `synth`,
/// so it could not tell if these five were never planted: its `planted >
/// 10_000` and zero leaks stay true without them. What this one closes is the
/// positive count: each must be planted before the hold and held after it, and
/// the count must be 5.
#[test]
fn the_five_named_operations_are_planted_and_held() {
    const NAMED_FIVE: [(&str, &str); 5] = [
        ("mailmanager", "CreateAddressListImportJob"),
        ("mailmanager", "GetAddressListImportJob"),
        ("mailmanager", "ListAddressListImportJobs"),
        ("security-ir", "GetCaseAttachmentDownloadUrl"),
        ("security-ir", "GetCaseAttachmentUploadUrl"),
    ];
    let signature =
        "X-Amz-Signature=1111111111111111111111111111111111111111111111111111111111111111";
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let b = board();
    let (mut held, mut bad) = (0usize, Vec::new());
    for (service, op_name) in NAMED_FIVE {
        let svc = c.service(service).expect("the service decodes");
        let Some(op) = svc.operations().find(|o| o.name() == op_name) else {
            bad.push(format!("{service}:{op_name} is not in the catalog"));
            continue;
        };
        let Some(output) = op.output() else {
            bad.push(format!("{service}:{op_name} has no output"));
            continue;
        };
        let cl = op.classify();
        let secret = cl.secret != SecretBearing::No && cl.secret.for_input(&json!({}));
        let mut body = synth(output, PRESIGNED_S3, &mut HashSet::new(), 0);
        if !carries(&body, signature) {
            bad.push(format!("{service}:{op_name} was not planted"));
            continue;
        }
        if secret {
            let _ = super::secret::hold(&b, service, op_name, &json!({}), &mut body);
        }
        let _ = super::presigned::hold(&b, op_name, &mut body);
        if carries(&body, signature) {
            bad.push(format!("{service}:{op_name} still carries the signature"));
        } else {
            held += 1;
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(held, 5, "every one of the five is planted and held");
}
