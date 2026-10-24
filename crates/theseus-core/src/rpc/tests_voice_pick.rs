//! The speaking voice (theseus-slvl.7): `voice.show`, and `voice.audition`
//! against a stand-in Deepgram: a clip per voice, the default spread, the
//! refusal without a key, the cost, and no key in any answer.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use theseus_protocol::voice_pick::{VoiceAuditionParams, VoiceShowParams};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::config::PersonaConfig;
use crate::provider::FakeProvider;
use crate::secrets::Secret;

const KEY: &str = "tv-deepgram-9d41c2";

/// A request as the stand-in saw it: its query and its Authorization header.
type Seen = (BTreeMap<String, String>, String);

/// A stand-in Deepgram: it logs each `/v1/speak` request's query and
/// Authorization header and answers `status` with `body`.
struct Fake {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    async fn start(status: u16, body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (log, body) = (Arc::clone(&log), body.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let (head, mut have, want) = loop {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..at]).to_string();
                            let want = head
                                .lines()
                                .filter_map(|l| l.split_once(':'))
                                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (head, buf.len() - at - 4, want);
                        }
                    };
                    while have < want {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        have += n;
                    }
                    let target = head
                        .lines()
                        .next()
                        .and_then(|l| l.split(' ').nth(1))
                        .unwrap_or("")
                        .to_string();
                    let query = target
                        .split_once('?')
                        .map(|(_, q)| q)
                        .unwrap_or("")
                        .split('&')
                        .filter_map(|kv| kv.split_once('='))
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();
                    let auth = head
                        .lines()
                        .filter_map(|l| l.split_once(':'))
                        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                        .map(|(_, v)| v.trim().to_string())
                        .unwrap_or_default();
                    log.lock().unwrap().push((query, auth));
                    let head = format!(
                        "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { addr, seen }
    }

    fn models(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(q, _)| q.get("model").cloned().unwrap_or_default())
            .collect()
    }
}

fn wav() -> Vec<u8> {
    let mut b = b"RIFF\x24\0\0\0WAVEfmt ".to_vec();
    b.extend_from_slice(&[0u8; 32]);
    b
}

/// A core with the voice key configured and resolved (or not), speaking to
/// `api`.
fn voice_core(api: &str, key: bool, edit: impl FnOnce(&mut Config)) -> Arc<Core> {
    core_resolving(api, key, key, edit)
}

/// `core`, with the key configured (`key`) and resolved (`resolved`) apart.
fn core_resolving(
    api: &str,
    key: bool,
    resolved: bool,
    edit: impl FnOnce(&mut Config),
) -> Arc<Core> {
    let dir = crate::scratch::dir("t");
    let store = Store::open(&dir.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.voice.enabled = true;
    cfg.voice.api_base = api.to_string();
    if key {
        cfg.secrets
            .insert("deepgram_api_key".into(), "env:UNUSED_IN_THIS_TEST".into());
    }
    edit(&mut cfg);
    let secrets = SecretBoard::new(["deepgram_api_key".to_string()], std::time::Instant::now());
    if resolved {
        secrets.publish(
            [("deepgram_api_key".to_string(), Ok(Secret::new(KEY.into())))].into(),
            "fake",
        );
    }
    Core::build(Parts {
        secrets,
        ..Parts::for_tests(cfg, Arc::new(FakeProvider::default()), store)
    })
    .unwrap()
}

fn persona(name: &str, voice: Option<&str>) -> PersonaConfig {
    PersonaConfig {
        name: Some(name.into()),
        tts_voice: voice.map(str::to_string),
        files: vec![crate::personas::test_soul(name)],
        ..PersonaConfig::default()
    }
}

#[tokio::test]
async fn voice_show_names_the_voice_and_where_it_comes_from() {
    let core = voice_core("http://127.0.0.1:9", true, |c| {
        c.voice.tts_voice = "aura-2-thalia-en".into();
        c.personas
            .insert("sage".into(), persona("Sage", Some("aura-2-orion-en")));
        c.context.default_persona = Some("sage".into());
    });
    let r = core.voice_show(VoiceShowParams::default()).unwrap();
    assert_eq!(
        (r.voice.as_str(), r.from.as_str(), r.name.as_str()),
        ("aura-2-orion-en", "persona", "Sage")
    );
    assert_eq!(r.global, "aura-2-thalia-en");
    assert_eq!(r.greeting.as_deref(), Some("Sage here. Go ahead."));
    assert!(r.key_ready && r.enabled);
    // No persona voice: the global one, and the built-in persona's name.
    let core = voice_core("http://127.0.0.1:9", false, |c| {
        c.voice.tts_voice = "aura-2-thalia-en".into();
    });
    let r = core.voice_show(VoiceShowParams::default()).unwrap();
    assert_eq!(
        (r.voice.as_str(), r.from.as_str(), r.name.as_str()),
        ("aura-2-thalia-en", "[voice] tts_voice", "Theseus")
    );
    assert!(!r.key_ready);
}

/// A clip a voice, in the order asked, as WAV; the run's estimate is the sum
/// of the voices' prices for the characters said; the key rode in the
/// Authorization header and is in no answer.
#[tokio::test]
async fn an_audition_makes_a_clip_for_each_voice_and_never_carries_the_key() {
    let fake = Fake::start(200, wav()).await;
    let core = voice_core(&format!("http://{}", fake.addr), true, |_| {});
    let r = core
        .voice_audition(
            VoiceAuditionParams {
                voices: vec!["aura-2-orion-en".into(), "aura-2-luna-en".into()],
                text: Some("Sage here".into()),
                session_id: None,
            },
            "the CLI",
        )
        .await
        .unwrap();
    assert_eq!(r.text, "Sage here");
    let voices: Vec<&str> = r.clips.iter().map(|c| c.voice.as_str()).collect();
    assert_eq!(voices, ["aura-2-orion-en", "aura-2-luna-en"]);
    for c in &r.clips {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&c.wav)
            .unwrap();
        assert!(bytes.starts_with(b"RIFF"));
    }
    // 9 characters at Aura-2's $0.030 a thousand: 270 µ$ a voice.
    assert_eq!(r.estimate_micros, 540);
    assert_eq!(fake.models(), ["aura-2-orion-en", "aura-2-luna-en"]);
    let seen = fake.seen.lock().unwrap().clone();
    assert!(seen.iter().all(|(_, auth)| auth == &format!("Token {KEY}")));
    assert!(seen.iter().all(|(q, _)| q["container"] == "wav"));
    assert!(!serde_json::to_string(&r).unwrap().contains(KEY));
}

