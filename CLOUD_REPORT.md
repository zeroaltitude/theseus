# CLOUD_REPORT: p3b-flux-follow (theseus-82rbm)

## 1. For the maintainer
Store format 51, untouched. No ceiling raised. No OWNER CALL, no STOP. Three commits on top of the task commit.

## 2. Item 1: `Event::Streamed` booking test (87ac4bc3)
Found: no Discord-side test. Added `trailing_flux_audio_is_booked_as_a_trailing_row` in `voice/tests_flux.rs`. It sends one `Event::Streamed` (30 s of flux-general-en) through `pump` and asserts: exactly one `speech.transcribed` row; `trailing: true`; the speaker; `latency_ms` 0; `seconds` 30; model; `cost_usd` equal to `micros_to_usd(SpeechCall::cost())` for that usage (derived, not hard-coded).
- Green: `cargo nextest run -p theseus-discord trailing_flux`: 1 passed.
- P1 (the Streamed arm's `book` call wrapped in `if false`): FAIL after 10 s, "the booking came". Restored from a copy, touched, `git status` clean of voice.rs.

## 3. Item 2: dead supersede branch (157ca988)
`start_turn` (engine.rs) begins with `if self.turn.is_some() { return; }`, and nothing between that and the assignment sets `self.turn`, so `turn.replace(..)` always returned `None` and the `first_words.remove(&old.id)` never ran. Replaced with `self.turn = Some(Turn { .. })`; deleted the branch and its false comment. Gave `an_eager_turn_superseded_by_the_whole_sentence_leaves_no_entry` a doc comment saying it proves the empty-reply removal of the `first_words` entry (`speakable`); name and assertions unchanged.
No red-first possible (dead code). Finding on the early return: none; I did not change it. The engine's turns, judged, flux, flux_socket and aec tests pass untouched.

## 4. Item 3: literal 20 (e79a6bdc)
Tick-meaning 20s, now derived from `FRAME` through a new `ticks_in` in flux_path.rs: `silence_ticks` (was :137), `TURN_MAX_TICKS` (:63), the ring's first-tick computation (:660), and `tick_at` in `tests/flux.rs`. No literal 20 meaning something else was touched. New test `a_silence_is_counted_in_frames` (`engine/tests_ticks.rs`) for 0, 19 ms, 20 ms, 1 s. It guards the derivation; it passes at the base value, and I did not plant a changed `FRAME`.

## 5. Gate
- `cargo fmt --check`: ok. `cargo clippy -p theseus-voice -p theseus-discord --all-targets -- -D warnings`: clean.
- `TZ=America/Phoenix cargo nextest run -p theseus-voice -p theseus-discord`: 843 run, 843 passed.
- `scripts/shape.sh`: ok.
- I did not run the full `scripts/gate.sh` (suite, deny, cockpit) here; only the row's gate commands above.

## 6. Keel
`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`: no output, exit 0.

## 7. DM golden
`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt`: no diff, exit 0.

## 8. Join notes
Files: `theseus-discord/src/runtime/voice/tests_flux.rs`; `theseus-voice/src/engine.rs` (the assignment, one `mod tests_ticks` line), `engine/flux_path.rs`, `engine/tests_leaks.rs`, `engine/tests_ticks.rs` (new), `tests/flux.rs`. `voice.rs` is untouched, so `FRAMING` is too.

## 9. Follow-ups
None.
