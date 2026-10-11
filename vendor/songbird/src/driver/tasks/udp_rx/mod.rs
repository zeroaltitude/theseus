mod decode_sizes;
mod playout_buffer;
mod ssrc_state;

use self::{decode_sizes::*, playout_buffer::*, ssrc_state::*};

use super::message::*;
use crate::driver::{CryptoMode, DecodeMode};
use crate::{
    constants::*,
    driver::crypto::Cipher,
    events::{context_data::VoiceTick, internal_data::*, CoreContext},
    Config,
};
use bytes::BytesMut;
use discortp::{
    demux::{self, DemuxedMut},
    rtp::RtpPacket,
};
use discortp::{MutablePacket, Packet};
use flume::Receiver;
use std::sync::{
    atomic::{AtomicU16, Ordering},
    RwLock,
};
use std::{
    collections::{HashMap, HashSet},
    num::Wrapping,
    sync::Arc,
    time::Duration,
};
use tokio::{net::UdpSocket, select, time::Instant};
use tracing::{error, info, instrument, trace, warn};

type RtpSequence = Wrapping<u16>;
type RtpTimestamp = Wrapping<u32>;
type RtpSsrc = u32;

struct UdpRx {
    cipher: Cipher,
    crypto_mode: CryptoMode,
    decoder_map: HashMap<RtpSsrc, SsrcState>,
    config: Config,
    rx: Receiver<UdpRxMessage>,
    ssrc_signalling: Arc<SsrcTracker>,
    udp_socket: UdpSocket,
    dave_session: Arc<RwLock<Option<davey::DaveSession>>>,
    dave_protocol_version: Arc<AtomicU16>,
    dave_counts: DaveCounts,
}

/// What became of the DAVE frames received since the last cleanup (Theseus's addition,
/// vendor/songbird/VENDORED.md): upstream drops a frame it can't decrypt without a word, so a call
/// that hears nothing said nothing either.
#[derive(Debug, Default)]
struct DaveCounts {
    decrypted: u64,
    unmarked: u64,
    no_user: u64,
    no_session: u64,
    not_ready: u64,
    no_decryptor: u64,
    no_cryptor: u64,
    failed: u64,
}

impl DaveCounts {
    fn dropped(&self) -> u64 {
        self.no_user
            + self.no_session
            + self.not_ready
            + self.no_decryptor
            + self.no_cryptor
            + self.failed
    }
}

impl UdpRx {
    #[instrument(skip(self))]
    async fn run(&mut self, interconnect: &mut Interconnect) {
        let mut cleanup_time = Instant::now();
        let mut playout_time = Instant::now() + TIMESTEP_LENGTH;
        let mut byte_dest: Option<BytesMut> = None;

        loop {
            if byte_dest.is_none() {
                byte_dest = Some(BytesMut::zeroed(VOICE_PACKET_MAX));
            }

            select! {
                Ok((len, _addr)) = self.udp_socket.recv_from(byte_dest.as_mut().unwrap()) => {
                    let mut pkt = byte_dest.take().unwrap();
                    pkt.truncate(len);

                    self.process_udp_message(interconnect, pkt).await;
                },
                msg = self.rx.recv_async() => {
                    match msg {
                        Ok(UdpRxMessage::ReplaceInterconnect(i)) => {
                            *interconnect = i;
                        },
                        Ok(UdpRxMessage::SetConfig(new_config)) => {
                            if let DecodeMode::Decode(old_config) = &mut self.config.decode_mode {
                                if *old_config != new_config {
                                    *old_config = new_config;
                                    self.decoder_map.values_mut().for_each(|v| v.reconfigure_decoder(new_config));
                                }
                            }
                        },
                        Err(flume::RecvError::Disconnected) => break,
                    }
                },
                () = tokio::time::sleep_until(playout_time) => {
                    let mut tick = VoiceTick {
                        speaking: HashMap::new(),
                        silent: HashSet::new(),
                    };

                    for (ssrc, state) in &mut self.decoder_map {
                        match state.get_voice_tick(&self.config) {
                            Ok(Some(data)) => {
                                tick.speaking.insert(*ssrc, data);
                            },
                            Ok(None) => {
                                if !state.disconnected {
                                    tick.silent.insert(*ssrc);
                                }
                            },
                            Err(e) => {
                                warn!("Decode error for SSRC {ssrc}: {e:?}");
                                tick.silent.insert(*ssrc);
                            },
                        }
                    }

                    playout_time += TIMESTEP_LENGTH;

                    drop(interconnect.events.send(EventMessage::FireCoreEvent(CoreContext::VoiceTick(tick))));
                },
                () = tokio::time::sleep_until(cleanup_time) => {
                    // periodic cleanup.
                    let now = Instant::now();

                    // check ssrc map to see if the WS task has informed us of any disconnects.
                    loop {
                        // This is structured in an odd way to prevent deadlocks.
                        // while-let seemed to keep the dashmap iter() alive for block scope, rather than
                        // just the initialiser.
                        let id = {
                            if let Some(id) = self.ssrc_signalling.disconnected_users.iter().next().map(|v| *v.key()) {
                                id
                            } else {
                                break;
                            }
                        };

                        _ = self.ssrc_signalling.disconnected_users.remove(&id);
                        if let Some((_, ssrc)) = self.ssrc_signalling.user_ssrc_map.remove(&id) {
                            let _ = self.ssrc_signalling.ssrc_user_map.remove(&ssrc);

                            if let Some(state) = self.decoder_map.get_mut(&ssrc) {
                                // don't cleanup immediately: leave for later cycle
                                // this is key with reorder/jitter buffers where we may
                                // still need to decode post disconnect for ~0.2s.
                                state.prune_time = now + Duration::from_secs(1);
                                state.disconnected = true;
                            }
                        }
                    }

                    let counts = std::mem::take(&mut self.dave_counts);
                    if counts.dropped() > 0 {
                        warn!(?counts, "DAVE frames dropped since the last cleanup");
                    } else if counts.decrypted + counts.unmarked > 0 {
                        info!(?counts, "DAVE frames since the last cleanup");
                    }

                    // now remove all dead ssrcs.
                    self.decoder_map.retain(|_, v| v.prune_time > now);

                    cleanup_time = now + Duration::from_secs(5);
                },
            }
        }
    }

