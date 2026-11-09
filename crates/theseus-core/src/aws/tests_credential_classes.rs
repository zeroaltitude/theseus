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
//! Added after that pass, from the second review's sweep (theseus-3i6d1):
//! pinpoint's Firebase server key (`GCMChannelResponse.Credential`) and its
//! Baidu twin, grafana's workspace token (`grafanaToken`), an IoT managed
//! integrations device's claim certificate (`GetProvisioningProfile`), and a
//! chime user's `PersonalPIN`, and the PIN blocks of payment-cryptography's data
//! plane (`TranslatePinData`, `GeneratePinData`, `GenerateMacEmvPinChange`). An EKS Anywhere licence's `token` also reaches
//! the model and is left to the `ALLOWED` row that decides it.
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
const FCM_SERVER_KEY: &str = "example-fcm-server-key-0010";
const WORKSPACE_TOKEN: &str = "example-workspace-token-0011";
const CLAIM_CERTIFICATE: &str = "example-claim-certificate-0012";
const BAIDU_CREDENTIAL: &str = "example-baidu-credential-0013";
const CHIME_PIN: &str = "example-chime-personal-pin-0014";
const TRANSLATED_PIN_BLOCK: &str = "example-translated-pin-block-0015";
const GENERATED_PIN_BLOCK: &str = "example-generated-pin-block-0016";
const EMV_PIN_BLOCK: &str = "example-emv-pin-change-block-0017";

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

/// Pinpoint's GCM channel and Grafana's workspaces (theseus-3i6d1), by their
/// paths: each carries a credential no model marks.
fn pinpoint_and_grafana(path: &str, n: usize) -> Option<Reply> {
    if path.ends_with("/channels/gcm") {
        return Some(rest(
            n,
            json!({"id": "example-app", "ApplicationId": "example-app", "Platform": "GCM",
                   "Credential": FCM_SERVER_KEY, "Enabled": true, "IsArchived": false, "Version": 1}),
        ));
    }
    if path.starts_with("/workspaces") {
        let workspace = json!({"id": "g-example", "name": "example-workspace",
                               "status": "ACTIVE", "grafanaToken": WORKSPACE_TOKEN});
        let body = if path == "/workspaces" {
            json!({ "workspaces": [workspace] })
        } else {
            json!({ "workspace": workspace })
        };
        return Some(rest(n, body));
    }
    None
}

/// The classes theseus-3i6d1's second review named and the first pass left:
/// a device's claim certificate, by its path.
fn more_classes(path: &str, n: usize) -> Option<Reply> {
    if path.starts_with("/provisioning-profiles/") {
        return Some(rest(
            n,
            json!({"Id": "example-profile", "Name": "example-profile-name",
                   "ProvisioningType": "FLEET_PROVISIONING",
                   "ClaimCertificate": CLAIM_CERTIFICATE}),
        ));
    }
    if path.ends_with("/channels/baidu") {
        return Some(rest(
            n,
            json!({"id": "example-app", "ApplicationId": "example-app", "Platform": "BAIDU",
                   "Credential": BAIDU_CREDENTIAL, "Enabled": true, "Version": 1}),
        ));
    }
    if let Some(r) = pin_blocks(path, n) {
        return Some(r);
    }
    if path.contains("/users/") {
        return Some(rest(
            n,
            json!({"User": {"UserId": "example-user", "PersonalPIN": CHIME_PIN,
                            "DisplayName": "example-display-name"}}),
        ));
    }
    None
}

