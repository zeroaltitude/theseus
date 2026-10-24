//! A call's bounds (theseus-5pg8u): `total_secs` bounds a call until its
//! stream begins, and a live stream (events arriving within
//! `stream_idle_secs`) goes on to `Timeouts::whole_call`, three times that.
//! The server is a loopback stand-in that sends the stream's head at once and
//! then pings on a timer.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    Anthropic, Provider, ProviderError, ProviderRequest, TimeoutPhase, Timeouts, LIVE_STREAM_TOTALS,
};
use crate::secrets::{Secret, SecretBoard};

#[derive(Clone, Copy)]
enum Stream {
    /// Pings every 200 ms for this long, then a whole message.
    Pings(Duration),
    /// Pings until the client goes away.
    Forever,
    /// Headers, then nothing.
    Silent,
}

const START: &str = concat!(
    "event: message_start\n",
    r#"data: {"type":"message_start","message":{"model":"claude-x","usage":{"input_tokens":1}}}"#,
    "\n\n",
);
const PING: &str = "event: ping\ndata: {\"type\":\"ping\"}\n\n";
const END: &str = concat!(
    r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
    "\n\n",
    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
    "\n\n",
    r#"data: {"type":"content_block_stop","index":0}"#,
    "\n\n",
    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}"#,
    "\n\n",
    r#"data: {"type":"message_stop"}"#,
    "\n\n",
);

async fn server(stream: Stream) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut conn, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 65536];
                let mut head = Vec::new();
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match conn.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let ok = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
                if conn
                    .write_all(format!("{ok}{START}").as_bytes())
                    .await
                    .is_err()
                {
                    return;
                }
                let began = Instant::now();
                match stream {
                    Stream::Silent => tokio::time::sleep(Duration::from_secs(30)).await,
                    Stream::Pings(len) => {
                        while began.elapsed() < len {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            if conn.write_all(PING.as_bytes()).await.is_err() {
                                return;
                            }
                        }
                        let _ = conn.write_all(END.as_bytes()).await;
                    }
                    Stream::Forever => loop {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        if conn.write_all(PING.as_bytes()).await.is_err() {
                            return;
                        }
                    },
                }
            });
        }
    });
    base
}

fn client(base: &str) -> Anthropic {
    let board = SecretBoard::new(["k".to_string()], Instant::now());
    board.hold("k", Secret::new("sk-test-key".into()));
    let timeouts = Timeouts {
        connect_secs: 5,
        first_byte_secs: 5,
        stream_idle_secs: 1,
        total_secs: 1,
    };
    Anthropic::new(base, board, "k", timeouts).unwrap()
}

async fn call(stream: Stream) -> (anyhow::Result<super::ModelResponse>, Duration) {
    let base = server(stream).await;
    let req = ProviderRequest::simple("claude-sonnet-5-5", 64, "hello");
    let started = Instant::now();
    let got = Provider::stream_message(&client(&base), &req, &mut |_| {}).await;
    (got, started.elapsed())
}

fn phase(e: &anyhow::Error) -> Option<TimeoutPhase> {
    match e.downcast_ref::<ProviderError>() {
        Some(ProviderError::Timeout { phase, .. }) => Some(*phase),
        _ => None,
    }
}

#[test]
fn the_whole_call_is_the_live_stream_totals_times_total_secs() {
    let t = Timeouts {
        total_secs: 600,
        ..Timeouts::default()
    };
    assert_eq!(LIVE_STREAM_TOTALS, 3);
    assert_eq!(t.whole_call(), Duration::from_secs(1800));
    assert_eq!(Timeouts::default().whole_call(), Duration::from_secs(1800));
}

/// A stream that keeps pinging past `total_secs` is alive, and finishes.
#[tokio::test]
async fn a_live_stream_is_not_cut_at_total_secs() {
    let (got, took) = call(Stream::Pings(Duration::from_millis(2500))).await;
    let out = got.unwrap_or_else(|e| panic!("cut after {took:?}: {e:#}"));
    assert_eq!(out.text, "Hi");
    assert!(took > Duration::from_secs(2), "{took:?}");
}

/// A stream that pings forever ends at the hard bound, as a `Total` timeout.
#[tokio::test]
async fn a_stream_that_never_ends_is_cut_at_the_hard_bound() {
    let (got, took) = call(Stream::Forever).await;
    let e = got.unwrap_err();
    assert_eq!(phase(&e), Some(TimeoutPhase::Total), "{e:#}");
    assert!(
        took >= Duration::from_secs(3) && took < Duration::from_secs(4),
        "{took:?}"
    );
}

/// A stream that goes silent is cut by the idle bound, long before.
#[tokio::test]
async fn a_silent_stream_is_cut_as_idle() {
    let (got, took) = call(Stream::Silent).await;
    let e = got.unwrap_err();
    assert_eq!(phase(&e), Some(TimeoutPhase::StreamIdle), "{e:#}");
    assert!(took < Duration::from_secs(3), "{took:?}");
}
