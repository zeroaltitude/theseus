# songbird, vendored

songbird 0.6.1 as published on crates.io (ISC licence, the serenity-rs project), patched in through the workspace's
`[patch.crates-io]`. Unchanged except:

- **`Cargo.toml`**: the `authors` line is removed (the repository names no person). Dev-dependencies stay listed;
  cargo ignores them for a dependency outside the workspace.
- **`src/driver/tasks/udp_rx/mod.rs`**, the receive fix (theseus-voice-dave). With DAVE, the receive task decrypts the
  transport, then DAVE's frame, and wrote the shorter Opus over the frame's start while the packet kept its old
  length. The playout buffer stores only the packet's bytes, and `ssrc_state.rs` finds the Opus's end from the
  packet's length less the transport's suffix, so the decoder got the Opus followed by DAVE's leftover trailer
  (8-byte tag, nonce, supplemental size, magic marker): `opus_decode` failed with `InvalidPacket`, or decoded the
  wrong audio, for every frame, and voice heard nothing. `reseat_plaintext` now moves the transport's tail up behind
  the plaintext and the packet is truncated before it is stored, so it is laid out as a packet that never had DAVE.
  Upstream's closed pull request serenity-rs/songbird#314 describes the same defect.
- **`src/driver/tasks/udp_rx/mod.rs`** and **`src/driver/mod.rs`**: `decode_dave_received`, `#[doc(hidden)]`, runs
  one synthesized packet through the same steps (reseat, truncate, store, play out) for
  `crates/theseus-voice/tests/dave_receive.rs`.
- **`src/driver/tasks/udp_rx/mod.rs`**: `DaveCounts`. Upstream drops a DAVE frame it can't decrypt without a word
  (no speaking event mapped its SSRC to a user yet, no session, the session not ready, no decryptor for the user, no
  key for the frame's generation). The receive task now counts each outcome, and the frames that came with no DAVE
  marker while DAVE is on (handed to the decoder as they came), and at its 5-second cleanup logs the counts: a
  `warn` when any frame was dropped, an `info` otherwise, so a call that hears nothing says why.
- **`src/driver/tasks/udp_rx/ssrc_state.rs`**: a failed decode's log line carries the Opus's first byte (its TOC) and
  length, to tell a stray trailer or a ciphertext from a bad frame. No audio is logged.
- **`src/driver/tasks/ws.rs`**: an upgrade from transport-only encryption set DAVE's passthrough with
  `set_passthrough_mode(true, Some(10))`. davey ignores the expiry when passthrough is turned on, so unencrypted frames
  stayed accepted for the rest of the call; `(false, Some(10))` accepts them for 10 s more and then requires
  encryption, as the upgrade intends. This never handed ciphertext to the decoder: passthrough copies only frames
  that don't parse as DAVE frames.
- **`src/driver/scheduler/idle.rs`**: `#[allow(unused_imports)]` on an import only songbird's own tests use: a path
  dependency's warnings show in every build, where a registry crate's are capped.

Drop the vendored copy, and the `[patch.crates-io]` entry, once a songbird release carries the receive fix.
