//! Deepgram Flux as a call's listener (theseus-v774k), on the binding's side:
//! the settings the REST client and the socket are given, the budget in front
//! of a socket, a Flux utterance's price, and the stop latency in the barge-in
//! row.

use std::sync::Arc;
use std::time::Duration;

use theseus_core::config::Config;
use theseus_core::voice::{SpeechCall, SpeechKind};
use theseus_voice::{Event, Listen, Listened, Speaker, Spoken, TurnId, Usage};
use tokio::sync::mpsc;

use super::tests::{heard, spending, LOUNGE, OWNER};
use super::{flux_listen, pump, settings, VoicePlace};

fn flux_cfg(section: &str) -> theseus_core::voice::VoiceConfig {
    let section = format!("[voice]\nstt_model = \"flux-general-en\"\n{section}");
    Config::parse(&format!("{}\n{section}\n", Config::EXAMPLE_TOML))
        .unwrap()
        .0
        .voice
}

/// Flux streams a call, but the REST client (the fallback's recogniser) is
/// asked for nova-3, and the socket for Flux with the knobs set.
#[test]
fn the_rest_client_keeps_nova_3_and_the_socket_gets_flux() {
    let cfg = flux_cfg("eot_threshold = 0.8\neager_eot_threshold = 0.5\neot_timeout_ms = 6000");
    assert_eq!(settings(&cfg).stt_model, "nova-3");
    let listen = flux_listen("dg-test-key", &cfg, std::sync::Weak::new(), "k".into()).unwrap();
    let url = listen.settings().url();
    assert!(
        url.starts_with("wss://api.deepgram.com/v2/listen?model=flux-general-en&"),
        "{url}"
    );
    assert!(
        url.ends_with("&eot_threshold=0.8&eager_eot_threshold=0.5&eot_timeout_ms=6000"),
        "{url}"
    );
    assert!(!url.contains("dg-test-key"));
}

/// Flux is the default listener (the owner, 2026-10-24): a config with no
/// `[voice]` table, or the owner's own (on, a lead-in, no model named),
/// listens through Flux English, and the REST client still asks for nova-3.
#[test]
fn a_config_with_no_voice_table_listens_through_flux() {
    for section in ["", "[voice]\nenabled = true\nlead_in_ms = 600"] {
        let cfg = Config::parse(&format!("{}\n{section}\n", Config::EXAMPLE_TOML))
            .unwrap()
            .0
            .voice;
        assert!(cfg.uses_flux(), "{section:?}");
        assert_eq!(settings(&cfg).stt_model, "nova-3", "{section:?}");
        let listen = flux_listen("dg-test-key", &cfg, std::sync::Weak::new(), "k".into()).unwrap();
        let url = listen.settings().url();
        assert!(
            url.starts_with("wss://api.deepgram.com/v2/listen?model=flux-general-en&"),
            "{url}"
        );
    }
}

/// `stt_model = "nova-3"` is the fallback by config: no Flux socket, the
/// pause rules end a turn, and both clients ask for nova-3.
#[test]
fn stt_model_nova_3_still_takes_the_pause_rules_path() {
    let cfg = Config::parse(&format!(
        "{}\n[voice]\nenabled = true\nstt_model = \"nova-3\"\n",
        Config::EXAMPLE_TOML
    ))
    .unwrap()
    .0
    .voice;
    assert!(!cfg.uses_flux());
    assert_eq!((cfg.pause_ms, cfg.merge_ms), (1_200, 1_500));
    assert_eq!(settings(&cfg).stt_model, "nova-3");
    assert_eq!(cfg.prerecorded_model(), "nova-3");
}

/// A local `api_base` is a local socket, as it is for the REST client.
#[test]
fn a_local_api_base_is_a_local_socket() {
    let mut cfg = flux_cfg("");
    cfg.api_base = "http://127.0.0.1:9".into();
    let listen = flux_listen("dg-test-key", &cfg, std::sync::Weak::new(), "k".into()).unwrap();
    assert!(listen
        .settings()
        .url()
        .starts_with("ws://127.0.0.1:9/v2/listen?"));
}

/// A session past its limit opens no socket: the listener says it is
/// unavailable, so the call uses the pause rules (which are refused in turn).
#[tokio::test]
async fn a_session_past_its_limit_opens_no_flux_socket() {
    let d = tempfile::tempdir().unwrap();
    let (core, p, _sid) = spending(d.path(), 0.0).await;
    let cfg = flux_cfg("");
    let mut listen =
        flux_listen("dg-test-key", &cfg, Arc::downgrade(&core), p.key.clone()).unwrap();
    listen.push(Speaker(OWNER), &vec![1_000; 960]);
    let event = tokio::time::timeout(Duration::from_secs(5), listen.next())
        .await
        .expect("an answer")
        .expect("the listener is up");
    let Listened::Unavailable { error } = event else {
        panic!("unavailable, got {event:?}");
    };
    assert!(error.contains("spend limit"), "{error}");
    listen.close().await;
}

fn lounge(key: &str) -> VoicePlace {
    VoicePlace {
        key: key.to_string(),
        label: "#lounge".into(),
        users: vec![OWNER],
        guild: 100_000_000_000_000_001,
        status: None,
    }
}

