//! songbird's side of the seam (design §2.8): a joined call's receive and
//! send, as a [`VoiceIo`]. Built with the `voice` feature.
//!
//! Receive: songbird hands decoded audio per SSRC every 20 ms (`VoiceTick`),
//! and speaking events map each SSRC to a user. A frame from an SSRC with no
//! user yet is counted and dropped: it can't be checked against the listed
//! users. With DAVE, expect a short silent start while the map and the keys
//! arrive (the 44a spike, Q1).
//!
//! Discord sends a speaking event once per SSRC, and with DAVE it sends one
//! for each member already talking during the voice handshake. songbird
//! fires those the moment its connection starts, before `Songbird::join`
//! returns, so handlers added after the join miss them, and that SSRC's audio
//! is dropped for the whole call (theseus-sro04). [`SongbirdIo::join`]
//! attaches first, then joins.
//!
//! The call's audio flows ([`SongbirdIo::flowing`]) once one SSRC's packets
//! have decoded for [`STEADY_TICKS`] ticks in a row: what plays before that,
//! while DAVE's group is still forming, may reach the others garbled, so the
//! binding holds its greeting for it.
//!
//! Send: each clip is a songbird track, in songbird's raw container, which
//! its codec registry decodes with symphonia's `pcm` codec.
//!
//! Deafness (theseus-d93y): songbird drops a DAVE packet it can't decrypt
//! before any event sees it, and keeps DAVE's session private, so a call
//! whose MLS welcome never came hears nothing and says nothing. What it
//! does show: RTCP sender reports, which DAVE never encrypts, per SSRC. A
//! sender report from an SSRC is a packet that arrived; a tick with its
//! packet and decoded audio is one that decrypted (a tick with audio and no
//! packet is songbird concealing a loss, and is neither). [`Link`] reads
//! both, and rejoins the call in place.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use songbird::driver::{Channels, DecodeConfig, DecodeMode, SampleRate};
use songbird::events::context_data::DisconnectReason;
use songbird::events::{CoreEvent, Event, EventContext, EventHandler, TrackEvent};
use songbird::id::UserId;
use songbird::input::Input;
use songbird::packet::rtcp::RtcpPacket;
use songbird::packet::Packet as _;
use songbird::shards::TwilightMap;
use songbird::tracks::{Track, TrackHandle};
use songbird::{Call, Songbird};
use tokio::sync::{mpsc, watch};

use crate::audio::{Audio, FRAME_SAMPLES, SAMPLE_RATE};
use crate::io::{ClipId, Frame, Heard, Speaker, VoiceIo};
use crate::warm::{led_in, warm_silence, AfterClip, LeadIn, Silence, Spell, Warm, Warmth};

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
    /// The packets its sender reports say it sent since its first report
    /// (theseus-sro04): beside `frames`, what was sent and never arrived or
    /// never decrypted. Silent, it sends none, and this stays.
    pub sent: u64,
}

/// A sender report's packet count that grows faster than this a second, plus
/// [`SENT_SLACK`], is not a count: a report that did not decrypt.
const SENT_PER_SEC: u64 = 60;
const SENT_SLACK: u64 = 50;

/// One SSRC's last believed sender-report count, and when; and a count that
/// was not believed, which the next report may confirm as a new start.
#[derive(Clone, Copy)]
struct Reported {
    last: u32,
    at: Instant,
    unsure: Option<(u32, Instant)>,
}

/// Ticks in a row with a decoded packet from one SSRC that say the call's
/// audio flows: 200 ms. While DAVE's group forms, a member's frames come
/// unmarked and most fail to decode (theseus-sro04: 52 of the first 80), so
/// a run this long is the transition over, not a stray frame that decoded.
pub const STEADY_TICKS: u32 = 10;

#[derive(Default)]
struct Ssrcs {
    users: HashMap<u32, Speaker>,
    counts: BTreeMap<u32, SsrcCount>,
    /// Each SSRC's decoded packets in a row, up to the last tick.
    streak: HashMap<u32, u32>,
    /// Some SSRC's run reached [`STEADY_TICKS`].
    steady: bool,
    /// The SSRCs whose audio was dropped for want of a user, said once each.
    unmapped: HashSet<u32>,
    /// Each SSRC's sender-report count, for `SsrcCount::sent`.
    reported: HashMap<u32, Reported>,
}

