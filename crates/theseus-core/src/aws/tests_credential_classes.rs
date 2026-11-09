//! Credential classes the output-shape rule could not see (theseus-37ds),
//! through `aws.call` against a fake AWS: a stream key and a channel's SRT
//! passphrase, a Grafana workspace key, a Cognito client secret, the TOTP
//! seed a software MFA factor is built from, a CHAP initiator and target
//! secret, and the pre-shared keys in a VPN device's sample configuration.
//!
//! Each class is named for the thing it unlocks rather than for what it is
//! (`streamKey.value`, `key`, `SecretCode`, `Srt.passphrase`), so the rule's
//! old name-ending test passed every one of them and `aws.call` pretty-
//! printed the credential into the model's context (`tools.rs`, `call_text`).
//! Here each answers with a handle instead, the board holds the value, and
//! the rest of the result comes through untouched.
//!
//! Two classes came with the shape rule (theseus-3i6d1) and are measured in
//! `tests_presigned.rs`, not here: a presigned URL, which carries its own
//! authorisation in the value and so is held by what the value looks like
//! and not by its member's name, and the continuation `call_text` prints
//! beside the body, which the catalog's paginators show to be an opaque
//! cursor or an id and which is therefore left whole.
//!
//! The control at the end is the other half of the plant: an ordinary call
//! with no credential in it still answers whole. A guard that withheld
//! every result would pass the tests above and fail this one.
//!
//! Every value below is synthetic.

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};
use super::tests_c3::{json_reply, target};

const STREAM_KEY: &str = "sk-us-west-2-EXAMPLEstreamkeyvalue0001";
const SRT_PASSPHRASE: &str = "example-srt-passphrase-0002";
const GRAFANA_KEY: &str = "glsa-EXAMPLEgrafanaworkspacekey-0003";
const CLIENT_SECRET: &str = "example-cognito-client-secret-0004";
const TOTP_SEED: &str = "EXAMPLEBASE32TOTPSEEDVALUE0005";
const MFA_SESSION: &str = "example-mfa-association-session-0006";
const CHAP_INITIATOR: &str = "example-chap-initiator-secret-0007";
const CHAP_TARGET: &str = "example-chap-target-secret-0008";
const VPN_PRE_SHARED_KEY: &str = "example-vpn-pre-shared-key-0009";

/// The channel a listing shows: no credential anywhere in it.
const CHANNEL_NAME: &str = "example-channel-name";

fn rest(n: usize, body: Value) -> Reply {
    (
        200,
        vec![
            ("x-amzn-requestid", format!("req-{n}")),
            ("content-type", "application/json".into()),
        ],
        body.to_string(),
    )
}

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

fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(ACCOUNT, n);
    }
    // EC2's query protocol: the sample configuration AWS writes for a
    // customer gateway, with the tunnel's pre-shared key inside it.
    if s.action() == Some("GetVpnConnectionDeviceSampleConfiguration") {
        return xml(
            n,
            format!(
                "<GetVpnConnectionDeviceSampleConfigurationResponse>\
                 <requestId>req-{n}</requestId>\
                 <vpnConnectionDeviceSampleConfiguration>! Tunnel 1\n\
                 crypto keyring keyring-vpn\n  pre-shared-key address 203.0.113.10 key \
                 {VPN_PRE_SHARED_KEY}\n</vpnConnectionDeviceSampleConfiguration>\
                 </GetVpnConnectionDeviceSampleConfigurationResponse>"
            ),
        );
    }
    // The JSON 1.1 services, by their target.
    match target(s) {
        Some("AddUserPoolClientSecret") => {
            return json_reply(
                n,
                json!({"ClientSecretDescriptor": {
                    "ClientSecretId": "example-client-secret-id",
                    "ClientSecretValue": CLIENT_SECRET,
                    "ClientSecretCreateDate": 1_791_028_800.0,
                }}),
            )
        }
        Some("AssociateSoftwareToken") => {
            return json_reply(n, json!({"SecretCode": TOTP_SEED, "Session": MFA_SESSION}))
        }
        Some("ListUserPoolClientSecrets") => {
            // The pool named `empty` has no client secret at all.
            let secrets = if s.body.contains("us-west-2_empty") {
                json!([])
            } else {
                json!([{
                    "ClientSecretId": "example-client-secret-id",
                    "ClientSecretValue": CLIENT_SECRET,
                    "ClientSecretCreateDate": 1_791_028_800.0,
                }])
            };
            return json_reply(n, json!({ "ClientSecrets": secrets }));
        }
        Some("DescribeChapCredentials") => {
            return json_reply(
                n,
                json!({"ChapCredentials": [{
                    "TargetARN": "arn:aws:storagegateway:us-west-2:111122223333:gateway/sgw-1/target/iqn.example",
                    "SecretToAuthenticateInitiator": CHAP_INITIATOR,
                    "InitiatorName": "iqn.1991-05.com.example:initiator",
                    "SecretToAuthenticateTarget": CHAP_TARGET,
                }]}),
            )
        }
        _ => {}
    }
    // The REST-JSON services, by their path.
    let path = s.target.split('?').next().unwrap_or("");
    if path.ends_with("/apikeys") {
        return rest(
            n,
            json!({"keyName": "example-key", "key": GRAFANA_KEY, "workspaceId": "g-example"}),
        );
    }
    let stream_key = json!({
        "arn": "arn:aws:ivs:us-west-2:111122223333:stream-key/example",
        "channelArn": "arn:aws:ivs:us-west-2:111122223333:channel/example",
        "value": STREAM_KEY,
        "tags": {},
    });
    match path {
        "/CreateStreamKey" => rest(n, json!({ "streamKey": stream_key })),
        "/GetChannel" => rest(
            n,
            json!({"channel": {
                "arn": "arn:aws:ivs:us-west-2:111122223333:channel/example",
                "name": CHANNEL_NAME,
                "ingestEndpoint": "example.global-contribute.live-video.net",
                "playbackUrl": "https://example.us-west-2.playback.live-video.net/api/video/v1/x.m3u8",
                "srt": {"endpoint": "example.srt.live-video.net", "passphrase": SRT_PASSPHRASE},
                "type": "STANDARD",
            }}),
        ),
        "/ListChannels" => rest(
            n,
            json!({"channels": [{
                "arn": "arn:aws:ivs:us-west-2:111122223333:channel/example",
                "name": CHANNEL_NAME,
                "authorized": false,
                "latencyMode": "LOW",
                "type": "STANDARD",
                "tags": {},
            }]}),
        ),
        _ => (400, vec![], "{}".into()),
    }
}