fn rows(core: &theseus_core::Core, sid: &str, kind: &str) -> Vec<serde_json::Value> {
    let tail: Vec<(u64, theseus_core::ledger::LedgerRow)> = core.store.ledger_tail(200).unwrap();
    tail.into_iter()
        .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid))
        .map(|(_, r)| r.data)
        .collect()
}

/// Flux audio is booked as transcription is, at Flux's price by the minute
/// streamed: one minute is $0.0077.
#[tokio::test]
async fn a_flux_utterance_is_booked_at_fluxs_price() {
    let d = tempfile::tempdir().unwrap();
    let (core, p, sid) = spending(d.path(), 100.0).await;
    let (tx, events) = mpsc::unbounded_channel();
    let mut u = heard("Book a table for two.");
    u.usage.model = "flux-general-en".into();
    u.usage.audio = Duration::from_secs(60);
    tx.send(Event::Utterance(u)).unwrap();
    drop(tx);
    pump(Arc::clone(&p.shared), 1, lounge(&p.key), events).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while rows(&core, &sid, "speech.transcribed").is_empty() {
        assert!(std::time::Instant::now() < deadline, "the booking came");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let t = &rows(&core, &sid, "speech.transcribed")[0];
    assert_eq!(
        (t["model"].as_str(), t["cost_usd"].as_f64()),
        (Some("flux-general-en"), Some(0.0077))
    );
}

/// Audio a streaming listener was sent past its last end of turn comes as
/// `Event::Streamed`, and is booked all the same: one `speech.transcribed`
/// row, marked trailing, with the speaker, no latency, and the usage's price.
#[tokio::test]
async fn trailing_flux_audio_is_booked_as_a_trailing_row() {
    let d = tempfile::tempdir().unwrap();
    let (core, p, sid) = spending(d.path(), 100.0).await;
    let (tx, events) = mpsc::unbounded_channel();
    let usage = Usage {
        provider: "deepgram".into(),
        model: "flux-general-en".into(),
        audio: Duration::from_secs(30),
        chars: 0,
    };
    let price = SpeechCall {
        kind: SpeechKind::Transcribed,
        provider: &usage.provider,
        model: &usage.model,
        audio: usage.audio,
        chars: usage.chars,
        latency: Duration::ZERO,
        detail: serde_json::Value::Null,
    }
    .cost()
    .expect("flux has a price");
    let expected = theseus_kernel::micros_to_usd(price);
    assert!(expected > 0.0);
    tx.send(Event::Streamed {
        speaker: Speaker(OWNER),
        usage,
    })
    .unwrap();
    drop(tx);
    pump(Arc::clone(&p.shared), 1, lounge(&p.key), events).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while rows(&core, &sid, "speech.transcribed").is_empty() {
        assert!(std::time::Instant::now() < deadline, "the booking came");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let all = rows(&core, &sid, "speech.transcribed");
    assert_eq!(all.len(), 1, "{all:?}");
    let t = &all[0];
    assert_eq!(t["trailing"], serde_json::json!(true), "{t}");
    assert_eq!(t["speaker"], serde_json::json!(OWNER.to_string()), "{t}");
    assert_eq!(t["latency_ms"], serde_json::json!(0), "{t}");
    assert_eq!(t["seconds"], serde_json::json!(30.0), "{t}");
    assert_eq!(t["model"].as_str(), Some("flux-general-en"), "{t}");
    assert_eq!(t["cost_usd"].as_f64(), Some(expected), "{t}");
}

/// The time to the stop is in the barge-in's row; a barge-in the pause rules
/// heard has no such figures, and its row is what it was.
#[tokio::test]
async fn the_stop_latency_is_in_the_barge_in_row() {
    let d = tempfile::tempdir().unwrap();
    let (core, p, sid) = spending(d.path(), 100.0).await;
    let (tx, events) = mpsc::unbounded_channel();
    let barge = |dropped| Event::BargeIn {
        speaker: Speaker(OWNER),
        what: Spoken::Reply(TurnId(0)),
        dropped,
    };
    tx.send(Event::StopLatency {
        speaker: Speaker(OWNER),
        to_stop: Some(Duration::from_millis(4)),
        to_ended: Some(Duration::from_millis(23)),
    })
    .unwrap();
    tx.send(barge(2)).unwrap();
    tx.send(barge(1)).unwrap();
    drop(tx);
    let _ = LOUNGE;
    pump(Arc::clone(&p.shared), 1, lounge(&p.key), events).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while rows(&core, &sid, "voice.barge_in").len() < 2 {
        assert!(std::time::Instant::now() < deadline, "the rows came");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let all = rows(&core, &sid, "voice.barge_in");
    let timed = all.iter().find(|r| r["dropped"] == 2).unwrap();
    assert_eq!(
        (&timed["stop_latency_ms"], &timed["ended_latency_ms"]),
        (&serde_json::json!(4), &serde_json::json!(23))
    );
    let plain = all.iter().find(|r| r["dropped"] == 1).unwrap();
    assert!(plain.get("stop_latency_ms").is_none(), "{plain}");
}