impl Ssrcs {
    /// A speaking event: `ssrc` is `user`'s.
    fn speaking(&mut self, ssrc: u32, user: Speaker) {
        if self.users.insert(ssrc, user) != Some(user) {
            tracing::info!(
                ssrc,
                user = user.0,
                "voice: a speaking event mapped an SSRC to its user"
            );
        }
        self.counts.entry(ssrc).or_default().user = Some(user);
    }

    /// One tick's packets, each SSRC with its decoded audio if any, and
    /// whether a packet arrived for it: a frame for each SSRC mapped to a
    /// user, in speaker order. Every packet is counted; one from an SSRC with
    /// no user yet goes no further. A tick with no packet is songbird
    /// concealing a lost one (theseus-voice-dave): its audio is passed on, but
    /// it is neither a packet nor a decrypted one, or the deaf check would
    /// take a call whose every packet fails for a call that hears.
    fn tick<'a>(
        &mut self,
        packets: impl IntoIterator<Item = (u32, Option<&'a [i16]>, bool)>,
    ) -> Vec<Frame> {
        let mut frames = Vec::new();
        let mut streak = HashMap::new();
        for (ssrc, pcm, arrived) in packets {
            let user = self.users.get(&ssrc).copied();
            let count = self.counts.entry(ssrc).or_default();
            if arrived {
                count.frames += 1;
            }
            let Some(pcm) = pcm else {
                continue;
            };
            if arrived {
                count.decoded += 1;
                let run = self.streak.get(&ssrc).copied().unwrap_or(0) + 1;
                streak.insert(ssrc, run);
                self.steadied(ssrc, run);
            }
            match user {
                Some(speaker) => frames.push(Frame {
                    speaker,
                    samples: mono(pcm),
                }),
                None if arrived => self.dropped(ssrc),
                None => {}
            }
        }
        // An SSRC missing from this tick (a decode error, a loss) starts again.
        self.streak = streak;
        frames.sort_by_key(|f| f.speaker);
        frames
    }

    /// `ssrc`'s decoded packets in a row are `run`: the call's audio flows
    /// at [`STEADY_TICKS`].
    fn steadied(&mut self, ssrc: u32, run: u32) {
        if !self.steady && run >= STEADY_TICKS {
            self.steady = true;
            tracing::info!(
                ssrc,
                "voice: the call's audio flows (an SSRC decoded {run} packets in a row)"
            );
        }
    }

    /// `ssrc`'s decoded audio went no further: no speaking event mapped it.
    fn dropped(&mut self, ssrc: u32) {
        if self.unmapped.insert(ssrc) {
            tracing::warn!(
                ssrc,
                "voice: audio from an SSRC no speaking event mapped to a user is dropped"
            );
        }
    }

    /// An RTCP sender report from `ssrc`, with the packet count it says its
    /// sender sent, if it carried one, at `now`. A count is believed when it
    /// grew no faster than a sender can send since the last believed one; a
    /// report that did not decrypt carries noise, which is not. Two reports
    /// in a row that agree with each other start again from the second.
    fn report(&mut self, ssrc: u32, count: Option<u32>, now: Instant) {
        let c = self.counts.entry(ssrc).or_default();
        c.reports += 1;
        let Some(count) = count else {
            return;
        };
        let grew = |from: u32, at: Instant| {
            let d = u64::from(count.wrapping_sub(from));
            let secs = now.saturating_duration_since(at).as_secs_f64();
            let most = (SENT_PER_SEC as f64 * secs) as u64 + SENT_SLACK;
            (d <= most).then_some(d)
        };
        let fresh = Reported {
            last: count,
            at: now,
            unsure: None,
        };
        match self.reported.get_mut(&ssrc) {
            None => {
                self.reported.insert(ssrc, fresh);
            }
            Some(r) => match grew(r.last, r.at) {
                Some(d) => {
                    c.sent += d;
                    *r = fresh;
                }
                None if r.unsure.is_some_and(|(u, at)| grew(u, at).is_some()) => *r = fresh,
                None => r.unsure = Some((count, now)),
            },
        }
    }

    /// `user` left the channel: their SSRCs map to no one.
    fn left(&mut self, user: Speaker) {
        self.users.retain(|_, s| *s != user);
    }