/// A call's arguments.
fn call(service: &str, op: &str, input: Value) -> Value {
    json!({"service": service, "operation": op, "input": input})
}

/// One credential class: the call that returns it, each credential the
/// result must hold as (the member's path, its value), and a string of the
/// same result that is no credential and must still come through.
struct Case {
    call: Value,
    held: Vec<(&'static str, &'static str)>,
    shown: &'static str,
}

fn case(call: Value, held: Vec<(&'static str, &'static str)>, shown: &'static str) -> Case {
    Case { call, held, shown }
}

/// Every class, and the credentials each must hold.
fn cases() -> Vec<Case> {
    vec![
        case(
            call(
                "ivs",
                "CreateStreamKey",
                json!({"channelArn": "arn:aws:ivs:us-west-2:111122223333:channel/example"}),
            ),
            vec![("streamKey.value", STREAM_KEY)],
            "stream-key/example",
        ),
        case(
            call(
                "ivs",
                "GetChannel",
                json!({"arn": "arn:aws:ivs:us-west-2:111122223333:channel/example"}),
            ),
            vec![("channel.srt.passphrase", SRT_PASSPHRASE)],
            CHANNEL_NAME,
        ),
        case(
            call(
                "grafana",
                "CreateWorkspaceApiKey",
                json!({"workspaceId": "g-example", "keyName": "example-key", "keyRole": "ADMIN", "secondsToLive": 300}),
            ),
            vec![("key", GRAFANA_KEY)],
            "example-key",
        ),
        case(
            call(
                "cognito-idp",
                "AddUserPoolClientSecret",
                json!({"UserPoolId": "us-west-2_example", "ClientId": "exampleclientid"}),
            ),
            vec![("ClientSecretDescriptor.ClientSecretValue", CLIENT_SECRET)],
            "example-client-secret-id",
        ),
        case(
            call(
                "cognito-idp",
                "AssociateSoftwareToken",
                json!({"AccessToken": "example-access-token"}),
            ),
            vec![("SecretCode", TOTP_SEED), ("Session", MFA_SESSION)],
            "request req-",
        ),
        case(
            call(
                "storagegateway",
                "DescribeChapCredentials",
                json!({"TargetARN": "arn:aws:storagegateway:us-west-2:111122223333:gateway/sgw-1/target/iqn.example"}),
            ),
            vec![
                (
                    "ChapCredentials[0].SecretToAuthenticateInitiator",
                    CHAP_INITIATOR,
                ),
                ("ChapCredentials[0].SecretToAuthenticateTarget", CHAP_TARGET),
            ],
            "iqn.1991-05.com.example:initiator",
        ),
        case(
            call(
                "ec2",
                "GetVpnConnectionDeviceSampleConfiguration",
                json!({"VpnConnectionId": "vpn-example", "VpnConnectionDeviceTypeId": "example-device"}),
            ),
            vec![("VpnConnectionDeviceSampleConfiguration", VPN_PRE_SHARED_KEY)],
            "request req-",
        ),
    ]
}

