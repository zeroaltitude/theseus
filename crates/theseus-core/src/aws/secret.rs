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
//! closed), or a member [`HELD`] names by its shape in one service (API
//! Gateway's and AppSync's API keys, Lightsail's new private keys, which
//! until theseus-u4pe failed closed on every call). A secret-bearing call in which nothing is found to hold returns
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

/// Compound names that are a credential whatever the model marks, matched
/// at the **end** of a member's name in any case, and only on a scalar
/// (theseus-37ds). A model that marks nothing still names these plainly, and
/// they were passing a secret-bearing call's own result to the model beside
/// the one secret it did hold: Connect's `ParticipantToken` beside its held
/// join token, `connectparticipant`'s `ConnectionToken`, Amplify Backend's
/// `ClientSecret`, Athena's and Glue's `AuthToken`.
///
/// `Password` and `SharedSecret` are here for the same reason: a pending
/// master password, a cluster's Kerberos passwords, an HSM's
/// pre-crypto-officer password and a procurement portal's shared secret are
/// all returned unmarked.
///
/// Matched at the end, not anywhere, so `apiKeyRequired` and
/// `PrivateKeyFlags` — which merely contain one — do not match. Matched only
/// on a string or a blob, because a credential is a value: without that,
/// `ApiKey` would swallow AppSync's `apiKey` **structure** whole and hide the
/// description and expiry beside the key, and `Password` would swallow
/// Amplify Backend's `ForgotPassword` configuration.
pub(super) const ENDS: &[&str] = &[
    "ParticipantToken",
    "ConnectionToken",
    "AuthToken",
    "HECToken",
    "SecretToken",
    "ClientSecret",
    "PreSharedKey",
    "ApiKey",
    "Password",
    "SharedSecret",
];

/// `name` ends with `suffix`, in any case, without allocating.
pub(super) fn ends_with_ci(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len()
        && name.as_bytes()[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

/// Members that hold a secret in one service's answers though no model marks
/// them, and whose names are everywhere, so [`NAMED`] is no place for them
/// (theseus-u4pe): a service, the shape, and the member. The walk holds them
/// in that service's secret-bearing answers; the output-shape rule reads
/// them too, and fails a row that no secret-bearing answer reaches.
pub(super) const HELD: &[(&str, &str, &str)] = &[
    // API Gateway's API key is its `value` (its `id` only names it).
    ("apigateway", "ApiKey", "value"),
    // AppSync's API key is its `id`: callers send it as `x-api-key`.
    ("appsync", "ApiKey", "id"),
    // Lightsail's new key pair's private half.
    ("lightsail", "CreateKeyPairResult", "privateKeyBase64"),
    (
        "lightsail",
        "DownloadDefaultKeyPairResult",
        "privateKeyBase64",
    ),
    // The presigned websocket a participant connects on: the URL is the
    // credential, and it rode out beside the held `ConnectionToken`
    // (theseus-37ds).
    ("connectparticipant", "Websocket", "Url"),
    // Pinpoint's Firebase server key, and Grafana's workspace token: neither
    // is marked, and `Credential` is no place in `NAMED` (its name is
    // everywhere). Each needs its operations `WHEN_PRESENT` in the catalog,
    // because the walk never runs on a `secret=No` operation
    // (theseus-3i6d1).
    ("pinpoint", "GCMChannelResponse", "Credential"),
    ("pinpoint", "BaiduChannelResponse", "Credential"),
    // A chime user's `PersonalPIN`: the member's plain reading, unmarked in
    // the model (theseus-3i6d1).
    ("chime", "User", "PersonalPIN"),
    ("grafana", "WorkspaceDescription", "grafanaToken"),
    ("grafana", "WorkspaceSummary", "grafanaToken"),
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
pub(super) fn handle(label: &str, path: &str) -> String {
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
/// `sessionToken` as `Credentials` and `SessionToken`; or a scalar whose
/// name ends in one of [`ENDS`].
pub(super) fn named(name: &str, shape: ShapeRef<'_>) -> bool {
    NAMED.iter().any(|n| n.eq_ignore_ascii_case(name))
        || (matches!(shape.kind(), Kind::String | Kind::Blob)
            && ENDS.iter().any(|e| ends_with_ci(name, e)))
}

/// Walk `v` along `shape`, replacing every secret with its mask, and
/// holding its value on `board`. `held` is the service's [`HELD`] rows, as
/// (shape, member).
fn walk(
    v: &mut Value,
    shape: ShapeRef<'_>,
    name: Option<&str>,
    path: &str,
    sensitive: bool,
    held: &[(&str, &str)],
    keep: &mut dyn FnMut(&str, &Value) -> String,
) {
    let secret = sensitive || shape.is_sensitive() || name.is_some_and(|n| named(n, shape));
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
                    let by_shape = held
                        .iter()
                        .any(|(s, n)| *s == shape.name() && *n == m.name());
                    walk(x, m.shape(), Some(m.name()), &p, by_shape, held, keep);
                }
            }
        }
        (Kind::List, Value::Array(a)) => {
            if let Some(m) = shape.list_member() {
                for (i, x) in a.iter_mut().enumerate() {
                    walk(
                        x,
                        m.shape(),
                        name,
                        &format!("{path}[{i}]"),
                        false,
                        held,
                        keep,
                    );
                }
            }
        }
        (Kind::Map, Value::Object(o)) => walk_map(o, shape, path, held, keep),
        _ => {}
    }
}

