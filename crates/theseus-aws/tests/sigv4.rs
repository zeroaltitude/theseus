//! SigV4 against AWS's test suite: the 40 cases of the suite that the
//! aws-sigv4 crate ships (`fixtures/sigv4-suite.json`, packed by
//! `fixtures/sigv4-pack.py`), signed through the client's own entry point.
//! Each case checks the header signature, the presigned (query) signature,
//! and the whole `Authorization` header.
//!
//! Four cases' canonical requests encode the path once, as the client signs
//! S3's paths, though this version of the suite has no `double_uri_encode`
//! flag to say so: three carry a raw path or query (a space, UTF-8), which
//! the test encodes, and `get-space-normalized` an encoded one. aws-sigv4's
//! own tests skip those four; here they are signed under the rule their
//! vectors were made with.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_smithy_types::date_time::Format;
use aws_smithy_types::DateTime;
use serde_json::Value;
use theseus_aws::sign::{sign_parts, SigningRules};
use theseus_aws::Credentials;

struct Req {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// `request.txt`: the request line, headers (a line that starts with
/// whitespace continues the last one, unfolded into a space, as RFC 7230
/// lets a recipient do), a blank line, the body.
fn parse(text: &str) -> Req {
    let text = text.trim();
    let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
    let mut lines = head.split('\n');
    let first = lines.next().unwrap();
    let method = first.split(' ').next().unwrap().to_owned();
    let rest = &first[method.len() + 1..];
    let target = rest.rsplit_once(' ').map_or(rest, |(t, _)| t).to_owned();
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = headers.last_mut() {
                v.push(' ');
                v.push_str(line);
            }
        } else if let Some((k, v)) = line.split_once(':') {
            headers.push((k.to_owned(), v.to_owned()));
        }
    }
    Req {
        method,
        target,
        headers,
        body: body.as_bytes().to_vec(),
    }
}

/// Encodes what a URI cannot carry (a space, UTF-8), keeping what it can.
fn encode_raw(target: &str) -> String {
    let mut out = String::new();
    for c in target.chars() {
        if c.is_ascii_graphic() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn time(s: &str) -> SystemTime {
    let t = DateTime::from_str(s, Format::DateTime).unwrap();
    UNIX_EPOCH + Duration::from_secs(t.secs() as u64)
}

/// `20150830T123600Z`, the form of `X-Amz-Date`.
fn amz_date(s: &str) -> SystemTime {
    let iso = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &s[0..4],
        &s[4..6],
        &s[6..8],
        &s[9..11],
        &s[11..13],
        &s[13..15]
    );
    time(&iso)
}

fn authorization(signed_request: &str) -> String {
    signed_request
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        })
        .map(|(_, v)| v.trim().to_owned())
        .expect("an Authorization line")
}

#[test]
fn the_sigv4_suite() {
    let suite: Value = serde_json::from_str(include_str!("fixtures/sigv4-suite.json")).unwrap();
    let cases = suite["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 40);
    let (mut checked, mut failures) = (0, Vec::new());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let req = parse(case["request"].as_str().unwrap());
        let host = req
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("host"))
            .map(|(_, v)| v.trim().to_owned())
            .unwrap();
        let target = encode_raw(&req.target);
        let raw = target != req.target || name == "get-space-normalized";
        let url = format!("https://{host}{target}");

        let (creds, region, service, when, rules, expires) = match &case["context"] {
            Value::Null => {
                // The two cases migrated from the old suite: aws-sigv4's
                // `for_tests` key (ANOTREAL) for the first, the suite's
                // example key for the second; default rules.
                let date = req
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("x-amz-date"))
                    .map(|(_, v)| v.trim().to_owned())
                    .unwrap();
                let (creds, region, service) = if name == "double-encode-path" {
                    (
                        Credentials::new("ANOTREAL", "notrealrnrELgWzOk3IfjzDKtFBhDby", None, None),
                        "us-east-1",
                        "service",
                    )
                } else {
                    (
                        Credentials::new(
                            "AKIDEXAMPLE",
                            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                            None,
                            None,
                        ),
                        "us-east-2",
                        "lambda",
                    )
                };
                let rules = SigningRules {
                    double_encode: true,
                    normalize_path: true,
                    content_sha256: false,
                    sign_session_token: true,
                };
                (
                    creds,
                    region.to_owned(),
                    service.to_owned(),
                    amz_date(&date),
                    rules,
                    None,
                )
            }
            ctx => {
                let c = &ctx["credentials"];
                let creds = Credentials::new(
                    c["access_key_id"].as_str().unwrap(),
                    c["secret_access_key"].as_str().unwrap(),
                    c["token"].as_str().map(str::to_owned),
                    None,
                );
                let rules = SigningRules {
                    double_encode: !raw,
                    normalize_path: ctx["normalize"].as_bool().unwrap(),
                    content_sha256: ctx["sign_body"].as_bool().unwrap_or(true),
                    sign_session_token: !ctx["omit_session_token"].as_bool().unwrap_or(false),
                };
                (
                    creds,
                    ctx["region"].as_str().unwrap().to_owned(),
                    ctx["service"].as_str().unwrap().to_owned(),
                    time(ctx["timestamp"].as_str().unwrap()),
                    rules,
                    Some(Duration::from_secs(
                        ctx["expiration_in_seconds"].as_u64().unwrap(),
                    )),
                )
            }
        };

        let signed = sign_parts(
            &req.method,
            &url,
            &req.headers,
            Some(&req.body),
            &creds,
            &region,
            &service,
            rules,
            when,
            None,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        if let Some(want) = case["header_signature"].as_str() {
            if signed.signature != want {
                failures.push(format!(
                    "{name}: header signature {} != {want}",
                    signed.signature
                ));
            }
        }
        let want_auth = authorization(case["header_signed_request"].as_str().unwrap());
        let got_auth = signed
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        if got_auth != want_auth {
            failures.push(format!(
                "{name}: Authorization\n  got  {got_auth}\n  want {want_auth}"
            ));
        }
        if let (Some(want), Some(expires)) = (case["query_signature"].as_str(), expires) {
            let q = sign_parts(
                &req.method,
                &url,
                &req.headers,
                Some(&req.body),
                &creds,
                &region,
                &service,
                rules,
                when,
                Some(expires),
            )
            .unwrap_or_else(|e| panic!("{name} (presigned): {e}"));
            if q.signature != want {
                failures.push(format!("{name}: query signature {} != {want}", q.signature));
            }
            let sig_param = q
                .params
                .iter()
                .find(|(k, _)| k == "X-Amz-Signature")
                .map(|(_, v)| v.as_str());
            if sig_param != Some(q.signature.as_str()) {
                failures.push(format!("{name}: the presigned URL carries {sig_param:?}"));
            }
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {checked}:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(checked, 40);
}
