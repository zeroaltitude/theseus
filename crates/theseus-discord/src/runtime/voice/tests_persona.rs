//! The persona's speaking voice and the greeting (theseus-slvl.7): the
//! request Deepgram gets carries the voice of the persona in play (the global
//! one when it has none, the new one after a change mid-call), the greeting is
//! the engine's first words and nothing is spoken for a join that failed, and
//! each sentence's row names its voice.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use theseus_core::config::PersonaConfig;
use theseus_core::secrets::SecretBoard;
use theseus_core::Core;
use theseus_voice::{
    Command, Config as EngineConfig, DeepgramSettings, DeepgramSpeech, Event, Speaker, Speech,
    SpeechFuture, Spoken, StandInSpeech, Synthesis, Transcript, WavIo,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::super::tests::core_with;
use super::tests::{place, LOUNGE, OWNER};
use super::{begin, Metered, VoicePlace};

/// A stand-in Deepgram: it logs each request's query and answers every
/// `/v1/speak` with 20 ms of linear16 silence.
struct Fake {
    addr: SocketAddr,
    queries: Arc<Mutex<Vec<BTreeMap<String, String>>>>,
}

impl Fake {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queries: Arc<Mutex<Vec<BTreeMap<String, String>>>> = Arc::default();
        let log = Arc::clone(&queries);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    if let Some(q) = read_query(&mut sock).await {
                        log.lock().unwrap().push(q);
                    }
                    let body = vec![0u8; 1_920];
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: audio/l16\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { addr, queries }
    }

    fn settings(&self) -> DeepgramSettings {
        DeepgramSettings {
            api_base: format!("http://{}", self.addr),
            connect_timeout: Duration::from_secs(2),
            timeout: Duration::from_secs(5),
            ..DeepgramSettings::default()
        }
    }

    /// The `model` of each request so far, in order.
    fn voices(&self) -> Vec<String> {
        self.queries
            .lock()
            .unwrap()
            .iter()
            .map(|q| q.get("model").cloned().unwrap_or_default())
            .collect()
    }
}

/// The query of one request, once its head and body have been read.
async fn read_query(sock: &mut TcpStream) -> Option<BTreeMap<String, String>> {
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
    let target = head.lines().next()?.split(' ').nth(1)?.to_string();
    let length: usize = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut have = buf.len() - head_end - 4;
    while have < length {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        have += n;
    }
    let (_, query) = target.split_once('?')?;
    Some(
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    )
}

fn persona(name: &str, voice: Option<&str>) -> PersonaConfig {
    PersonaConfig {
        name: Some(name.into()),
        tts_voice: voice.map(str::to_string),
        ..PersonaConfig::default()
    }
}

/// A core with two personas, `sage` (in play by default; `sage_voice`) and
/// `wren` (a voice of its own), and the global voice `thalia`; a session
/// bound to the lounge.
fn rig(dir: &std::path::Path, sage_voice: Option<&str>) -> (Arc<Core>, String) {
    let sage_voice = sage_voice.map(str::to_string);
    let core = core_with(dir, SecretBoard::empty(), move |c| {
        c.voice.enabled = true;
        c.voice.tts_voice = "aura-2-thalia-en".into();
        c.personas
            .insert("sage".into(), persona("Sage", sage_voice.as_deref()));
        c.personas
            .insert("wren".into(), persona("Wren", Some("aura-2-luna-en")));
        c.context.default_persona = Some("sage".into());
    });
    let rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    core.outbox
        .bind_place(&format!("channel:{LOUNGE}"), &sid)
        .unwrap();
    (core, sid)
}

fn metered(core: &Arc<Core>, fake: &Fake) -> Metered {
    Metered {
        inner: DeepgramSpeech::new("tv-deepgram-7f3a9c", fake.settings()).unwrap(),
        core: Arc::downgrade(core),
        key: format!("channel:{LOUNGE}"),
    }
}

