//! Deepgram Flux (theseus-v774k): the call's listener, over Deepgram's v2
//! streaming socket. Flux's own model decides where a speaker's turn ends, so
//! the engine's silence timer (`[voice] pause_ms`, `merge_ms`) does not apply.
//!
//! - **One socket per listed speaker** for the whole call, opened at their
//!   first audio and closed with the docs' `CloseStream` at the call's end.
//!   Audio goes as the engine has it: 16-bit mono at 48 kHz (`encoding=linear16`,
//!   `sample_rate=48000`), in 80 ms chunks, the size the docs recommend.
//! - **The key** rides in the handshake's `Authorization` header alone, marked
//!   sensitive: never in the URL, a log line, a `Debug` or an error
//!   ([`crate::deepgram::scrub_key`] is the one scrub).
//! - **The messages** become [`Listened`] events. A turn's confidence is the
//!   mean of its words' confidences, never the end-of-turn probability.
//! - **A long call's socket survives.** A quiet speaker's socket gets 80 ms of
//!   silence every [`FluxSettings::keep_alive`] so no idle timeout closes it.
//!   A socket that drops tells the engine ([`Listened::Down`]), which hears that
//!   speaker with its VAD and the pre-recorded recogniser, and the client
//!   reconnects with a bounded backoff (1 s doubling to [`BACKOFF_CAP`]) and says [`Listened::Up`].
//!   A first socket that cannot be opened at all is [`Listened::Unavailable`].
//! - **Bounds.** Each speaker's outbound queue holds [`FluxSettings::queue`] of
//!   audio (8 s); past it the oldest audio is dropped and counted, said once a
//!   spell. The events queue holds [`EVENTS`] and the socket's task waits when
//!   it is full. A connect waits [`FluxSettings::connect_timeout`].
//!
//! The client spawns one task per speaker, at that speaker's first audio.
//! Nothing is spawned or sent before it (FAST).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::{SinkExt as _, StreamExt as _};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use tokio::sync::{mpsc, watch, Notify};
use tokio::task::JoinHandle;
use tokio::time::{sleep, sleep_until, timeout, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_tls_with_config, Connector};

use crate::audio::{duration_of, SAMPLE_RATE};
use crate::deepgram::{chain, scrub_key};
use crate::io::Speaker;
use crate::listen::{Listen, Listened, FLUX_EOT_TIMEOUT};
use crate::speech::SpeechError;

/// The model id (Deepgram's docs, "Flux quickstart").
pub const FLUX_MODEL: &str = "flux-general-en";

/// The reconnect backoff after a drop doubles to this: 30 s (it starts at
/// [`FluxSettings::backoff`], 1 s).
pub const BACKOFF_CAP: Duration = Duration::from_secs(30);

/// The events queue between the sockets and the engine.
pub const EVENTS: usize = 256;

/// The chunk the docs recommend: 80 ms.
const CHUNK: Duration = Duration::from_millis(80);

/// How long a closing socket waits for the server's own close.
const CLOSE_WAIT: Duration = Duration::from_secs(1);

/// The most of an answer an error quotes.
const QUOTED: usize = 200;

/// How Flux is asked. [`Default`] is Deepgram's own: its thresholds unset.
#[derive(Clone, Debug, PartialEq)]
pub struct FluxSettings {
    /// Where the socket is: `wss://api.deepgram.com`, or a stand-in's `ws://`.
    pub ws_base: String,
    pub model: String,
    /// End-of-turn confidence, 0.5 to 0.9 (Flux's default 0.7 when unset).
    pub eot_threshold: Option<f64>,
    /// Turns on eager end of turn, 0.3 to 0.9.
    pub eager_eot_threshold: Option<f64>,
    /// The forced end of a turn, 500 to 10000 ms (Flux's default 5000).
    pub eot_timeout_ms: Option<u32>,
    pub connect_timeout: Duration,
    /// The first wait before a dropped socket is opened again.
    pub backoff: Duration,
    /// How long a socket may be quiet before it is sent 80 ms of silence.
    pub keep_alive: Duration,
    /// The audio a speaker's outbound queue holds.
    pub queue: Duration,
}

impl Default for FluxSettings {
    fn default() -> Self {
        Self {
            ws_base: "wss://api.deepgram.com".into(),
            model: FLUX_MODEL.into(),
            eot_threshold: None,
            eager_eot_threshold: None,
            eot_timeout_ms: None,
            connect_timeout: Duration::from_secs(5),
            backoff: Duration::from_secs(1),
            keep_alive: Duration::from_secs(5),
            queue: Duration::from_secs(8),
        }
    }
}