    async fn process_udp_message(&mut self, interconnect: &Interconnect, mut packet: BytesMut) {
        // NOTE: errors here (and in general for UDP) are not fatal to the connection.
        // Panics should be avoided due to adversarial nature of rx'd packets,
        // but correct handling should not prompt a reconnect.
        //
        // For simplicity, if the event task fails then we nominate the mixing thread
        // to rebuild their context etc. (hence, the `let _ =` statements.), as it will
        // try to make contact every 20ms.
        let crypto_mode = self.crypto_mode;

        match demux::demux_mut(packet.as_mut()) {
            DemuxedMut::Rtp(mut rtp) => {
                if !rtp_valid(&rtp.to_immutable()) {
                    error!("Illegal RTP message received.");
                    return;
                }

                let packet_data = if self.config.decode_mode.should_decrypt() {
                    let out = self.cipher.decrypt_rtp_in_place(&mut rtp).map(|(s, t)| {
                        if rtp.get_padding() != 0 {
                            let payload = rtp.payload();
                            let payload_length = payload.len();
                            let padding_count = payload[payload_length - t - 1] as usize;

                            (s, t + padding_count, true)
                        } else {
                            (s, t, true)
                        }
                    });

                    if let Err(ref e) = out {
                        warn!("RTP decryption failed: {:?}", e);
                    }

                    out.ok()
                } else {
                    None
                };

                let mut dave_shrink = 0;
                if let Some((rtp_body_start, rtp_body_tail, decrypted)) = packet_data {
                    let ssrc = rtp.get_ssrc();
                    let payload = rtp.payload_mut();
                    let payload_length = payload.len();
                    let body = &mut payload[rtp_body_start..payload_length - rtp_body_tail];
                    let body_length = body.len();

                    // If the packet is transport-decrypted, DAVE is enabled, and the packet
                    // looks encrypted: https://daveprotocol.com/#protocol-frame-check
                    // The packet must be at least 11 bytes consisting of:
                    // - 8 byte truncated AES128-GCM authentication tag
                    // - 1 byte protocol supplemental data size
                    // - 2 byte magic marker
                    if decrypted
                        && self.dave_protocol_version.load(Ordering::Relaxed) != 0
                        && body_length >= 11
                        && body[body_length - DAVE_MAGIC_MARKER.len()..body_length]
                            == DAVE_MAGIC_MARKER
                    {
                        // Silently drop encrypted packets if it's not possible to decrypt them for
                        // one reason or another; otherwise there'd be error logs for trying to decode
                        // Opus
                        let Some(user_id) = self.ssrc_signalling.ssrc_user_map.get(&ssrc) else {
                            self.dave_counts.no_user += 1;
                            return;
                        };
                        let Some(ref mut dave_session) = *self.dave_session.write().unwrap() else {
                            self.dave_counts.no_session += 1;
                            return;
                        };

                        if !dave_session.is_ready() {
                            self.dave_counts.not_ready += 1;
                            return;
                        }

                        let result = dave_session.decrypt(user_id.0, davey::MediaType::AUDIO, body);

                        match result {
                            Ok(decrypted_body) => {
                                self.dave_counts.decrypted += 1;
                                // Theseus's fix (vendor/songbird/VENDORED.md): the packet is made
                                // self-consistent here, as a packet that never had DAVE, because
                                // playout finds the body's end again from the packet's length.
                                dave_shrink = reseat_plaintext(
                                    payload,
                                    rtp_body_start,
                                    rtp_body_tail,
                                    &decrypted_body,
                                );
                            },
                            Err(davey::errors::DecryptError::NoDecryptorForUser) => {
                                // Silently drop encrypted packets for users whose ratchets are not configured yet.
                                self.dave_counts.no_decryptor += 1;
                                return;
                            },
                            Err(davey::errors::DecryptError::DecryptionFailed(
                                davey::errors::DecryptorDecryptError::NoValidCryptorFound {
                                    ..
                                },
                            )) => {
                                self.dave_counts.no_cryptor += 1;
                                return;
                            },
                            Err(e) => {
                                self.dave_counts.failed += 1;
                                error!(error = ?e, "DAVE decryption failed");
                                return;
                            },
                        }
                    } else if decrypted && self.dave_protocol_version.load(Ordering::Relaxed) != 0 {
                        // No DAVE frame (no magic marker): handed to the decoder as it came.
                        self.dave_counts.unmarked += 1;
                    }
                }

                let rtp = rtp.to_immutable();
                let (rtp_body_start, rtp_body_tail, decrypted) = packet_data.unwrap_or_else(|| {
                    (
                        crypto_mode.payload_prefix_len(),
                        crypto_mode.payload_suffix_len(),
                        false,
                    )
                });

                let entry = self
                    .decoder_map
                    .entry(rtp.get_ssrc())
                    .or_insert_with(|| SsrcState::new(&rtp, crypto_mode, &self.config));

                // Only do this on RTP, rather than RTCP -- this pins decoder state liveness
                // to *speech* rather than just presence.
                entry.refresh_timer(self.config.decode_state_timeout.into());

                // What DAVE's decryption freed: the packet ends where the moved tail now ends.
                packet.truncate(packet.len() - dave_shrink);

                let store_pkt = StoredPacket {
                    packet: packet.freeze(),
                    decrypted,
                };
                let packet = store_pkt.packet.clone();
                entry.store_packet(store_pkt, &self.config);

                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::RtpPacket(InternalRtpPacket {
                        packet,
                        payload_offset: rtp_body_start,
                        payload_end_pad: rtp_body_tail,
                    }),
                )));
            },
            DemuxedMut::Rtcp(mut rtcp) => {
                let packet_data = if self.config.decode_mode.should_decrypt() {
                    let out = self.cipher.decrypt_rtcp_in_place(&mut rtcp);

                    if let Err(ref e) = out {
                        warn!("RTCP decryption failed: {:?}", e);
                    }

                    out.ok()
                } else {
                    None
                };

                let (start, tail) = packet_data.unwrap_or_else(|| {
                    (
                        crypto_mode.payload_prefix_len(),
                        crypto_mode.payload_suffix_len(),
                    )
                });

                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::RtcpPacket(InternalRtcpPacket {
                        packet: packet.freeze(),
                        payload_offset: start,
                        payload_end_pad: tail,
                    }),
                )));
            },
            DemuxedMut::FailedParse(t) => {
                warn!("Failed to parse message of type {:?}.", t);
            },
            DemuxedMut::TooSmall => {
                warn!("Illegal UDP packet from voice server.");
            },
        }
    }
}

