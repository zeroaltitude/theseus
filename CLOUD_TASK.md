<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2j-flux`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `3bac00ab` (release 1.0.1 plus the patch2 joins so far; the owner's installed
build). Line numbers below are at `3bac00ab` and may be off by a few: search by name. Where the release preamble above
disagrees with this note about the base, ceilings or the work in flight, this note wins.

**Store format at your base: 50** (`const MANIFEST_FORMAT: u32 = 50;` in `crates/theseus-store/src/store.rs`, near
:685). This row should need no store change. If you find you must bump it, STOP that part and say so in your report's
first section.

**Line ceilings.** None of the files this row is expected to touch is in `scripts/long-files.txt`, so each caps at
2,500 lines. At the base: `crates/theseus-voice/src/engine.rs` 1887, `src/deepgram.rs` 454, `src/speech.rs` 254,
`src/vad.rs` 299, `crates/theseus-core/src/voice.rs` 610, `crates/theseus-discord/src/runtime/voice.rs` 1901. Put new
code in new files (for example `crates/theseus-voice/src/flux.rs` and `src/listen.rs`) so `engine.rs` and the runtime's
`voice.rs` grow by little. Never raise a ceiling.

**Your lane:** `integ/p2-voice`, new. This row is its only row. Other patch2 lanes do not touch `theseus-voice` or the
runtime's voice files as far as we know; name every file you touch in the join notes.

## Your task: p2j-flux, Deepgram Flux as the call's listener (theseus-v774k)

Branch: `cloud/20261024-p2j-flux`. Every commit subject carries `theseus-v774k`.

The owner wants Deepgram **Flux** as the listener on voice calls. Flux is a streaming speech-to-text model whose own
model decides when a speaker's turn ends, so it replaces our silence timer, whose pauses chop the owner's sentences
(today: `[voice] pause_ms` 1200, `merge_ms` 1500). Aura-2 stays the voice: synthesis does not change. Nova-3 plus our
pause rules stay as the fallback, chosen by config, and **the default stays nova-3**. The owner flips the default only
after a live A/B. This row holds the 1.0.2 tag: do it well, keep it switchable, keep the nova-3 path byte for byte as
it is.

## Read first

1. `crates/theseus-voice/AGENTS.md` in full: barge-in holds then decides, the echo rules, echo cancellation
   (theseus-yaxcp, `src/echo.rs`, `src/aec.rs`), the unclear-transcript judge (theseus-s3dwj, `src/unclear.rs`), the
   join guard, the merge. Then the repo's root `AGENTS.md` and `crates/theseus-discord/AGENTS.md` for the gate.
2. `crates/theseus-voice/src/engine.rs`: the module doc (top 45 lines), `Engine::tick` (~:751: VAD per speaker, the
   residual-based barge count, `Closed::Utterance` -> `transcribe`), `transcribe` (~:934), `hold` (~:974), `resume`,
   `commit`, `heard`/`judged`/`settle` (~:1391-1567).
3. `crates/theseus-voice/src/deepgram.rs`: today's REST client. `listen` (~:212) posts one WAV per utterance to
   `/v1/listen` with `smart_format=true`. The key rides only in a sensitive header and is scrubbed from every error.
4. `crates/theseus-voice/src/speech.rs` (the `Speech` trait, `StandInSpeech`), `tests/deepgram.rs` (a local TCP
   stand-in for Deepgram), `tests/turns.rs` and `tests/judged.rs` (the engine in virtual time).
5. `crates/theseus-core/src/voice.rs` (`VoiceConfig`, its checks ~:190-230, `stt_model` default "nova-3"),
   `crates/theseus-core/config/theseus.example.toml` (~:950-990, the `[voice]` block), and
   `crates/theseus-discord/src/runtime/voice.rs`: `engine_config` and `settings` (~:900-930), `Metered` (~:1300-1360,
   spend checks per call), `Event::BargeIn` (~:1043, the ledger row `LedgerKind::VoiceBargeIn`).
6. **Deepgram's Flux docs, from this session** (you may fetch them: developers.deepgram.com, the Flux pages and the
   v2 listen API reference). Read for: the exact model id (we believe `flux-general-en`), the endpoint (we believe
   `wss://api.deepgram.com/v2/listen`), the audio formats and sample rates it takes, the events (we believe
   `StartOfTurn`, `Update`, `EagerEndOfTurn`, `TurnResumed`, `EndOfTurn` in a `TurnInfo` message, plus `Connected`
   and errors), the end-of-turn knobs (we believe `eot_threshold`, `eager_eot_threshold`, `eot_timeout_ms`),
   keep-alive and close messages, idle timeouts, word confidences, formatting options, languages, and pricing. Trust
   the docs over our beliefs, and cite the page for each fact in the report.

