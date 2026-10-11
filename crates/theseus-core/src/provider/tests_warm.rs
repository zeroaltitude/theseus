//! The provider's connection (theseus-tnky): the client keeps an idle
//! connection for [`POOL_IDLE`] (300 s, where reqwest's own default is 90),
//! and its warm-up is a keyless `HEAD`, never a model call, sent only when
//! the connection is cold. The server is a local TLS-less stand-in that
//! counts connections and records each request's head.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{Anthropic, Provider, Timeouts, POOL_IDLE};
use crate::secrets::SecretBoard;

/// What the stand-in saw: connections accepted, and each request's head.
#[derive(Default)]
struct Seen {
    accepts: AtomicUsize,
    heads: Mutex<Vec<String>>,
}

/// A keep-alive HTTP/1.1 server on a loopback port: every request answered
/// 405 with no body, as the edge answers a `HEAD` of the messages path.
async fn server() -> (String, Arc<Seen>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Seen::default());
    let counted = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut conn, _)) = listener.accept().await else {
                return;
            };
            counted.accepts.fetch_add(1, Ordering::SeqCst);
            let seen = counted.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = conn.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        buf.drain(..end + 4);
                        seen.heads.lock().unwrap().push(head);
                        let ok = conn
                            .write_all(
                                b"HTTP/1.1 405 Method Not Allowed\r\ncontent-length: 0\r\n\r\n",
                            )
                            .await;
                        if ok.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    (base, seen)
}

fn client(base: &str, idle: Duration) -> Anthropic {
    Anthropic::with_pool_idle(
        base,
        SecretBoard::empty(),
        "anthropic_api_key",
        Timeouts::default(),
        idle,
    )
    .unwrap()
}

/// A `HEAD` straight through the client's pool, which `warm` would skip while
/// the connection was warm.
async fn head(a: &Anthropic, base: &str) {
    a.http
        .head(format!("{base}/v1/messages"))
        .send()
        .await
        .unwrap();
}

/// The setting is reqwest's pool idle time, and the daemon's client takes
/// 300 s: an idle connection is reused inside it, and a new one made past it.
#[tokio::test]
async fn the_pool_keeps_an_idle_connection_for_the_time_it_is_given() {
    assert_eq!(POOL_IDLE, Duration::from_secs(300));
    let built = Anthropic::new(
        "http://127.0.0.1:1",
        SecretBoard::empty(),
        "anthropic_api_key",
        Timeouts::default(),
    )
    .unwrap();
    assert_eq!(built.pool_idle, POOL_IDLE, "the daemon's client");

    let (base, seen) = server().await;
    let long = client(&base, Duration::from_secs(30));
    head(&long, &base).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    head(&long, &base).await;
    assert_eq!(
        seen.accepts.load(Ordering::SeqCst),
        1,
        "reused while idle inside it"
    );

    let short = client(&base, Duration::from_millis(300));
    head(&short, &base).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    head(&short, &base).await;
    assert_eq!(
        seen.accepts.load(Ordering::SeqCst),
        3,
        "the short client's second request made a new connection past its idle time"
    );
}

/// The warm-up is a `HEAD` with no key and no body, sent when the connection
/// is cold and not while it is warm.
#[tokio::test]
async fn the_warm_up_is_a_keyless_head_sent_only_when_cold() {
    let (base, seen) = server().await;
    let a = client(&base, Duration::from_secs(30));
    let first = Provider::warm(&a).await.unwrap();
    assert!(!first.already, "a cold client opens a connection");
    assert_eq!(seen.accepts.load(Ordering::SeqCst), 1);
    {
        let heads = seen.heads.lock().unwrap();
        assert_eq!(heads.len(), 1);
        let h = heads[0].to_ascii_lowercase();
        assert!(h.starts_with("head /v1/messages "), "{h}");
        assert!(!h.contains("x-api-key"), "no key leaves: {h}");
        assert!(
            !h.contains("content-length: ") || h.contains("content-length: 0"),
            "no body: {h}"
        );
    }
    let again = Provider::warm(&a).await.unwrap();
    assert!(again.already, "a warm client sends nothing");
    assert_eq!(seen.heads.lock().unwrap().len(), 1, "no second request");
}

/// A warm-up that cannot connect says why, and is no panic.
#[tokio::test]
async fn a_warm_up_that_cannot_connect_says_so() {
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let a = client(&base, Duration::from_secs(30));
    let why = Provider::warm(&a).await.unwrap_err();
    assert!(!why.is_empty());
    assert!(!a.is_warm(), "nothing is warm after a failure");
}
