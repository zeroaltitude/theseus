//! The credential mints the catalog's tables learned late (theseus-ye7o),
//! through `aws.call`: STS's `GetDelegatedAccessToken` and
//! `GetWebIdentityToken` (the query protocol) and EKS's
//! `AssumeRoleForPodIdentity` (REST-JSON), and ECR's `GetAuthorizationToken`
//! (JSON), which failed closed until `walk` read names in any case. Each is a
//! secret-bearing write:
//! its text and meta hold handles and never a value, and the board holds
//! each value (AWS design §3.5).

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};

const DELEGATED_SECRET: &str = "delegated-secret-0001";
const DELEGATED_TOKEN: &str = "delegated-token-0001";
const WEB_IDENTITY_JWT: &str = "eyJraWQiOi.web-identity-jwt-0002.c2ln";
const POD_SECRET: &str = "pod-secret-0003";
const POD_TOKEN: &str = "pod-token-0003";
/// ECR's token: base64 of `AWS:` and a password.
const ECR_TOKEN: &str = "QVdTOmVjci1wYXNzd29yZC0wMDA0";

fn xml(n: usize, body: String) -> Reply {
    (
        200,
        vec![
            ("x-amzn-requestid", format!("req-{n}")),
            ("content-type", "text/xml".into()),
        ],
        body,
    )
}

/// STS's mints as the query protocol writes them, and EKS's by its path.
fn answers(s: &Seen, n: usize) -> Reply {
    match s.action() {
        Some("GetCallerIdentity") => return sts(ACCOUNT, n),
        Some("GetDelegatedAccessToken") => {
            return xml(
                n,
                format!(
                    "<GetDelegatedAccessTokenResponse><GetDelegatedAccessTokenResult>\
                     <Credentials><AccessKeyId>ASIADELEGATEDEXAMPLE</AccessKeyId>\
                     <SecretAccessKey>{DELEGATED_SECRET}</SecretAccessKey>\
                     <SessionToken>{DELEGATED_TOKEN}</SessionToken>\
                     <Expiration>2026-10-04T00:00:00Z</Expiration></Credentials>\
                     <PackedPolicySize>6</PackedPolicySize>\
                     <AssumedPrincipal>arn:aws:sts::{ACCOUNT}:assumed-role/example/delegated</AssumedPrincipal>\
                     </GetDelegatedAccessTokenResult>\
                     <ResponseMetadata><RequestId>req-{n}</RequestId></ResponseMetadata>\
                     </GetDelegatedAccessTokenResponse>"
                ),
            )
        }
        Some("GetWebIdentityToken") => {
            return xml(
                n,
                format!(
                    "<GetWebIdentityTokenResponse><GetWebIdentityTokenResult>\
                     <WebIdentityToken>{WEB_IDENTITY_JWT}</WebIdentityToken>\
                     <Expiration>2026-10-04T00:00:00Z</Expiration>\
                     </GetWebIdentityTokenResult>\
                     <ResponseMetadata><RequestId>req-{n}</RequestId></ResponseMetadata>\
                     </GetWebIdentityTokenResponse>"
                ),
            )
        }
        _ => {}
    }
    if s.target.ends_with("/assume-role-for-pod-identity") {
        let body = json!({
            "subject": {"namespace": "default", "serviceAccount": "example-app"},
            "audience": "pods.eks.amazonaws.com",
            "podIdentityAssociation": {
                "associationArn": format!("arn:aws:eks:us-west-2:{ACCOUNT}:podidentityassociation/example/a-1"),
                "associationId": "a-1"
            },
            "assumedRoleUser": {
                "arn": format!("arn:aws:sts::{ACCOUNT}:assumed-role/example/eks-pod"),
                "assumeRoleId": "AROAEXAMPLE:eks-pod"
            },
            "credentials": {
                "sessionToken": POD_TOKEN,
                "secretAccessKey": POD_SECRET,
                "accessKeyId": "ASIAPODEXAMPLE",
                "expiration": 1_791_028_800.0
            }
        });
        return (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "application/json".into()),
            ],
            body.to_string(),
        );
    }
    if super::tests_c3::target(s) == Some("GetAuthorizationToken") {
        return super::tests_c3::json_reply(
            n,
            json!({"authorizationData": [{
                "authorizationToken": ECR_TOKEN,
                "expiresAt": 1_791_028_800.0,
                "proxyEndpoint": format!("https://{ACCOUNT}.dkr.ecr.us-west-2.amazonaws.com")
            }]}),
        );
    }
    (400, vec![], "{}".into())
}

