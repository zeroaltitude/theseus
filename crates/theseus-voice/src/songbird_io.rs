//! songbird's side of the seam (design §2.8): a joined call's receive and
//! send, as a [`VoiceIo`]. Built with the `voice` feature.
//!
//! Receive: songbird hands decoded audio per SSRC every 20 ms (`VoiceTick`),
//! and speaking events map each SSRC to a user. A frame from an SSRC with no
//! user yet is counted and dropped: it can't be checked against the listed
//! users. With DAVE, expect a short silent start while the map and the keys
//! arrive (the 44a spike, Q1).
//!
//! Send: each clip is a songbird track, in songbird's raw container, which
//! its codec registry decodes with symphonia's `pcm` codec.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use songbird::driver::{Channels, DecodeConfig, DecodeMode, SampleRate};
use songbird::events::{CoreEvent, Event, EventContext, EventHandler, TrackEvent};
use songbird::id::UserId;
use songbird::input::Input;
use songbird::shards::TwilightMap;
use songbird::tracks::TrackHandle;
use songbird::{Call, Songbird};
use tokio::sync::mpsc;

use crate::audio::{Audio, FRAME_SAMPLES, SAMPLE_RATE};
use crate::io::{ClipId, Frame, Heard, Speaker, VoiceIo};

/// songbird's configuration as the engine needs it: what's received is
/// decoded to 48 kHz mono. songbird's default only decrypts, and its events
/// then carry no audio. songbird takes a decode mode only when it connects,
/// so the manager is built with it ([`manager`]).
pub fn songbird_config() -> songbird::Config {
    songbird::Config::default().decode_mode(decoding())
}

fn decoding() -> DecodeMode {
    DecodeMode::Decode(DecodeConfig::new(Channels::Mono, SampleRate::Hz48000))
}

/// songbird's manager for the binding (44b), over its shards' senders, with
/// [`songbird_config`]. Building it spawns nothing: songbird's tasks and
/// threads start at the first join (FAST).
pub fn manager(shards: Arc<TwilightMap>, user: impl Into<UserId>) -> Songbird {
    Songbird::twilight_from_config(shards, user, songbird_config())
}

/// `clip` as a songbird input: its raw container (`SbirdRaw`, the rate, the
/// channel count, then little-endian `f32` samples).
pub fn clip_input(clip: &Audio) -> Input {
    let mut bytes = Vec::with_capacity(16 + 4 * clip.len());
    bytes.extend_from_slice(b"SbirdRaw");
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for &s in clip.samples() {
        bytes.extend_from_slice(&(f32::from(s) / 32768.0).to_le_bytes());
    }
    Input::from(bytes)
}

/// What one SSRC has sent so far: the live check's log (44a), and Health's
/// SSRC map (design §2.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SsrcCount {
    /// The user songbird mapped it to, once a speaking event said.
    pub user: Option<Speaker>,
    /// Ticks with a packet from it.
    pub frames: u64,
    /// Ticks with decoded audio from it.
    pub decoded: u64,
}

#[derive(Default)]
struct Ssrcs {
    users: HashMap<u32, Speaker>,
    counts: BTreeMap<u32, SsrcCount>,
}

impl Ssrcs {
    /// A speaking event: `ssrc` is `user`'s.
    fn speaking(&mut self, ssrc: u32, user: Speaker) {
        self.users.insert(ssrc, user);
        self.counts.entry(ssrc).or_default().user = Some(user);
    }

    /// One tick's packets, each SSRC with its decoded audio if any: a frame
    /// for each SSRC mapped to a user, in speaker order. Every packet is
    /// counted; one from an SSRC with no user yet goes no further.
    fn tick<'a>(
        &mut self,
        packets: impl IntoIterator<Item = (u32, Option<&'a [i16]>)>,
    ) -> Vec<Frame> {
        let mut frames = Vec::new();
        for (ssrc, pcm) in packets {
            let user = self.users.get(&ssrc).copied();
            let count = self.counts.entry(ssrc).or_default();
            count.frames += 1;
            if let Some(pcm) = pcm {
                count.decoded += 1;
                if let Some(speaker) = user {
                    frames.push(Frame {
                        speaker,
                        samples: mono(pcm),
                    });
                }
            }
        }
        frames.sort_by_key(|f| f.speaker);
        frames
    }

