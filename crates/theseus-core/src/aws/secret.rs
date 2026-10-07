//! Secret-bearing results (AWS design §3.5; C3 = 14c): a value an AWS call
//! returns that is a secret (a Secrets Manager secret, a decrypted SSM
//! parameter or KMS plaintext, a role session's keys, a token) goes onto the
//! secrets board as a runtime secret, under a handle (`aws-secret:…`), in
//! zeroizing memory and never written. The result keeps the handle and the
//! value's shape with the secret masked. So the value never reaches the
//! model, a node, the WAL, the ledger, or a span; and the scrubber, which
//! reads the board, withholds it from anything that would carry it later.
//!
//! What is secret is read from the operation's output shape: a member whose
//! shape the model marks sensitive, or whose name is one of [`NAMED`] in any
//! case (STS's `SessionToken` is not marked, nor ECR's `authorizationToken`,
//! which until theseus-ye7o was held by no name, so ECR's call failed
//! closed). A secret-bearing call in which nothing is found to hold returns
//! nothing of its output, and says so: it fails closed. One whose secret a
//! resource keeps only sometimes (`SecretBearing::WhenPresent`,
//! theseus-qan5) holds what is found and answers whole when nothing is.

use serde_json::{json, Map, Value};
use theseus_aws::catalog::{Catalog, Kind, ShapeRef};

use crate::secrets::{Secret, SecretBoard};

/// The handle's prefix.
pub const PREFIX: &str = "aws-secret:";

/// Members that hold a secret whether or not the model marks them, matched
/// in any case. The output-shape rule (`tests_secret_shapes`) reads them too.
pub(super) const NAMED: &[&str] = &[
    "SecretString",
    "SecretBinary",
    "SecretAccessKey",
    "SessionToken",
    "Plaintext",
    "PrivateKeyPlaintext",
    // Lightsail's temporary SSH key, `privateKey`, which no model marks
    // (theseus-xscd).
    "PrivateKey",
    "Password",
    "RandomPassword",
    "AuthorizationToken",
    "AccessToken",
    "IdToken",
    "RefreshToken",
    "Token",
    "Credentials",
];

/// One secret a result held: its handle, and where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub handle: String,
    /// The member's path in the output: `SecretString`, `Parameters[1].Value`.
    pub path: String,
}

/// A handle's name: `aws-secret:` and what names the secret (the input's
/// `SecretId` or `Name`, else the operation), then the member's path, in
/// the characters a board name keeps.
fn handle(label: &str, path: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "/_.-+=@[]".contains(c) {
                    c
                } else {
                    '-'
                }
            })
            .take(128)
            .collect()
    };
    format!("{PREFIX}{}#{}", clean(label), clean(path))
}

/// What names the secrets of a call: its input's `SecretId`, `Name`, or
/// `KeyId`, or else its operation.
pub fn label(operation: &str, input: &Value) -> String {
    ["SecretId", "Name", "KeyId", "RoleSessionName"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .unwrap_or(operation)
        .trim_start_matches("arn:aws:")
        .to_string()
}

/// A secret's shape with its value masked: a JSON object's keys with each
/// value masked (a Secrets Manager secret is often one), else its length.
fn masked(v: &Value, handle: &str) -> Value {
    let len = |s: &str| format!("<masked: {} characters>", s.chars().count());
    let shape = match v {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(Value::Object(o)) => Value::Object(
                o.keys()
                    .map(|k| (k.clone(), json!("<masked>")))
                    .collect::<Map<_, _>>(),
            ),
            _ => json!(len(s)),
        },
        Value::Object(o) if o.contains_key("base64") => {
            json!(format!(
                "<masked: {} bytes>",
                o["base64"].as_str().map_or(0, |b| b.len() * 3 / 4)
            ))
        }
        other => json!(format!("<masked: {}>", kind_word(other))),
    };
    json!({"secret": handle, "shape": shape})
}

fn kind_word(v: &Value) -> &'static str {
    match v {
        Value::Number(_) => "a number",
        Value::Bool(_) => "a boolean",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
        _ => "a value",
    }
}

/// The value to hold for a secret: a string as itself, a blob's base64 as
/// written, anything else as its JSON.
fn value_of(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Object(o) if o.len() == 1 && o.contains_key("base64") => {
            o["base64"].as_str().map(String::from)
        }
        other => Some(other.to_string()),
    }
}

