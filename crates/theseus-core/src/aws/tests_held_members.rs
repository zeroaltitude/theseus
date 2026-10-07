//! The secrets the walk holds by their shapes (theseus-u4pe), through
//! `aws.call`: API Gateway's API key (`ApiKey.value`), AppSync's (`ApiKey.id`)
//! and Lightsail's default private key (`privateKeyBase64`; `CreateKeyPair`'s
//! is held the same way, and `aws.call` refuses it as IaC-only), none marked
//! by its model. Each family answers with a handle and the board holds the key,
//! where every call failed closed before; a listing with no key answers
//! whole.

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};

const APIGW_KEY: &str = "apigateway-key-value-0001";
const APPSYNC_KEY: &str = "da2-appsync-key-0002";
const LIGHTSAIL_KEY: &str = "lightsail-private-key-0003";

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

/// API Gateway and AppSync by their paths, Lightsail by its target; a
/// listing named `empty` has no key.
fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(ACCOUNT, n);
    }
    let empty = s.target.contains("empty");
    let apigw =
        json!({"id": "apigw-key-id", "value": APIGW_KEY, "name": "example", "enabled": true});
    let appsync = json!({"id": APPSYNC_KEY, "description": "example", "expires": 1_791_028_800, "deletes": 1_791_633_600});
    match (s.method.as_str(), s.target.split('?').next().unwrap_or("")) {
        ("POST", "/apikeys") => return rest(n, apigw),
        ("GET", "/apikeys") => {
            let items = if empty { json!([]) } else { json!([apigw]) };
            // The model names the member `items`, and the wire `item`.
            return rest(n, json!({ "item": items }));
        }
        ("POST", p) if p.ends_with("/apikeys") => return rest(n, json!({ "apiKey": appsync })),
        ("GET", p) if p.ends_with("/apikeys") => {
            let keys = if empty { json!([]) } else { json!([appsync]) };
            return rest(n, json!({ "apiKeys": keys }));
        }
        _ => {}
    }
    if super::tests_c3::target(s) == Some("DownloadDefaultKeyPair") {
        return super::tests_c3::json_reply(
            n,
            json!({
                "publicKeyBase64": "ssh-rsa AAAAexample-public",
                "privateKeyBase64": LIGHTSAIL_KEY,
                "createdAt": 1_791_028_800.0,
            }),
        );
    }
    (400, vec![], "{}".into())
}

#[tokio::test]
async fn each_family_answers_with_its_keys_handle_and_an_empty_listing_answers_whole() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    let call = |service: &str, op: &str, input: Value| json!({"service": service, "operation": op, "input": input});
    for (input, handle, value, shown) in [
        (
            call("apigateway", "CreateApiKey", json!({"name": "example"})),
            Some("aws-secret:CreateApiKey#value"),
            APIGW_KEY,
            "apigw-key-id",
        ),
        (
            call("apigateway", "GetApiKeys", json!({"includeValues": true})),
            Some("aws-secret:GetApiKeys#items[0].value"),
            APIGW_KEY,
            "apigw-key-id",
        ),
        (
            call(
                "apigateway",
                "GetApiKeys",
                json!({"includeValues": true, "nameQuery": "empty"}),
            ),
            None,
            APIGW_KEY,
            "\"items\": []",
        ),
        (
            call("appsync", "CreateApiKey", json!({"apiId": "example-api"})),
            Some("aws-secret:CreateApiKey#apiKey.id"),
            APPSYNC_KEY,
            "1791633600",
        ),
        (
            call("appsync", "ListApiKeys", json!({"apiId": "example-api"})),
            Some("aws-secret:ListApiKeys#apiKeys[0].id"),
            APPSYNC_KEY,
            "1791633600",
        ),
        (
            call("appsync", "ListApiKeys", json!({"apiId": "empty-api"})),
            None,
            APPSYNC_KEY,
            "\"apiKeys\": []",
        ),
        (
            call("lightsail", "DownloadDefaultKeyPair", json!({})),
            Some("aws-secret:DownloadDefaultKeyPair#privateKeyBase64"),
            LIGHTSAIL_KEY,
            "ssh-rsa AAAAexample-public",
        ),
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
        assert!(out.text.contains(shown), "{what}: {}", out.text);
        assert!(!said.contains(value), "{what}: the key in {said}");
        match handle {
            Some(h) => {
                assert_eq!(out.meta["secrets"], json!([h]), "{what}: {said}");
                let kept = b.get(h).unwrap_or_else(|| panic!("{h} is on the board"));
                assert_eq!(kept.expose(), value, "{what}");
                let rows: Vec<Value> = binding.requests().into_iter().map(|r| r.row).collect();
                assert!(!json!(rows).to_string().contains(value), "{what}: the rows");
            }
            None => assert!(out.meta.get("secrets").is_none(), "{what}: {said}"),
        }
    }
}