/// With no voices named: the default spread of six, saying the persona's
/// greeting.
#[tokio::test]
async fn an_audition_with_no_voices_tries_the_default_spread_with_the_greeting() {
    let fake = Fake::start(200, wav()).await;
    let core = voice_core(&format!("http://{}", fake.addr), true, |c| {
        c.personas.insert("sage".into(), persona("Sage", None));
        c.context.default_persona = Some("sage".into());
    });
    let r = core
        .voice_audition(VoiceAuditionParams::default(), "the CLI")
        .await
        .unwrap();
    assert_eq!(r.text, "Sage here. Go ahead.");
    assert_eq!(r.clips.len(), 6);
    assert!((4..=6).contains(&r.clips.len()));
    assert_eq!(fake.models().len(), 6);
}

/// No Deepgram key configured: refused, saying which entry to add, before any
/// request; a key that has not resolved is said as that. Nothing in the words
/// is a secret.
#[tokio::test]
async fn an_audition_is_refused_without_a_key() {
    let fake = Fake::start(200, wav()).await;
    let core = voice_core(&format!("http://{}", fake.addr), false, |_| {});
    let e = core
        .voice_audition(VoiceAuditionParams::default(), "the CLI")
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("no Deepgram key is configured"), "{e}");
    assert!(e.contains("deepgram_api_key"), "{e}");
    assert!(fake.models().is_empty());
    // Configured, not resolved yet.
    let core = core_unresolved(&format!("http://{}", fake.addr));
    let e = core
        .voice_audition(VoiceAuditionParams::default(), "the CLI")
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("has not resolved"), "{e}");
    assert!(fake.models().is_empty());
}

fn core_unresolved(api: &str) -> Arc<Core> {
    core_resolving(api, true, false, |_| {})
}

/// A refusal from Deepgram is a failure of that voice, with its words and the
/// key cut out of them, and the other voices still go.
#[tokio::test]
async fn a_refused_voice_is_a_failure_with_the_key_cut_out() {
    let said = format!("bad credentials {KEY}");
    let fake = Fake::start(401, said.into_bytes()).await;
    let core = voice_core(&format!("http://{}", fake.addr), true, |_| {});
    let r = core
        .voice_audition(
            VoiceAuditionParams {
                voices: vec!["aura-2-orion-en".into()],
                text: Some("Hello".into()),
                session_id: None,
            },
            "the CLI",
        )
        .await
        .unwrap();
    assert!(r.clips.is_empty());
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].error.contains("401"), "{:?}", r.failed);
    assert!(r.failed[0].error.contains("<key>"), "{:?}", r.failed);
    assert!(!serde_json::to_string(&r).unwrap().contains(KEY));
    assert_eq!(r.estimate_micros, 0);
}

/// A voice id that is not one, too many voices, and too long a text are
/// refused before any request.
#[tokio::test]
async fn an_audition_refuses_what_it_could_not_bound() {
    let fake = Fake::start(200, wav()).await;
    let core = voice_core(&format!("http://{}", fake.addr), true, |_| {});
    let ask = |voices: Vec<String>, text: Option<String>| {
        let core = Arc::clone(&core);
        async move {
            core.voice_audition(
                VoiceAuditionParams {
                    voices,
                    text,
                    session_id: None,
                },
                "the CLI",
            )
            .await
            .unwrap_err()
            .to_string()
        }
    };
    assert!(ask(vec!["Bad Voice!".into()], None)
        .await
        .contains("not an Aura voice id"));
    let nine: Vec<String> = (0..9).map(|i| format!("aura-2-v{i}-en")).collect();
    assert!(ask(nine, None).await.contains("at most 8 voices"));
    let long = "x".repeat(501);
    assert!(ask(vec![], Some(long))
        .await
        .contains("at most 500 characters"));
    assert!(fake.models().is_empty());
}
