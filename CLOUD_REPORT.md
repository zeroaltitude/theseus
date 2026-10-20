# Cloud report: voice-aec (theseus-yaxcp)

Branch `cloud/20261019-voice-aec`, base `74a797ed`. Feature commit `5203daa`.

## Step: Theseus cancels its own voice from what it hears

**Found.** The engine already held both signals: each tick's listener frames, and every clip it played. No seam extension was
needed for samples. One was needed for timing: `songbird_io` puts 100 ms of silence before the first clip of a spell, so
`VoiceIo::lead_in()` (default 0) now says it. Echo cancellation also exposed a model bug of mine, fixed in the same commit: a clip
begun in the tick its predecessor ended in (sentence 2 after sentence 1) replaced the predecessor's last samples, leaving a hole in
the far-end timeline and a 850-RMS leakage frame at every sentence join. `echo.rs` now keeps a list of clips and sums them.

**Changed (`5203daa`).**
- `src/aec.rs` (new, pure DSP, no dependency): 8 kHz; delay by correlating 10 ms energy envelopes (lags 40-500 ms, last 1 s,
  re-estimated every 100 ms, a better delay needs +0.15 correlation twice running); NLMS of 384 taps (48 ms, 20 ms before the
  delay) on pre-emphasised audio, regularised, frozen while the frame is >2.5x the expected echo; a frame is left raw if
  subtraction would add energy; idle (`None`, nothing computed) after 1 s of Theseus silence, delay and taps kept for the next reply.
- `src/echo.rs` (new): the engine's far-end timeline (clip start placed to within the tick via `Instant`), one `Aec` per listener,
  journal lines `voice: the echo's delay` and `voice: an echo spell ended` (mean ERLE).
- `engine.rs`: the field, the `Config::echo_cancel` field, calls at `Out::Play`/`Out::Stop`, and the barge-in's speech test reads
  the residual. VAD, hold, transcript rules and the join guard read the raw frame, unchanged.
- `Config::echo_cancel` (off in the engine, like `join_guard`); `[voice] echo_cancel` (default true) in
  `theseus-core/src/voice.rs`, the example template (commented line, parse test passes) and `theseus-discord/src/runtime/voice.rs`.
- Test stand-ins: `Audio::babble` (a voice with an envelope; a tone has none to read a delay from) and `WavIo::echo` (delay,
  attenuation, one-pole low-pass; cut at the played part when a clip is stopped).
- `theseus-voice/AGENTS.md` updated.

**Proof.**
- `cargo fmt --all --check` ok; `scripts/shape.sh` ok; `cargo clippy -p theseus-voice -p theseus-discord --all-targets -D warnings` clean.
- `cargo nextest run -p theseus-voice -p theseus-discord`: 699 run; the 2 failures (`tests_task_threads::a_task_in_a_guild_channel_gets_a_thread_...`
  and `...in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing`, each ~101 s) are not mine: they fail 2 of 3 runs on the unmodified base
  too (stashed and rerun) and pass alone with my change.
- `cargo nextest run -p theseus-core -E 'test(voice)|test(example_template)|test(template)'`: 56 passed.
- New tests: `tests/aec.rs` (5, virtual time), `aec::tests` (6), `echo::tests` (2).
  - KEY: reply of two ~4 s sentences, owner's speakers return it 180 ms late, -12 dB, low-passed. On: no cut, both sentences play to the end.
    Plus an owner voice from 6.0 s: the reply stops at 6.28-6.32 s (assert window 280-320 ms).
  - Off: the events and play log equal a golden (`tests/aec_off.golden`) recorded from the unmodified engine (diff identical).
  - Silence from Theseus: no residual, and events equal with the switch on or off; `Aec::step` returns `None`.
  - aec unit: delay within +-10 ms at 100/180/300 ms; change 120 -> 260 ms tracked within 2 s; echo alone >=15 dB down after convergence
    (about 15-18 dB in the engine run); near-end voice within 3 dB of what was said.
- Cost, release-thin build, 60 s synthetic audio: **119 us per 20 ms frame per listener** (goal < 0.2 ms). Debug build: 1.87 ms.
  Print: `cargo test --profile release-thin -p theseus-voice --lib the_cost -- --nocapture`.

