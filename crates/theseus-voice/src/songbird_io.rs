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
//!
//! Deafness (theseus-d93y): songbird drops a DAVE packet it can't decrypt
//! before any event sees it, and keeps DAVE's session private, so a call
//! whose MLS welcome never came hears nothing and says nothing. What it
//! does show: RTCP sender reports, which DAVE never encrypts, per SSRC. A
//! sender report from an SSRC is a packet that arrived; a tick with its
//! decoded audio is one that decrypted. [`Link`] reads both, and rejoins
//! the call in place.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use songbird::driver::{Channels, DecodeConfig, DecodeMode, SampleRate};
use songbird::events::context_data::DisconnectReason;
use songbird::events::{CoreEvent, Event, EventContext, EventHandler, TrackEvent};
use songbird::id::UserId;
use songbird::input::Input;
use songbird::packet::rtcp::RtcpPacket;
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
    /// RTCP sender reports from it (theseus-d93y): it is sending, whether or
    /// not its audio decrypts.
    pub reports: u64,
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

    /// An RTCP sender report from `ssrc`.
    fn report(&mut self, ssrc: u32) {
        self.counts.entry(ssrc).or_default().reports += 1;
    }

    /// `user` left the channel: their SSRCs map to no one.
    fn left(&mut self, user: Speaker) {
        self.users.retain(|_, s| *s != user);
    }

    /// A fresh connection: every count from zero, the users kept.
    fn restart(&mut self) {
        for c in self.counts.values_mut() {
            *c = SsrcCount {
                user: c.user,
                ..SsrcCount::default()
            };
        }
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
    /// A rejoin is leaving the call: its requested disconnect is no drop.
    rejoining: Arc<AtomicBool>,
    /// Why the connection dropped, once it did.
    gone: Option<String>,
}

/// A joined call's counts and its rejoin, apart from the engine that owns
/// the [`SongbirdIo`] (theseus-d93y).
#[derive(Clone)]
pub struct Link {
    call: Arc<tokio::sync::Mutex<Call>>,
    ssrcs: Arc<Mutex<Ssrcs>>,
    rejoining: Arc<AtomicBool>,
}

impl Link {
    /// What each SSRC has sent since the connection began.
    pub fn counts(&self) -> BTreeMap<u32, SsrcCount> {
        self.ssrcs
            .lock()
            .expect("the SSRC map's lock")
            .counts
            .clone()
    }

    /// Leave the call's channel and join it again, `gap` apart, within
    /// `within`: a fresh voice connection, so a fresh DAVE key package and
    /// welcome. The engine and its handlers stay; the counts start again.
    pub async fn rejoin(&self, gap: Duration, within: Duration) -> Result<(), String> {
        self.rejoining.store(true, Ordering::SeqCst);
        let r = self.leave_and_join(gap, within).await;
        self.rejoining.store(false, Ordering::SeqCst);
        r
    }

    async fn leave_and_join(&self, gap: Duration, within: Duration) -> Result<(), String> {
        let channel = {
            let mut c = self.call.lock().await;
            let channel = c.current_channel().ok_or("the call has no channel")?;
            c.leave().await.map_err(|e| e.to_string())?;
            channel
        };
        tokio::time::sleep(gap).await;
        self.ssrcs.lock().expect("the SSRC map's lock").restart();
        // The join's connect waits for the gateway's voice updates, which
        // songbird hands to the call under its lock: drop it first.
        let join = {
            let mut c = self.call.lock().await;
            c.join(channel).await.map_err(|e| e.to_string())?
        };
        match tokio::time::timeout(within, join).await {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(_) => Err(format!("no voice connection in {} s", within.as_secs())),
        }
    }
}

impl SongbirdIo {
    /// Listen to a call songbird has joined. songbird's own tasks run
    /// already; this adds its event handlers, and spawns nothing.
    pub async fn attach(call: Arc<tokio::sync::Mutex<Call>>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let ssrcs = Arc::new(Mutex::new(Ssrcs::default()));
        let rejoining = Arc::new(AtomicBool::new(false));
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
                rejoining: Arc::clone(&rejoining),
            };
            for event in [
                CoreEvent::SpeakingStateUpdate,
                CoreEvent::VoiceTick,
                CoreEvent::RtcpPacket,
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
            rejoining,
            gone: None,
        }
    }

    /// The call's counts and its rejoin, for the binding's deaf check.
    pub fn link(&self) -> Link {
        Link {
            call: Arc::clone(&self.call),
            ssrcs: Arc::clone(&self.ssrcs),
            rejoining: Arc::clone(&self.rejoining),
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
    rejoining: Arc<AtomicBool>,
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
            EventContext::RtcpPacket(r) => {
                if let RtcpPacket::SenderReport(s) = r.rtcp() {
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    ssrcs.report(s.get_ssrc());
                }
            }
            EventContext::ClientDisconnect(gone) => {
                let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                ssrcs.left(Speaker(gone.user_id.0));
            }
            EventContext::DriverDisconnect(d) => {
                // A rejoin's own leave: the call goes on.
                if self.rejoining.load(Ordering::SeqCst)
                    && d.reason == Some(DisconnectReason::Requested)
                {
                    return None;
                }
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
            reports: 0,
        };
        assert_eq!(ssrcs.counts[&11], count(Some(Speaker(101)), 2, 2));
        assert_eq!(ssrcs.counts[&22], count(Some(Speaker(202)), 4, 2));
        assert_eq!(ssrcs.counts[&33], count(None, 1, 1));
    }

    /// A sender report counts as a packet from its SSRC, with no audio; a
    /// rejoin's fresh connection counts from zero and keeps who is who
    /// (theseus-d93y).
    #[test]
    fn a_sender_report_is_counted_and_a_restart_zeroes_the_counts() {
        let mut ssrcs = Ssrcs::default();
        ssrcs.speaking(11, Speaker(101));
        ssrcs.report(11);
        ssrcs.report(11);
        ssrcs.report(44);
        assert_eq!(
            (ssrcs.counts[&11].reports, ssrcs.counts[&11].decoded),
            (2, 0)
        );
        assert_eq!(ssrcs.counts[&44].user, None);
        ssrcs.restart();
        assert_eq!(
            ssrcs.counts[&11],
            SsrcCount {
                user: Some(Speaker(101)),
                ..SsrcCount::default()
            }
        );
        let pcm = vec![7i16; FRAME_SAMPLES];
        assert_eq!(
            ssrcs.tick([(11, Some(pcm.as_slice()))]).len(),
            1,
            "still mapped"
        );
    }

    #[test]
    fn a_stereo_frame_is_mixed_down() {
        let stereo: Vec<i16> = (0..FRAME_SAMPLES).flat_map(|_| [100, 300]).collect();
        assert_eq!(mono(&stereo), vec![200; FRAME_SAMPLES]);
        assert_eq!(mono(&[5; FRAME_SAMPLES]), vec![5; FRAME_SAMPLES]);
    }
}
