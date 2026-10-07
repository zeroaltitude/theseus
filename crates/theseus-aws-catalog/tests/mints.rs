//! The credential mints (theseus-ye7o; pinned by theseus-b586): every
//! operation whose `CLASS` row carries the MINT note is a secret-bearing
//! write, and each is listed here with its retry class. So a mint that loses
//! one of its rows (`CLASS`, `SECRET`, `RETRY`), or a new one added to some
//! of the tables and not the others, fails here (AWS design §3.1, "every STS
//! credential mint").

use theseus_aws_catalog::{Catalog, Class, RetryClass, SecretBearing};
use RetryClass::{NonRepeatable as Once, SafeToRepeat as Again};

/// What the override table says of a mint.
const MINT: &str = "mints a credential, whatever its name says";

/// Every mint in the catalog, with its retry class. A repeat of most mints
/// another short-lived credential (`Again`); those left `Once` are main's
/// own older rows, which no `RETRY` row has settled, a code that a repeat
/// would redeem twice (signin's, amplifyuibuilder's exchange), and
/// mediapackage's rotations, whose repeat would rotate the password again.
const MINTS: &[(&str, &str, RetryClass)] = &[
    ("sts", "AssumeRole", Again),
    ("sts", "AssumeRoleWithSAML", Again),
    ("sts", "AssumeRoleWithWebIdentity", Again),
    ("sts", "AssumeRoot", Again),
    ("sts", "GetSessionToken", Again),
    ("sts", "GetFederationToken", Again),
    ("sts", "GetDelegatedAccessToken", Again),
    ("sts", "GetWebIdentityToken", Again),
    ("sso", "GetRoleCredentials", Again),
    ("eks-auth", "AssumeRoleForPodIdentity", Again),
    ("deadline", "AssumeFleetRoleForRead", Again),
    ("deadline", "AssumeFleetRoleForWorker", Again),
    ("deadline", "AssumeQueueRoleForRead", Again),
    ("deadline", "AssumeQueueRoleForUser", Again),
    ("deadline", "AssumeQueueRoleForWorker", Again),
    ("s3", "CreateSession", Again),
    ("s3control", "GetDataAccess", Again),
    ("ssm", "GetAccessToken", Again),
    (
        "lakeformation",
        "GetTemporaryDataLocationCredentials",
        Again,
    ),
    ("lakeformation", "AssumeDecoratedRoleWithSAML", Again),
    ("gamelift", "GetComputeAccess", Again),
    ("gamelift", "GetInstanceAccess", Again),
    ("gamelift", "RequestUploadCredentials", Again),
    ("finspace-data", "GetProgrammaticAccessCredentials", Again),
    ("finspace-data", "GetExternalDataViewAccessDetails", Again),
    ("emr", "GetClusterSessionCredentials", Again),
    (
        "emr-containers",
        "GetManagedEndpointSessionCredentials",
        Again,
    ),
    ("datazone", "GetEnvironmentCredentials", Again),
    // Redeems an authorization code: "NOT idempotent", says its model.
    ("signin", "CreateOAuth2Token", Once),
    // Spends an access code.
    ("amplifyuibuilder", "ExchangeCodeForToken", Once),
    ("amplifyuibuilder", "RefreshToken", Again),
    ("bedrock-agentcore", "GetWorkloadAccessToken", Again),
    ("bedrock-agentcore", "GetWorkloadAccessTokenForJWT", Again),
    (
        "bedrock-agentcore",
        "GetWorkloadAccessTokenForUserId",
        Again,
    ),
    ("bedrock-agentcore", "GetResourceOauth2Token", Again),
    ("connect", "GetFederationToken", Again),
    ("ivs-realtime", "CreateParticipantToken", Again),
    ("ivschat", "CreateChatToken", Again),
    ("mwaa", "CreateCliToken", Again),
    ("mwaa", "CreateWebLoginToken", Again),
    ("license-manager", "GetAccessToken", Again),
    ("redshift", "GetIdentityCenterAuthToken", Again),
    ("redshift-serverless", "GetIdentityCenterAuthToken", Again),
    ("workmail", "AssumeImpersonationRole", Again),
    ("kinesis-video-signaling", "GetIceServerConfig", Again),
    ("mediapackage", "RotateChannelCredentials", Once),
    ("mediapackage", "RotateIngestEndpointCredentials", Once),
    ("cognito-identity", "GetCredentialsForIdentity", Once),
    ("cognito-identity", "GetOpenIdToken", Once),
    (
        "cognito-identity",
        "GetOpenIdTokenForDeveloperIdentity",
        Once,
    ),
    ("redshift", "GetClusterCredentials", Once),
    ("redshift", "GetClusterCredentialsWithIAM", Once),
    ("redshift-serverless", "GetCredentials", Once),
    (
        "lakeformation",
        "GetTemporaryGluePartitionCredentials",
        Once,
    ),
    ("lakeformation", "GetTemporaryGlueTableCredentials", Once),
    ("ecr", "GetAuthorizationToken", Again),
    ("ecr-public", "GetAuthorizationToken", Once),
    ("codeartifact", "GetAuthorizationToken", Once),
];