impl FluxSettings {
    /// The socket base for an `[voice] api_base`: `https` is `wss`, `http` is `ws`.
    pub fn ws_base_of(api_base: &str) -> String {
        let base = api_base.trim_end_matches('/');
        match (base.strip_prefix("https://"), base.strip_prefix("http://")) {
            (Some(rest), _) => format!("wss://{rest}"),
            (_, Some(rest)) => format!("ws://{rest}"),
            _ => base.to_string(),
        }
    }

    /// The socket's address, with no key in it.
    pub fn url(&self) -> String {
        let mut url = format!(
            "{}/v2/listen?model={}&encoding=linear16&sample_rate={SAMPLE_RATE}",
            self.ws_base.trim_end_matches('/'),
            self.model
        );
        if let Some(t) = self.eot_threshold {
            url.push_str(&format!("&eot_threshold={t}"));
        }
        if let Some(t) = self.eager_eot_threshold {
            url.push_str(&format!("&eager_eot_threshold={t}"));
        }
        if let Some(ms) = self.eot_timeout_ms {
            url.push_str(&format!("&eot_timeout_ms={ms}"));
        }
        url
    }
}

/// What a turn event was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnEvent {
    Start,
    Update,
    Eager,
    Resumed,
    End,
}

/// A message from Flux, as much of it as is read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Incoming {
    Turn {
        event: TurnEvent,
        transcript: String,
        confidence: Option<f32>,
    },
    /// The server's own error, in its words.
    Error(String),
    /// `Connected`, or a message this client does not read.
    Other,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    transcript: String,
    #[serde(default)]
    words: Vec<Word>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct Word {
    #[serde(default)]
    confidence: Option<f32>,
}

/// One text message from the socket.
pub(crate) fn parse(text: &str) -> Result<Incoming, String> {
    let raw: Raw = serde_json::from_str(text).map_err(|e| format!("unreadable message: {e}"))?;
    match raw.kind.as_str() {
        "TurnInfo" => {
            let event = match raw.event.as_str() {
                "StartOfTurn" => TurnEvent::Start,
                "Update" => TurnEvent::Update,
                "EagerEndOfTurn" => TurnEvent::Eager,
                "TurnResumed" => TurnEvent::Resumed,
                "EndOfTurn" => TurnEvent::End,
                _ => return Ok(Incoming::Other),
            };
            let confidences: Vec<f32> = raw.words.iter().filter_map(|w| w.confidence).collect();
            let confidence = (!confidences.is_empty())
                .then(|| confidences.iter().sum::<f32>() / confidences.len() as f32);
            Ok(Incoming::Turn {
                event,
                transcript: raw.transcript.trim().to_string(),
                confidence,
            })
        }
        "Error" | "FatalError" => {
            let why = raw.description.or(raw.message).unwrap_or(raw.kind);
            Ok(Incoming::Error(why))
        }
        _ => Ok(Incoming::Other),
    }
}

/// A speaker's outbound audio, bounded: 80 ms chunks, and past the bound the
/// oldest is dropped and counted.
pub(crate) struct AudioQueue {
    inner: Mutex<Queued>,
    notify: Notify,
    bound: usize,
}

#[derive(Default)]
struct Queued {
    chunks: std::collections::VecDeque<Vec<i16>>,
    staging: Vec<i16>,
    queued: usize,
    dropped: usize,
    /// Said for this spell of dropping.
    said: bool,
}

impl AudioQueue {
    pub fn new(bound: Duration) -> Self {
        Self {
            inner: Mutex::default(),
            notify: Notify::new(),
            bound: crate::audio::samples_in(bound).max(crate::audio::samples_in(CHUNK)),
        }
    }

    /// `samples`, added. Returns the samples dropped the first time a spell
    /// of dropping begins, for one journal line.
    pub fn push(&self, samples: &[i16]) -> Option<usize> {
        let chunk = crate::audio::samples_in(CHUNK);
        let mut q = self.inner.lock().expect("the audio queue's lock");
        q.staging.extend_from_slice(samples);
        let mut told = None;
        while q.staging.len() >= chunk {
            let rest = q.staging.split_off(chunk);
            let full = std::mem::replace(&mut q.staging, rest);
            q.queued += full.len();
            q.chunks.push_back(full);
            while q.queued > self.bound {
                let Some(old) = q.chunks.pop_front() else {
                    break;
                };
                q.queued -= old.len();
                q.dropped += old.len();
                if !q.said {
                    q.said = true;
                    told = Some(old.len());
                }
            }
        }
        drop(q);
        self.notify.notify_one();
        told
    }