## Your row

### 1. The Flux client (new `crates/theseus-voice/src/flux.rs`)
- One streaming socket per listed speaker for the whole call, opened at that speaker's first audio, closed cleanly at
  the call's end (the docs' close message). Audio goes as the engine has it (16-bit mono; resample only if Flux
  refuses 48 kHz, and say so). The key rides only in the handshake's `Authorization` header (or the docs' way), never
  in a URL, a log line, a `Debug`, or an error: reuse `deepgram.rs`'s scrub. One helper for both, not two.
- WebSocket crate: one already in `Cargo.lock` (`tokio-tungstenite` or `tokio-websockets`), with rustls on ring as the
  workspace does. No new TLS stack. `cargo deny check` must stay green; if it needs a `deny.toml` change, name it.
- The client turns Flux's messages into a small event type of our own (for example `Heard::Start`, `Heard::Words`,
  `Heard::EagerEnd`, `Heard::Resumed`, `Heard::End { text, confidence, audio }`). Confidence for the judge is the
  words' confidence (the mean, or the docs' turn-level one if it is a recognition confidence), never the
  end-of-turn probability. Say which in the report.
- A seam the engine owns: a `Listen` trait (or an extension of `Speech`) that the engine calls to push frames and
  poll events, so the engine stays testable in virtual time. A `StandInListen` scripted with fixture events at given
  ticks, like `StandInSpeech`'s fixtures.

### 2. The engine on the Flux path (`[voice] stt_model` names a Flux model)
- An utterance opens at the speaker's `StartOfTurn` and closes at their `EndOfTurn` with Flux's transcript. No
  separate transcription call, no `pause_ms` close, no `merge_ms` wait: Flux decides the end of a turn. From the
  `Pending` onward the engine is unchanged: `heard.rs`'s rules, the judge, `Utterance`, turns, coalescing.
- Eager end of turn: when `[voice] eager_eot_threshold` is set (unset by default), an `EagerEndOfTurn` may start the
  turn early, and a `TurnResumed` before its reply plays supersedes it, as the merge's `goes_on` does today (no audio
  of the superseded reply ever plays). `EndOfTurn` confirms it. Unset: only `EndOfTurn` starts a turn.
- The VAD keeps running on the Flux path only where the engine still needs it: the residual speech count that gates
  barge-in (below) and the floor. Say in the report which VAD uses remain.
- **The nova-3 path is untouched.** With `stt_model = "nova-3"` (the default) every existing test passes unchanged,
  and `tests/aec.rs`'s golden stays byte-identical. Prove it: no existing test's expected value changes.

### 3. Barge-in on `StartOfTurn`, with echo removal and the judge in front
- On the Flux path, a listed speaker's `StartOfTurn` while Theseus is speaking is the barge-in trigger: the engine
  calls the existing `hold` at once (stop the clip, hold the queue), in place of waiting for 300 ms of speech frames.
- **Order, which the reviewer checks:** (a) echo removal first. The `StartOfTurn` holds only if the speaker's
  echo-cancelled residual (`self.echo.tick` -> `r.speaks(...)`, as `tick` counts it today) shows speech since the turn
  began, and the speaker is not echo-prone, not a wordless sound, not at the join (the `prone` rule in `tick`). With
  `echo_cancel = false` the raw frame counts, as today. Feed Flux the raw audio (its transcripts read best on it) and
  gate on the residual; if you choose otherwise, say why. (b) Then the words decide, as today: `heard.rs`'s rules and,
  with `judge_unclear`, the judge, over the turn's transcript (`Update`/`EndOfTurn` words), choose resume (echo, cough,
  backchannel, no words) or commit the cut (`commit`, `Cut`, `BargeIn`). Theseus's own echo or a cough must never
  commit a cut. An ungated `StartOfTurn` (residual silent) does nothing to playback; its turn still goes through the
  rules as an utterance over speech.
