//! A DAVE-encrypted voice packet, received (theseus-voice-dave): songbird
//! decrypts the transport, then DAVE's frame, and its playout decodes the Opus.
//! Upstream songbird 0.6.1 wrote the decrypted Opus over the frame's start and
//! kept the packet's old length, so playout handed the decoder the Opus
//! followed by DAVE's leftover trailer: every frame failed with
//! `InvalidPacket`, or decoded to the wrong audio, and voice heard nothing.
//! The vendored songbird (vendor/songbird/VENDORED.md) fixes it; these drive
//! its receive steps on a synthesized packet.
#![cfg(feature = "voice")]

use songbird::driver::opus::{Application, Channels, Decoder, Encoder};
use songbird::driver::{decode_dave_received, CryptoMode};

/// 20 ms at 48 kHz, mono: what Discord sends and songbird decodes to.
const FRAME: usize = 960;

/// A real Opus frame: the third of a 220 Hz tone with an overtone, so the
/// encoder is past its start-up.
fn opus_frame() -> Vec<u8> {
    let mut enc = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
    let mut out = vec![0u8; 1275];
    let mut len = 0;
    for f in 0..3 {
        let pcm: Vec<i16> = (0..FRAME)
            .map(|i| {
                let t = (f * FRAME + i) as f32 / 48_000.0;
                let s = (t * 220.0 * std::f32::consts::TAU).sin() * 0.5
                    + (t * 660.0 * std::f32::consts::TAU).sin() * 0.2;
                (s * 12_000.0) as i16
            })
            .collect();
        len = enc.encode(&pcm, &mut out).unwrap();
    }
    out.truncate(len);
    out
}

/// What a fresh decoder makes of `opus`: songbird's playout decodes each
/// speaker's first packet with a fresh decoder too.
fn reference(opus: &[u8]) -> Vec<i16> {
    let mut dec = Decoder::new(48_000, Channels::Mono).unwrap();
    let mut pcm = vec![0i16; FRAME * 6];
    let n = dec.decode(opus, &mut pcm, false).unwrap();
    pcm.truncate(n);
    pcm
}

/// DAVE's trailer on an Opus frame (daveprotocol.com, "Frame format"): the
/// 8-byte truncated tag, the nonce (ULEB128), no unencrypted ranges (Opus is
/// encrypted whole), the supplemental size, and the magic marker.
fn dave_trailer() -> Vec<u8> {
    let mut t = vec![0x5a; 8];
    t.push(0x07);
    let size = t.len() as u8 + 3;
    t.push(size);
    t.extend_from_slice(&[0xfa, 0xfa]);
    t
}

/// A received packet as transport decryption leaves it in place: the RTP
/// header, then (with `extension`) the header extension, the DAVE frame (its
/// ciphertext stood in by `opus` inverted), and AES-GCM's tag and nonce.
fn packet(opus: &[u8], extension: bool) -> Vec<u8> {
    let mut p = vec![if extension { 0x90 } else { 0x80 }, 0x78];
    p.extend_from_slice(&7u16.to_be_bytes());
    p.extend_from_slice(&96_000u32.to_be_bytes());
    p.extend_from_slice(&6423u32.to_be_bytes());
    if extension {
        // One 32-bit word of one-byte extensions: id 1, one byte, padding.
        p.extend_from_slice(&[0xbe, 0xde, 0x00, 0x01, 0x10, 0x2a, 0x00, 0x00]);
    }
    p.extend(opus.iter().map(|b| !b));
    p.extend(dave_trailer());
    p.extend_from_slice(&[0xc3; 16 + 4]);
    p
}

/// The fix: the decrypted frame plays out as the Opus it is, sample for
/// sample, with the header extension and without.
#[test]
fn a_dave_decrypted_frame_decodes_to_its_audio() {
    let opus = opus_frame();
    let want = reference(&opus);
    assert_eq!(want.len(), FRAME);
    for extension in [true, false] {
        let got = decode_dave_received(&packet(&opus, extension), CryptoMode::Aes256Gcm, &opus)
            .unwrap_or_else(|e| panic!("extension {extension}: {e}"));
        assert_eq!(got.len(), want.len(), "extension {extension}");
        assert!(got == want, "extension {extension}: the audio differs");
    }
}

/// What upstream handed the decoder, the Opus with DAVE's trailer behind it,
/// does not decode to the Opus's audio: the bug this guards.
#[test]
fn the_opus_with_the_trailer_behind_it_is_not_the_audio() {
    let opus = opus_frame();
    let want = reference(&opus);
    let mut stale = opus;
    stale.extend(dave_trailer());
    let mut dec = Decoder::new(48_000, Channels::Mono).unwrap();
    let mut pcm = vec![0i16; FRAME * 6];
    match dec.decode(&stale, &mut pcm, false) {
        Ok(n) => {
            eprintln!("Opus with the trailer: decoded {n} samples");
            assert!(pcm[..n] != want[..], "the trailer changed nothing");
        }
        Err(e) => eprintln!("Opus with the trailer: {e:?}"),
    }
}