    /// The next chunk, if any.
    pub fn pop(&self) -> Option<Vec<i16>> {
        let mut q = self.inner.lock().expect("the audio queue's lock");
        let chunk = q.chunks.pop_front()?;
        q.queued -= chunk.len();
        if q.chunks.is_empty() {
            q.said = false;
        }
        Some(chunk)
    }

    /// Everything waiting, discarded and counted as dropped: audio from before
    /// a dropped session is stale (its turn went to the pre-recorded
    /// recogniser). The samples discarded.
    pub fn clear(&self) -> usize {
        let mut q = self.inner.lock().expect("the audio queue's lock");
        let cleared = q.queued + q.staging.len();
        q.chunks.clear();
        q.staging.clear();
        q.queued = 0;
        q.dropped += cleared;
        q.said = false;
        cleared
    }

    /// Waits for audio to be pushed. Cancel-safe.
    pub async fn ready(&self) {
        self.notify.notified().await;
    }

    /// The samples dropped so far.
    pub fn dropped(&self) -> usize {
        self.inner.lock().expect("the audio queue's lock").dropped
    }

    /// The samples held now.
    #[cfg(test)]
    pub fn held(&self) -> usize {
        self.inner.lock().expect("the audio queue's lock").queued
    }
}

/// What the budget says of opening a stream or starting a turn.
pub type Admit = Arc<dyn Fn(Speaker) -> Result<(), String> + Send + Sync>;

/// Everything a socket's task needs.
struct Shared {
    settings: FluxSettings,
    auth: HeaderValue,
    key: String,
    tls: Arc<rustls::ClientConfig>,
    admit: Option<Admit>,
    /// Some socket has opened: a later failure is one speaker's, not the call's.
    ever: AtomicBool,
}

impl Shared {
    fn scrub(&self, text: &str) -> String {
        scrub_key(&self.key, text, QUOTED)
    }
}

/// The audio streamed to a speaker's socket since their last end of turn,
/// shared by the socket's task and the listener: the engine takes what no end
/// carried (see [`Listen::unbooked`]).
#[derive(Clone, Default)]
struct Streamed(Arc<Mutex<Duration>>);

impl Streamed {
    fn add(&self, audio: Duration) {
        *self.0.lock().expect("the streamed lock") += audio;
    }

    fn take(&self) -> Duration {
        std::mem::take(&mut *self.0.lock().expect("the streamed lock"))
    }
}

struct Lane {
    queue: Arc<AudioQueue>,
    streamed: Streamed,
    /// Taken when the call closes; the lane stays for its streamed count.
    task: Option<JoinHandle<()>>,
}

/// Deepgram Flux as the engine's [`Listen`].
pub struct FluxListen {
    shared: Arc<Shared>,
    lanes: BTreeMap<Speaker, Lane>,
    tx: mpsc::Sender<Listened>,
    rx: mpsc::Receiver<Listened>,
    closing: watch::Sender<bool>,
}

