//! Deepgram (45a) against a stand-in on 127.0.0.1: each request's method,
//! path, query, auth header and body, and the answers that fail it: a 4xx, a
//! 5xx, no answer at all, and a body that is not what was asked for. Every
//! connect is to a port the stand-in listens on, and each call is bounded by
//! the provider's own timeouts.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_voice::{
    audio::parse_wav, Audio, DeepgramSettings, DeepgramSpeech, Speaker, Speech, SAMPLE_RATE,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The key the stand-in expects. Invented, as every key in a test is.
const KEY: &str = "tv-deepgram-7f3a9c";

/// A request as the stand-in read it.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

/// What the stand-in answers every request with.
#[derive(Clone)]
enum Answer {
    Ok {
        content_type: &'static str,
        extra: Vec<(&'static str, String)>,
        body: Vec<u8>,
    },
    Status(u16, &'static str, String),
    /// Reads the request, then never answers.
    Silent,
}

struct Fake {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    async fn start(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("its address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let (log, answer) = (Arc::clone(&log), answer.clone());
                tokio::spawn(serve(sock, log, answer));
            }
        });
        Self { addr, seen }
    }

    fn settings(&self) -> DeepgramSettings {
        DeepgramSettings {
            api_base: format!("http://{}", self.addr),
            connect_timeout: Duration::from_secs(2),
            timeout: Duration::from_secs(5),
            ..DeepgramSettings::default()
        }
    }

    fn speech(&self) -> DeepgramSpeech {
        DeepgramSpeech::new(KEY, self.settings()).expect("a provider")
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log's lock").clone()
    }
}

async fn serve(mut sock: TcpStream, log: Arc<Mutex<Vec<Seen>>>, answer: Answer) {
    let Some(seen) = read_request(&mut sock).await else {
        return;
    };
    log.lock().expect("the log's lock").push(seen);
    let (status, reason, content_type, extra, body) = match answer {
        Answer::Ok {
            content_type,
            extra,
            body,
        } => (200, "OK", content_type, extra, body),
        Answer::Status(code, reason, body) => {
            (code, reason, "application/json", vec![], body.into_bytes())
        }
        Answer::Silent => {
            tokio::time::sleep(Duration::from_secs(60)).await;
            return;
        }
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(&body).await;
    let _ = sock.shutdown().await;
}

/// One HTTP/1.1 request: its line, its headers (names in lower case), and a
/// body of its content-length.
async fn read_request(sock: &mut TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (unescape(k), unescape(v)))
        .collect();
    Some(Seen {
        method,
        path: path.to_string(),
        query,
        headers,
        body,
    })
}

/// A query part's `%XX` escapes and `+`, decoded.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Deepgram's pre-recorded answer, as much of it as the provider reads, with
/// the noise a real one carries around it.
fn listened(transcript: &str, duration: f64) -> Vec<u8> {
    serde_json::json!({
        "metadata": {"request_id": "req-1", "duration": duration, "channels": 1, "models": ["m-1"]},
        "results": {"channels": [{"alternatives": [{"transcript": transcript, "confidence": 0.98, "words": []}]}]}
    })
    .to_string()
    .into_bytes()
}

fn json_ok(body: Vec<u8>) -> Answer {
    Answer::Ok {
        content_type: "application/json",
        extra: vec![],
        body,
    }
}

/// A second of a tone at 48 kHz: what a listed speaker said.
fn spoken() -> Audio {
    Audio::tone(220.0, Duration::from_secs(1), 0.3)
}