**Planted reverts** (restored, touched, `git status` clean after each):
1. KEY. `aec.rs` `step`: `let samples = self.cancel(&mic);` -> `let samples = mic.to_vec();`. Fails `tests/aec.rs::the_echo_alone_never_stops_the_reply`
   (`assert!(!call.cut())`, aec.rs:117) and `a_voice_over_the_echo_stops_it_within_300_ms_and_a_frame`; unit `the_echo_alone_comes_out_15_db_down...`: "came out 0.0 dB down".
2. Frozen taps. `aec.rs` `cancel`: `if !near {` -> `if !near && false {`. Fails `the_echo_alone_comes_out_15_db_down_after_convergence` ("0.0 dB down").
3. Settle window. `const SETTLE: u32 = 25;` -> `0`. Fails the voice test: "the voice began at 6s and the reply stopped at 6.18s" (convergence frames count toward the cumulative barge count).
4. Switch. `engine.rs`: `Echo::new(config.echo_cancel)` -> `Echo::new(true)`. Fails `with_it_off_the_echo_alone_stops_the_reply_as_it_did` and `with_it_off_the_call_is_as_it_was`.
5. Clip join. `echo.rs` `play`: add `self.sent.clear();` before the push. Fails `a_clip_begun_as_the_last_one_ends_shares_its_tick` ("both are on their way", left 1).
Not planted: the `[voice] echo_cancel` default (its assert is in `voice::tests::a_voice_section_reads_and_each_key_is_optional`), and the near-end DTD freeze.

**Live check (maintainer).** On laptop speakers, a long reply, `[voice] echo_cancel = true` (default): stay silent for the first half
(expect no barge-in), then say "wait, stop" mid-sentence (expect the reply to stop within about 300 ms). Repeat with `echo_cancel = false` and count false
barge-ins and missed interrupts. Journal lines (daemon log at info, `theseus_voice`): `voice: the echo's delay` (`delay_ms`; none or absent = the
client's own canceller already removed the echo, or no echo found: the engine then judges the raw frame, as before) and
`voice: an echo spell ended` (`mean_erle_db`; expect >10 dB when an echo was cancelled), plus `voice.barge_in` events as before.

**Uncertain / owner should hear.**
- Warm-up: until the delay is found (800 ms of Theseus's speech, first reply of a call only; later replies start locked) and for 500 ms after,
  the residual is not counted, so a barge-in in the first ~1 s of the first reply is not stopped by the 300 ms rule (the utterance still goes through the transcript path).
  With headphones (no echo, never locks) the raw frame counts after 800 ms.
- The barge count is cumulative per utterance (existing behaviour), so leaked echo frames add up; hence the settle window.
- Synthetic echoes only: real rooms add nonlinear speaker distortion, Opus coding, jitter-buffer delay changes, and a client canceller already running. Untested live.
- The brief's "echo-return-loss" figure is `Residual::erle_db` (enhancement, mic energy vs residual).
- Playout latency inside songbird is unmodelled beyond the lead-in; the delay estimate and the 20 ms pre-delay taps absorb it.
- Docs to update at review: spec Part III (new item, voice layer 3), `docs/status.md`, the design doc for voice; mention `[voice] echo_cancel`.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (CARGO_INCREMENTAL=0 after the first run hit "No space left on device" in clippy; I cleared target caches): door, keel (0 findings), fmt, shape, features, clippy passed; **suite failed**: 6987 of 7023 passed, 36 failed:
- 34 L1/root-VM failures, known: `theseus-sandbox::contract` (20 tests), `bench spawn_100`, `theseusd::sandbox` (13).
- `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`, `theseus::herdr a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile`,
  `theseus::herdr_up a_finished_tab_closes_a_grace_after_it_is_seen_...`: load-sensitive timing in code I did not touch; all three pass rerun alone.
The gate stopped at the suite; I did not run `machine_checks` after it. `cargo deny --offline check` reports advisories FAILED (bans, licenses, sources ok); I added no dependency (Cargo.lock unchanged), so I take it to be the offline advisory database, but did not confirm.