/// The `Map` arm of [`walk`]: each entry walked under the map's value shape,
/// with the entry's **key in the name position**, and its **value's
/// characters** read when nothing else has held it (theseus-3i6d1).
///
/// The arm used to recurse with `None` there, so [`named`] — [`NAMED`] and
/// the compound [`ENDS`] — was never consulted on a map value, and an entry
/// keyed `SessionToken`, or keyed anything ending in `ApiKey` or `Password`,
/// reached the model verbatim while the identical name on a structure
/// member was held. The arm already read the key's *shape*
/// (`sensitive_key`); it dropped only the key's text. A map's key is a name
/// as a member's is, so the name rules read it as one.
///
/// The value rule is [`super::value_shape::credential_shaped`], read last
/// and only on a scalar `Kind::String` the key's name, the value's shape
/// and the key's shape have all left alone: a credential pasted into a tag
/// or an environment variable is under a key nobody names, so no name rule
/// can ever reach it.
///
/// Both rules only ever hold more: nothing masked before is unmasked here.
fn walk_map(
    o: &mut Map<String, Value>,
    shape: ShapeRef<'_>,
    path: &str,
    held: &[(&str, &str)],
    keep: &mut dyn FnMut(&str, &Value) -> String,
) {
    let Some(m) = shape.map_value() else { return };
    let sensitive_key = shape.map_key().is_some_and(|k| k.shape().is_sensitive());
    for (k, x) in o.iter_mut() {
        // The value rule, last: only where the key's shape, the value's
        // shape and the key's name have all said nothing, and only on a
        // scalar string. A value rule on every unmarked scalar of every
        // answer is a far larger blast radius and is not what this is.
        let by_value = !sensitive_key
            && m.shape().kind() == Kind::String
            && !m.shape().is_sensitive()
            && !named(k, m.shape())
            && x.as_str()
                .is_some_and(super::value_shape::credential_shaped);
        walk(
            x,
            m.shape(),
            Some(k.as_str()),
            &format!("{path}.{k}"),
            sensitive_key || by_value,
            held,
            keep,
        );
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
    let by_shape: Vec<(&str, &str)> = HELD
        .iter()
        .filter(|(s, _, _)| *s == service)
        .map(|(_, shape, member)| (*shape, *member))
        .collect();
    walk(body, shape, None, "", false, &by_shape, &mut keep);
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

    /// A secret-bearing answer holds **every** credential in it, not only
    /// the one its model happens to mark (theseus-37ds).
    ///
    /// This is the partial hold, and it is worse than holding nothing,
    /// because the answer reads as covered. Connect's `StartWebRTCContact`
    /// is `SecretBearing::Always`: its `Attendee.JoinToken` is marked
    /// sensitive and was held, while `ParticipantToken` — which no model
    /// marks, and which is the credential a participant actually connects
    /// with — was pretty-printed into the model's context in the same
    /// result. `connectparticipant`'s `ConnectionToken` and its presigned
    /// `Websocket.Url` went the same way.
    ///
    /// Empty [`ENDS`] and drop the `connectparticipant` [`HELD`] row and
    /// this test fails on exactly those members, which is how it was before.
    #[test]
    fn a_secret_bearing_answer_holds_every_credential_not_only_the_marked_one() {
        const JOIN: &str = "example-join-token-0011";
        const PARTICIPANT: &str = "example-participant-token-0012";
        const CONNECTION: &str = "example-connection-token-0013";
        const WEBSOCKET: &str = "wss://example.connect.invalid/?signature=example-sig-0014";
        let board = SecretBoard::empty();
        for (service, op, mut body, want) in [
            (
                "connect",
                "StartWebRTCContact",
                json!({
                    "ConnectionData": {
                        "Attendee": {"AttendeeId": "attendee-example", "JoinToken": JOIN},
                        "Meeting": {"MeetingId": "meeting-example", "MediaRegion": "us-west-2"}
                    },
                    "ContactId": "contact-example",
                    "ParticipantId": "participant-example",
                    "ParticipantToken": PARTICIPANT
                }),
                vec![
                    ("ConnectionData.Attendee.JoinToken", JOIN),
                    ("ParticipantToken", PARTICIPANT),
                ],
            ),
            (
                "connectparticipant",
                "CreateParticipantConnection",
                json!({
                    "Websocket": {"Url": WEBSOCKET, "ConnectionExpiry": "2026-11-07T00:00:00Z"},
                    "ConnectionCredentials": {"ConnectionToken": CONNECTION, "Expiry": "2026-11-07T00:00:00Z"}
                }),
                vec![
                    ("Websocket.Url", WEBSOCKET),
                    ("ConnectionCredentials.ConnectionToken", CONNECTION),
                ],
            ),
        ] {
            let held = hold(&board, service, op, &json!({}), &mut body).unwrap();
            let after = body.to_string();
            for (path, value) in want {
                assert!(
                    held.iter().any(|h| h.path == path),
                    "{service}:{op}: {path} is not held: {held:?}"
                );
                assert!(
                    !after.contains(value),
                    "{service}:{op}: {path} is still in the answer:\n{after}"
                );
            }
            // What is not a credential still comes through.
            assert!(after.contains("contact-example") || after.contains("ConnectionExpiry"));
        }
    }

    /// A map's **key** is a name, and the name rules read it (theseus-3i6d1).
    ///
    /// Until this test the `Map` arm recursed with `None` in the walk's
    /// `name` position, so [`named`] — `NAMED` and the compound `ENDS` — was
    /// never consulted on a map value. An entry keyed `SessionToken`, or
    /// keyed anything ending in `ApiKey`, reached the model verbatim, while
    /// the identical name on a structure member was held. The arm already
    /// read the key's *shape* (`sensitive_key`); it dropped only its text.
    ///
    /// `amplify:GetApp` is one of the 85 secret-bearing operations whose
    /// output reaches an unmarked `Kind::Map` of plain strings (the count is
    /// in this commit's message): an app's `environmentVariables`, which is
    /// where a deployment's credentials actually live.
    ///
    /// **Planted revert**: put `None` back in the `Map` arm's `name`
    /// position and this test names the keys it expected held.
    #[test]
    fn a_map_entry_keyed_like_a_credential_is_held_as_a_member_of_that_name_is() {
        const SESSION: &str = "example-session-token-0021";
        const DEPLOY: &str = "example-deploy-api-key-0022";
        const ORDINARY: &str = "staging";
        let board = SecretBoard::empty();
        let mut body = json!({
            "app": {
                "appId": "example-app-0023",
                "name": "example-app",
                "environmentVariables": {
                    "SessionToken": SESSION,
                    "DeployApiKey": DEPLOY,
                    "env": ORDINARY,
                }
            }
        });
        let held = hold(
            &board,
            "amplify",
            "GetApp",
            &json!({"appId": "example-app-0023"}),
            &mut body,
        )
        .expect("amplify:GetApp is in the catalog");
        let mut paths: Vec<&str> = held.iter().map(|h| h.path.as_str()).collect();
        paths.sort_unstable();
        // Positive bars: exactly the two credential-named keys, so an empty
        // result cannot pass this test.
        assert_eq!(
            paths,
            [
                "app.environmentVariables.DeployApiKey",
                "app.environmentVariables.SessionToken"
            ],
            "the keys held: {held:?}"
        );
        for h in &held {
            assert!(
                board.get(&h.handle).is_some(),
                "{} is on the board",
                h.handle
            );
        }
        let after = body.to_string();
        assert!(
            !after.contains(SESSION),
            "the session token is still there:\n{after}"
        );
        assert!(
            !after.contains(DEPLOY),
            "the api key is still there:\n{after}"
        );
        // The ordinary key's value comes through unchanged.
        assert_eq!(body["app"]["environmentVariables"]["env"], json!(ORDINARY));
    }

    /// A map's **value** is read too, when no name and no marking holds it
    /// (theseus-3i6d1).
    ///
    /// Until this test nothing under `aws/` read a value's characters but
    /// `presigned::is_presigned`, which recognises signed URLs and nothing
    /// else. So a credential pasted into a tag or an environment variable
    /// — an `AKIA…`, a `ghp_…`, a JWT — under a key nobody names and a
    /// shape nobody marks went to the model whole. `super::value_shape` is
    /// the seam that reads it; its own table pins the AWS corpus.
    ///
    /// Same operation as the key test, so the two rules are seen on one
    /// answer: one entry carries an invented access-key-shaped value under
    /// an ordinary key, the other a benign tag value.
    ///
    /// **Planted revert**: drop the value branch from `walk_map` and this
    /// test names the value it expected held.
    #[test]
    fn a_map_value_shaped_like_a_credential_is_held_though_no_name_or_marking_says_so() {
        const PLANTED: &str = "AKIAEXAMPLEACCESSKEY";
        const BENIGN: &str = "us-west-2";
        let board = SecretBoard::empty();
        let mut body = json!({
            "app": {
                "appId": "example-app-0024",
                "name": "example-app",
                "environmentVariables": {
                    "DEPLOY_TARGET": PLANTED,
                    "REGION": BENIGN,
                }
            }
        });
        let held = hold(
            &board,
            "amplify",
            "GetApp",
            &json!({"appId": "example-app-0024"}),
            &mut body,
        )
        .expect("amplify:GetApp is in the catalog");
        // Positive bar: exactly the one entry, so an empty result cannot
        // pass, and the benign tag is not swept up with it.
        let paths: Vec<&str> = held.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            ["app.environmentVariables.DEPLOY_TARGET"],
            "the values held: {held:?}"
        );
        assert_eq!(
            board.get(&held[0].handle).map(|s| s.expose().to_string()),
            Some(PLANTED.to_string()),
            "the value is on the board"
        );
        let after = body.to_string();
        assert!(!after.contains(PLANTED), "the key is still there:\n{after}");
        assert_eq!(body["app"]["environmentVariables"]["REGION"], json!(BENIGN));
    }
}