/// The request Deepgram gets carries the persona in play's voice; with the
/// persona silent about it, the global `[voice] tts_voice`; and after a
/// persona change in the middle of a call (the session chooses another), the
/// new persona's, at the next sentence, on the same provider.
#[tokio::test]
async fn the_request_carries_the_personas_voice_the_global_one_and_the_new_one() {
    let d = tempfile::tempdir().unwrap();
    let (core, sid) = rig(d.path(), Some("aura-2-orion-en"));
    let fake = Fake::start().await;
    let speech = metered(&core, &fake);
    speech.synthesize("Good morning.").await.unwrap();
    // The persona speaks in a voice of its own: that, not the global one.
    assert_eq!(fake.voices(), ["aura-2-orion-en"]);
    // A change mid-call: the next sentence, no reconnect, no new provider.
    core.runner.persona.choose(&core.store, &sid, "wren");
    speech.synthesize("Good evening.").await.unwrap();
    assert_eq!(fake.voices(), ["aura-2-orion-en", "aura-2-luna-en"]);

    // A persona with no voice of its own: the global one.
    let d = tempfile::tempdir().unwrap();
    let (core, _sid) = rig(d.path(), None);
    let fake = Fake::start().await;
    metered(&core, &fake).synthesize("Hello.").await.unwrap();
    assert_eq!(fake.voices(), ["aura-2-thalia-en"]);
}

/// A speech provider that records the sentences it is asked for.
struct Said {
    inner: StandInSpeech,
    texts: Mutex<Vec<String>>,
}

impl Speech for Said {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a theseus_voice::Audio,
    ) -> SpeechFuture<'a, Transcript> {
        self.inner.transcribe(speaker, audio)
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        self.texts.lock().unwrap().push(text.to_string());
        self.inner.synthesize(text)
    }
}

/// The greeting is the call's first words: the engine says it, in the
/// persona's name, before it answers anyone who spoke from the first
/// instant; with no greeting nothing is said first.
#[tokio::test(start_paused = true)]
async fn the_greeting_is_spoken_before_anything_heard_is_answered() {
    let d = tempfile::tempdir().unwrap();
    let (core, sid) = rig(d.path(), Some("aura-2-orion-en"));
    let greeting = core.greeting_for(Some(&sid));
    assert_eq!(greeting.as_deref(), Some("Sage here. Go ahead."));

    let mut io = WavIo::new(Duration::from_secs(8));
    io.say(
        Speaker(OWNER),
        Duration::ZERO,
        theseus_voice::Audio::tone(220.0, Duration::from_millis(600), 0.3),
    );
    let said = Arc::new(Said {
        inner: StandInSpeech::new().transcript(Speaker(OWNER), "hello there"),
        texts: Mutex::default(),
    });
    let (engine, mut handle) = begin(
        EngineConfig::new([Speaker(OWNER)]),
        Box::new(io),
        said.clone(),
        greeting,
    );
    let run = tokio::spawn(engine.run());
    let mut order = Vec::new();
    while let Some(e) = handle.events.recv().await {
        match e {
            Event::Synthesized {
                what: Spoken::Report,
                ..
            } => order.push("greeting"),
            Event::Utterance(_) => {
                order.push("heard");
                break;
            }
            _ => {}
        }
    }
    let _ = handle.commands.send(Command::Leave);
    let _ = run.await;
    assert_eq!(order.first(), Some(&"greeting"), "{order:?}");
    assert_eq!(order.last(), Some(&"heard"), "{order:?}");
    let texts = said.texts.lock().unwrap().join(" ");
    assert_eq!(texts, "Sage here. Go ahead.");

    // None: the engine says nothing first.
    let (_engine, mut handle) = begin(
        EngineConfig::new([Speaker(OWNER)]),
        Box::new(WavIo::new(Duration::from_secs(1))),
        Arc::new(StandInSpeech::new()),
        None,
    );
    assert!(handle.events.try_recv().is_err());
}