/// The payment-cryptography data plane's PIN blocks, by their paths: marked
/// by the model, wordless, and on operations no table named (theseus-3i6d1).
fn pin_blocks(path: &str, n: usize) -> Option<Reply> {
    let body = match path {
        "/pindata/translate" => json!({"PinBlock": TRANSLATED_PIN_BLOCK, "KeyArn": "example-key-arn"}),
        "/pindata/generate" => json!({"EncryptedPinBlock": GENERATED_PIN_BLOCK,
                                       "EncryptionKeyArn": "example-key-arn"}),
        "/macemvpinchange/generate" => json!({"EncryptedPinBlock": EMV_PIN_BLOCK,
                                               "Mac": "example-mac-value",
                                               "NewPinPekArn": "example-key-arn"}),
        _ => return None,
    };
    Some(rest(n, body))
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
    if let Some(r) = pinpoint_and_grafana(path, n).or_else(|| more_classes(path, n)) {
        return r;
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

/// The classes of `base_cases` and `more_cases`.
fn cases() -> Vec<Case> {
    let mut all = base_cases();
    all.extend(more_cases());
    all
}

/// The classes that came after the first pass.
fn more_cases() -> Vec<Case> {
    vec![case(
        call(
            "iot-managed-integrations",
            "GetProvisioningProfile",
            json!({"Identifier": "example-profile"}),
        ),
        vec![("ClaimCertificate", CLAIM_CERTIFICATE)],
        "example-profile-name",
    )]
    .into_iter()
    .chain(measured_cases())
    .collect()
}

/// Members the second review's sweep named, held only where the catalog walk
/// measured them reaching the model (theseus-3i6d1). "A PIN" and "a licence
/// token" are the members' plain readings, not sourced facts.
fn measured_cases() -> Vec<Case> {
    vec![
        case(
            call(
                "pinpoint",
                "GetBaiduChannel",
                json!({"ApplicationId": "example-app"}),
            ),
            vec![("BaiduChannelResponse.Credential", BAIDU_CREDENTIAL)],
            "example-app",
        ),
        case(
            call(
                "chime",
                "GetUser",
                json!({"AccountId": "example-account", "UserId": "example-user"}),
            ),
            vec![("User.PersonalPIN", CHIME_PIN)],
            // The user's DisplayName is a marked member of the same answer
            // and is now masked with it (the owner's question, carried).
            "example-user",
        ),
        case(
            call("payment-cryptography-data", "TranslatePinData", translate_input()),
            vec![("PinBlock", TRANSLATED_PIN_BLOCK)],
            "example-key-arn",
        ),
        case(
            call("payment-cryptography-data", "GeneratePinData", generate_input()),
            vec![("EncryptedPinBlock", GENERATED_PIN_BLOCK)],
            "example-key-arn",
        ),
        case(
            call("payment-cryptography-data", "GenerateMacEmvPinChange", emv_input()),
            vec![("EncryptedPinBlock", EMV_PIN_BLOCK)],
            "example-key-arn",
        ),
    ]
}

/// The inputs of the three PIN operations; the account number is a
/// 12-digit invention, so no id-shaped run is in the file.
fn translate_input() -> Value {
    let iso = json!({"IsoFormat0": {"PrimaryAccountNumber": "400000000001"}});
    json!({"IncomingKeyIdentifier": "alias/example-in", "OutgoingKeyIdentifier": "alias/example-out",
           "IncomingTranslationAttributes": iso, "OutgoingTranslationAttributes": iso,
           "EncryptedPinBlock": "0123456789ABCDEF"})
}

fn generate_input() -> Value {
    json!({"GenerationKeyIdentifier": "alias/example-gen", "EncryptionKeyIdentifier": "alias/example-enc",
           "GenerationAttributes": {"Ibm3624NaturalPin": {"DecimalizationTable": "0123456789012345",
               "PinValidationDataPadCharacter": "F", "PinValidationData": "400000000001"}},
           "PrimaryAccountNumber": "400000000001", "PinBlockFormat": "ISO_FORMAT_0"})
}

fn emv_input() -> Value {
    json!({"NewEncryptedPinBlock": "0123456789ABCDEF", "PinBlockFormat": "ISO_FORMAT_0",
           "NewPinPekIdentifier": "alias/example-pek",
           "MessageData": "00112233",
           "SecureMessagingIntegrityKeyIdentifier": "alias/example-int",
           "SecureMessagingConfidentialityKeyIdentifier": "alias/example-conf",
           "DerivationMethodAttributes": {"EmvCommon": {"MajorKeyDerivationMode": "EMV_OPTION_A",
               "PrimaryAccountNumber": "400000000001", "PanSequenceNumber": "01",
               "ApplicationCryptogram": "0011223344556677", "Mode": "ECB",
               "PinBlockPaddingType": "NO_PADDING", "PinBlockLengthPosition": "NONE"}}})
}

/// Every class of the first passes, and the credentials each must hold.
fn base_cases() -> Vec<Case> {
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
        case(
            call(
                "pinpoint",
                "GetGcmChannel",
                json!({"ApplicationId": "example-app"}),
            ),
            vec![("GCMChannelResponse.Credential", FCM_SERVER_KEY)],
            "example-app",
        ),
        case(
            call(
                "grafana",
                "DescribeWorkspace",
                json!({"workspaceId": "g-example"}),
            ),
            vec![("workspace.grafanaToken", WORKSPACE_TOKEN)],
            "g-example",
        ),
        case(
            call("grafana", "ListWorkspaces", json!({})),
            vec![("workspaces[0].grafanaToken", WORKSPACE_TOKEN)],
            "g-example",
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