fn cat() -> &'static Catalog {
    Catalog::embedded().expect("the embedded catalog decodes")
}

/// Each listed mint is a secret-bearing write (`W 🔑`, `Always`), with the
/// MINT note and its retry class.
#[test]
fn each_mint_is_a_secret_bearing_write_with_its_retry_class() {
    for &(service, op, retry) in MINTS {
        let svc = cat().service(service).unwrap();
        let c = svc
            .operation(op)
            .unwrap_or_else(|| panic!("{service} has no {op}"))
            .classify();
        let what = format!("{service}:{op}");
        assert_eq!(c.label(), "W 🔑", "{what}");
        assert_eq!(c.class, Class::Write, "{what}");
        assert_eq!(c.secret, SecretBearing::Always, "{what}");
        assert_eq!(c.retry, retry, "{what}");
        assert_eq!(c.note, Some(MINT), "{what}");
    }
}

/// The invariant over the whole catalog: every operation with the MINT note
/// is a write that always holds its secret (so never `WhenPresent`), and is
/// listed above; a mint whose `SECRET` row went, or a new one, fails here.
#[test]
fn every_operation_with_the_mint_note_is_a_listed_secret_bearing_write() {
    let mut found = Vec::new();
    let mut bad = Vec::new();
    for e in cat().services() {
        let svc = cat().service(&e.name).unwrap();
        for op in svc.operations() {
            let c = op.classify();
            if c.note != Some(MINT) {
                continue;
            }
            let what = format!("{}:{}", e.name, op.name());
            if c.class != Class::Write || c.secret != SecretBearing::Always {
                bad.push(format!("{what} is {:?} and {:?}", c.class, c.secret));
            }
            found.push(what);
        }
    }
    found.sort();
    let mut listed: Vec<String> = MINTS.iter().map(|(s, o, _)| format!("{s}:{o}")).collect();
    listed.sort();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert_eq!(found, listed, "the mints in the catalog, and the list");
}

/// The AWS session mints, which `aws.call` puts to the operator at every
/// posture (theseus-a3s3): STS's and no other operation in the catalog, and
/// not `GetWebIdentityToken`, which mints no AWS session. Each is a
/// secret-bearing write, and `aws.describe` says it always asks.
#[test]
fn the_aws_session_mints_are_stss_and_always_ask() {
    let want = [
        "sts:AssumeRole",
        "sts:AssumeRoleWithSAML",
        "sts:AssumeRoleWithWebIdentity",
        "sts:AssumeRoot",
        "sts:GetDelegatedAccessToken",
        "sts:GetFederationToken",
        "sts:GetSessionToken",
    ];
    let mut got = Vec::new();
    for e in cat().services() {
        let svc = cat().service(&e.name).unwrap();
        for op in svc.operations() {
            let c = op.classify();
            if !c.session_mint {
                continue;
            }
            let what = format!("{}:{}", e.name, op.name());
            assert_eq!(c.label(), "W 🔑", "{what}");
            let d = theseus_aws_catalog::describe_operation(op);
            assert_eq!(d["approval"], "always", "{what}: {d}");
            got.push(what);
        }
    }
    got.sort();
    assert_eq!(got, want);
    let sts = cat().service("sts").unwrap();
    let web = sts.operation("GetWebIdentityToken").unwrap();
    assert!(!web.classify().session_mint);
    assert!(theseus_aws_catalog::describe_operation(web)
        .get("approval")
        .is_none());
}
