//! P1's tests against the embedded catalog (AWS design §5): every service
//! decodes, golden classifications, the size budget, the decode budget, and
//! the endpoints of the services the design names.

use std::time::{Duration, Instant};

use theseus_aws_catalog::{Catalog, Class, Protocol, RetryClass, SecretBearing};

fn cat() -> &'static Catalog {
    Catalog::embedded().expect("the embedded catalog decodes")
}

#[test]
fn every_service_decodes() {
    let c = cat();
    assert_eq!(c.snapshot(), "aws-cli/2.34.15");
    assert_eq!(c.services().len(), 416);
    let (mut ops, mut shapes) = (0, 0);
    for e in c.services() {
        let s = c
            .service(&e.name)
            .unwrap_or_else(|err| panic!("{}: {err}", e.name));
        assert_eq!(s.name(), e.name);
        assert_eq!(s.operation_count() as u32, e.operations, "{}", e.name);
        ops += s.operation_count();
        shapes += s.shape_count();
        for op in s.operations() {
            // Every operation classifies, and every reference resolves.
            let _ = op.classify();
            for m in op.input().into_iter().flat_map(|i| i.members()) {
                let _ = m.shape().kind();
            }
            let _ = op.output().map(|o| o.member_count());
            let _ = op.errors().count();
        }
    }
    assert_eq!(ops, 17_928);
    assert_eq!(shapes, 111_253);
}

#[test]
fn the_compressed_catalog_is_at_most_4_mb() {
    let len = Catalog::embedded_len();
    assert!(len <= 4 * 1024 * 1024, "the catalog is {len} bytes");
}

/// This thread's CPU time (`CLOCK_THREAD_CPUTIME_ID`), as theseus-core's
/// learning loop reads its own.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes the timespec it is given, nothing else.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// The decode budget, timed by this thread's CPU time, not the wall clock
/// (theseus-rnl3): a decode is pure computation over the embedded blob, so
/// its CPU time is what it costs, and a loaded machine that leaves the test
/// waiting for a core no longer counts against it. What it does not prove: a
/// decode that waited (on a lock, or a read) would pass, since a wait spends
/// no CPU; the catalog has neither.
#[test]
fn one_service_decodes_in_under_5_ms() {
    // The budget is the release build's (the binary's). The gate tests a
    // debug build, which decompresses roughly twenty times slower, so it
    // gets a wider bound; `cargo test --release` checks the real one.
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(50)
    } else {
        Duration::from_millis(5)
    };
    let c = cat();
    for name in ["ec2", "sagemaker", "s3", "sts"] {
        let best = (0..5)
            .map(|_| {
                let (t, wall) = (thread_cpu(), Instant::now());
                let s = c.decode_uncached(name).unwrap();
                assert!(s.operation_count() > 0);
                (thread_cpu() - t, wall.elapsed())
            })
            .min()
            .unwrap();
        eprintln!(
            "decode {name}: {:?} on the CPU, {:?} on the wall",
            best.0, best.1
        );
        assert!(best.0 < budget, "{name} took {:?}, over {budget:?}", best.0);
    }
}