/// Puts a DAVE-decrypted frame back into its packet (Theseus's fix, vendor/songbird/VENDORED.md).
///
/// `payload` is a transport-decrypted RTP payload: `body_start` bytes of header extension, the
/// DAVE frame, then `body_tail` bytes (any RTP padding, the transport's tag and nonce). The frame's
/// `plaintext` is shorter than the frame, so it is written over the frame's start and the tail is
/// moved up behind it. Returns by how many bytes the packet must then shrink: once it has, the
/// payload is laid out exactly as a packet that never had DAVE, which is what playout assumes when
/// it finds the end of the Opus from the packet's length. Upstream 0.6.1 rewrote the plaintext but
/// kept the old length, and the decoder got the Opus followed by DAVE's leftover trailer.
pub(crate) fn reseat_plaintext(
    payload: &mut [u8],
    body_start: usize,
    body_tail: usize,
    plaintext: &[u8],
) -> usize {
    let body_end = payload.len() - body_tail;
    let shrink = (body_end - body_start).saturating_sub(plaintext.len());
    let plain_end = body_start + plaintext.len();
    payload[body_start..plain_end].copy_from_slice(plaintext);
    payload.copy_within(body_end.., plain_end);
    shrink
}

/// Theseus's check of [`reseat_plaintext`] (crates/theseus-voice's `dave_receive` test): one
/// received packet, as transport decryption left it, whose DAVE frame decrypted to `plaintext`,
/// through the same steps as the receive task (reseat, truncate, store, play out). Returns the
/// samples playout decoded, or the decode's error.
#[doc(hidden)]
pub fn decode_dave_received(
    packet: &[u8],
    crypto_mode: CryptoMode,
    plaintext: &[u8],
) -> Result<Vec<i16>, String> {
    use crate::driver::{Channels, DecodeConfig, SampleRate};
    use discortp::rtp::MutableRtpPacket;
    use std::num::NonZeroU8;

    let mut packet = BytesMut::from(packet);
    let shrink = {
        let mut rtp = MutableRtpPacket::new(&mut packet[..]).ok_or("not an RTP packet")?;
        let body_start = if rtp.get_extension() != 0 {
            let ext = discortp::rtp::RtpExtensionPacket::new(rtp.payload())
                .ok_or("extension indicated, but no room for it")?;
            ext.packet().len() - ext.payload().len()
        } else {
            crypto_mode.payload_prefix_len()
        };
        reseat_plaintext(
            rtp.payload_mut(),
            body_start,
            crypto_mode.payload_suffix_len(),
            plaintext,
        )
    };
    packet.truncate(packet.len() - shrink);

    let config = Config::default()
        .decode_mode(DecodeMode::Decode(DecodeConfig::new(
            Channels::Mono,
            SampleRate::Hz48000,
        )))
        .playout_buffer_length(NonZeroU8::MIN)
        .playout_spike_length(0);
    let packet = packet.freeze();
    let rtp = RtpPacket::new(&packet).ok_or("not an RTP packet")?;
    let mut state = SsrcState::new(&rtp, crypto_mode, &config);
    state.store_packet(
        StoredPacket {
            packet: packet.clone(),
            decrypted: true,
        },
        &config,
    );
    match state.get_voice_tick(&config) {
        Ok(Some(data)) => data.decoded_voice.ok_or_else(|| "no audio decoded".into()),
        Ok(None) => Err("the playout buffer held the packet back".into()),
        Err(e) => Err(format!("{e:?}")),
    }
}

#[instrument(skip(interconnect, rx, cipher))]
pub(crate) async fn runner(
    mut interconnect: Interconnect,
    rx: Receiver<UdpRxMessage>,
    cipher: Cipher,
    crypto_mode: CryptoMode,
    config: Config,
    udp_socket: UdpSocket,
    ssrc_signalling: Arc<SsrcTracker>,
    dave_session: Arc<RwLock<Option<davey::DaveSession>>>,
    dave_protocol_version: Arc<AtomicU16>,
) {
    trace!("UDP receive handle started.");

    let mut state = UdpRx {
        cipher,
        crypto_mode,
        decoder_map: HashMap::new(),
        config,
        rx,
        ssrc_signalling,
        udp_socket,
        dave_session,
        dave_protocol_version,
        dave_counts: DaveCounts::default(),
    };

    state.run(&mut interconnect).await;

    trace!("UDP receive handle stopped.");
}

#[inline]
fn rtp_valid(packet: &RtpPacket<'_>) -> bool {
    packet.get_version() == RTP_VERSION && packet.get_payload_type() == RTP_PROFILE_TYPE
}