impl fmt::Debug for FluxListen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FluxListen")
            .field("settings", &self.shared.settings)
            .field("speakers", &self.lanes.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl FluxListen {
    /// A listener for `key`, a Deepgram API key. Builds the TLS config and
    /// opens nothing: no socket before a speaker's first audio.
    pub fn new(
        key: &str,
        settings: FluxSettings,
        admit: Option<Admit>,
    ) -> Result<Self, SpeechError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(SpeechError("Deepgram: the API key is empty".into()));
        }
        let mut auth = HeaderValue::from_str(&format!("Token {key}"))
            .map_err(|_| SpeechError("Deepgram: the API key is not a valid header value".into()))?;
        auth.set_sensitive(true);
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| SpeechError(format!("Deepgram: the TLS config: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let (tx, rx) = mpsc::channel(EVENTS);
        let (closing, _) = watch::channel(false);
        Ok(Self {
            shared: Arc::new(Shared {
                settings,
                auth,
                key: key.to_string(),
                tls: Arc::new(tls),
                admit,
                ever: AtomicBool::new(false),
            }),
            lanes: BTreeMap::new(),
            tx,
            rx,
            closing,
        })
    }

    pub fn settings(&self) -> &FluxSettings {
        &self.shared.settings
    }

    /// The audio dropped so far from `speaker`'s outbound queue, past its bound.
    pub fn dropped(&self, speaker: Speaker) -> Duration {
        self.lanes
            .get(&speaker)
            .map_or(Duration::ZERO, |l| duration_of(l.queue.dropped()))
    }

    fn lane(&mut self, speaker: Speaker) -> &Lane {
        self.lanes.entry(speaker).or_insert_with(|| {
            let queue = Arc::new(AudioQueue::new(self.shared.settings.queue));
            let streamed = Streamed::default();
            let task = tokio::spawn(socket(
                speaker,
                Arc::clone(&self.shared),
                Arc::clone(&queue),
                self.tx.clone(),
                self.closing.subscribe(),
                streamed.clone(),
            ));
            Lane {
                queue,
                streamed,
                task: Some(task),
            }
        })
    }
}

impl Listen for FluxListen {
    fn push(&mut self, speaker: Speaker, samples: &[i16]) {
        if let Some(dropped) = self.lane(speaker).queue.push(samples) {
            tracing::warn!(
                speaker = speaker.0,
                dropped_ms = duration_of(dropped).as_millis() as u64,
                "voice: Flux's outbound audio is backed up; the oldest audio is dropped"
            );
        }
    }

    fn tick(&mut self, _tick: u64) {}

    fn unbooked(&mut self, speaker: Speaker) -> Duration {
        self.lanes
            .get(&speaker)
            .map_or(Duration::ZERO, |l| l.streamed.take())
    }

    fn silence(&self) -> Duration {
        self.shared
            .settings
            .eot_timeout_ms
            .map_or(FLUX_EOT_TIMEOUT, |ms| Duration::from_millis(u64::from(ms)))
    }

    fn next(&mut self) -> BoxFuture<'_, Option<Listened>> {
        Box::pin(async move { self.rx.recv().await })
    }

    fn close(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let _ = self.closing.send(true);
            for (speaker, lane) in &mut self.lanes {
                let dropped = duration_of(lane.queue.dropped());
                if !dropped.is_zero() {
                    tracing::info!(
                        speaker = speaker.0,
                        dropped_ms = dropped.as_millis() as u64,
                        "voice: Flux's outbound audio dropped over the call"
                    );
                }
                if let Some(task) = lane.task.take() {
                    let _ = timeout(CLOSE_WAIT * 2, task).await;
                }
            }
        })
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Why a session ended.
enum Ended {
    /// The call asked for it.
    Closed,
    Dropped(String),
    /// The event queue is gone: the engine is.
    Gone,
}

/// One speaker's socket for the call: connect, stream, reconnect.
async fn socket(
    speaker: Speaker,
    shared: Arc<Shared>,
    queue: Arc<AudioQueue>,
    events: mpsc::Sender<Listened>,
    mut closing: watch::Receiver<bool>,
    streamed: Streamed,
) {
    let (mut backoff, cap) = (shared.settings.backoff, BACKOFF_CAP);
    let mut down = false;
    loop {
        if *closing.borrow() {
            return;
        }
        let opened = match shared.admit.as_ref().map(|admit| admit(speaker)) {
            Some(Err(why)) => Err(why),
            _ => connect(&shared).await,
        };
        match opened {
            Err(error) => {
                if !shared.ever.load(Ordering::SeqCst) {
                    let _ = events.send(Listened::Unavailable { error }).await;
                    return;
                }
                if !down {
                    down = true;
                    let down_event = Listened::Down { speaker, error };
                    if events.send(down_event).await.is_err() {
                        return;
                    }
                }
                tokio::select! {
                    () = sleep(backoff) => {}
                    _ = closing.changed() => return,
                }
                backoff = (backoff * 2).min(cap);
            }
            Ok(ws) => {
                shared.ever.store(true, Ordering::SeqCst);
                backoff = shared.settings.backoff;
                if down {
                    discard_stale(speaker, &queue);
                    if events.send(Listened::Up { speaker }).await.is_err() {
                        return;
                    }
                }
                let ended = session(
                    speaker,
                    &shared,
                    ws,
                    &queue,
                    &events,
                    &mut closing,
                    &streamed,
                )
                .await;
                match ended {
                    Ended::Closed | Ended::Gone => return,
                    Ended::Dropped(error) => {
                        down = true;
                        if events
                            .send(Listened::Down { speaker, error })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        tokio::select! {
                            () = sleep(backoff) => {}
                            _ = closing.changed() => return,
                        }
                        backoff = (backoff * 2).min(cap);
                    }
                }
            }
        }
    }
}

