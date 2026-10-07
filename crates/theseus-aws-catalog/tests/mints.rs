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
