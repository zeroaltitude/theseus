//! The AWS session mints wait for the operator (theseus-a3s3; the owner,
//! 2026-10-07: "allow request to operator, not refuse"): STS `AssumeRole*`,
//! `AssumeRoot`, `GetSessionToken`, `GetFederationToken` and
//! `GetDelegatedAccessToken`, called by the model through `aws.call`, wait
//! for approval at every posture, whatever `[policy.aws]` or
//! `[policy.tools]` says; the approval names the call, its role or target,
//! and that its keys stay held; an approved one runs with its keys held as
//! handles, a declined one never reaches AWS. `GetWebIdentityToken` mints no
//! AWS session and keeps its posture.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};
use crate::policy::{Posture, ToolPolicy};
use crate::provider::Scripted;

const MINTED_SECRET: &str = "minted-secret-0001";
const MINTED_TOKEN: &str = "minted-token-0001";
const ROLE: &str = "arn:aws:iam::111122223333:role/example-deployer";

/// STS: the account check, and `AssumeRole`'s keys.
fn answers(s: &Seen, n: usize) -> Reply {
    match s.action() {
        Some("GetCallerIdentity") => sts(ACCOUNT, n),
        Some("AssumeRole") => (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "text/xml".into()),
            ],
            format!(
                "<AssumeRoleResponse><AssumeRoleResult><Credentials>\
                 <AccessKeyId>ASIAMINTEDEXAMPLE</AccessKeyId>\
                 <SecretAccessKey>{MINTED_SECRET}</SecretAccessKey>\
                 <SessionToken>{MINTED_TOKEN}</SessionToken>\
                 <Expiration>2026-10-07T00:00:00Z</Expiration></Credentials>\
                 <AssumedRoleUser><Arn>arn:aws:sts::{ACCOUNT}:assumed-role/example-deployer/model</Arn>\
                 <AssumedRoleId>AROAEXAMPLE:model</AssumedRoleId></AssumedRoleUser>\
                 </AssumeRoleResult><ResponseMetadata><RequestId>req-{n}</RequestId>\
                 </ResponseMetadata></AssumeRoleResponse>"
            ),
        ),
        _ => (400, vec![], "<ErrorResponse/>".into()),
    }
}

fn policy(aws: &[(&str, Posture)], tools: &[(&str, Posture)]) -> ToolPolicy {
    ToolPolicy {
        roots: vec![],
        approve_paths: vec![],
        allow_argv: vec![],
        approve_argv: vec![],
        enforcement: Posture::Open,
        tools: tools.iter().map(|(k, p)| (k.to_string(), *p)).collect(),
        mcp: BTreeMap::new(),
        aws: aws.iter().map(|(k, p)| (k.to_string(), *p)).collect(),
        confirmer: "operator".into(),
        floor_paths: vec![],
        floor_argv: crate::policy::floor_argv(),
        private_addresses: Default::default(),
    }
}

/// Each mint, with the words its approval names it by.
fn mints() -> Vec<(Value, &'static str)> {
    let call = |op: &str, input: Value| json!({"service": "sts", "operation": op, "input": input});
    vec![
        (
            call(
                "AssumeRole",
                json!({"RoleArn": ROLE, "RoleSessionName": "model"}),
            ),
            "role arn:aws:iam::111122223333:role/example-deployer",
        ),
        (
            call(
                "AssumeRoleWithWebIdentity",
                json!({"RoleArn": ROLE, "RoleSessionName": "model", "WebIdentityToken": "example-oidc-token"}),
            ),
            "role arn:aws:iam::111122223333:role/example-deployer",
        ),
        (
            call(
                "AssumeRoot",
                json!({"TargetPrincipal": "444455556666",
                    "TaskPolicyArn": {"arn": "arn:aws:iam::aws:policy/root-task/IAMAuditRootUserCredentials"}}),
            ),
            "root of account 444455556666, for task policy arn:aws:iam::aws:policy/root-task/IAMAuditRootUserCredentials",
        ),
        (
            call("GetSessionToken", json!({})),
            "a session of the signing identity itself",
        ),
        (
            call("GetFederationToken", json!({"Name": "example-user"})),
            "a session for federated user example-user",
        ),
        (
            call(
                "GetDelegatedAccessToken",
                json!({"TradeInToken": "example-trade-in-0002"}),
            ),
            "a session delegated by its trade-in token",
        ),
    ]
}