/// Audio queued before a drop belongs to a turn that went to the
/// pre-recorded recogniser: it is not the new stream's first.
fn discard_stale(speaker: Speaker, queue: &AudioQueue) {
    let stale = queue.clear();
    if stale > 0 {
        tracing::info!(
            speaker = speaker.0,
            discarded_ms = duration_of(stale).as_millis() as u64,
            "voice: Flux's audio from before the drop is discarded"
        );
    }
}

async fn connect(shared: &Shared) -> Result<Socket, String> {
    let s = &shared.settings;
    let mut request = s
        .url()
        .into_client_request()
        .map_err(|e| shared.scrub(&format!("Deepgram Flux: the address: {e}")))?;
    request
        .headers_mut()
        .insert(AUTHORIZATION, shared.auth.clone());
    let connector = Connector::Rustls(Arc::clone(&shared.tls));
    match timeout(
        s.connect_timeout,
        connect_async_tls_with_config(request, None, true, Some(connector)),
    )
    .await
    {
        Err(_) => Err(format!(
            "Deepgram Flux: no socket within {} s",
            s.connect_timeout.as_secs_f64()
        )),
        Ok(Err(e)) => Err(shared.scrub(&format!(
            "Deepgram Flux: could not open the socket: {}",
            chain(&e)
        ))),
        Ok(Ok((ws, _))) => Ok(ws),
    }
}