/// The three mints, through the tool as the runtime runs a write: planned
/// as a secret-bearing write, run with a binding, and answered with handles.
#[tokio::test]
async fn each_mint_answers_with_handles_and_the_board_holds_its_values() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    for (input, held) in [
        (
            json!({"service": "sts", "operation": "GetDelegatedAccessToken", "input": {"TradeInToken": "example-trade-in"}}),
            vec![(
                "aws-secret:GetDelegatedAccessToken#Credentials",
                vec![DELEGATED_SECRET, DELEGATED_TOKEN],
            )],
        ),
        (
            json!({"service": "sts", "operation": "GetWebIdentityToken", "input": {"Audience": ["https://example.invalid"], "SigningAlgorithm": "RS256"}}),
            vec![(
                "aws-secret:GetWebIdentityToken#WebIdentityToken",
                vec![WEB_IDENTITY_JWT],
            )],
        ),
        (
            json!({"service": "eks-auth", "operation": "AssumeRoleForPodIdentity", "input": {"clusterName": "example", "token": "example-projected-token"}}),
            vec![(
                "aws-secret:AssumeRoleForPodIdentity#credentials",
                vec![POD_SECRET, POD_TOKEN],
            )],
        ),
        // A mint the tables named before (theseus-ye7o found it failing
        // closed): its lower-case `authorizationToken` is held only because
        // `walk` matches `NAMED` in any case.
        (
            json!({"service": "ecr", "operation": "GetAuthorizationToken", "input": {}}),
            vec![(
                "aws-secret:GetAuthorizationToken#authorizationData[0].authorizationToken",
                vec![ECR_TOKEN],
            )],
        ),
    ] {
        let op = input["operation"].as_str().unwrap().to_string();
        let binding = aws.bind("exe_test", "act_test_1", "toolu_test_1");
        let ctx = theseus_tools::ToolCtx {
            aws: Some(binding.clone()),
            ..plain_ctx()
        };
        let plan = t.plan(&input, &ctx).unwrap_or_else(|e| panic!("{op}: {e}"));
        assert_eq!(plan.class, Some(theseus_tools::ToolClass::Write), "{op}");
        let (out, _) = t
            .run_async(&input, &ctx)
            .await
            .unwrap_or_else(|f| panic!("{op}: {}", f.message));
        let said = format!("{} {}", out.text, out.meta);
        let handles: Vec<&str> = held.iter().map(|(h, _)| *h).collect();
        assert_eq!(out.meta["secrets"], json!(handles), "{op}: {said}");
        for (handle, values) in &held {
            assert!(out.text.contains(handle), "{op}: {}", out.text);
            let kept = b
                .get(handle)
                .unwrap_or_else(|| panic!("{handle} is on the board"));
            for v in values {
                assert!(!said.contains(v), "{op}: {v} in {said}");
                assert!(kept.expose().contains(v), "{op}: the board holds {v}");
            }
        }
        let rows: Vec<Value> = binding.requests().into_iter().map(|r| r.row).collect();
        let rows = json!(rows);
        for (_, values) in &held {
            for v in values {
                assert!(
                    !rows.to_string().contains(v),
                    "{op}: {v} in the call's rows"
                );
            }
        }
    }
}
