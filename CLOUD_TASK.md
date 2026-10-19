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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-voice-join`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: voice-join, the greeting plays whole and a sound at the join is not a turn (theseus-yf9ct)

Branch: `cloud/20261019-voice-join`. Every commit's subject carries `theseus-yf9ct`. Deadline for the report: 5 hours
after you start.

**Why.** The owner's live voice test on 2026-10-19 (installed main, a Discord voice channel, the owner on laptop
speakers, not headphones, and saying nothing at the join) showed two faults, both also seen the night before:

1. **The greeting's head is clipped.** "Theseus here" is heard as "seus here": the first few hundred milliseconds of
   the first clip after the bot starts speaking never reach the listener.
2. **A sound at the join is answered as speech.** At every join, 0.3 to 1.4 s of audio arrives from the owner's SSRC
   *before the greeting has started*, is closed as an utterance, transcribed ("ECCR", "Is miss here") and run as a
   turn; the spoken answer ("I didn't catch ECCR…") then talks over the call. The likeliest source is Discord's own
   join chime (played on the owner's speakers when the bot enters) coming back through the owner's microphone.

**What the record shows** (the daemon's journal, call 1; times in seconds after the minute):
- 13.22 the bot joins; 13.28 a speaking event maps SSRC 4057 to the owner; 13.36 "the first audio from a listed
  speaker" (tick 3); 14.80 "the call's audio flows (10 packets in a row)"; 15.40 "the call's first utterance closed,
  sent to be transcribed, ms=1360".
- 16.50 SpeakingState(MICROPHONE), 16.54 the first Playable fires (the greeting, 1.2 s), 17.70 SpeakingState(0x0).
- 20.8 a 0.5 s clip; 26.6-31.9 the reply to "ECCR".
- Call 2 the same: a 320 ms utterance at 41.5-42.5, the greeting at 43.3.
- In the window: 52 "Decode error for SSRC …: Other" and 49 "RTCP decryption failed" warnings from songbird (noise,
  not the fault, but note them).

## What exists (read these; lines move)
- `crates/theseus-voice/src/engine.rs`: the engine's tick loop, utterance closing, reports, barge-in, the slow-turn
  acknowledgment. `crates/theseus-voice/src/vad.rs`: voice activity. `crates/theseus-voice/src/heard.rs`: `classify`,
  `is_echo` (an echo needs a run of `ECHO_RUN` = 3 words; `head_run` for the first echo), `is_backchannel`.
- `crates/theseus-voice/src/songbird_io.rs`: the songbird side (tracks, `Input`, speaking events, SSRC map).
- `crates/theseus-discord/src/runtime/voice.rs`: the join (`begin`, `greet`: the greeting waits for `flowing` or
  `GREETING_WAIT` = 3 s, then `Command::Report`), settings, status. Its tests under `runtime/voice/` (`tests_*.rs`).
- `crates/theseus-voice/AGENTS.md` and `crates/theseus-discord/AGENTS.md` (the voice paragraphs).

## What to build
1. **A lead-in of silence before the first audio after each speaking start.** When the bot goes from silent to
   speaking (the first clip of a reply, a report or the greeting, after the call was quiet), send a short run of
   silent 20 ms Opus frames (start at 5 frames, 100 ms; one constant, named, with a comment on why) before the clip, so
   the listener's jitter buffer and Discord's speaking-state flip don't eat the head. Back-to-back sentences of one
   reply get no extra lead-in. Find the simplest place (a silent prefix in the first `Input`, or songbird's own way to
   send silence frames); measure nothing you can't test here, but test that the first clip after quiet carries the
   lead-in and a following sentence does not.
2. **A join guard.** Audio from a listener that starts before the greeting has finished playing (or, with no greeting,
   within a named `JOIN_GUARD` of about 1.5 s after the join) is not a turn when it is short (under ~1.5 s) or its
   transcript is not clearly speech: it is heard as `Noise`/ignored and logged once ("voice: a sound at the join was
   not taken as speech: <ms> ms, '<transcript>'"). A long, clear utterance in that window still counts (the owner may
   really talk at once). Keep it in `heard.rs`/`engine.rs`'s existing classify path, one new `HeardAs` reason if needed.
3. **Echo of a sentence's head, one word.** Today `is_echo` needs 3 words, so a one- or two-word echo of the greeting's
   head ("…seus here") is a turn. When the bot's own sentence played within the echo tail and the heard text is one or
   two words that match its head or tail fuzzily (a phonetic or edit-distance match on the sentence's words; no new
   dependency), classify it `Echo`. Keep the existing rules and their tests unchanged.
4. **Rollback:** one config switch, `[voice] join_guard = true` (default true) for 2 and 3 together; the lead-in has
   its own constant (0 turns it off) — say in the report which tests show the old behaviour byte for byte with the
   switch off. Add the key to the config template with a one-line comment.

Out of scope here: a Jev judgment on short transcripts (a later row), Deepgram endpointing, the RTCP/decode warnings.

## Tests (each with a planted revert; the first is the key test)
- KEY: an engine test on the fake io: the bot joins, a 600 ms burst from the listener arrives before the greeting
  plays and its transcript is "ECCR": no turn starts, the greeting plays, the log line is written. Plant: the guard's
  condition made false -> the test fails because a turn started.
- A long clear utterance (2 s, "what's the weather like today") in the same window does start a turn.
- The lead-in: the first clip after quiet begins with the silent frames; the second sentence of the same reply does not.
- The one-word echo: the greeting "Theseus here." played; "seus here" heard in the tail -> `Echo`; "yes" in the tail
  is still a backchannel/answer as today.
- `join_guard = false` restores today's behaviour (the key scenario starts a turn).

## After the join (the maintainer's, not yours)
The maintainer installs it and the owner re-tests on speakers: the greeting whole, no answer to the join chime.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-voice -p theseus-discord --all-targets -- -D warnings`.
- `cargo nextest run -p theseus-voice` and `cargo nextest run -p theseus-discord -E 'test(/voice/)'` (scope runs to these
  crates: the VM is 4 cores with no sccache); `python3 scripts/keel-guard.py --base <your base> --head HEAD --quiet` empty.
- Not here: the full gate, the benches, cargo-deny. Say so in the report.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