    /// `user` left the channel: their SSRCs map to no one.
    fn left(&mut self, user: Speaker) {
        self.users.retain(|_, s| *s != user);
    }
}

enum Signal {
    Heard(Heard),
    /// The connection dropped, and why: the binding rejoins, or doesn't.
    Gone(String),
}

/// A joined call, as the engine's seam.
pub struct SongbirdIo {
    call: Arc<tokio::sync::Mutex<Call>>,
    rx: mpsc::UnboundedReceiver<Signal>,
    tx: mpsc::UnboundedSender<Signal>,
    track: Option<TrackHandle>,
    ssrcs: Arc<Mutex<Ssrcs>>,
    /// Why the connection dropped, once it did.
    gone: Option<String>,
}

impl SongbirdIo {
    /// Listen to a call songbird has joined. songbird's own tasks run
    /// already; this adds its event handlers, and spawns nothing.
    pub async fn attach(call: Arc<tokio::sync::Mutex<Call>>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let ssrcs = Arc::new(Mutex::new(Ssrcs::default()));
        {
            let mut c = call.lock().await;
            if !matches!(c.config().decode_mode, DecodeMode::Decode(_)) {
                tracing::warn!(
                    "the call was joined without decoding, so it hears no audio until it reconnects; build songbird's manager with songbird_config()"
                );
                let config = c.config().clone().decode_mode(decoding());
                c.set_config(config);
            }
            let receive = Receive {
                tx: tx.clone(),
                ssrcs: Arc::clone(&ssrcs),
            };
            for event in [
                CoreEvent::SpeakingStateUpdate,
                CoreEvent::VoiceTick,
                CoreEvent::ClientDisconnect,
                CoreEvent::DriverDisconnect,
            ] {
                c.add_global_event(Event::Core(event), receive.clone());
            }
        }
        Self {
            call,
            rx,
            tx,
            track: None,
            ssrcs,
            gone: None,
        }
    }

    /// Frames received per SSRC so far, with the user each maps to.
    pub fn ssrcs(&self) -> BTreeMap<u32, SsrcCount> {
        self.ssrcs
            .lock()
            .expect("the SSRC map's lock")
            .counts
            .clone()
    }
}

impl VoiceIo for SongbirdIo {
    fn next(&mut self) -> BoxFuture<'_, Option<Heard>> {
        // `recv` is cancel-safe.
        Box::pin(async move {
            match self.rx.recv().await? {
                Signal::Heard(heard) => Some(heard),
                Signal::Gone(why) => {
                    self.gone = Some(why);
                    None
                }
            }
        })
    }

    fn gone(&self) -> Option<String> {
        self.gone.clone()
    }

    fn play(&mut self, id: ClipId, clip: Audio) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let track = self.call.lock().await.play_input(clip_input(&clip));
            let ended = Ended {
                tx: self.tx.clone(),
                id,
            };
            // A track that ends or fails is over either way. If the track is
            // already gone, the error says so, and its `Ended` is sent here.
            let added = track
                .add_event(Event::Track(TrackEvent::End), ended.clone())
                .and_then(|()| track.add_event(Event::Track(TrackEvent::Error), ended));
            if added.is_err() {
                let _ = self.tx.send(Signal::Heard(Heard::Ended(id)));
            }
            self.track = Some(track);
        })
    }

    fn stop(&mut self) -> BoxFuture<'_, ()> {
        if let Some(track) = self.track.take() {
            // A track already gone has sent its `Ended`.
            let _ = track.stop();
        }
        Box::pin(std::future::ready(()))
    }
}

#[derive(Clone)]
struct Receive {
    tx: mpsc::UnboundedSender<Signal>,
    ssrcs: Arc<Mutex<Ssrcs>>,
}

#[async_trait]
impl EventHandler for Receive {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(speaking) => {
                if let Some(user) = speaking.user_id {
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    ssrcs.speaking(speaking.ssrc, Speaker(user.0));
                }
            }
            EventContext::VoiceTick(tick) => {
                let packets = tick
                    .speaking
                    .iter()
                    .map(|(ssrc, data)| (*ssrc, data.decoded_voice.as_deref()));
                let frames = self
                    .ssrcs
                    .lock()
                    .expect("the SSRC map's lock")
                    .tick(packets);
                let _ = self.tx.send(Signal::Heard(Heard::Tick(frames)));
            }
            EventContext::ClientDisconnect(gone) => {
                let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                ssrcs.left(Speaker(gone.user_id.0));
            }
            EventContext::DriverDisconnect(d) => {
                let why = match d.reason {
                    Some(r) => format!("{:?}: {r:?}", d.kind),
                    None => format!("{:?}", d.kind),
                };
                let _ = self.tx.send(Signal::Gone(why));
            }
            _ => {}
        }
        None
    }
}