    /// A fresh connection: every count from zero, the users kept.
    fn restart(&mut self) {
        self.streak.clear();
        self.reported.clear();
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
    spell: Spell,
    /// The silent tracks that keep the stream warm between clips.
    warm: Warm<TrackHandle>,
    /// The silence sent before the clip last played (`VoiceIo::lead_in`).
    lead_in: usize,
    ssrcs: Arc<Mutex<Ssrcs>>,
    /// A rejoin is leaving the call: its requested disconnect is no drop.
    rejoining: Arc<AtomicBool>,
    /// Why the connection dropped, once it did.
    gone: Option<String>,
    /// Whether the call's audio flows (`Ssrcs::steady`).
    flowing: watch::Receiver<bool>,
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
    /// `wanted` is asked under the call's lock just before the join: a call
    /// that ended in the gap (a `/leave`, which keeps songbird's `Call`) is
    /// not joined again.
    pub async fn rejoin(
        &self,
        gap: Duration,
        within: Duration,
        wanted: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), String> {
        self.rejoining.store(true, Ordering::SeqCst);
        let r = self.leave_and_join(gap, within, wanted).await;
        self.rejoining.store(false, Ordering::SeqCst);
        r
    }

    async fn leave_and_join(
        &self,
        gap: Duration,
        within: Duration,
        wanted: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), String> {
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
            // A `/leave` takes the call's slot before songbird's leave takes
            // this lock, so asked here a call that ended says so.
            if !wanted() {
                return Err("the call ended during the rejoin".into());
            }
            c.join(channel).await.map_err(|e| e.to_string())?
        };
        match tokio::time::timeout(within, join).await {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(_) => Err(format!("no voice connection in {} s", within.as_secs())),
        }
    }
}

impl SongbirdIo {
    /// Join `channel` in `guild` with handlers that hear the connection
    /// from its start: attached to the guild's call first, then the join
    /// (theseus-sro04). songbird fires the speaking events Discord sends
    /// during the handshake as its connection starts, before its join
    /// returns; attached after, the call never learns whose SSRC is whose.
    /// The caller bounds the wait; a join that fails or times out leaves
    /// songbird's call for the caller to remove, as `Songbird::join` does.
    pub async fn join(
        songbird: &Songbird,
        guild: NonZeroU64,
        channel: NonZeroU64,
        lead_in: LeadIn,
    ) -> songbird::error::JoinResult<Self> {
        Self::join_call(songbird.get_or_insert(guild), channel, lead_in).await
    }

    async fn join_call(
        call: Arc<tokio::sync::Mutex<Call>>,
        channel: NonZeroU64,
        lead_in: LeadIn,
    ) -> songbird::error::JoinResult<Self> {
        let io = Self::attach(Arc::clone(&call), lead_in).await;
        // songbird hands the gateway's voice updates to the call under its
        // lock while the join's connection waits for them: drop it first.
        let join = call.lock().await.join(channel).await?;
        join.await?;
        Ok(io)
    }

