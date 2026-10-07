//! The credential mints the CLI's models hold outside STS's own (theseus-ye7o):
//! each is a write whatever its name says, holds a secret, and is safe to
//! repeat, as `sts:GetSessionToken` is (AWS design §3.1, "every STS
//! credential mint").

use theseus_aws_catalog::{Catalog, Class, RetryClass, SecretBearing};

/// What the override table says of a mint.
const MINT: &str = "mints a credential, whatever its name says";

/// The mints the CLI's models brought after the tables were first written.
const MINTS: &[(&str, &str)] = &[
    ("sts", "GetDelegatedAccessToken"),
    ("sts", "GetWebIdentityToken"),
    ("eks-auth", "AssumeRoleForPodIdentity"),
];

fn cat() -> &'static Catalog {
    Catalog::embedded().expect("the embedded catalog decodes")
}

#[test]
fn each_mint_is_a_secret_bearing_write_safe_to_repeat() {
    let sts = cat().service("sts").unwrap();
    let known = sts.operation("GetSessionToken").unwrap().classify();
    assert_eq!(known.note, Some(MINT), "STS's own mint keeps its note");
    for (service, op) in MINTS {
        let svc = cat().service(service).unwrap();
        let c = svc
            .operation(op)
            .unwrap_or_else(|| panic!("{service} has no {op}"))
            .classify();
        let what = format!("{service}:{op}");
        assert_eq!(c.label(), "W 🔑", "{what}");
        assert_eq!(c.class, Class::Write, "{what}");
        assert_eq!(c.secret, SecretBearing::Always, "{what}");
        assert_eq!(c.retry, RetryClass::SafeToRepeat, "{what}");
        assert_eq!(c.note, known.note, "{what}");
    }
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