/// The greeting's words are `[voice] greeting` with the persona's name in;
/// empty is none; and the built-in persona is Theseus.
#[tokio::test]
async fn the_greeting_names_the_persona_and_the_config_changes_the_words() {
    let d = tempfile::tempdir().unwrap();
    let core = core_with(d.path(), SecretBoard::empty(), |c| {
        c.voice.greeting = "Hi, {name} is listening.".into();
        c.personas.insert("sage".into(), persona("Sage", None));
        c.context.default_persona = Some("sage".into());
    });
    assert_eq!(
        core.greeting_for(None).as_deref(),
        Some("Hi, Sage is listening.")
    );
    let d = tempfile::tempdir().unwrap();
    let core = core_with(d.path(), SecretBoard::empty(), |c| c.voice.greeting.clear());
    assert_eq!(core.greeting_for(None), None);
    let d = tempfile::tempdir().unwrap();
    let core = core_with(d.path(), SecretBoard::empty(), |_| {});
    assert_eq!(
        core.greeting_for(None).as_deref(),
        Some("Theseus here. Go ahead.")
    );
}

/// A join that fails speaks nothing: with the key and a stand-in Deepgram
/// configured, the refused join (no gateway yet) reaches Deepgram with no
/// request at all.
#[tokio::test]
async fn a_join_that_fails_says_nothing() {
    let d = tempfile::tempdir().unwrap();
    let fake = Fake::start().await;
    let api = format!("http://{}", fake.addr);
    let board = SecretBoard::new(["deepgram_api_key".to_string()], std::time::Instant::now());
    board.publish(
        BTreeMap::from([(
            "deepgram_api_key".to_string(),
            Ok(theseus_core::secrets::Secret::new(
                "tv-deepgram-7f3a9c".into(),
            )),
        )]),
        "test",
    );
    let core = core_with(d.path(), board, move |c| {
        c.voice.enabled = true;
        c.voice.api_base = api;
    });
    let (mut p, _rx) = place(&core, "ses_dm");
    core.bind_places(vec![theseus_core::places::BoundPlace {
        target: format!("discord:dm:{OWNER}"),
        name: "DM".into(),
        private: false,
        ..Default::default()
    }]);
    p.target = format!("discord:dm:{OWNER}");
    let said = p
        .join(
            Some(LOUNGE),
            Some(theseus_protocol::DiscordOrigin {
                user_id: OWNER.to_string(),
                channel_id: "43".into(),
                guild_id: None,
            }),
            "discord:zeroaltitude",
        )
        .await;
    assert_eq!(said, "The gateway isn't up yet; try again in a moment.");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(fake.voices().is_empty(), "{:?}", fake.voices());
}

/// Each synthesized sentence's row names the voice that said it, so a persona
/// change shows at the sentence it took effect.
#[tokio::test]
async fn each_sentences_row_names_its_voice() {
    let d = tempfile::tempdir().unwrap();
    let (core, sid) = rig(d.path(), Some("aura-2-orion-en"));
    let (mut p, _rx) = place(&core, &sid);
    p.key = format!("channel:{LOUNGE}");
    let lounge = VoicePlace {
        key: p.key.clone(),
        label: "#lounge".into(),
        users: vec![OWNER],
        guild: 100_000_000_000_000_001,
        status: None,
    };
    let (tx, events) = tokio::sync::mpsc::unbounded_channel();
    tx.send(Event::Synthesized {
        what: Spoken::Report,
        usage: theseus_voice::Usage {
            provider: "deepgram".into(),
            model: "aura-2-orion-en".into(),
            audio: Duration::from_millis(900),
            chars: 20,
        },
        latency: Duration::from_millis(400),
    })
    .unwrap();
    drop(tx);
    super::pump(Arc::clone(&p.shared), 1, lounge, events).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let row = loop {
        let tail: Vec<(u64, theseus_core::ledger::LedgerRow)> =
            core.store.ledger_tail(100).unwrap();
        if let Some((_, r)) = tail
            .into_iter()
            .find(|(_, r)| r.kind == "speech.synthesized")
        {
            break r.data;
        }
        assert!(std::time::Instant::now() < deadline, "the booking came");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(row["voice"], json!("aura-2-orion-en"));
}