/// `(service, operation, label, retry, idempotency token)`. The label is the
/// catalog's notation: the class, then `$`, `🔑`, and `IaC` as they apply.
const GOLDEN: &[(&str, &str, &str, RetryClass, Option<&str>)] = &[
    // The design's examples (§5, P1).
    ("s3", "ListObjectsV2", "R", RetryClass::SafeToRepeat, None),
    (
        "ec2",
        "RunInstances",
        "W $ IaC",
        RetryClass::IdempotentWithKey,
        Some("ClientToken"),
    ),
    (
        "sqs",
        "ReceiveMessage",
        "W",
        RetryClass::NonRepeatable,
        None,
    ),
    ("lambda", "Invoke", "Run $", RetryClass::NonRepeatable, None),
    (
        "secretsmanager",
        "GetSecretValue",
        "R 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    // §3.1's overrides.
    (
        "sts",
        "GetSessionToken",
        "W 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "sts",
        "GetFederationToken",
        "W 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "athena",
        "StartQueryExecution",
        "W $",
        RetryClass::IdempotentWithKey,
        Some("ClientRequestToken"),
    ),
    (
        "cloudformation",
        "CreateChangeSet",
        "W",
        RetryClass::NonRepeatable,
        None,
    ),
    // The live check's reads, one per protocol family.
    ("iam", "ListRoles", "R", RetryClass::SafeToRepeat, None),
    (
        "ec2",
        "DescribeRegions",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "lambda",
        "ListFunctions",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    ("s3", "ListBuckets", "R", RetryClass::SafeToRepeat, None),
    (
        "route53",
        "ListHostedZones",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "sts",
        "GetCallerIdentity",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "sagemaker",
        "ListEndpoints",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    // The audit's most frequent (§1).
    (
        "sagemaker",
        "DescribeEndpoint",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "cloudformation",
        "DescribeStacks",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "application-autoscaling",
        "DescribeScalableTargets",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "logs",
        "FilterLogEvents",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "service-quotas",
        "ListServiceQuotas",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "cloudformation",
        "DescribeStackEvents",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "iam",
        "SimulatePrincipalPolicy",
        "R",
        RetryClass::SafeToRepeat,
        None,
    ),
    ("ecr", "DescribeImages", "R", RetryClass::SafeToRepeat, None),
    ("ecr", "BatchGetImage", "R", RetryClass::SafeToRepeat, None),
    // §4's rows.
    ("s3", "GetObject", "R", RetryClass::SafeToRepeat, None),
    ("s3", "PutObject", "W", RetryClass::SafeToRepeat, None),
    ("s3", "DeleteObjects", "W", RetryClass::SafeToRepeat, None),
    (
        "s3",
        "CreateBucket",
        "W IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "s3",
        "PutBucketPolicy",
        "W IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "s3",
        "PutBucketTagging",
        "W",
        RetryClass::NonRepeatable,
        None,
    ),
    ("logs", "StartQuery", "R $", RetryClass::SafeToRepeat, None),
    (
        "ce",
        "GetCostAndUsage",
        "R $",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "ecs",
        "RunTask",
        "Run $",
        RetryClass::IdempotentWithKey,
        Some("clientToken"),
    ),
    (
        "batch",
        "SubmitJob",
        "Run $",
        RetryClass::NonRepeatable,
        None,
    ),
    ("ssm", "SendCommand", "Run", RetryClass::NonRepeatable, None),
    (
        "ssm",
        "GetParameter",
        "R 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    ("ssm", "PutParameter", "W", RetryClass::NonRepeatable, None),
    ("kms", "Decrypt", "R 🔑", RetryClass::SafeToRepeat, None),
    (
        "kms",
        "GenerateDataKey",
        "R 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "kms",
        "CreateKey",
        "W $ IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "ec2",
        "StartInstances",
        "W $",
        RetryClass::NonRepeatable,
        None,
    ),
    ("ec2", "StopInstances", "W", RetryClass::NonRepeatable, None),
    (
        "ec2",
        "TerminateInstances",
        "W",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "ec2",
        "AuthorizeSecurityGroupIngress",
        "W IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    ("ec2", "CreateTags", "W", RetryClass::NonRepeatable, None),
    (
        "ec2",
        "CreateKeyPair",
        "W 🔑 IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "iam",
        "CreateRole",
        "W IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "iam",
        "CreateAccessKey",
        "W 🔑 IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    ("sts", "AssumeRole", "W 🔑", RetryClass::SafeToRepeat, None),
    (
        "sso",
        "GetRoleCredentials",
        "W 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "ecr",
        "GetAuthorizationToken",
        "W 🔑",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "route53",
        "ChangeResourceRecordSets",
        "W IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "lambda",
        "CreateFunction",
        "W IaC",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "cloudformation",
        "CreateStack",
        "W IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "cloudtrail",
        "StopLogging",
        "W",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "budgets",
        "ExecuteBudgetAction",
        "W",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "service-quotas",
        "RequestServiceQuotaIncrease",
        "W",
        RetryClass::NonRepeatable,
        None,
    ),
    ("dynamodb", "GetItem", "R", RetryClass::SafeToRepeat, None),
    ("dynamodb", "PutItem", "W", RetryClass::NonRepeatable, None),
    (
        "dynamodb",
        "CreateTable",
        "W $ IaC",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "stepfunctions",
        "StartExecution",
        "Run $",
        RetryClass::SafeToRepeat,
        None,
    ),
    (
        "sagemaker",
        "CreateTrainingJob",
        "Run $",
        RetryClass::NonRepeatable,
        None,
    ),
    (
        "bedrock-runtime",
        "InvokeModel",
        "Run $",
        RetryClass::NonRepeatable,
        None,
    ),
];

#[test]
fn golden_classifications() {
    let c = cat();
    let mut wrong = Vec::new();
    for &(svc, op, label, retry, token) in GOLDEN {
        let s = c.service(svc).unwrap();
        let o = s
            .operation(op)
            .unwrap_or_else(|| panic!("{svc}:{op} is not in the catalog"));
        let k = o.classify();
        let got = (k.label(), k.retry, k.idempotency_token.as_deref());
        if got != (label.to_owned(), retry, token) {
            wrong.push(format!(
                "{svc}:{op}: want {label} {retry:?} {token:?}, got {got:?} ({:?})",
                k.reason
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(GOLDEN.len() >= 50);
}

#[test]
fn the_flags_say_why() {
    let c = cat();
    let ssm = c.service("ssm").unwrap();
    let k = ssm.operation("GetParameter").unwrap().classify();
    assert_eq!(k.secret, SecretBearing::WhenInputTrue("WithDecryption"));
    assert!(k
        .secret
        .for_input(&serde_json::json!({"Name": "x", "WithDecryption": true})));
    assert!(!k.secret.for_input(&serde_json::json!({"Name": "x"})));
    let cfn = c.service("cloudformation").unwrap();
    let k = cfn.operation("CreateChangeSet").unwrap().classify();
    assert!(k.inert && !k.iac_only);
    assert_eq!(k.note, Some("changes nothing until it is executed"));
    let sqs = c.service("sqs").unwrap();
    let k = sqs.operation("ReceiveMessage").unwrap().classify();
    assert_eq!(k.class, Class::Write);
    assert!(k.note.is_some());
}

#[test]
fn the_protocols_are_the_six_and_every_service_has_one() {
    let c = cat();
    let mut seen = std::collections::BTreeMap::new();
    for e in c.services() {
        *seen.entry(e.protocol.as_str()).or_insert(0) += 1;
        assert!(
            e.protocol.is_supported(),
            "{} speaks only {:?}",
            e.name,
            e.protocol
        );
    }
    // CloudWatch prefers CBOR, then JSON; the client takes JSON.
    assert_eq!(c.service("cloudwatch").unwrap().protocol(), Protocol::Json);
    assert_eq!(c.service("ec2").unwrap().protocol(), Protocol::Ec2);
    assert_eq!(c.service("iam").unwrap().protocol(), Protocol::Query);
    assert_eq!(c.service("s3").unwrap().protocol(), Protocol::RestXml);
    eprintln!("protocols: {seen:?}");
}

#[test]
fn endpoints_of_the_named_services() {
    let c = cat();
    let check = |svc: &str, region: &str, url: &str, scope: &str| {
        let e = c.endpoint(svc, region).unwrap();
        assert_eq!(
            (e.url.as_str(), e.signing_region.as_str()),
            (url, scope),
            "{svc} in {region}"
        );
    };
    check("iam", "us-west-2", "https://iam.amazonaws.com", "us-east-1");
    check(
        "route53",
        "us-west-2",
        "https://route53.amazonaws.com",
        "us-east-1",
    );
    check(
        "cloudfront",
        "us-west-2",
        "https://cloudfront.amazonaws.com",
        "us-east-1",
    );
    check(
        "organizations",
        "us-west-2",
        "https://organizations.us-east-1.amazonaws.com",
        "us-east-1",
    );
    check(
        "budgets",
        "us-west-2",
        "https://budgets.amazonaws.com",
        "us-east-1",
    );
    check(
        "ce",
        "us-west-2",
        "https://ce.us-east-1.amazonaws.com",
        "us-east-1",
    );
    check(
        "sts",
        "us-west-2",
        "https://sts.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "s3",
        "us-west-2",
        "https://s3.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "s3",
        "us-east-1",
        "https://s3.us-east-1.amazonaws.com",
        "us-east-1",
    );
    check(
        "ec2",
        "us-west-2",
        "https://ec2.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "lambda",
        "us-west-2",
        "https://lambda.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "sagemaker",
        "us-west-2",
        "https://api.sagemaker.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "logs",
        "us-west-2",
        "https://logs.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "iam",
        "cn-north-1",
        "https://iam.cn-north-1.amazonaws.com.cn",
        "cn-north-1",
    );
    // The signing name is the model's (SageMaker signs as `sagemaker`).
    assert_eq!(
        c.endpoint("sagemaker", "us-west-2").unwrap().signing_name,
        "sagemaker"
    );
    // A region newer than the snapshot still resolves, by its partition.
    assert_eq!(
        c.endpoint("lambda", "us-west-9").unwrap().url,
        "https://lambda.us-west-9.amazonaws.com"
    );
    assert!(c.endpoint("lambda", "us-west-2.evil.example").is_err());
}

/// Operations whose static context parameters choose another host than
/// their service's other operations.
#[test]
fn an_operation_can_have_its_own_endpoint() {
    let c = cat();
    let check = |svc: &str, op: &str, region: &str, url: &str, scope: &str| {
        let s = c.service(svc).unwrap();
        let o = s
            .operation(op)
            .unwrap_or_else(|| panic!("{svc}:{op} is not in the catalog"));
        let e = c.operation_endpoint(o, region).unwrap();
        assert_eq!(
            (e.url.as_str(), e.signing_region.as_str()),
            (url, scope),
            "{svc}:{op} in {region}"
        );
    };
    // Neptune Analytics: the control plane, and the data plane (whose host
    // prefix, the graph's id, the client adds). Its rule set has no endpoint
    // at all without the parameter.
    check(
        "neptune-graph",
        "ListGraphs",
        "us-west-2",
        "https://neptune-graph.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "neptune-graph",
        "ExecuteQuery",
        "us-west-2",
        "https://us-west-2.neptune-graph.amazonaws.com",
        "us-west-2",
    );
    // ARC Region switch: a global control plane, and regional data planes.
    check(
        "arc-region-switch",
        "CreatePlan",
        "us-west-2",
        "https://arc-region-switch-control-plane.us-east-1.api.aws",
        "us-east-1",
    );
    // S3's directory buckets have their own control endpoint.
    check(
        "s3",
        "ListDirectoryBuckets",
        "us-west-2",
        "https://s3express-control.us-west-2.amazonaws.com",
        "us-west-2",
    );
    check(
        "s3",
        "ListBuckets",
        "us-west-2",
        "https://s3.us-west-2.amazonaws.com",
        "us-west-2",
    );
}

#[test]
fn services_answer_to_their_aliases() {
    let c = cat();
    assert_eq!(c.service("states").unwrap().name(), "stepfunctions");
    assert_eq!(c.service("monitoring").unwrap().name(), "cloudwatch");
    assert_eq!(c.service("Cost Explorer").unwrap().name(), "ce");
    let ec2 = c.service("ec2").unwrap();
    assert_eq!(
        ec2.operation("describe-instances").unwrap().name(),
        "DescribeInstances"
    );
}

#[test]
fn describe_is_compact() {
    let c = cat();
    let all = theseus_aws_catalog::describe_services(c);
    assert_eq!(all["count"], 416);
    let s3 = c.service("s3").unwrap();
    let svc = theseus_aws_catalog::describe_service(&s3);
    assert!(svc["operations"]["read"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o == "ListObjectsV2"));
    assert!(svc["iac_only"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o == "CreateBucket"));
    let op = theseus_aws_catalog::describe_operation(s3.operation("ListObjectsV2").unwrap());
    assert_eq!(op["class"], "read");
    assert_eq!(op["input"]["required"], serde_json::json!(["Bucket"]));
    assert_eq!(
        op["paginated"]["result_keys"],
        serde_json::json!(["Contents", "CommonPrefixes"])
    );
    // EC2's largest input stays a reasonable size for a model's context.
    let ec2 = c.service("ec2").unwrap();
    let run = theseus_aws_catalog::describe_operation(ec2.operation("RunInstances").unwrap());
    let len = run.to_string().len();
    assert!(len < 64 * 1024, "RunInstances describes in {len} bytes");
}