/// A frame as the engine takes it: mono. songbird decodes to mono as
/// configured; a stereo frame (twice the samples) is mixed down.
fn mono(pcm: &[i16]) -> Vec<i16> {
    if pcm.len() == 2 * FRAME_SAMPLES {
        pcm.as_chunks::<2>()
            .0
            .iter()
            .map(|&[l, r]| ((i32::from(l) + i32::from(r)) / 2) as i16)
            .collect()
    } else {
        pcm.to_vec()
    }
}

#[derive(Clone)]
struct Ended {
    tx: mpsc::UnboundedSender<Signal>,
    id: ClipId,
}

#[async_trait]
impl EventHandler for Ended {
    async fn act(&self, _: &EventContext<'_>) -> Option<Event> {
        let _ = self.tx.send(Signal::Heard(Heard::Ended(self.id)));
        // Once is enough: remove this handler.
        Some(Event::Cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use songbird::input::codecs::{get_codec_registry, get_probe};

    #[tokio::test]
    async fn a_clip_decodes_through_songbirds_own_registry() {
        let chime = Audio::chime();
        let mut input = clip_input(&chime)
            .make_playable_async(get_codec_registry(), get_probe())
            .await
            .expect("songbird parses the raw container and finds a decoder for it");
        let parsed = input.parsed_mut().expect("a playable input is parsed");
        let mut decoded = 0;
        while let Ok(packet) = parsed.format.next_packet() {
            decoded += parsed
                .decoder
                .decode(&packet)
                .expect("symphonia's pcm codec decodes it")
                .frames();
        }
        assert_eq!(decoded, chime.len());
    }

    #[tokio::test]
    async fn building_the_manager_spawns_nothing() {
        let metrics = tokio::runtime::Handle::current().metrics();
        let before = metrics.num_alive_tasks();
        let user = std::num::NonZeroU64::new(1).expect("nonzero");
        let songbird = manager(Arc::new(TwilightMap::new(HashMap::new())), user);
        assert_eq!(metrics.num_alive_tasks(), before);
        drop(songbird);
    }

    #[test]
    fn only_mapped_ssrcs_become_frames_and_every_packet_is_counted() {
        let mut ssrcs = Ssrcs::default();
        let pcm = vec![7i16; FRAME_SAMPLES];
        let pcm = Some(pcm.as_slice());
        // Before any speaking event (DAVE's silent start): counted, not passed on.
        assert!(ssrcs.tick([(11, pcm), (22, None)]).is_empty());
        ssrcs.speaking(11, Speaker(101));
        ssrcs.speaking(22, Speaker(202));
        let frames = ssrcs.tick([(22, pcm), (11, pcm), (33, pcm)]);
        let who: Vec<_> = frames.iter().map(|f| f.speaker).collect();
        assert_eq!(who, [Speaker(101), Speaker(202)]);
        // A packet that didn't decode is a frame received, but not decoded audio.
        assert!(ssrcs.tick([(22, None)]).is_empty());
        // Once its user leaves, an SSRC's audio goes no further.
        ssrcs.left(Speaker(202));
        assert!(ssrcs.tick([(22, pcm)]).is_empty());
        let count = |user, frames, decoded| SsrcCount {
            user,
            frames,
            decoded,
        };
        assert_eq!(ssrcs.counts[&11], count(Some(Speaker(101)), 2, 2));
        assert_eq!(ssrcs.counts[&22], count(Some(Speaker(202)), 4, 2));
        assert_eq!(ssrcs.counts[&33], count(None, 1, 1));
    }

    #[test]
    fn a_stereo_frame_is_mixed_down() {
        let stereo: Vec<i16> = (0..FRAME_SAMPLES).flat_map(|_| [100, 300]).collect();
        assert_eq!(mono(&stereo), vec![200; FRAME_SAMPLES]);
        assert_eq!(mono(&[5; FRAME_SAMPLES]), vec![5; FRAME_SAMPLES]);
    }
}
