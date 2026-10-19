# Cloud report: voice-join (theseus-yf9ct)

Branch `cloud/20261019-voice-join`. One step, commit 133e3e00.

## What I found
- The lead-in and the guard both belong where the audio is: the lead-in in `songbird_io::play` (the fake io has no
  speaking-state flip, and putting silence in the engine's `Out::Play` would move every exact time in `tests/turns.rs`).
- The guard needs the engine to know the join: it records `joined` at `Engine::new`; the greeting reaches it only as a
  `Command::Report`, so "before the greeting finished" is read as: before the first turn, and (within `JOIN_GUARD` of the
  join, or a report/clip is queued or playing).

## What I changed (133e3e00)
1. **Lead-in** (`songbird_io.rs`): `LEAD_IN_FRAMES = 5` (0 = off). `Spell` marks the first clip after quiet (no clip
   playing and the last ended at least 500 ms ago, `SPELL_GAP`); that clip is prefixed with 100 ms of silence. The next
   sentence gets none. A stopped clip's late `Ended` is ignored.
2. **Join guard** (`engine.rs`, `heard.rs`): new `HeardAs::Join`. An utterance that opens at the join (rule above) and is
   under 1.5 s (`JOIN_SPEECH`) or has under 3 words is `Join`: no turn, journal line
   "voice: a sound at the join was not taken as speech: <ms> ms, '<transcript>'".
3. **Fragment echo** (`heard.rs::is_fragment_echo`): 1-2 words that fuzzily match the head or tail of a sentence heard in
   the tail or over speech (clipped end of 4+ letters, or one edit per 4 letters) are `Echo`. A lone exact word,
   backchannels and "go on" are excluded ("stop" over "Stop the build." still cuts). `is_echo` and its tests are untouched.
4. **Switch**: `[voice] join_guard` (default true) in `theseus-core/src/voice.rs`, in the template, set on the engine's
   `Config::join_guard` at the join in `theseus-discord/src/runtime/voice.rs`. The engine's own default is **off**, so
   all 116 existing voice tests are unchanged and show the old behaviour byte for byte. With the switch off:
   `tests/join.rs::with_the_guard_off_a_sound_at_the_join_is_a_turn_as_before` and the last assertion of
   `..._echo_and_a_yes_is_not`. The lead-in is not behind the switch; its constant 0 turns it off.
5. AGENTS.md of theseus-voice and theseus-discord updated.

## Proof
- `cargo nextest run -p theseus-voice`: 120/120. `-p theseus-discord -E 'test(/voice/)'`: 70/70.
  `-p theseus-core -E 'test(/example_template|voice/)'`: 21/21 (the template parses with the new key).
- `cargo fmt --all --check` clean; `bash scripts/shape.sh` ok; `cargo clippy -p theseus-voice -p theseus-discord
  --all-targets -- -D warnings` clean; `keel-guard.py --base origin/cloud/20261019-voice-join --head HEAD --quiet` empty.
- Tests: `tests/join.rs` (key: `a_short_sound_before_the_greeting_is_no_turn_and_the_greeting_plays`, the long clear
  utterance, the guard off, the one-word echo and "yes"), `heard.rs` unit tests, `songbird_io` test
  `the_first_clip_after_quiet_leads_in_and_the_next_sentence_does_not`.
- Planted reverts (each restored, `touch`ed, `git status` clean):
  - KEY, `engine.rs` `let at_join = self.config.join_guard` -> `let at_join = false && self.config.join_guard`:
    `join::a_short_sound_before_the_greeting_is_no_turn_and_the_greeting_plays` failed at join.rs:102
    (`left: 1, right: 0`, a turn started).
  - `engine.rs` `&& is_fragment_echo(...)` -> `&& false && is_fragment_echo(...)`:
    `join::the_greetings_head_heard_back_in_the_tail_is_an_echo_and_a_yes_is_not` failed (`left: [Words]`).
  - `songbird_io.rs` `lead && LEAD_IN_FRAMES > 0` -> `false && lead`: the songbird_io lead-in test failed.

## Live check for the maintainer
Install, `[voice] enabled` with the owner's key, join a voice channel on speakers, say nothing:
the greeting plays whole ("Theseus here"), no "I didn't catch..." answer, and the journal shows
"voice: a sound at the join was not taken as speech" if the chime came back. To compare, set `[voice] join_guard = false`.

## Not run / uncertain
- Not run: the full gate, benches, cargo-deny, `npm`, L1 tests (as the task said). No live audio here, so the 100 ms
  lead-in's effect on the clipped head is unmeasured; 5 frames is the brief's starting value.
- Design choices to hear: the guard window cannot see a greeting that has not been queued yet and is past 1.5 s after
  the join (the greeting waits up to 3 s); an utterance opening in that gap is not guarded. A lone exact word never
  counts as a fragment echo. A tail "here" alone is therefore a turn.
- Doc to change at review: spec Part III item and status.md for yf9ct.
