//! The client's live check (AWS design §5, P2): one read per protocol
//! family against a real account, printing counts and shapes, never values.
//!
//! ```text
//! AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=… AWS_REGION=us-west-2 \
//!     cargo run -p theseus-aws --example aws-live-reads
//! ```
//!
//! Every call is checked to classify as a read before it is sent, so this
//! never writes. `THESEUS_AWS_EXPECT_ACCOUNT`, if set, is compared with
//! STS's answer, and only the comparison is printed.

use serde_json::{json, Value};
use theseus_aws::catalog::{Catalog, Class};
use theseus_aws::{Attribution, Call, Client, ClientConfig, Credentials};

/// A member's shape, without its value: `Roles[3]`, `Owner{2}`, `IsTruncated`.
fn shape(body: &Value) -> String {
    match body {
        Value::Object(o) => o
            .iter()
            .map(|(k, v)| match v {
                Value::Array(a) => format!("{k}[{}]", a.len()),
                Value::Object(m) => format!("{k}{{{}}}", m.len()),
                _ => k.clone(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

#[tokio::main(flavor = "current_thread")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn main() {
    let key = std::env::var("AWS_ACCESS_KEY_ID").expect("AWS_ACCESS_KEY_ID");
    let secret = std::env::var("AWS_SECRET_ACCESS_KEY").expect("AWS_SECRET_ACCESS_KEY");
    let token = std::env::var("AWS_SESSION_TOKEN").ok();
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".into());
    let creds = Credentials::new(key, secret, token, None);
    let client = Client::new(ClientConfig::new(region));
    let catalog = Catalog::embedded().expect("the catalog");
    let attribution = Attribution {
        execution: Some("exe_live_check".into()),
        call: Some("call_live_check".into()),
    };
    let calls: [(&str, &str, Value, u32); 8] = [
        ("sts", "GetCallerIdentity", json!({}), 1),
        ("iam", "ListRoles", json!({"MaxItems": 2}), 3),
        ("ec2", "DescribeRegions", json!({}), 1),
        ("dynamodb", "ListTables", json!({"Limit": 10}), 1),
        ("sagemaker", "ListEndpoints", json!({"MaxResults": 10}), 1),
        ("lambda", "ListFunctions", json!({"MaxItems": 10}), 1),
        ("s3", "ListBuckets", json!({}), 1),
        ("route53", "ListHostedZones", json!({"MaxItems": "10"}), 1),
    ];
    let mut failed = 0;
    for (service, operation, input, pages) in calls {
        let svc = catalog.service(service).expect("a service");
        let op = svc.operation(operation).expect("an operation");
        let class = op.classify().class;
        assert_eq!(class, Class::Read, "{service}:{operation} is not a read");
        let call = Call {
            service,
            operation,
            input: &input,
            region: None,
            pages,
            attribution: &attribution,
        };
        let started = std::time::Instant::now();
        match client.call(&call, &creds).await {
            Ok(out) => {
                let mut line = format!(
                    "{:<26} {:<10} {} pages {} attempts {} request-id {} {:>5} ms  {}",
                    format!("{service}:{operation}"),
                    svc.protocol().as_str(),
                    out.status,
                    out.pages,
                    out.attempts,
                    if out.request_id.is_some() {
                        "yes"
                    } else {
                        "no"
                    },
                    started.elapsed().as_millis(),
                    shape(&out.body)
                );
                if service == "sts" {
                    if let Ok(want) = std::env::var("THESEUS_AWS_EXPECT_ACCOUNT") {
                        let got = out.body["Account"].as_str().unwrap_or_default();
                        line.push_str(if got == want {
                            "  account: as expected"
                        } else {
                            "  account: DIFFERS"
                        });
                    }
                }
                println!("{line}");
            }
            Err(e) => {
                failed += 1;
                // AWS's code and enforcer only: a message can name resources.
                match e.aws() {
                    Some(a) => println!(
                        "{service}:{operation} FAILED: {} (HTTP {}) enforcer {:?}",
                        a.code,
                        a.status,
                        a.denial.as_ref().map(|d| d.enforcer)
                    ),
                    None => println!("{service}:{operation} FAILED: {e}"),
                }
            }
        }
    }
    // Errors, one per protocol family: reads of what does not exist, so AWS
    // answers each with its error, which must read as the code expected.
    let missing: [(&str, &str, Value, &str); 7] = [
        (
            "iam",
            "GetRole",
            json!({"RoleName": "theseus-live-check-no-such-role"}),
            "NoSuchEntity",
        ),
        // EC2 calls the documentation's example id malformed, not missing.
        (
            "ec2",
            "DescribeInstances",
            json!({"InstanceIds": ["i-1234567890abcdef0"]}),
            "InvalidInstanceID.Malformed",
        ),
        (
            "dynamodb",
            "DescribeTable",
            json!({"TableName": "theseus-live-check-no-such-table"}),
            "ResourceNotFoundException",
        ),
        (
            "sagemaker",
            "DescribeEndpoint",
            json!({"EndpointName": "theseus-live-check-no-such-endpoint"}),
            "ValidationException",
        ),
        (
            "lambda",
            "GetFunction",
            json!({"FunctionName": "theseus-live-check-no-such-function"}),
            "ResourceNotFoundException",
        ),
        (
            "route53",
            "GetHostedZone",
            json!({"Id": "Z0THESEUSLIVECHECK0"}),
            "NoSuchHostedZone",
        ),
        (
            "s3",
            "HeadBucket",
            json!({"Bucket": "theseus-live-check-no-such-bucket-7f3a91c2"}),
            "404",
        ),
    ];
    for (service, operation, input, want) in missing {
        let svc = catalog.service(service).expect("a service");
        let op = svc.operation(operation).expect("an operation");
        assert_eq!(
            op.classify().class,
            Class::Read,
            "{service}:{operation} is not a read"
        );
        let call = Call {
            service,
            operation,
            input: &input,
            region: None,
            pages: 1,
            attribution: &attribution,
        };
        match client.call(&call, &creds).await {
            Err(e) => match e.aws() {
                Some(a) => {
                    let ok = a.code == want;
                    failed += usize::from(!ok);
                    println!(
                        "{:<26} {:<10} error {} (HTTP {}) request-id {} {}",
                        format!("{service}:{operation}"),
                        svc.protocol().as_str(),
                        a.code,
                        a.status,
                        if a.request_id.is_some() { "yes" } else { "no" },
                        if ok { "as expected" } else { "UNEXPECTED" }
                    );
                }
                None => {
                    failed += 1;
                    println!("{service}:{operation} FAILED: {e}");
                }
            },
            Ok(_) => {
                failed += 1;
                println!("{service}:{operation} answered, but should not exist");
            }
        }
    }
    std::process::exit(i32::from(failed > 0));
}