#[tokio::test]
async fn speech_to_text_sends_one_wav_with_the_model_the_language_and_the_key() {
    let fake = Fake::start(json_ok(listened("Hello there, Robin.", 1.25))).await;
    let audio = spoken();
    let t = fake
        .speech()
        .transcribe(Speaker(7), &audio)
        .await
        .expect("a transcript");
    assert_eq!(t.text, "Hello there, Robin.");
    assert_eq!(t.usage.provider, "deepgram");
    assert_eq!(t.usage.model, "nova-3");
    assert_eq!(
        t.usage.audio,
        Duration::from_millis(1250),
        "Deepgram's own figure"
    );
    assert_eq!(t.usage.chars, 19);
    let seen = fake.seen();
    assert_eq!(seen.len(), 1, "one request per utterance");
    let r = &seen[0];
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/v1/listen"));
    let q: Vec<(&str, &str)> = r
        .query
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        q,
        [
            ("language", "en-US"),
            ("model", "nova-3"),
            ("smart_format", "true")
        ]
    );
    assert_eq!(r.headers["authorization"], format!("Token {KEY}"));
    assert_eq!(r.headers["content-type"], "audio/wav");
    assert_eq!(r.headers["accept"], "application/json");
    // The body is the utterance as a WAV, sample for sample.
    let sent = parse_wav(&r.body).expect("a WAV body");
    assert_eq!(sent, audio);
    assert_eq!(&r.body[24..28], &SAMPLE_RATE.to_le_bytes(), "48 kHz");
}

#[tokio::test]
async fn synthesis_sends_the_sentence_and_reads_raw_linear16_at_48_khz() {
    // 0.1 s of a ramp, as Deepgram sends it: little-endian 16-bit, no header.
    let samples: Vec<i16> = (0..4800).map(|i| (i % 300 - 150) as i16).collect();
    let body: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let fake = Fake::start(Answer::Ok {
        content_type: "audio/l16",
        extra: vec![("dg-char-count", "26".into())],
        body,
    })
    .await;
    let text = "The tide turns at four.";
    let s = fake.speech().synthesize(text).await.expect("audio");
    assert_eq!(s.audio.samples(), samples.as_slice());
    assert_eq!(s.audio.duration(), Duration::from_millis(100));
    assert_eq!(s.usage.provider, "deepgram");
    assert_eq!(s.usage.model, "aura-2-andromeda-en");
    assert_eq!(s.usage.audio, Duration::from_millis(100));
    assert_eq!(s.usage.chars, 26, "Deepgram's count, from its header");
    let seen = fake.seen();
    let r = &seen[0];
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/v1/speak"));
    let q: Vec<(&str, &str)> = r
        .query
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        q,
        [
            ("container", "none"),
            ("encoding", "linear16"),
            ("model", "aura-2-andromeda-en"),
            ("sample_rate", "48000")
        ]
    );
    assert_eq!(r.headers["authorization"], format!("Token {KEY}"));
    assert_eq!(r.headers["content-type"], "application/json");
    let sent: serde_json::Value = serde_json::from_slice(&r.body).expect("a JSON body");
    assert_eq!(sent, serde_json::json!({ "text": text }));
}

#[tokio::test]
async fn without_deepgrams_counts_the_request_gives_the_usage() {
    let fake = Fake::start(json_ok(
        br#"{"results":{"channels":[{"alternatives":[{"transcript":" ok "}]}]}}"#.to_vec(),
    ))
    .await;
    let audio = spoken();
    let t = fake.speech().transcribe(Speaker(1), &audio).await.unwrap();
    assert_eq!(t.text, "ok", "trimmed");
    assert_eq!(t.usage.audio, audio.duration(), "the utterance's length");
    let fake = Fake::start(Answer::Ok {
        content_type: "audio/l16",
        extra: vec![],
        body: vec![0; 960],
    })
    .await;
    let s = fake.speech().synthesize("Two words.").await.unwrap();
    assert_eq!(s.usage.chars, 10, "the sentence's characters");
}

#[tokio::test]
async fn the_settings_choose_the_model_the_language_and_the_voice() {
    let fake = Fake::start(json_ok(listened("hallo", 0.5))).await;
    let settings = DeepgramSettings {
        stt_model: "nova-2".into(),
        language: "de".into(),
        tts_voice: "aura-2-thalia-en".into(),
        ..fake.settings()
    };
    let speech = DeepgramSpeech::new(KEY, settings).unwrap();
    let t = speech.transcribe(Speaker(1), &spoken()).await.unwrap();
    assert_eq!(t.usage.model, "nova-2");
    let _ = speech.synthesize("Hallo.").await;
    let seen = fake.seen();
    assert_eq!(seen[0].query["model"], "nova-2");
    assert_eq!(seen[0].query["language"], "de");
    assert_eq!(seen[1].query["model"], "aura-2-thalia-en");
}

