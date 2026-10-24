//! Deepgram Flux's client (theseus-v774k) against a stand-in socket server on
//! 127.0.0.1: the handshake (path, query, auth header; the key in no URL), the
//! events in order, the clean close, a long call's keep-alive, a dropped socket
//! and its reconnect, a bounded queue, and a socket that cannot be opened.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use theseus_voice::{FluxListen, FluxSettings, Listen, Listened, Speaker, FLUX_MODEL};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message;

/// The key the stand-in expects. Invented, as every key in a test is.
const KEY: &str = "tv-flux-key-5c2e81";
const OWNER: Speaker = Speaker(101);

/// What the stand-in saw on one connection.
#[derive(Clone, Debug, Default)]
struct Conn {
    uri: String,
    headers: BTreeMap<String, String>,
    audio_msgs: usize,
    audio_bytes: usize,
    /// Audio messages that were all zeros (the keep-alive).
    silent_msgs: usize,
    texts: Vec<String>,
}

/// What the stand-in does with the nth connection.
#[derive(Clone, Default)]
struct Plan {
    /// Answer the handshake 401, with a body that quotes the key.
    reject: bool,
    /// After the first audio, send these.
    on_audio: Vec<String>,
    /// After the first audio, drop the connection with no close.
    drop_after_audio: bool,
    /// Accept the connection and never answer the handshake.
    hang: bool,
}

struct Server {
    addr: SocketAddr,
    conns: Arc<Mutex<Vec<Conn>>>,
}

impl Server {
    /// Connection `n` follows `plans[n]`, and the last plan after that.
    async fn start(plans: Vec<Plan>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("its address");
        let conns = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&conns);
        tokio::spawn(async move {
            let mut n = 0;
            while let Ok((sock, _)) = listener.accept().await {
                let plan = plans.get(n).or(plans.last()).cloned().unwrap_or_default();
                let index = {
                    let mut log = log.lock().unwrap();
                    log.push(Conn::default());
                    log.len() - 1
                };
                n += 1;
                tokio::spawn(serve(sock, plan, Arc::clone(&log), index));
            }
        });
        Self { addr, conns }
    }

    fn settings(&self) -> FluxSettings {
        FluxSettings {
            ws_base: format!("ws://{}", self.addr),
            connect_timeout: Duration::from_secs(2),
            backoff: Duration::from_millis(50),
            keep_alive: Duration::from_secs(60),
            ..FluxSettings::default()
        }
    }

    fn conns(&self) -> Vec<Conn> {
        self.conns.lock().unwrap().clone()
    }
}