/// Bytes of 16-bit little-endian samples.
fn le_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// One open socket, until it ends.
async fn session(
    speaker: Speaker,
    shared: &Shared,
    ws: Socket,
    queue: &AudioQueue,
    events: &mpsc::Sender<Listened>,
    closing: &mut watch::Receiver<bool>,
    streamed: &Streamed,
) -> Ended {
    let (mut sink, mut stream) = ws.split();
    let keep_alive = shared.settings.keep_alive;
    let mut last_sent = Instant::now();
    loop {
        // Anything queued before a wait is sent first.
        while let Some(chunk) = queue.pop() {
            streamed.add(duration_of(chunk.len()));
            last_sent = Instant::now();
            if let Err(e) = sink.send(Message::binary(le_bytes(&chunk))).await {
                return Ended::Dropped(shared.scrub(&format!("Deepgram Flux: {}", chain(&e))));
            }
        }
        tokio::select! {
            biased;
            _ = closing.changed() => {
                let _ = sink.send(Message::text(r#"{"type":"CloseStream"}"#)).await;
                let _ = sink.close().await;
                let _ = timeout(CLOSE_WAIT, async { while stream.next().await.is_some() {} }).await;
                return Ended::Closed;
            }
            () = queue.ready() => {}
            () = sleep_until(last_sent + keep_alive) => {
                // A quiet speaker sends Discord's engine no frames: silence
                // keeps the socket from an idle timeout.
                let silence = vec![0i16; crate::audio::samples_in(CHUNK)];
                streamed.add(CHUNK);
                last_sent = Instant::now();
                if let Err(e) = sink.send(Message::binary(le_bytes(&silence))).await {
                    return Ended::Dropped(shared.scrub(&format!("Deepgram Flux: {}", chain(&e))));
                }
            }
            message = stream.next() => {
                let text = match message {
                    None => return Ended::Dropped("Deepgram Flux: the socket was closed".into()),
                    Some(Err(e)) => {
                        return Ended::Dropped(shared.scrub(&format!("Deepgram Flux: {}", chain(&e))));
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let why = frame.map_or_else(String::new, |f| format!(": {}", f.reason));
                        return Ended::Dropped(shared.scrub(&format!(
                            "Deepgram Flux: the socket was closed by the server{why}"
                        )));
                    }
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(_)) => continue,
                };
                let event = match parse(text.as_str()) {
                    Ok(Incoming::Turn { event, transcript, confidence }) => {
                        if event == TurnEvent::Start {
                            if let Some(Err(why)) = shared.admit.as_ref().map(|a| a(speaker)) {
                                return Ended::Dropped(why);
                            }
                        }
                        match event {
                            TurnEvent::Start => Listened::Start { speaker },
                            TurnEvent::Update => Listened::Words { speaker, text: transcript },
                            TurnEvent::Eager => Listened::Eager { speaker, text: transcript, confidence },
                            TurnEvent::Resumed => Listened::Resumed { speaker },
                            TurnEvent::End => Listened::End {
                                speaker,
                                text: transcript,
                                confidence,
                                streamed: streamed.take(),
                            },
                        }
                    }
                    Ok(Incoming::Error(why)) => {
                        return Ended::Dropped(shared.scrub(&format!("Deepgram Flux answered: {why}")));
                    }
                    Ok(Incoming::Other) => continue,
                    Err(why) => {
                        tracing::warn!(speaker = speaker.0, "voice: Flux sent a message that was not read: {}", shared.scrub(&why));
                        continue;
                    }
                };
                if events.send(event).await.is_err() {
                    return Ended::Gone;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listen() -> FluxListen {
        FluxListen::new("dg-test-key-0123456789", FluxSettings::default(), None).unwrap()
    }

    #[test]
    fn the_key_is_in_no_debug_no_url_and_no_error() {
        let l = listen();
        assert!(!format!("{l:?}").contains("dg-test-key"), "{l:?}");
        assert!(!l.settings().url().contains("dg-test-key"));
        assert!(l.shared.auth.is_sensitive());
        let shared = &l.shared;
        assert_eq!(
            shared.scrub("refused token dg-test-key-0123456789 here"),
            "refused token [key] here"
        );
        let e = FluxListen::new("  ", FluxSettings::default(), None).unwrap_err();
        assert!(e.0.contains("empty"), "{e}");
    }

    #[test]
    fn the_url_names_the_model_the_audio_and_only_the_knobs_set() {
        let plain = FluxSettings::default().url();
        assert_eq!(
            plain,
            "wss://api.deepgram.com/v2/listen?model=flux-general-en&encoding=linear16&sample_rate=48000"
        );
        let tuned = FluxSettings {
            eot_threshold: Some(0.8),
            eager_eot_threshold: Some(0.5),
            eot_timeout_ms: Some(6000),
            ..FluxSettings::default()
        };
        assert!(tuned
            .url()
            .ends_with("&eot_threshold=0.8&eager_eot_threshold=0.5&eot_timeout_ms=6000"));
        assert_eq!(
            FluxSettings::ws_base_of("https://api.deepgram.com/"),
            "wss://api.deepgram.com"
        );
        assert_eq!(
            FluxSettings::ws_base_of("http://127.0.0.1:9"),
            "ws://127.0.0.1:9"
        );
    }

    #[test]
    fn a_turn_info_is_read_and_its_confidence_is_the_words() {
        let text = r#"{"type":"TurnInfo","event":"EndOfTurn","turn_index":0,
            "transcript":" Is the build green. ","end_of_turn_confidence":0.31,
            "words":[{"word":"is","confidence":0.9},{"word":"the","confidence":0.7}]}"#;
        let Incoming::Turn {
            event,
            transcript,
            confidence,
        } = parse(text).unwrap()
        else {
            panic!("a turn")
        };
        assert_eq!(event, TurnEvent::End);
        assert_eq!(transcript, "Is the build green.");
        assert!(
            (confidence.unwrap() - 0.8).abs() < 1e-6,
            "the words' mean, not 0.31"
        );
        let start = r#"{"type":"TurnInfo","event":"StartOfTurn","transcript":"","words":[]}"#;
        assert_eq!(
            parse(start).unwrap(),
            Incoming::Turn {
                event: TurnEvent::Start,
                transcript: String::new(),
                confidence: None,
            }
        );
        assert_eq!(
            parse(r#"{"type":"Connected","request_id":"x"}"#).unwrap(),
            Incoming::Other
        );
        assert_eq!(
            parse(r#"{"type":"TurnInfo","event":"SomethingNew"}"#).unwrap(),
            Incoming::Other
        );
        assert_eq!(
            parse(r#"{"type":"Error","description":"bad audio"}"#).unwrap(),
            Incoming::Error("bad audio".into())
        );
        assert!(parse("not json").is_err());
    }

    #[test]
    fn the_queue_drops_the_oldest_audio_past_its_bound_and_says_so_once() {
        let q = AudioQueue::new(Duration::from_millis(240));
        let chunk = vec![1i16; crate::audio::samples_in(CHUNK)];
        let mut told = Vec::new();
        for _ in 0..10 {
            told.extend(q.push(&chunk));
        }
        assert_eq!(told.len(), 1, "said once for the spell");
        assert!(q.held() <= crate::audio::samples_in(Duration::from_millis(240)));
        assert_eq!(q.dropped() + q.held(), 10 * chunk.len());
        while q.pop().is_some() {}
        assert_eq!(q.held(), 0);
    }
}
