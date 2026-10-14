//! The token counter (theseus-7m7w.1): a stand-in server answers `count_tokens` and records what it was
//! sent. The body carries the request's input and nothing that makes a model run.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{Anthropic, Provider, ProviderRequest, Timeouts};
use crate::secrets::{Secret, SecretBoard};

/// A server that answers every request with `reply`, keeping each request's head and body.
async fn server(status: &'static str, reply: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut conn, _)) = listener.accept().await else {
                return;
            };
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let Ok(n) = conn.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let want = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() < end + 4 + want {
                        continue;
                    }
                    let body = String::from_utf8_lossy(&buf[end + 4..end + 4 + want]).to_string();
                    kept.lock().unwrap().push(format!("{head}\n\n{body}"));
                    buf.drain(..end + 4 + want);
                    let out = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{reply}",
                        reply.len()
                    );
                    if conn.write_all(out.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (base, seen)
}

fn client(base: &str, key: bool) -> Anthropic {
    let board = SecretBoard::new(["k".to_string()], std::time::Instant::now());
    if key {
        board.hold("k", Secret::new("sk-test-key".into()));
    }
    Anthropic::new(base, board, "k", Timeouts::default()).unwrap()
}

fn request() -> ProviderRequest {
    let mut r = ProviderRequest::simple("claude-sonnet-5-5", 4096, "hello");
    r.system = vec![serde_json::json!({"type": "text", "text": "You are a ship."})];
    r.tools = vec![
        serde_json::json!({"name": "fs_read", "description": "Reads.", "input_schema": {"type": "object"}}),
    ];
    r
}

#[tokio::test]
async fn a_count_sends_the_requests_input_and_reads_its_tokens() {
    let (base, seen) = server("200 OK", r#"{"input_tokens":1234}"#).await;
    let n = Provider::count_tokens(&client(&base, true), &request())
        .await
        .unwrap();
    assert_eq!(n, 1234);
    let sent = seen.lock().unwrap()[0].clone();
    let head = sent.to_ascii_lowercase();
    assert!(
        head.starts_with("post /v1/messages/count_tokens "),
        "{head}"
    );
    assert!(head.contains("x-api-key: sk-test-key"));
    let body: serde_json::Value = serde_json::from_str(sent.split("\n\n").nth(1).unwrap()).unwrap();
    for k in ["model", "messages", "system", "tools"] {
        assert!(body.get(k).is_some(), "{k} rides: {body}");
    }
    for k in ["max_tokens", "stream"] {
        assert!(body.get(k).is_none(), "{k} does not: {body}");
    }
}

#[tokio::test]
async fn a_refused_count_says_why_and_a_missing_key_says_so() {
    let (base, _) = server(
        "400 Bad Request",
        r#"{"error":{"type":"invalid_request_error","message":"nope"}}"#,
    )
    .await;
    let e = Provider::count_tokens(&client(&base, true), &request())
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("400") && e.contains("nope"), "{e}");
    let e = Provider::count_tokens(&client(&base, false), &request())
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("has not resolved"), "{e}");
    let _ = Duration::ZERO;
}