- **Stop latency, for the A/B.** Record the time from receiving the `StartOfTurn` to the engine pushing `Out::Stop`,
  and to the seam reporting the clip ended, in the hold's journal line and in the `BargeIn` event (a new field, for
  example `stop_latency: Option<Duration>`), and carry it into the `VoiceBargeIn` ledger row's detail JSON. Also log
  each turn's first-word latency (`EndOfTurn` received -> first reply audio queued) on both paths, so the A/B compares
  like with like. No store-format change: if the ledger detail needs one, STOP that part and say so.

### 4. A long call's socket survives
- Keep-alive per the docs while a speaker is silent (Discord sends no frames for a silent speaker: send the docs'
  keep-alive message or silence, whichever the docs say keeps a v2 socket open), so the socket is not closed by an
  idle timeout during a long call.
- Reconnect without losing a turn: keep the audio of the turn in progress (since its `StartOfTurn`, bounded, for
  example 30 s, in a ring). If the socket drops or errors mid-turn, that audio goes once through the existing
  pre-recorded path (`Speech::transcribe`, nova-3 REST) as one utterance, so the turn is heard, and the socket
  reconnects with a bounded backoff for the next turn. Say this in the journal once per drop. A drop between turns
  loses nothing.
- Bounded buffers everywhere: the outbound audio queue to the socket has a fixed capacity; past it the oldest audio is
  dropped and counted (logged once per spell), never unbounded memory. Name each bound in the report.
- A socket that cannot be opened at all (bad key, network) falls back for that call to the nova-3 path and says so once
  in health/the journal, with the scrubbed error. The call never goes deaf because Flux is down.

### 5. Config (`crates/theseus-core/src/voice.rs`, the example toml)
- `stt_model = "flux-general-en"` (the docs' exact id) turns on the Flux path for calls; one helper
  `is_flux(model)` decides it, used everywhere. Optional `eot_threshold`, `eager_eot_threshold`, `eot_timeout_ms`
  under `[voice]`, each validated to the docs' ranges with clear words, unset by default (Flux's defaults).
- Flux + a language Flux does not support is refused at config load, in words that name both.
- `pause_ms` and `merge_ms` keep their checks and still apply on the nova-3 path; the example toml says they do not
  apply on Flux.
- **Voice notes and file transcripts stay pre-recorded.** `toolrun.rs` (~:2292) hands `cfg.voice.stt_model` to
  `file_read`'s `Hearing`, which posts to `/v1/listen`. A Flux id there would break voice notes. With a Flux
  `stt_model`, file and voice-note transcription keep nova-3 (one helper, for example `prerecorded_model(cfg)`), with
  `smart_format` as today. A test proves a voice note still goes to `/v1/listen` with `model=nova-3` when
  `stt_model` is Flux. The Discord voice-note transcripts are unchanged.
- Spend: Flux audio is metered like nova-3's (`Metered`, `SpeechKind::Transcribed`, the model id Flux), by the audio
  streamed, with `speech_fits` checked before a socket opens and at each turn. Price Flux in code from the docs'
  per-minute price; cite it.

### 6. Formatting
Find in the docs whether Flux formats dates, times and numbers (a smart-format or similar option). If it does, turn it
on. If it does not: do NOT add a second recogniser call per turn. Document the gap in the report for the A/B's arm 3
(Nova-3 with smart_format on the same utterances). A small local formatter is out of this row.

## Tests (red first where a behaviour changes; no live key in the cloud)

Fixtures are synthetic or recorded Flux JSON messages with invented text, under the voice crate's tests (a
`tests/fixtures/flux/` folder is fine). No key, no real person's words, nothing from a real call.
- **Client:** a local stand-in WebSocket server (as `tests/deepgram.rs` stands in for REST) that checks the handshake
  (model, encoding, sample rate, the auth header present and never in the URL) and plays a fixture sequence; the
  client yields our events in order. The key appears in no error, `Debug` or log line (as
  `the_key_is_in_no_debug_and_no_quoted_body` does).
- **Turns:** a `StartOfTurn` ... `EndOfTurn` pair with a 2 s pause inside it is one turn (red at the base in spirit:
  name the nova-3-path test that shows the same audio is two turns or a merge). Eager on: `EagerEndOfTurn` starts a
  turn, `TurnResumed` supersedes it with no audio played, `EndOfTurn` makes the whole sentence one turn.
