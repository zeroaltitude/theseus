# Cloud report: tail4-voice, a reply's later sentence keeps its first word (theseus-9d9aq)

Branch `cloud/20261021-tail4-voice`, base `8260dbab`. One step, one commit: `a537b24` (`voice: keep the stream warm between a reply's sentences, and lead in after a cold gap`).

## First things to know

- **No file over its ceiling; no ceiling raised.** `songbird_io.rs` is 1,123 lines (it was 1,074; the row's cap is
  1,300). The new logic is in `crates/theseus-voice/src/warm.rs` (337 lines, mostly tests).
- **Existing spell tests whose assertions changed** (the rule changed: `Spell::begin` now returns the silent frames
  to put before the clip, and takes whether the warm silence plays; `Spell::new` takes a `LeadIn`):

  | test | old | new |
  |---|---|---|
  | `the_first_clip_after_quiet_leads_in_and_the_next_sentence_does_not` | `assert!(spell.begin(ClipId(0), t0))` | `assert_eq!(spell.begin(ClipId(0), t0, false), 5)` |
  | same | `assert!(!spell.begin(ClipId(1), t0 + ms(1210)), "the next sentence")` | `assert_eq!(spell.begin(ClipId(1), t0 + ms(1210), true), 0)` ("under the warm silence") |
  | same | `assert!(spell.begin(ClipId(2), t0 + ms(2600)))` / `ClipId(3)` | `assert_eq!(…, true), 5)` / `assert_eq!(…, false), 5)` |
  | `a_spell_leads_in_with_the_configured_silence` | `assert!(spell.begin(ClipId(0), now))`; `assert!(!off.begin(..))` | `assert_eq!(…, false), 20)`; `assert_eq!(off.begin(.., false), 0)` |

  Every number the old tests held (5 frames for 100 ms, 20 for 400, 21 for 410, none when off, the led clip's bytes)
  is still asserted.
- **Config test**: `the_pause_the_lead_in_and_the_merge_have_defaults_and_ranges` now asserts `lead_in_ms` 600 where it
  asserted 400. That is the requested default change, not a weakened test; it gained the new key's default (200), its
  range ends (0 and 1,000 read back) and its refusal (1,001).
- **`SongbirdIo::join` and `attach` take `theseus_voice::LeadIn { spell, gap }`** instead of one `Duration`. Callers
  changed: the Discord `join` call (the one call, written with a full path so the file's imports are untouched) and the
  crate's own example `examples/join.rs` (it did not compile otherwise; it now passes 600/200).
- **docs/status.md is not edited** (the brief's general rules forbid it; the row's table names it). Suggested sentence
  for the voice entry near :87: "`[voice] lead_in_ms` (600 ms, was 400), and between a reply's sentences the seam keeps
  sending silence (a 700 ms warm track after each clip), with `[voice] gap_lead_in_ms` (200 ms) before a sentence that
  found nothing warm (theseus-9d9aq)."
- **Signatures**: `git commit -S` signed with this VM's configured key, which is an **ssh** key (`gpg.format = ssh`), not a GPG key; it is no keel-signers key (none is needed: nothing here needs an ack).
- **The disk**: this VM's writable allowance is about 40 GB. A debug workspace build with all test targets and
  incremental data filled it (twice: `target/debug/deps` 17 GB plus incremental). I removed `target/` (inside the
  repository) and ran everything with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.

## What I found: songbird when no track plays

Read in `vendor/songbird/src/driver/tasks/mixer/mod.rs` (`mix_and_build_packet`, `mix_tracks`) and
`vendor/songbird/src/driver/scheduler/live.rs` (`demote_and_remove_mixers`):

- `mix_tracks` mixes every *playing* track; a track that plays all-zero PCM still returns `MixType::MixedPcm(n > 0)`,
  so an all-zero track is encoded and sent as audio, the same as speech, and resets songbird's trailing-silence count.
- When nothing mixed (`MixedPcm(0)`), songbird sends `silence_frames` (5) Opus silence frames (`SILENT_FRAME`), one per
  20 ms tick, and then `return Ok(0)`: **nothing is sent at all** (no RTP packet) until a track mixes again.
- A mixer with no tracks and no silence frames left is demoted out of the live scheduler; the next track promotes it.
- A new track is not playable at once: `get_or_ready_input` prepares it on songbird's thread pool
  (`InputReadyingError::Waiting` until then), so even back to back clips have ticks where the new track mixes nothing.
- A paused track is still prepared (`should_check_input` is true while `Preparing`) and keeps the mixer live, but
  mixes nothing.
- `send_gateway_speaking` is sent when the websocket changes, not per spell; the listener-side "speaking" is the RTP
  stream itself starting and stopping.

So between two sentences 300 ms apart, songbird sent 100 ms of silence frames and then nothing for ~200 ms plus the next
clip's preparation; that is the drop the owner heard, and it widens under load.

## The warm path, and why this shape

`crates/theseus-voice/src/warm.rs`:

- Each clip is played with a second track beside it, **added paused** (`Track::new(..).pause()`, in the same lock as
  the clip): 700 ms (`WARM`, 35 frames) of zeros. It is prepared while the clip plays, so it is ready the moment it is
  wanted.
- The clip's own end handler (`Ended`, songbird's event task) first calls `AfterClip::ended`, which plays that warm
  track (`TrackHandle::play`, a channel send, no lock) and marks `Warmth` (an `AtomicU64`: the clip whose warm plays),
  **then** sends `Heard::Ended` to the engine, so the engine's next `play` reads the warmth as on.
- The warm track's own end handler (`WarmEnded`) only clears the warmth for its clip (`compare_exchange`, so a later
  clip's warmth is kept); it holds no channel and the engine never hears it.
- 700 ms is past `SPELL_GAP` (500 ms): the spell reads a clip's end when the engine hears it, after songbird played it,
  so a clip begun just inside the gap still starts under warm silence. After 700 ms the warm track ends and songbird
  goes quiet as before (its 5 silence frames, then nothing).
- A `stop` (barge-in) stops every kept warm track first (the last 4; an older one has played out), then the clip, and
  replaces the warmth, so the stopped clip's late `End` finds its warm track gone (`play` fails) and starts nothing.
- Why not a looped silent track for the whole spell: songbird loops by seeking the input, which an in-memory raw input
  may not support, and stopping it needs a timer `SPELL_GAP` after the last end; the per-clip paused tail needs no
  timer and no seek, and starts from songbird's own end event. Why not a silent tail appended to each clip: the clip's
  `Ended` (and so the engine's turn-taking and the echo canceller's timing) would move by the tail.
- The engine, its turn-taking and the echo canceller are untouched. `VoiceIo::lead_in` still reports the silence put
  *in the clip* before its first sample; warm silence is a separate track of zeros the listener hears as silence, which
  adds nothing to the echo reference.

### The lead-in rules (`Spell::begin`)

| the clip starts | silence before it | `VoiceIo::lead_in` |
|---|---|---|
| a spell: first clip, or ≥ 500 ms after the last clip ended | `[voice] lead_in_ms` (600 ms, 30 frames) | 30 × 960 samples |
| inside the gap (< 500 ms), warm silence playing | none (the warm track is the lead-in) | 0 |
| inside the gap, nothing warm (warm failed or was stopped) | `[voice] gap_lead_in_ms` (200 ms, 10 frames) | 10 × 960 samples |
| while a clip still plays (the engine never does this) | none | 0 |

### Config

`crates/theseus-core/src/voice.rs`: `lead_in_ms` default 400 → 600; new `gap_lead_in_ms`, default 200, 0 to 1,000
(refused above, "the silence before a sentence after a cold gap is 0 to 1000 ms (200 is the default; 0 is off)"), 0 is
off. The template (`config/theseus.example.toml`) has the new key and the new default. Not stored: store format 49,
unchanged.

## Tests and planted reverts

| test | where | plant | what failed |
|---|---|---|---|
| `a_later_sentence_after_a_short_gap_is_kept_warm_and_led_in` (the key test) | `warm.rs` | today's rule: inside the gap, 0 frames whatever the warmth | `assert!(warm_sent + (sent.len() - chime.len()) > 0, "warm false: something before the clip's first sample")` at warm.rs:263 |
| `a_spell_after_quiet_still_gets_the_full_lead_in` | `warm.rs` | every clip gets the short lead-in (`_ => self.gap_lead_in`) | `assert_eq!(spell.begin(ClipId(0), t0, false), 30)`: left 10, right 30 |
| `a_stop_ends_the_warm_silence` | `warm.rs` | `Warm::stop` drops the tracks without stopping them | `assert!(tracks[2..].iter().all(|t| !t.playing()), "nothing plays on")` |
| `lead_in_reports_the_silence_that_led_the_clip` | `songbird_io.rs` (a real songbird `Call`, no connection, so no warm track plays: the gap's lead-in shows; then the warmth set as the end handler sets it) | `self.lead_in = 0` | `assert_eq!(io.lead_in(), 30 * FRAME_SAMPLES, "a spell's first clip")`: left 0, right 28800 |
| config | `voice.rs` | (the default change itself; see above) | |

Each plant: the file copied, the edit made, the one test run (`cargo nextest run -p theseus-voice --lib -E 'test(..)'`),
the file put back and `touch`ed; `git status` after showed only the step's own changes.

Runs:
- `cargo nextest run -p theseus-voice --all-targets`: 152 passed, 0 failed (with the `voice` feature, the default).
- `TZ=America/Phoenix cargo nextest run -p theseus-core --lib -E 'test(voice) | test(template) | test(example) | test(sparse)'`:
  71 passed.
- `cargo build -p theseus-discord`: clean.
- `cargo clippy -p theseus-voice -p theseus-discord --all-targets -- -D warnings` and
  `cargo clippy -p theseus-core --lib --tests -- -D warnings`: clean. `cargo fmt --check`: clean.
- No run under load: these tests are pure or use no clock edge (the seam test's times are milliseconds apart against a
  500 ms gap).

## The gate

`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`
on the step's tree (the debug-info variables only so the build fits this VM's disk):

- door, keel, keel tests, fmt, shape, features, clippy (and its probe), cockpit (lint, test, build), test build, reader rule: **pass**.
- suite: 7,823 run, **7,788 passed, 35 failed**, 53 skipped (447 s). The gate stops there (`gate: FAILED in suite`).
  - 34 are the known L1-as-root failures (theseus-pv6i): theseus-sandbox's `bench spawn_100` and its 19 `contract`
    tests, and 14 of theseusd's `sandbox` tests.
  - 1 is not on that list: `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`
    (`gone_jobs.rs:341`, "the live wrapper's job runs on: {dispatched: 2, …}", left 2, right 1: a timing count under the
    suite's load). Rerun alone three times (`cargo nextest run -p theseusd --test gone_jobs`): 3 of 3 passed. It is not
    in this row's code (no voice path); it is not on the flaky list, so the maintainer may want an issue for it.
  - `tests_output::the_cores_output_matches_its_golden` passed under `TZ=America/Phoenix`; no golden line moved.
- The phases after the suite, run by hand: protocol types **ok** (`cockpit/src/protocol.gen` unchanged); turn bench
  (`target/debug/theseus-sim bench turn --check --runs 5 --burst 0`) **ok**; size **skipped** (no release build);
  `cargo deny --offline check` **ok** (advisories, bans, licences, sources; the database fetched this session). The
  lifecycle and jobs benches are skipped by `THESEUS_GATE_NO_BENCH=1`.

So the commit counts green by the brief's rule.

## Keel guard

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` names 34 erosions, all in commits `origin/main`
gained after this branch's base (other lanes' joins); none in this branch's files. Against the base this branch was cut
from, `python3 scripts/keel-guard.py --base 8260dba --head HEAD --quiet` prints nothing (exit 0).

## Live check (the owner's, after install)

On a call, ask something whose answer is three or four sentences, ideally while the machine is busy (a build running).
Listen for: every sentence's first word whole; between sentences a quiet that does not click or cut (the warm silence
is zeros); the greeting's head still whole (600 ms now: a slightly later first sound). Barge in during a pause between
sentences: Theseus should stop at once and say nothing more. The journal's lines are unchanged: the engine and the
echo canceller see the same clips and ends as before.

## What is left or uncertain

- Not proven against a live Discord call here (no keys). The key uncertainty is songbird's event latency: the clip's
  `End` handler runs on songbird's event task, and the warm track's `play` reaches the mixer a tick or two later; songbird's
  own 5 silence frames (100 ms) cover that gap.
- A new clip's preparation on songbird's thread pool still leaves ticks where only the warm track mixes; that is the
  point of the warm track, but a spell's *first* clip has no warm track before it (its lead-in is inside the clip, so
  it is unaffected).
- The echo canceller is not told about warm silence beyond `lead_in`. It needs nothing more: warm silence is zeros.
  The warm silence does not change when the engine thinks a clip ended.
- `WARM` (700 ms) is a constant, not a key. If the owner hears a drop between sentences more than ~700 ms apart (slow
  synthesis), a key `[voice] warm_ms` would be the next lever.