    /// Listen to `call`. songbird's own tasks run already; this replaces any
    /// handlers an earlier call on it left (a `/leave` keeps songbird's
    /// `Call`) with its own, and spawns nothing. Attached after the call has
    /// joined, it misses the speaking events of anyone already talking:
    /// [`SongbirdIo::join`] attaches before. `lead_in` is the silence before
    /// a clip (`crate::warm`).
    pub async fn attach(call: Arc<tokio::sync::Mutex<Call>>, lead_in: LeadIn) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let ssrcs = Arc::new(Mutex::new(Ssrcs::default()));
        let rejoining = Arc::new(AtomicBool::new(false));
        let (flows, flowing) = watch::channel(false);
        {
            let mut c = call.lock().await;
            c.remove_all_global_events();
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
                flows: Arc::new(flows),
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
            spell: Spell::new(lead_in),
            warm: Warm::new(),
            lead_in: 0,
            ssrcs,
            rejoining,
            gone: None,
            flowing,
        }
    }

    /// Turns true once the call's audio flows: one SSRC's packets decoded
    /// [`STEADY_TICKS`] ticks in a row, so DAVE's group has formed and what
    /// is played now is heard. Never true in a call where nobody talks: a
    /// waiter bounds its wait.
    pub fn flowing(&self) -> watch::Receiver<bool> {
        self.flowing.clone()
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

/// The call's end, in the journal: per SSRC, whose it was and what it sent,
/// so a call that heard nothing says why (theseus-sro04). No audio.
impl Drop for SongbirdIo {
    fn drop(&mut self) {
        let Ok(ssrcs) = self.ssrcs.lock() else {
            return;
        };
        let lines: Vec<String> = ssrcs
            .counts
            .iter()
            .map(|(ssrc, c)| summary(*ssrc, c))
            .collect();
        let lines = lines.join("; ");
        if ssrcs.unmapped.is_empty() {
            tracing::info!(flowed = ssrcs.steady, "voice: the call ended: {lines}");
        } else {
            tracing::warn!(
                flowed = ssrcs.steady,
                unmapped = ssrcs.unmapped.len(),
                "voice: the call ended with audio dropped for want of a speaking event: {lines}"
            );
        }
    }
}

/// One SSRC's part of the call's end.
fn summary(ssrc: u32, c: &SsrcCount) -> String {
    let user = c
        .user
        .map_or_else(|| "no user".to_string(), |u| format!("user {}", u.0));
    format!(
        "ssrc {ssrc} ({user}): {} packets, {} decoded, {} sender reports ({} sent)",
        c.frames, c.decoded, c.reports, c.sent
    )
}

impl VoiceIo for SongbirdIo {
    fn next(&mut self) -> BoxFuture<'_, Option<Heard>> {
        // `recv` is cancel-safe.
        Box::pin(async move {
            match self.rx.recv().await? {
                Signal::Heard(heard) => {
                    if let Heard::Ended(id) = heard {
                        self.spell.end(id, Instant::now());
                    }
                    Some(heard)
                }
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

    fn lead_in(&self) -> usize {
        self.lead_in
    }

    fn play(&mut self, id: ClipId, clip: Audio) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let frames = self.spell.begin(id, Instant::now(), self.warm.warmth.on());
            self.lead_in = frames as usize * FRAME_SAMPLES;
            let input = clip_input(&led_in(&clip, frames));
            // The clip's warm silence waits beside it, paused, so its end
            // starts it at once (`crate::warm`).
            let (track, warm) = {
                let mut call = self.call.lock().await;
                let warm = call.play(Track::new(clip_input(&warm_silence())).pause());
                (call.play_input(input), warm)
            };
            let _ = warm.add_event(
                Event::Track(TrackEvent::End),
                WarmEnded {
                    warmth: self.warm.warmth.clone(),
                    id,
                },
            );
            let after = Arc::new(AfterClip {
                id,
                warm: Some(warm.clone()),
                warmth: self.warm.warmth.clone(),
            });
            let ended = Ended {
                tx: self.tx.clone(),
                after: after.clone(),
            };
            self.warm.add(warm);
            // A track that ends or fails is over either way. If the track is
            // already gone, the error says so, and its `Ended` is sent here.
            let added = track
                .add_event(Event::Track(TrackEvent::End), ended.clone())
                .and_then(|()| track.add_event(Event::Track(TrackEvent::Error), ended));
            if added.is_err() {
                after.abandoned();
                let _ = self.tx.send(Signal::Heard(Heard::Ended(id)));
            }
            self.track = Some(track);
        })
    }

    fn stop(&mut self) -> BoxFuture<'_, ()> {
        if let Some(id) = self.spell.clip {
            self.spell.end(id, Instant::now());
        }
        // The warm tracks first, so the stopped clip's end starts none.
        self.warm.stop();
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
    flows: Arc<watch::Sender<bool>>,
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
                let packets = tick.speaking.iter().map(|(ssrc, data)| {
                    let arrived = data.packet.is_some();
                    (*ssrc, data.decoded_voice.as_deref(), arrived)
                });
                let (frames, steady) = {
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    (ssrcs.tick(packets), ssrcs.steady)
                };
                if steady {
                    // Once: a watch that already says so is left alone.
                    self.flows.send_if_modified(|f| !std::mem::replace(f, true));
                }
                let _ = self.tx.send(Signal::Heard(Heard::Tick(frames)));
            }
            EventContext::RtcpPacket(r) => {
                if let RtcpPacket::SenderReport(s) = r.rtcp() {
                    // The sender info: NTP time (8 bytes), RTP time (4), then
                    // the packets sent (4), big-endian (RFC 3550, 6.4.1).
                    let sent = s
                        .payload()
                        .get(12..16)
                        .and_then(|b| b.try_into().ok())
                        .map(u32::from_be_bytes);
                    let mut ssrcs = self.ssrcs.lock().expect("the SSRC map's lock");
                    ssrcs.report(s.get_ssrc(), sent, Instant::now());
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
    after: Arc<AfterClip<TrackHandle>>,
}

#[async_trait]
impl EventHandler for Ended {
    async fn act(&self, _: &EventContext<'_>) -> Option<Event> {
        // The warm silence starts before the engine hears the end, so the
        // next clip reads it as playing.
        self.after.ended();
        let _ = self.tx.send(Signal::Heard(Heard::Ended(self.after.id)));
        // Once is enough: remove this handler.
        Some(Event::Cancel)
    }
}

impl Silence for TrackHandle {
    fn play(&self) -> bool {
        TrackHandle::play(self).is_ok()
    }

    fn stop(&self) {
        let _ = TrackHandle::stop(self);
    }
}

/// A warm track's end: the warmth goes out, and the engine hears nothing.
struct WarmEnded {
    warmth: Warmth,
    id: ClipId,
}

#[async_trait]
impl EventHandler for WarmEnded {
    async fn act(&self, _: &EventContext<'_>) -> Option<Event> {
        self.warmth.ended(self.id);
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

    /// A lead-in of `ms` before a spell, none after a gap.
    fn spell_of(ms: u64) -> LeadIn {
        LeadIn {
            spell: Duration::from_millis(ms),
            gap: Duration::ZERO,
        }
    }

    #[test]
    fn the_first_clip_after_quiet_leads_in_and_the_next_sentence_does_not() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut spell = Spell::new(spell_of(100));
        assert_eq!(
            spell.begin(ClipId(0), t0, false),
            5,
            "the greeting after a quiet call"
        );
        spell.end(ClipId(0), t0 + ms(1200));
        assert_eq!(
            spell.begin(ClipId(1), t0 + ms(1210), true),
            0,
            "the next sentence, under the warm silence"
        );
        spell.end(ClipId(1), t0 + ms(2000));
        assert_eq!(
            spell.begin(ClipId(2), t0 + ms(2600), true),
            5,
            "a reply after quiet"
        );
        // A stopped clip's late `Ended` leaves the next one's spell alone.
        spell.end(ClipId(2), t0 + ms(2700));
        assert_eq!(spell.begin(ClipId(3), t0 + ms(3300), false), 5);
        spell.end(ClipId(2), t0 + ms(3310));
        assert_eq!(spell.clip, Some(ClipId(3)));
        let chime = Audio::chime();
        let led = led_in(&chime, spell.lead_in);
        assert_eq!(spell.lead_in, 5, "100 ms is 5 frames");
        assert_eq!(led.len(), chime.len() + 5 * FRAME_SAMPLES);
        assert!(led.samples()[..5 * FRAME_SAMPLES].iter().all(|&s| s == 0));
        assert_eq!(&led.samples()[5 * FRAME_SAMPLES..], chime.samples());
    }

    /// A lead-in of 400 ms is 20 silent frames before the clip (a partial frame
    /// rounds up), and 0 puts none and reports no lead (theseus-bp0lp).
    #[test]
    fn a_spell_leads_in_with_the_configured_silence() {
        let chime = Audio::chime();
        let mut spell = Spell::new(spell_of(400));
        assert_eq!(spell.lead_in, 20);
        assert_eq!(spell.begin(ClipId(0), Instant::now(), false), 20);
        let led = led_in(&chime, spell.lead_in);
        assert_eq!(led.len(), chime.len() + 20 * FRAME_SAMPLES);
        assert!(led.samples()[..20 * FRAME_SAMPLES].iter().all(|&s| s == 0));
        assert_eq!(&led.samples()[20 * FRAME_SAMPLES..], chime.samples());
        assert_eq!(Spell::new(spell_of(410)).lead_in, 21);
        let mut off = Spell::new(LeadIn::default());
        assert_eq!(
            off.begin(ClipId(0), Instant::now(), false),
            0,
            "no lead when off"
        );
        assert_eq!(led_in(&chime, off.lead_in).len(), chime.len());
    }

    /// `VoiceIo::lead_in` is the silence that led the clip played last, which
    /// the echo canceller aligns by: the spell's after quiet, the gap's when
    /// nothing was warm, none under the warm silence (theseus-9d9aq).
    #[tokio::test]
    async fn lead_in_reports_the_silence_that_led_the_clip() {
        let id = |n| std::num::NonZeroU64::new(n).unwrap();
        let shard = songbird::shards::Shard::Generic(Arc::new(Sent::default()));
        let call = Call::new(
            songbird::id::GuildId::from(id(1)),
            shard,
            UserId::from(id(2)),
        );
        let lead = LeadIn {
            spell: Duration::from_millis(600),
            gap: Duration::from_millis(200),
        };
        let mut io = SongbirdIo::attach(Arc::new(tokio::sync::Mutex::new(call)), lead).await;
        let end = async |io: &mut SongbirdIo, n| {
            io.tx.send(Signal::Heard(Heard::Ended(ClipId(n)))).unwrap();
            assert_eq!(io.next().await, Some(Heard::Ended(ClipId(n))));
        };
        io.play(ClipId(0), Audio::chime()).await;
        assert_eq!(io.lead_in(), 30 * FRAME_SAMPLES, "a spell's first clip");
        end(&mut io, 0).await;
        // No connection here, so no warm track plays: the gap's lead-in.
        io.play(ClipId(1), Audio::chime()).await;
        assert_eq!(io.lead_in(), 10 * FRAME_SAMPLES, "after a gap kept cold");
        end(&mut io, 1).await;
        // As clip 1's end does once its warm track plays.
        io.warm.warmth.began(ClipId(1));
        io.play(ClipId(2), Audio::chime()).await;
        assert_eq!(io.lead_in(), 0, "under the warm silence");
        io.stop().await;
        assert!(!io.warm.warmth.on(), "a stop ends the warmth");
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
        assert!(ssrcs.tick([(11, pcm, true), (22, None, true)]).is_empty());
        ssrcs.speaking(11, Speaker(101));
        ssrcs.speaking(22, Speaker(202));
        let frames = ssrcs.tick([(22, pcm, true), (11, pcm, true), (33, pcm, true)]);
        let who: Vec<_> = frames.iter().map(|f| f.speaker).collect();
        assert_eq!(who, [Speaker(101), Speaker(202)]);
        // A packet that didn't decode is a frame received, but not decoded audio.
        assert!(ssrcs.tick([(22, None, true)]).is_empty());
        // Once its user leaves, an SSRC's audio goes no further.
        ssrcs.left(Speaker(202));
        assert!(ssrcs.tick([(22, pcm, true)]).is_empty());
        let count = |user, frames, decoded| SsrcCount {
            user,
            frames,
            decoded,
            ..SsrcCount::default()
        };
        assert_eq!(ssrcs.counts[&11], count(Some(Speaker(101)), 2, 2));
        assert_eq!(ssrcs.counts[&22], count(Some(Speaker(202)), 4, 2));
        assert_eq!(ssrcs.counts[&33], count(None, 1, 1));
    }

    /// A lost packet's concealment (theseus-voice-dave): songbird's tick
    /// carries audio for an SSRC with no packet. Its audio goes on to the
    /// engine, but it is no packet and no decrypted one: a call whose packets
    /// all fail to decode, with a lost one among them, is not a call that
    /// hears.
    #[test]
    fn a_concealed_loss_is_audio_but_neither_a_packet_nor_a_decrypted_one() {
        let mut ssrcs = Ssrcs::default();
        ssrcs.speaking(11, Speaker(101));
        let pcm = vec![7i16; FRAME_SAMPLES];
        let frames = ssrcs.tick([(11, Some(pcm.as_slice()), false)]);
        assert_eq!(frames.len(), 1, "the concealment is passed on");
        assert_eq!(
            (ssrcs.counts[&11].frames, ssrcs.counts[&11].decoded),
            (0, 0)
        );
        assert!(ssrcs.tick([(11, None, true)]).is_empty());
        assert_eq!(
            (ssrcs.counts[&11].frames, ssrcs.counts[&11].decoded),
            (1, 0)
        );
    }

    /// An SSRC no speaking event mapped (the handshake's event missed,
    /// theseus-sro04): its audio is counted, goes no further, and is said
    /// once; a speaking event later maps it and its audio is heard.
    #[test]
    fn an_unmapped_ssrcs_audio_is_counted_and_dropped_until_a_speaking_event() {
        let mut ssrcs = Ssrcs::default();
        let pcm = vec![7i16; FRAME_SAMPLES];
        for _ in 0..3 {
            assert!(ssrcs.tick([(3986, Some(pcm.as_slice()), true)]).is_empty());
        }
        assert_eq!(ssrcs.unmapped, HashSet::from([3986]));
        assert_eq!(
            (ssrcs.counts[&3986].frames, ssrcs.counts[&3986].decoded),
            (3, 3)
        );
        assert_eq!(
            summary(3986, &ssrcs.counts[&3986]),
            "ssrc 3986 (no user): 3 packets, 3 decoded, 0 sender reports (0 sent)"
        );
        ssrcs.speaking(3986, Speaker(101));
        let frames = ssrcs.tick([(3986, Some(pcm.as_slice()), true)]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].speaker, Speaker(101));
    }

    /// The call's audio flows only after one SSRC decoded STEADY_TICKS
    /// packets in a row, mapped or not: a frame that decodes among failures
    /// (DAVE's transition), a concealed loss, or a short run is not it.
    #[test]
    fn the_audio_flows_after_a_steady_run_not_a_stray_decoded_frame() {
        let mut ssrcs = Ssrcs::default();
        let pcm = vec![7i16; FRAME_SAMPLES];
        let pcm = Some(pcm.as_slice());
        // A decode error leaves the SSRC out of the tick's speaking map.
        for _ in 0..20 {
            ssrcs.tick([(5, pcm, true)]);
            ssrcs.tick(std::iter::empty());
        }
        assert!(
            !ssrcs.steady,
            "a frame in two decodes, as in the transition"
        );
        for _ in 0..STEADY_TICKS - 1 {
            ssrcs.tick([(5, pcm, true)]);
        }
        ssrcs.tick([(5, pcm, false)]);
        ssrcs.tick([(5, None, true)]);
        assert!(
            !ssrcs.steady,
            "a loss and an undecoded packet break the run"
        );
        for n in 1..=STEADY_TICKS {
            ssrcs.tick([(5, pcm, true), (6, None, true)]);
            assert_eq!(ssrcs.steady, n == STEADY_TICKS, "after {n} in a row");
        }
    }

    /// A sender report counts as a packet from its SSRC, with no audio; a
    /// rejoin's fresh connection counts from zero and keeps who is who
    /// (theseus-d93y).
    #[test]
    fn a_sender_report_is_counted_and_a_restart_zeroes_the_counts() {
        let mut ssrcs = Ssrcs::default();
        ssrcs.speaking(11, Speaker(101));
        let now = Instant::now();
        ssrcs.report(11, None, now);
        ssrcs.report(11, None, now);
        ssrcs.report(44, None, now);
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
            ssrcs.tick([(11, Some(pcm.as_slice()), true)]).len(),
            1,
            "still mapped"
        );
    }

    /// A sender report's packet count adds what its sender sent since the
    /// last one (theseus-sro04); one that grew faster than a sender can send
    /// is noise, a report that did not decrypt, and adds nothing; two in a
    /// row that agree start again from the second; a restart forgets.
    #[test]
    fn sender_reports_count_what_was_sent_and_noise_is_not_a_count() {
        let mut ssrcs = Ssrcs::default();
        let t = Instant::now();
        let at = |s: u64| t + Duration::from_secs(s);
        ssrcs.report(11, Some(1000), at(0));
        ssrcs.report(11, Some(1050), at(1));
        ssrcs.report(11, Some(1050), at(2));
        assert_eq!(ssrcs.counts[&11].sent, 50, "silent for a second: none");
        ssrcs.report(11, Some(3_000_000_000), at(3));
        ssrcs.report(11, Some(1100), at(4));
        assert_eq!(ssrcs.counts[&11].sent, 100, "the noise added nothing");
        // A new start: two that agree, far from the last.
        ssrcs.report(11, Some(90_000), at(5));
        ssrcs.report(11, Some(90_040), at(6));
        ssrcs.report(11, Some(90_090), at(7));
        assert_eq!(ssrcs.counts[&11].sent, 150);
        assert_eq!(ssrcs.counts[&11].reports, 8);
        // A report with no count is a report, and nothing sent.
        ssrcs.report(11, None, at(8));
        assert_eq!(ssrcs.counts[&11].sent, 150);
        ssrcs.restart();
        ssrcs.report(11, Some(5), at(9));
        ssrcs.report(11, Some(55), at(10));
        assert_eq!(ssrcs.counts[&11].sent, 50, "from the restart's first");
    }

    #[test]
    fn a_stereo_frame_is_mixed_down() {
        let stereo: Vec<i16> = (0..FRAME_SAMPLES).flat_map(|_| [100, 300]).collect();
        assert_eq!(mono(&stereo), vec![200; FRAME_SAMPLES]);
        assert_eq!(mono(&[5; FRAME_SAMPLES]), vec![5; FRAME_SAMPLES]);
    }

    /// The voice state updates a call sends, each as its channel (`None`: a
    /// leave).
    #[derive(Default)]
    struct Sent(Mutex<Vec<Option<u64>>>);

    #[async_trait]
    impl songbird::shards::VoiceUpdate for Sent {
        async fn update_voice_state(
            &self,
            _guild: songbird::id::GuildId,
            channel: Option<songbird::id::ChannelId>,
            _deaf: bool,
            _mute: bool,
        ) -> songbird::error::JoinResult<()> {
            self.0.lock().unwrap().push(channel.map(|c| c.0.get()));
            Ok(())
        }
    }

    /// A join attaches, then asks the gateway for the channel
    /// (theseus-sro04): the handlers are on the call before any voice
    /// connection can fire an event. With no voice server here, the join
    /// fails at songbird's gateway timeout, its one request sent.
    #[tokio::test(start_paused = true)]
    async fn a_join_attaches_then_asks_for_the_channel() {
        let id = |n| std::num::NonZeroU64::new(n).unwrap();
        let sent = Arc::new(Sent::default());
        let shard = songbird::shards::Shard::Generic(sent.clone());
        let guild = songbird::id::GuildId::from(id(1));
        let call = Arc::new(tokio::sync::Mutex::new(Call::new(
            guild,
            shard,
            UserId::from(id(2)),
        )));
        let r = SongbirdIo::join_call(Arc::clone(&call), id(3), LeadIn::default()).await;
        assert!(r.is_err(), "no voice server answers here");
        assert_eq!(*sent.0.lock().unwrap(), [Some(3)]);
    }

    /// A rejoin leaves the channel and joins it again, its counts from zero;
    /// a call that ends in its gap (a `/leave`) is not joined again
    /// (theseus-d93y).
    #[tokio::test(start_paused = true)]
    async fn a_rejoin_joins_again_unless_the_call_ended_in_its_gap() {
        let id = |n| std::num::NonZeroU64::new(n).unwrap();
        for leave_in_the_gap in [false, true] {
            let sent = Arc::new(Sent::default());
            let shard = songbird::shards::Shard::Generic(sent.clone());
            let guild = songbird::id::GuildId::from(id(1));
            let call = Arc::new(tokio::sync::Mutex::new(Call::new(
                guild,
                shard,
                UserId::from(id(2)),
            )));
            let channel = songbird::id::ChannelId::from(id(3));
            drop(call.lock().await.join(channel).await.unwrap());
            let io = SongbirdIo::attach(Arc::clone(&call), LeadIn::default()).await;
            io.ssrcs.lock().unwrap().report(11, None, Instant::now());
            let (link, wanted) = (io.link(), Arc::new(AtomicBool::new(true)));
            let still = Arc::clone(&wanted);
            let rejoin = tokio::spawn(async move {
                let wanted = move || still.load(Ordering::SeqCst);
                let (gap, within) = (Duration::from_millis(300), Duration::from_millis(200));
                link.rejoin(gap, within, &wanted).await
            });
            tokio::time::sleep(Duration::from_millis(100)).await;
            if leave_in_the_gap {
                // `/leave`: the call's slot taken, then songbird's leave.
                wanted.store(false, Ordering::SeqCst);
                call.lock().await.leave().await.unwrap();
            }
            let r = rejoin.await.unwrap();
            assert_eq!(
                io.ssrcs()[&11].reports,
                0,
                "a fresh connection counts from zero"
            );
            let sent = sent.0.lock().unwrap().clone();
            if leave_in_the_gap {
                assert_eq!(r, Err("the call ended during the rejoin".to_string()));
                assert_eq!(sent, [Some(3), None, None], "no join after the leave");
                assert!(call.lock().await.current_channel().is_none());
            } else {
                assert!(r.is_err(), "no voice server answers here: {r:?}");
                assert_eq!(sent, [Some(3), None, Some(3)]);
            }
        }
    }
}