- **Barge-in:** over Theseus's speech, a `StartOfTurn` with residual speech holds at once and its words commit the
  cut; a `StartOfTurn` whose residual is silent (Theseus's own echo) does not stop playback; a cough (a `StartOfTurn`
  whose words are none, or the judge says Other) resumes; the stop latency is in the `BargeIn` event and the ledger
  detail. Plant: remove the residual gate; the echo test must go red.
- **Long sessions:** the stand-in server drops the socket mid-turn: the turn's audio is transcribed once through the
  stand-in `Speech`, the turn happens, the socket reconnects, the next turn is heard. Keep-alive: a silent minute in
  virtual time sends keep-alives and the socket stays open. Overflow: a stalled server makes the outbound queue drop
  oldest audio within its bound. A socket that never opens: the call falls back to nova-3 and says so once.
- **Config:** default stays nova-3; a Flux id parses; bad thresholds and Flux + an unsupported language are refused;
  voice notes keep nova-3 under a Flux `stt_model`.
- **Unchanged:** every existing `theseus-voice` and `theseus-discord` voice test passes with no expected value changed;
  `tests/aec_off.golden` byte-identical.

Name each planted revert and what it turned red in the report.

## Not in this row
- Synthesis, Aura-2, personas' voices: unchanged.
- The live A/B itself (the owner runs it), and flipping the default.
- Any change to Discord DM task messages.
- Live learning (theseus-6f8t0) and other patch2 lanes.

## Rules for this row
- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
  (`git diff --exit-code 3bac00ab -- crates/theseus-discord/src/golden/dm_flow.txt`). If a change would alter any
  message of that flow, STOP that part and write `OWNER CALL: <what and why>, before/after text` in the report.
- **Keel:** no removed or weakened test, no raised budget/cap/ceiling, no edit of `scripts/keel-guard.py` or its tests,
  no renamed test. Run `python3 scripts/keel-guard.py --base 3bac00ab --head HEAD --quiet` before the report and paste
  its output.
- **Data:** voice audio and transcripts go only to Deepgram, as today. Nothing new leaves the host.
- **Public repo:** no person's name, real organisation, channel id, address or email anywhere, fixtures included;
  invented names under `example.test`; write "the owner". No API key or key-shaped string in any fixture.
- Right over compatible; one helper, not two; real behaviour, never a test special case.
- **Gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p
  theseus-voice` (with and without default features if the crate's AGENTS says so), `cargo test -p theseus-core
  voice`, the `theseus-discord` voice tests (`cargo test -p theseus-discord voice`) and the DM golden test,
  `cargo deny check`, `scripts/shape.sh`. If a cold build is too slow for the whole workspace, run the crates you
  touched and say what you skipped.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it is
not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections:
1. **For the maintainer:** store format (50, unchanged expected); any protocol or ledger-detail change; any dependency
   or `deny.toml` change; any ceiling near; any OWNER CALL.
2. **Flux facts from the docs, each with its page:** model id, endpoint, events, knobs, keep-alive and idle timeout,
   whether Flux is speech-to-text only (the owner's checklist said "Flux TTS"; say what is true), languages, pricing
   per minute, formatting support.
3. **What you built:** per section 1-6 above, with files and functions; which VAD uses remain on the Flux path; which
   confidence the judge reads; every bound.
4. **Barge-in order:** how echo removal and the judge sit in front of `StartOfTurn`, in the code's order, with the line
   references.
5. **Tests:** each test, red-first or not, the planted reverts and what each turned red.
6. **Formatting:** what Flux gives, and what arm 3 of the A/B must cover.
7. **How to turn it on:** the exact `[voice]` lines (and how to go back to nova-3).
8. **A live A/B script for the owner** on the test voice channel, under 25 lines: a scripted set of 10-15 sentences
   (long sentences with mid-sentence pauses, dates, times, phone-style numbers, a barge-in over a long reply, a cough
   over a reply, Theseus's echo on speakers); what to score per arm (chopped sentences per minute, first-word latency,
   barge-in stop latency, mis-hearings, formatted entities) and where each number is read (journal line names, ledger
   rows); arms: nova-3 (today), Flux, Flux with eager end of turn.
9. **Gate:** each command and its result. **Keel:** the output. **DM golden:** the diff result.
10. **Join notes for `integ/p2-voice`:** every file touched. **Follow-ups:** anything found and not done.