/// Each class answers with a handle, the board holds the value, and no part
/// of the result or its meta carries the credential.
#[tokio::test]
async fn each_credential_class_is_held_and_never_shown() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");

    // Every class is reported, not just the first: with the catalog's rows
    // reverted each one is MISSED, and the list says so in one run.
    let mut bad: Vec<String> = Vec::new();
    for Case {
        call: input,
        held,
        shown,
    } in cases()
    {
        let what = format!(
            "{}:{}",
            input["service"].as_str().unwrap_or("?"),
            input["operation"].as_str().unwrap_or("?")
        );
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
        // The result still answers: the call did not fail closed, and what
        // is not a credential came through.
        if !out.text.contains(shown) {
            bad.push(format!(
                "{what}: the rest of the result is gone: {}",
                out.text
            ));
        }
        let handles: Vec<String> = out.meta["secrets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| h.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        for (path, value) in held {
            // The credential is nowhere in what the model is handed.
            if said.contains(value) {
                bad.push(format!("MISSED {what}: {path} reaches the model"));
                continue;
            }
            let want = format!("#{path}");
            let Some(h) = handles.iter().find(|h| h.ends_with(&want)) else {
                bad.push(format!("{what}: no handle for {path} in {handles:?}"));
                continue;
            };
            // And the board holds it, so the scrubber can withhold it from
            // anything that would carry it later. A member that is free text
            // around a credential (EC2 writes the tunnel's pre-shared key
            // into a device configuration) is held whole, key and all.
            match b.get(h) {
                Some(kept) if kept.expose().contains(value) => {}
                Some(_) => bad.push(format!("{what}: the board's {path} lost the credential")),
                None => bad.push(format!("{what}: {h} is not on the board")),
            }
            if !out.text.contains(h) {
                bad.push(format!("{what}: the handle is not shown:\n{}", out.text));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} of the credential classes are not held:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The control, on an operation the guard actually runs on.
///
/// `ivs:ListChannels` was the first control here and it was vacuous: it
/// classifies `secret=No`, so `tools.rs` never calls the walk and the result
/// comes through whole no matter what the walk would have done. A control
/// has to exercise the thing it is controlling.
///
/// `cognito-idp:ListUserPoolClientSecrets` is `WhenPresent`, so the walk
/// runs on every call. Asked for a pool that has a client secret, it holds
/// the secret and answers with the rest; asked for one that has none, it
/// holds nothing and still answers whole rather than failing closed. That
/// second case is the one a guard which withheld everything would fail.
#[tokio::test]
async fn the_guard_runs_and_still_answers_whole() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    let run = |input: Value| {
        let t = t.clone();
        let binding = aws.bind("exe_test", "act_test_1", "toolu_test_1");
        async move {
            let ctx = theseus_tools::ToolCtx {
                aws: Some(binding),
                ..plain_ctx()
            };
            t.run_async(&input, &ctx)
                .await
                .unwrap_or_else(|f| panic!("ListUserPoolClientSecrets: {}", f.message))
        }
    };

    // A pool with a secret: held, and everything else still shown.
    let (out, _) = run(call(
        "cognito-idp",
        "ListUserPoolClientSecrets",
        json!({"UserPoolId": "us-west-2_example", "ClientId": "exampleclientid"}),
    ))
    .await;
    assert!(
        !format!("{} {}", out.text, out.meta).contains(CLIENT_SECRET),
        "the secret is in the result:\n{}",
        out.text
    );
    assert!(
        out.text.contains("example-client-secret-id"),
        "the rest of the listing still answers:\n{}",
        out.text
    );
    assert_eq!(
        out.meta["secrets"],
        json!(["aws-secret:ListUserPoolClientSecrets#ClientSecrets[0].ClientSecretValue"]),
        "{}",
        out.meta
    );

    // A pool with none: nothing held, nothing masked, and the call still
    // answers rather than failing closed.
    let (out, _) = run(call(
        "cognito-idp",
        "ListUserPoolClientSecrets",
        json!({"UserPoolId": "us-west-2_empty", "ClientId": "exampleclientid"}),
    ))
    .await;
    assert!(
        out.text.contains("\"ClientSecrets\": []"),
        "the empty listing answers whole:\n{}",
        out.text
    );
    assert!(
        !out.text.contains("<masked"),
        "nothing is masked:\n{}",
        out.text
    );
    assert!(
        out.meta.get("secrets").is_none(),
        "nothing is held: {}",
        out.meta
    );
}
