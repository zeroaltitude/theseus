//! A secret an answer holds only sometimes (theseus-qan5), through
//! `aws.call`: GameLift's build upload keys (JSON; "returned only when the
//! operation is called without a storage location") and DataZone's
//! connection credentials (REST-JSON), each `SecretBearing::WhenPresent`.
//! When the answer holds it, it is held as `Always` holds it (a handle in the
//! text and the meta, the value on the board and nowhere else); when it does
//! not, the answer comes back whole, where `Always` would withhold it all.

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};

const UPLOAD_SECRET: &str = "upload-secret-0001";
const UPLOAD_TOKEN: &str = "upload-token-0001";
const CONNECTION_SECRET: &str = "connection-secret-0002";
const CONNECTION_TOKEN: &str = "connection-token-0002";

/// GameLift's new build, with upload keys unless the call gave a storage
/// location; DataZone's connection, with credentials when its id says so.
fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(ACCOUNT, n);
    }
    if super::tests_c3::target(s) == Some("CreateBuild") {
        let mut b = json!({"Build": {"BuildId": "build-example", "Name": "example-build",
            "Status": "INITIALIZED"}});
        if s.body.contains("StorageLocation") {
            b["StorageLocation"] = json!({"Bucket": "example-bucket", "Key": "build.zip"});
        } else {
            b["UploadCredentials"] = json!({"AccessKeyId": "ASIAUPLOADEXAMPLE",
                "SecretAccessKey": UPLOAD_SECRET, "SessionToken": UPLOAD_TOKEN});
            b["StorageLocation"] = json!({"Bucket": "gamelift-owned", "Key": "upload/build"});
        }
        return super::tests_c3::json_reply(n, b);
    }
    let with = s.target.contains("with-secret");
    if s.target.contains("/connections/") {
        let mut c = json!({"connectionId": "example-connection", "domainId": "dzd_example",
            "name": "example", "type": "REDSHIFT"});
        if with {
            c["connectionCredentials"] = json!({"accessKeyId": "ASIACONNEXAMPLE",
                "secretAccessKey": CONNECTION_SECRET, "sessionToken": CONNECTION_TOKEN,
                "expiration": "2026-10-07T00:00:00Z"});
        }
        return (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "application/json".into()),
            ],
            c.to_string(),
        );
    }
    (400, vec![], "{}".into())
}

/// Each operation with its secret and without: present, a handle and the
/// board; absent, the whole answer and no handle.
#[tokio::test]
async fn a_present_secret_is_held_and_an_absent_one_answers_whole() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    let build =
        |input: Value| json!({"service": "gamelift", "operation": "CreateBuild", "input": input});
    let datazone = |id: &str| {
        json!({"service": "datazone", "operation": "GetConnection",
            "input": {"domainIdentifier": "dzd_example", "identifier": id}})
    };
    for (input, held, plain) in [
        (
            build(json!({"Name": "example-build", "OperatingSystem": "AMAZON_LINUX_2023"})),
            Some((
                "aws-secret:example-build#UploadCredentials",
                vec![UPLOAD_SECRET, UPLOAD_TOKEN],
            )),
            "build-example",
        ),
        (
            build(
                json!({"Name": "example-build", "OperatingSystem": "AMAZON_LINUX_2023",
                "StorageLocation": {"Bucket": "example-bucket", "Key": "build.zip",
                    "RoleArn": format!("arn:aws:iam::{ACCOUNT}:role/example")}}),
            ),
            None,
            "example-bucket",
        ),
        (
            datazone("with-secret"),
            Some((
                "aws-secret:GetConnection#connectionCredentials",
                vec![CONNECTION_SECRET, CONNECTION_TOKEN],
            )),
            "example-connection",
        ),
        (datazone("plain"), None, "example-connection"),
    ] {
        let what = input.to_string();
        let binding = aws.bind("exe_test", "act_test_1", "toolu_test_1");
        let ctx = theseus_tools::ToolCtx {
            aws: Some(binding.clone()),
            ..plain_ctx()
        };
        t.plan(&input, &ctx)
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let (out, _) = t
            .run_async(&input, &ctx)
            .await
            .unwrap_or_else(|f| panic!("{what}: {}", f.message));
        let said = format!("{} {}", out.text, out.meta);
        assert!(out.text.contains(plain), "{what}: the answer in {said}");
        match held {
            Some((handle, values)) => {
                assert_eq!(out.meta["secrets"], json!([handle]), "{what}: {said}");
                assert!(out.text.contains(handle), "{what}: {}", out.text);
                let kept = b
                    .get(handle)
                    .unwrap_or_else(|| panic!("{handle} is on the board"));
                let rows: Vec<Value> = binding.requests().into_iter().map(|r| r.row).collect();
                let rows = json!(rows).to_string();
                for v in values {
                    assert!(!said.contains(v), "{what}: {v} in {said}");
                    assert!(!rows.contains(v), "{what}: {v} in the call's rows");
                    assert!(kept.expose().contains(v), "{what}: the board holds {v}");
                }
            }
            None => {
                assert!(out.meta.get("secrets").is_none(), "{what}: {said}");
                assert!(!out.text.contains("aws-secret:"), "{what}: {}", out.text);
            }
        }
    }
}
