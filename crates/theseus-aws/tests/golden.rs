//! The six protocols against botocore, the reference implementation: for
//! each case in `fixtures/golden.json` (generated offline by
//! `fixtures/golden.py` from the AWS CLI 2.34.15's own botocore, over the
//! same models the catalog compiles), the request the client builds is the
//! one botocore sends, and the answer it reads is botocore's reading.

mod fake;

use std::time::{Duration, UNIX_EPOCH};

use fake::{Fake, Reply};
use serde_json::Value;
use theseus_aws::{Attribution, Call, Client, ClientConfig, Credentials};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/golden.json")).unwrap()
}

fn creds() -> Credentials {
    Credentials::new(
        "AKIDEXAMPLE",
        "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        None,
        None,
    )
}

/// Headers that differ per send, or that the two choose differently and
/// both validly (S3's payload hash and checksums), as the generator skips them.
fn compared(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !matches!(
        n.as_str(),
        "authorization"
            | "x-amz-date"
            | "user-agent"
            | "x-amz-user-agent"
            | "content-length"
            | "expect"
            | "x-amz-security-token"
            | "x-amz-content-sha256"
            | "content-md5"
            | "x-amz-sdk-checksum-algorithm"
    ) && !n.starts_with("x-amz-checksum-")
        && !n.starts_with("amz-sdk-")
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .unwrap()
        .into_owned()
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    let mut p: Vec<(String, String)> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|kv| {
            (
                kv[0].as_str().unwrap().to_owned(),
                kv[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    p.sort();
    p
}

fn form(body: &[u8]) -> Vec<(String, String)> {
    let text = std::str::from_utf8(body).unwrap();
    let mut p: Vec<(String, String)> = text
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (decode(k), decode(v))
        })
        .collect();
    p.sort();
    p
}

fn xml(s: &str) -> String {
    let s = s.trim();
    let s = match s.strip_prefix("<?xml") {
        Some(rest) => rest.split_once("?>").map_or(rest, |(_, r)| r).trim(),
        None => s,
    };
    s.to_owned()
}

/// The client's blob convention, for a raw body.
fn blob(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s)
            if s.chars()
                .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')) =>
        {
            Value::String(s.to_owned())
        }
        _ => {
            use base64::Engine as _;
            serde_json::json!({"base64": base64::engine::general_purpose::STANDARD.encode(b)})
        }
    }
}

#[test]
fn requests_are_the_ones_botocore_sends() {
    let fx = fixture();
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() >= 30, "{} cases", cases.len());
    let client = Client::new(ClientConfig::new("us-west-2"));
    let now = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
    let attribution = Attribution::default();
    let mut failures = Vec::new();
    let mut by_protocol = std::collections::BTreeMap::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        *by_protocol
            .entry(case["protocol"].as_str().unwrap().to_owned())
            .or_insert(0) += 1;
        let call = Call {
            service: case["service"].as_str().unwrap(),
            operation: case["operation"].as_str().unwrap(),
            input: &case["input"],
            region: case["region"].as_str(),
            pages: 1,
            attribution: &attribution,
        };
        let p = match client.prepare(&call, &creds(), now) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let r = &p.request;
        let want = &case["request"];
        let mut check = |what: &str, got: String, want: String| {
            if got != want {
                failures.push(format!("{name}: {what}\n  got  {got}\n  want {want}"));
            }
        };
        check(
            "method",
            r.method.to_owned(),
            want["method"].as_str().unwrap().to_owned(),
        );
        check(
            "origin",
            r.origin.clone(),
            want["origin"].as_str().unwrap().to_owned(),
        );
        check(
            "path",
            r.path.clone(),
            want["path"].as_str().unwrap().to_owned(),
        );
        let mut query: Vec<(String, String)> = r
            .query
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().unwrap_or_default()))
            .collect();
        query.sort();
        check(
            "query",
            format!("{query:?}"),
            format!("{:?}", pairs(&want["query"])),
        );
        let mut headers: Vec<(String, String)> = r
            .headers
            .iter()
            .filter(|(k, _)| compared(k))
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
            .collect();
        headers.sort();
        let mut want_headers: Vec<(String, String)> = want["headers"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| compared(k))
            .map(|(k, v)| (k.to_ascii_lowercase(), v.as_str().unwrap().to_owned()))
            .collect();
        want_headers.sort();
        check(
            "headers",
            format!("{headers:?}"),
            format!("{want_headers:?}"),
        );
        let body = &want["body"];
        if body.is_null() {
            check("body", blob(&r.body).to_string(), "\"\"".to_owned());
        } else if let Some(f) = body.get("form") {
            check(
                "form",
                format!("{:?}", form(&r.body)),
                format!("{:?}", pairs(f)),
            );
        } else if let Some(j) = body.get("json") {
            let got: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            check("json", got.to_string(), j.to_string());
        } else if let Some(x) = body.get("xml") {
            check(
                "xml",
                xml(&String::from_utf8_lossy(&r.body)),
                xml(x.as_str().unwrap()),
            );
        } else if let Some(raw) = body.get("raw") {
            check("raw", blob(&r.body).to_string(), raw.to_string());
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Every protocol is covered.
    for protocol in ["query", "ec2", "json", "rest-json", "rest-xml"] {
        assert!(
            by_protocol.get(protocol).copied().unwrap_or(0) >= 4,
            "{protocol}: {by_protocol:?}"
        );
    }
}

#[tokio::test]
async fn answers_are_read_as_botocore_reads_them() {
    let fx = fixture();
    let attribution = Attribution::default();
    let mut failures = Vec::new();
    let mut read = 0;
    for case in fx["cases"].as_array().unwrap() {
        let Some(resp) = case.get("response") else {
            continue;
        };
        let name = case["name"].as_str().unwrap();
        let mut reply = Reply::new(
            resp["status"].as_u64().unwrap() as u16,
            resp["body"].as_str().unwrap(),
        );
        for (k, v) in resp["headers"].as_object().unwrap() {
            reply = reply.header(k, v.as_str().unwrap());
        }
        let fake = Fake::start(vec![reply]).await;
        let mut config = ClientConfig::new("us-west-2");
        config.endpoint_override = Some(fake.url.clone());
        config.retry.max_attempts = 1;
        config.attempt_timeout = Duration::from_secs(10);
        let client = Client::new(config);
        let call = Call {
            service: case["service"].as_str().unwrap(),
            operation: case["operation"].as_str().unwrap(),
            input: &case["input"],
            region: case["region"].as_str(),
            pages: 1,
            attribution: &attribution,
        };
        let result = client.call(&call, &creds()).await;
        read += 1;
        match (case.get("output"), case.get("error"), result) {
            (Some(want), _, Ok(out)) => {
                if &out.body != want {
                    failures.push(format!("{name}:\n  got  {}\n  want {want}", out.body));
                }
            }
            (_, Some(want), Err(e)) => {
                let Some(aws) = e.aws() else {
                    failures.push(format!("{name}: not an AWS error: {e}"));
                    continue;
                };
                let got = serde_json::json!({
                    "code": aws.code, "message": aws.message, "request_id": aws.request_id,
                });
                if &got != want {
                    failures.push(format!("{name}:\n  got  {got}\n  want {want}"));
                }
            }
            (_, _, other) => failures.push(format!("{name}: unexpected {other:?}")),
        }
    }
    assert!(read >= 20, "{read} answers");
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