#[tokio::test]
async fn a_4xx_says_the_status_and_deepgrams_words_and_never_the_key() {
    // An answer that echoes the key back, to prove it is scrubbed.
    let body = format!(
        r#"{{"err_code":"INVALID_AUTH","err_msg":"Invalid credentials.","sent":"Token {KEY}"}}"#
    );
    let fake = Fake::start(Answer::Status(401, "Unauthorized", body)).await;
    let speech = fake.speech();
    let e = speech.transcribe(Speaker(1), &spoken()).await.unwrap_err();
    let said = e.to_string();
    assert!(said.starts_with("Deepgram speech to text: "), "{said}");
    assert!(said.contains("401 Unauthorized"), "{said}");
    assert!(said.contains("INVALID_AUTH"), "{said}");
    assert!(!said.contains(KEY), "the key is never in an error: {said}");
    let e = speech.synthesize("Hello.").await.unwrap_err();
    assert!(e.to_string().starts_with("Deepgram synthesis: "), "{e}");
    assert!(!e.to_string().contains(KEY), "{e}");
}

#[tokio::test]
async fn a_5xx_is_a_failed_call() {
    let fake = Fake::start(Answer::Status(
        503,
        "Service Unavailable",
        r#"{"err_msg":"busy"}"#.into(),
    ))
    .await;
    let speech = fake.speech();
    let e = speech.transcribe(Speaker(1), &spoken()).await.unwrap_err();
    assert!(e.to_string().contains("503"), "{e}");
    let e = speech.synthesize("Hello.").await.unwrap_err();
    assert!(e.to_string().contains("503 Service Unavailable"), "{e}");
}

#[tokio::test]
async fn a_call_with_no_answer_ends_at_its_timeout() {
    let fake = Fake::start(Answer::Silent).await;
    let settings = DeepgramSettings {
        timeout: Duration::from_millis(300),
        ..fake.settings()
    };
    let speech = DeepgramSpeech::new(KEY, settings).unwrap();
    let t0 = Instant::now();
    let e = speech.synthesize("Hello.").await.unwrap_err();
    let took = t0.elapsed();
    assert!(
        e.to_string()
            .contains("no answer from Deepgram within 0.3 s"),
        "{e}"
    );
    assert!(took >= Duration::from_millis(300), "{took:?}");
    assert!(took < Duration::from_secs(5), "bounded: {took:?}");
    let e = speech.transcribe(Speaker(1), &spoken()).await.unwrap_err();
    assert!(e.to_string().contains("no answer"), "{e}");
    assert_eq!(fake.seen().len(), 2, "both reached the stand-in");
}

#[tokio::test]
async fn a_body_that_is_not_what_was_asked_for_is_refused() {
    let fake = Fake::start(json_ok(b"<html>not json".to_vec())).await;
    let e = fake
        .speech()
        .transcribe(Speaker(1), &spoken())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("could not be read"), "{e}");
    assert!(e.to_string().contains("<html>not json"), "quoted: {e}");
    let fake = Fake::start(json_ok(br#"{"results":{"channels":[]}}"#.to_vec())).await;
    let e = fake
        .speech()
        .transcribe(Speaker(1), &spoken())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("no channel"), "{e}");
    let fake = Fake::start(Answer::Ok {
        content_type: "audio/l16",
        extra: vec![],
        body: vec![1, 2, 3],
    })
    .await;
    let e = fake.speech().synthesize("Odd.").await.unwrap_err();
    assert!(e.to_string().contains("not 16-bit audio"), "{e}");
    assert!(e.to_string().contains("odd count"), "{e}");
}
