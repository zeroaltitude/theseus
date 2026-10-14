# CLOUD REPORT: voice-status (theseus-arp9l)

Branch `cloud/20261013-voice-status`. Code commit: 5e112aae.

## Step: the voice channel's status and the board's line

**Found.**
- The binding's own `work.watch` is guarded: it needs `roots` and sees shared places only. The call's tasks are the owner's, so counts through it would be wrong. I used the in-process `core.bus.watch_work` with the whole-board scope (interactive false) and kept only a map of open tasks and questions. The status holds counts only. Titles stay in memory for the spoken line, and that line goes only to a private place. **Owner should hear this choice.**
- twilight has no typed route for `voice-status`, and the brief's "courier raw HTTP path" does not exist. The write is a plain `reqwest` PUT as the bot (token from the secret board, base from `rest_proxy`).
- The config section is `[voice]`, not `[discord.voice]`, so the key is `[voice] status` (default true). A channel overrides it with `voice_status` in its `[[channel]]`. This is in the template and in `bindings.example.toml`. The key turns off both the status and the spoken line.
- A blocking task (the push applier) stops tokio's paused clock from auto-advancing, so the paced tests move time with `advance`.

**Changed.**
- New `runtime/voice_status.rs`: `Tally`, `Pacer`, `Put`/`Rest`, `Handle`.
- `voice.rs`: calls only, plus the `Call.status` field.
- `runtime.rs`: one `mod` line.
- `tests_status.rs`.
- `fake_discord`: the PUT route, `refuse_voice_status`, `voice_statuses()`.
- `bindings.rs`.
- `VoiceConfig.status`.
- tokio `test-util` as a dev-dependency feature (no new package; Cargo.lock unchanged).
- AGENTS.md.
- No store or protocol change.

**Behaviour.**
- Status: `🎧 Theseus · ⚙️ 3 tasks · 🔴 1 needs you`. "Tasks" are `WorkKind::Task` views that work. "Needs you" is open `WorkKind::Question` views.
- Pace: the change window is 2 s from the first change since the last write. At most one write a minute. The last state is always written. Nothing is written while counts are unchanged.
- Strict one-per-minute means a "needs you" arriving within 60 s of a write waits for the minute. The owner may want a bypass for needs-you rises.
- Cleared at `/leave`, on a drop, and at the daemon's stop. The stop-clear is best effort: the shutdown does not wait for it.
- A 403 is put in health's voice `last_error` once, and that call never writes again.
- Spoken line: `[The board: 3 tasks working; 1 needs you: Beta summary asks fs.write …]`. It is cut at 200 characters, built from memory, and only in a private place. It is about 54 tokens at most (chars/4). I did not record voice first-audio time before and after: no voice stand-in was run. The line is a mutex read plus a string build.

**Proof.**
- `cargo test -p theseus-discord voice`: 44 passed.
- `cargo nextest run` on theseus-discord and theseus-sim: 339 passed, 1 skipped.
- theseus-core filtered (template, registry, voice and config tests): 130 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --check` and `scripts/shape.sh`: ok.
- New tests:
  - The pace test (paused clock) covers join sets the status, a change within the debounce, two changes inside a minute writing once then the last state, and leave clearing.
  - A test for a 403 told once, with no retry and no clear.
  - A test for the stop clearing.
  - A fake-Discord REST test.
  - A private place's voice turn carries the line; a shared place's turn does not, and never names the title.
  - Pure tests for `Tally`, `Pacer` and the status text.
- Planted reverts, restored and touched afterwards:
  - Title appended to the status: the pace test and the REST test failed.
  - Debounce and gap removed from `Pacer::due`: `the_pacer_debounces_waits_a_minute_and_writes_the_last_state` failed.

**Live check.**
- Not done. I did not run `theseus-sim discord rig`; the fake-REST test is the stand-in proof.
- Maintainer rig check: run the rig with a voice join on the fake, then three tasks and one asking, and read `FakeDiscord::voice_statuses()` (also in the log as `PUT /channels/<id>/voice-status`, outcome `voice_status`). Expect `🎧 Theseus`, then a status with `3 tasks · 🔴 1 needs you`. After `/leave` the last write is `""`.
- Owner's check, in `#theseus-test`'s voice channel:
  1. Give the bot Set Voice Channel Status.
  2. `/join`.
  3. Start a task that asks for approval.
  4. Within about a minute the channel status shows the counts.
  5. Ask "what's going on?". The answer should be one sentence naming the task.
  6. `/leave` clears the status.
  7. Without the permission, health shows one voice error and no retries.

**Left or uncertain.**
- The voice first-audio measurement.
- The live rig run.
- Whether to bypass the minute for needs-you.
- Docs to update at review: the spec's Part III item, and a line in `docs/status.md`.

## Gate
- `scripts/gate.sh` fails in the door and keel phases, before any build, on a finding I did not make. It comes from commit 1c2beb2747: "budget-raised crates/theseus-sim/src/lifecycle.rs", whose ack signature does not verify here (`%G? is E`). A second keel message names a renamed store test (`a_checkpoint_for_close_is_made_durable_by_the_close`), also not mine.
- **Keel findings expected:** none from this branch.
- I ran the later phases by hand: fmt, shape, clippy and the targeted suites above. I did not run the full suite, the benches or `cargo deny` (not run: `cargo deny fetch` was not completed by the setup chain I checked).
- No flaky timing tests were hit.