async fn serve(sock: tokio::net::TcpStream, plan: Plan, log: Arc<Mutex<Vec<Conn>>>, index: usize) {
    if plan.hang {
        tokio::time::sleep(Duration::from_secs(60)).await;
        return;
    }
    let (seen, reject) = (Arc::clone(&log), plan.reject);
    // The handshake callback's error type is tungstenite's, large as it is.
    #[allow(clippy::result_large_err)]
    let callback = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let mut log = seen.lock().unwrap();
        log[index].uri = req.uri().to_string();
        for (k, v) in req.headers() {
            log[index]
                .headers
                .insert(k.to_string(), v.to_str().unwrap_or_default().to_string());
        }
        if reject {
            let mut refused = ErrorResponse::new(Some(format!("invalid credentials {KEY}")));
            *refused.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
            return Err(refused);
        }
        Ok(resp)
    };
    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(sock, callback).await else {
        return;
    };
    let _ = ws
        .send(Message::text(r#"{"type":"Connected","request_id":"r1"}"#))
        .await;
    while let Some(Ok(message)) = ws.next().await {
        match message {
            Message::Binary(bytes) => {
                let first = {
                    let mut log = log.lock().unwrap();
                    let c = &mut log[index];
                    c.audio_msgs += 1;
                    c.audio_bytes += bytes.len();
                    if bytes.iter().all(|b| *b == 0) {
                        c.silent_msgs += 1;
                    }
                    c.audio_msgs == 1
                };
                if first {
                    for text in &plan.on_audio {
                        let _ = ws.send(Message::text(text.clone())).await;
                    }
                    if plan.drop_after_audio {
                        return;
                    }
                }
            }
            Message::Text(text) => {
                log.lock().unwrap()[index].texts.push(text.to_string());
                if text.contains("CloseStream") {
                    let _ = ws.close(None).await;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

fn frames(n: usize) -> Vec<i16> {
    vec![1_000; 960 * n]
}

async fn event(listen: &mut FluxListen) -> Listened {
    timeout(Duration::from_secs(5), listen.next())
        .await
        .expect("an event within 5 s")
        .expect("the listener is up")
}

fn turn_info(event: &str, transcript: &str, words: &[f32]) -> String {
    let words: Vec<String> = words
        .iter()
        .map(|c| format!(r#"{{"word":"w","confidence":{c}}}"#))
        .collect();
    format!(
        r#"{{"type":"TurnInfo","event":"{event}","turn_index":0,"transcript":"{transcript}","end_of_turn_confidence":0.4,"words":[{}]}}"#,
        words.join(",")
    )
}

/// The handshake names the model, the encoding and the rate, carries the key
/// in the Authorization header alone, and the events come in order with the
/// words' confidence; the call's end closes the stream with the docs' message.
#[tokio::test]
async fn the_handshake_and_the_events() {
    let plan = Plan {
        on_audio: vec![
            turn_info("StartOfTurn", "", &[]),
            turn_info("Update", "book a", &[0.9]),
            turn_info("EndOfTurn", "Book a table.", &[0.9, 0.7]),
        ],
        ..Plan::default()
    };
    let server = Server::start(vec![plan]).await;
    let mut listen = FluxListen::new(KEY, server.settings(), None).unwrap();
    listen.push(OWNER, &frames(4));
    assert_eq!(event(&mut listen).await, Listened::Start { speaker: OWNER });
    assert_eq!(
        event(&mut listen).await,
        Listened::Words {
            speaker: OWNER,
            text: "book a".into()
        }
    );
    let Listened::End {
        text,
        confidence,
        streamed,
        ..
    } = event(&mut listen).await
    else {
        panic!("an end of turn")
    };
    assert_eq!(text, "Book a table.");
    assert!(
        (confidence.unwrap() - 0.8).abs() < 1e-6,
        "the words' mean, not 0.4"
    );
    assert_eq!(streamed, Duration::from_millis(80));
    listen.close().await;
    let conns = server.conns();
    assert_eq!(conns.len(), 1);
    let c = &conns[0];
    assert!(c.uri.starts_with("/v2/listen?"), "{}", c.uri);
    for want in [
        format!("model={FLUX_MODEL}"),
        "encoding=linear16".to_string(),
        "sample_rate=48000".to_string(),
    ] {
        assert!(c.uri.contains(&want), "{} lacks {want}", c.uri);
    }
    assert!(!c.uri.contains(KEY), "the key is in no URL: {}", c.uri);
    assert_eq!(
        c.headers.get("authorization").map(String::as_str),
        Some(&*format!("Token {KEY}"))
    );
    assert_eq!(
        c.audio_bytes,
        960 * 4 * 2,
        "80 ms of 16-bit samples at 48 kHz"
    );
    assert!(
        c.texts.iter().any(|t| t.contains("CloseStream")),
        "the docs' close message: {:?}",
        c.texts
    );
}

/// A socket the server refuses is the call's: unavailable, in words that
/// carry neither the key nor the server's quote of it.
#[tokio::test]
async fn a_refused_handshake_is_unavailable_and_the_key_is_in_no_error() {
    let server = Server::start(vec![Plan {
        reject: true,
        ..Plan::default()
    }])
    .await;
    let mut listen = FluxListen::new(KEY, server.settings(), None).unwrap();
    listen.push(OWNER, &frames(4));
    let Listened::Unavailable { error } = event(&mut listen).await else {
        panic!("unavailable")
    };
    assert!(error.contains("401"), "{error}");
    assert!(!error.contains(KEY), "{error}");
    assert!(!format!("{listen:?}").contains(KEY));
}

/// Nothing listens: the socket cannot be opened, and that is said without
/// the key.
#[tokio::test]
async fn a_socket_that_cannot_be_opened_is_unavailable() {
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    let settings = FluxSettings {
        ws_base: format!("ws://{addr}"),
        connect_timeout: Duration::from_secs(2),
        ..FluxSettings::default()
    };
    let mut listen = FluxListen::new(KEY, settings, None).unwrap();
    listen.push(OWNER, &frames(4));
    let Listened::Unavailable { error } = event(&mut listen).await else {
        panic!("unavailable")
    };
    assert!(error.contains("could not open the socket"), "{error}");
    assert!(!error.contains(KEY));
}

/// The budget says no: no socket is opened.
#[tokio::test]
async fn a_budget_that_says_no_opens_no_socket() {
    let server = Server::start(vec![Plan::default()]).await;
    let admit: theseus_voice::flux::Admit = Arc::new(|_| Err("the session's limit".into()));
    let mut listen = FluxListen::new(KEY, server.settings(), Some(admit)).unwrap();
    listen.push(OWNER, &frames(4));
    let Listened::Unavailable { error } = event(&mut listen).await else {
        panic!("unavailable")
    };
    assert!(error.contains("limit"), "{error}");
    assert!(server.conns().is_empty());
}

/// A silent minute (here 400 ms, with a 50 ms keep-alive) sends silence, so
/// the socket stays open and is not closed by an idle timeout.
#[tokio::test]
async fn a_quiet_speakers_socket_is_kept_alive() {
    let server = Server::start(vec![Plan::default()]).await;
    let settings = FluxSettings {
        keep_alive: Duration::from_millis(50),
        ..server.settings()
    };
    let mut listen = FluxListen::new(KEY, settings, None).unwrap();
    listen.push(OWNER, &frames(4));
    tokio::time::sleep(Duration::from_millis(420)).await;
    let conns = server.conns();
    assert_eq!(conns.len(), 1, "one socket for the quiet stretch");
    assert!(conns[0].silent_msgs >= 5, "{:?}", conns[0]);
    listen.close().await;
}

/// The server drops the socket in the middle of a turn: the client says the
/// stream is down, opens another after its backoff, says it is up, and the
/// next turn is heard.
#[tokio::test]
async fn a_dropped_socket_is_reopened_and_the_next_turn_is_heard() {
    let drops = Plan {
        on_audio: vec![turn_info("StartOfTurn", "", &[])],
        drop_after_audio: true,
        ..Plan::default()
    };
    let serves = Plan {
        on_audio: vec![turn_info("EndOfTurn", "Next one.", &[0.9])],
        ..Plan::default()
    };
    let server = Server::start(vec![drops, serves]).await;
    let mut listen = FluxListen::new(KEY, server.settings(), None).unwrap();
    listen.push(OWNER, &frames(4));
    assert_eq!(event(&mut listen).await, Listened::Start { speaker: OWNER });
    assert!(matches!(event(&mut listen).await, Listened::Down { .. }));
    assert_eq!(event(&mut listen).await, Listened::Up { speaker: OWNER });
    listen.push(OWNER, &frames(4));
    assert!(matches!(
        event(&mut listen).await,
        Listened::End { text, .. } if text == "Next one."
    ));
    assert_eq!(server.conns().len(), 2);
    listen.close().await;
}

/// A server that never answers the handshake stalls the speaker's stream: the
/// outbound queue holds its bound of audio and drops the oldest, counted.
#[tokio::test]
async fn a_stalled_server_makes_the_queue_drop_the_oldest_audio() {
    let server = Server::start(vec![Plan {
        hang: true,
        ..Plan::default()
    }])
    .await;
    let settings = FluxSettings {
        queue: Duration::from_millis(320),
        ..server.settings()
    };
    let mut listen = FluxListen::new(KEY, settings, None).unwrap();
    // Three seconds of audio, a frame at a time.
    for _ in 0..150 {
        listen.push(OWNER, &frames(1));
    }
    let dropped = listen.dropped(OWNER);
    assert!(
        dropped >= Duration::from_millis(2_600),
        "kept 320 ms of 3 s, dropped {dropped:?}"
    );
    listen.close().await;
}