/// A member's name is one of [`NAMED`], in any case: `credentials` and
/// `sessionToken` as `Credentials` and `SessionToken`.
fn named(name: &str) -> bool {
    NAMED.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Walk `v` along `shape`, replacing every secret with its mask, and
/// holding its value on `board`.
fn walk(
    v: &mut Value,
    shape: ShapeRef<'_>,
    name: Option<&str>,
    path: &str,
    sensitive: bool,
    keep: &mut dyn FnMut(&str, &Value) -> String,
) {
    let secret = sensitive || shape.is_sensitive() || name.is_some_and(named);
    if secret {
        if let Some(_value) = value_of(v) {
            let handle = keep(path, v);
            *v = masked(v, &handle);
        }
        return;
    }
    match (shape.kind(), v) {
        (Kind::Structure, Value::Object(o)) => {
            for m in shape.members() {
                if let Some(x) = o.get_mut(m.name()) {
                    let p = if path.is_empty() {
                        m.name().to_string()
                    } else {
                        format!("{path}.{}", m.name())
                    };
                    walk(x, m.shape(), Some(m.name()), &p, false, keep);
                }
            }
        }
        (Kind::List, Value::Array(a)) => {
            if let Some(m) = shape.list_member() {
                for (i, x) in a.iter_mut().enumerate() {
                    walk(x, m.shape(), name, &format!("{path}[{i}]"), false, keep);
                }
            }
        }
        (Kind::Map, Value::Object(o)) => {
            if let Some(m) = shape.map_value() {
                let sensitive_key = shape.map_key().is_some_and(|k| k.shape().is_sensitive());
                for (k, x) in o.iter_mut() {
                    walk(
                        x,
                        m.shape(),
                        None,
                        &format!("{path}.{k}"),
                        sensitive_key,
                        keep,
                    );
                }
            }
        }
        _ => {}
    }
}

/// Hold every secret of a secret-bearing call's output on `board`, and mask
/// it in `body`: the handles held. An empty list means none was found, and
/// the caller returns none of the output.
pub fn hold(
    board: &SecretBoard,
    service: &str,
    operation: &str,
    input: &Value,
    body: &mut Value,
) -> Result<Vec<Held>, String> {
    let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
    let svc = catalog.service(service).map_err(|e| e.to_string())?;
    let op = svc
        .operation(operation)
        .ok_or_else(|| format!("{service} has no operation {operation}"))?;
    let Some(shape) = op.output() else {
        return Ok(Vec::new());
    };
    let label = label(operation, input);
    let mut held = Vec::new();
    let mut keep = |path: &str, v: &Value| -> String {
        let h = handle(&label, path);
        if let Some(value) = value_of(v) {
            board.hold(&h, Secret::new(value));
        }
        held.push(Held {
            handle: h.clone(),
            path: path.to_string(),
        });
        h
    };
    walk(body, shape, None, "", false, &mut keep);
    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_names_its_secret_and_keeps_board_characters() {
        assert_eq!(
            handle("prod/db credentials", "SecretString"),
            "aws-secret:prod/db-credentials#SecretString"
        );
        assert_eq!(
            label(
                "GetSecretValue",
                &json!({"SecretId": "arn:aws:secretsmanager:us-west-2:1:secret:x"})
            ),
            "secretsmanager:us-west-2:1:secret:x"
        );
        assert_eq!(label("GenerateRandom", &json!({})), "GenerateRandom");
    }

    #[test]
    fn a_json_secret_shows_its_keys_masked() {
        let m = masked(
            &json!("{\"user\":\"u\",\"password\":\"p\"}"),
            "aws-secret:x",
        );
        assert_eq!(
            m["shape"],
            json!({"user": "<masked>", "password": "<masked>"})
        );
        let m = masked(&json!("plain-value"), "aws-secret:x");
        assert_eq!(m["shape"], "<masked: 11 characters>");
        assert_eq!(m["secret"], "aws-secret:x");
    }

    /// The values the walk test plants: none may be left in a masked body.
    const PLANTED: &[&str] = &[
        "s3cr3t",
        "first-secret",
        "second-secret",
        "c2VjcmV0",
        "sts-secret",
        "sts-token",
        "jwt-secret",
        "pod-token",
        "pod-secret",
        "ZWNyLXRva2Vu",
    ];

    /// Every secret of the catalog's own outputs is found: Secrets Manager's
    /// string, an SSM parameter's value, KMS's plaintext, STS's keys, and the
    /// mints the tables learned late (theseus-ye7o): STS's delegated keys and
    /// web identity token, EKS's pod identity keys, and ECR's token, whose
    /// lower-case name no model marks.
    #[test]
    fn the_catalogs_secret_members_are_found_and_masked() {
        let board = SecretBoard::empty();
        for (service, op, input, mut body, paths) in [
            (
                "secretsmanager",
                "GetSecretValue",
                json!({"SecretId": "app/db"}),
                json!({"Name": "app/db", "SecretString": "s3cr3t-value-0001", "VersionId": "v1"}),
                vec!["SecretString"],
            ),
            (
                "ssm",
                "GetParameters",
                json!({"Names": ["a", "b"], "WithDecryption": true}),
                json!({"Parameters": [{"Name": "a", "Value": "first-secret-001"}, {"Name": "b", "Value": "second-secret-02"}]}),
                vec!["Parameters[0].Value", "Parameters[1].Value"],
            ),
            (
                "kms",
                "Decrypt",
                json!({"CiphertextBlob": "x"}),
                json!({"KeyId": "k", "Plaintext": {"base64": "c2VjcmV0LWJ5dGVzLTAwMQ=="}}),
                vec!["Plaintext"],
            ),
            (
                "sts",
                "AssumeRole",
                json!({"RoleArn": "arn:aws:iam::1:role/r", "RoleSessionName": "s"}),
                json!({"Credentials": {"AccessKeyId": "ASIAEXAMPLE", "SecretAccessKey": "sts-secret-0001", "SessionToken": "sts-token-0001", "Expiration": "2026-10-04T00:00:00Z"}}),
                vec!["Credentials"],
            ),
            (
                "sts",
                "GetDelegatedAccessToken",
                json!({"TradeInToken": "trade-in"}),
                json!({"Credentials": {"AccessKeyId": "ASIAEXAMPLE", "SecretAccessKey": "sts-secret-0002", "SessionToken": "sts-token-0002", "Expiration": "2026-10-04T00:00:00Z"}, "PackedPolicySize": 6, "AssumedPrincipal": "arn:aws:sts::1:assumed-role/r/s"}),
                vec!["Credentials"],
            ),
            (
                "sts",
                "GetWebIdentityToken",
                json!({"Audience": ["https://example.invalid"], "SigningAlgorithm": "RS256"}),
                json!({"WebIdentityToken": "eyJhbGciOi.jwt-secret-0003.sig", "Expiration": "2026-10-04T00:00:00Z"}),
                vec!["WebIdentityToken"],
            ),
            (
                "eks-auth",
                "AssumeRoleForPodIdentity",
                json!({"clusterName": "example", "token": "projected"}),
                json!({
                    "subject": {"namespace": "default", "serviceAccount": "app"},
                    "audience": "pods.eks.amazonaws.com",
                    "podIdentityAssociation": {"associationArn": "arn:aws:eks:us-west-2:1:podidentityassociation/example/a-1", "associationId": "a-1"},
                    "assumedRoleUser": {"arn": "arn:aws:sts::1:assumed-role/r/eks-pod", "assumeRoleId": "AROAEXAMPLE:eks-pod"},
                    "credentials": {"sessionToken": "pod-token-0004", "secretAccessKey": "pod-secret-0004", "accessKeyId": "ASIAPODEXAMPLE", "expiration": "2026-10-04T00:00:00Z"}
                }),
                vec!["credentials"],
            ),
            (
                "ecr",
                "GetAuthorizationToken",
                json!({}),
                json!({"authorizationData": [{"authorizationToken": "ZWNyLXRva2VuLTAwMDU=", "expiresAt": 1_791_028_800.0, "proxyEndpoint": "https://111122223333.dkr.ecr.us-west-2.amazonaws.com"}]}),
                vec!["authorizationData[0].authorizationToken"],
            ),
        ] {
            let before = body.to_string();
            let held = hold(&board, service, op, &input, &mut body).unwrap();
            let got: Vec<&str> = held.iter().map(|h| h.path.as_str()).collect();
            assert_eq!(got, paths, "{service}:{op}");
            let after = body.to_string();
            for secret in PLANTED {
                if before.contains(secret) {
                    assert!(!after.contains(secret), "{secret} in {after}");
                }
            }
            for h in &held {
                assert!(
                    board.get(&h.handle).is_some(),
                    "{} is on the board",
                    h.handle
                );
                assert!(after.contains(&h.handle), "{after}");
            }
        }
        assert_eq!(
            board
                .get("aws-secret:app/db#SecretString")
                .unwrap()
                .expose(),
            "s3cr3t-value-0001"
        );
    }
}