/// Under `[policy.aws]` at open or notify, by class, service or operation,
/// and under a `[policy.tools]` line that opens `aws.call`: each mint waits
/// for approval, named by its call and its role or target, with its keys
/// held; never as the floor, and never with a token from its input.
/// `GetWebIdentityToken` takes the line's posture.
#[test]
fn a_session_mint_waits_for_approval_at_every_posture() {
    let fake = Fake::start(answers);
    let aws = layer(&fake, board());
    let t = tool(&aws, "aws.call");
    for (aws_lines, tools) in [
        (vec![("write", Posture::Open)], vec![]),
        (vec![("write", Posture::Notify)], vec![]),
        (vec![("sts", Posture::Open)], vec![]),
        (vec![("sts:AssumeRole", Posture::Open)], vec![]),
        (vec![], vec![("aws.call", Posture::Open)]),
    ] {
        let p = policy(&aws_lines, &tools);
        for (input, target) in mints() {
            let op = input["operation"].as_str().unwrap();
            let plan = t
                .plan(&input, &plain_ctx())
                .unwrap_or_else(|e| panic!("{op}: {e}"));
            let d = p.decide(t.as_ref(), &plan);
            let at = format!("{op} under {aws_lines:?} {tools:?}");
            assert_eq!(d.posture, Posture::Approve, "{at}: {}", d.reason);
            assert!(!d.floor, "{at}");
            for words in [
                format!("write sts:{op} in us-west-2"),
                format!("an AWS session mint, {target}"),
                "the credentials it mints stay held as secret handles".into(),
            ] {
                assert!(d.reason.contains(&words), "{at}: {words} in {}", d.reason);
            }
            assert!(!d.reason.contains("example-trade-in"), "{}", d.reason);
            assert!(!d.reason.contains("example-oidc-token"), "{}", d.reason);
        }
        let web = json!({"service": "sts", "operation": "GetWebIdentityToken",
            "input": {"Audience": ["https://example.invalid"], "SigningAlgorithm": "RS256"}});
        let plan = t.plan(&web, &plain_ctx()).unwrap();
        assert_eq!(plan.aws.as_ref().unwrap().session_mint, None);
        let d = p.decide(t.as_ref(), &plan);
        let line = aws_lines.first().map_or(Posture::Open, |(_, p)| *p);
        assert_eq!(d.posture, line, "GetWebIdentityToken: {}", d.reason);
    }
}

/// Through the whole core under `[policy.aws] write = "open"`: the model's
/// `AssumeRole` waits; approved, it runs, and its keys are handles on the
/// board, in no message to the model; declined, it is refused and AWS never
/// sees it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approved_mint_runs_held_and_a_declined_one_is_refused() {
    for approve in [true, false] {
        let fake = Fake::start(answers);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
        cfg.policy.enforcement = Posture::Open;
        cfg.policy.aws.insert("write".into(), Posture::Open);
        cfg.aws = super::tests::account(&fake.url);
        let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
        let mint = json!({"service": "sts", "operation": "AssumeRole",
            "input": {"RoleArn": ROLE, "RoleSessionName": "model"}});
        let model = Arc::new(crate::provider::FakeProvider::scripted(vec![
            Scripted::tools("", &[("t_mint", "aws_call", mint)]),
            Scripted::text("Done."),
        ]));
        let b = board();
        let core = crate::Core::build(crate::rpc::Parts {
            secrets: b.clone(),
            ..crate::rpc::Parts::for_tests(cfg, model.clone(), store)
        })
        .unwrap();

        let first = super::tests::turn(&core, "assume the deployer role").await;
        assert_eq!(first.stop_reason, "awaiting_confirm", "{first:?}");
        let pending = core.pending_confirms(&first.session_id).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(
            pending[0]
                .reason
                .contains(&format!("an AWS session mint, role {ROLE}")),
            "{}",
            pending[0].reason
        );
        let minted = |f: &Fake| f.seen().iter().any(|s| s.action() == Some("AssumeRole"));
        assert!(!minted(&fake), "nothing is sent before the approval");

        core.confirm_action(&pending[0].correlation_id, approve, None, "test")
            .unwrap();
        let exec = first.execution_id.clone().unwrap();
        let cont = core.continue_execution(&exec).await.unwrap().unwrap();
        assert_eq!(cont.output, "Done.");
        let asked = format!("{:?}", model.requests());
        assert!(!asked.contains(MINTED_SECRET) && !asked.contains(MINTED_TOKEN));
        let handle = "aws-secret:model#Credentials";
        if approve {
            assert!(minted(&fake), "the approved mint ran");
            assert!(asked.contains(handle), "the model sees the handle");
            let kept = b.get(handle).expect("the keys are on the board");
            assert!(kept.expose().contains(MINTED_SECRET));
            assert!(kept.expose().contains(MINTED_TOKEN));
            assert_eq!(super::tests::ledgered(&core, "aws.called").len(), 1);
        } else {
            assert!(!minted(&fake), "a declined mint never reaches AWS");
            assert!(b.get(handle).is_none());
            assert!(super::tests::ledgered(&core, "aws.called").is_empty());
        }
    }
}
