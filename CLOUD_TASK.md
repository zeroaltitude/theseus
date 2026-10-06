<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n*). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report. Batch 9's sessions (your siblings, below) are fixing the ones marked *; batch 8's timing-flakes, which joins main about now, fixes those marked †: if your clone holds its commits (their subjects name theseus-cs71, ynia, 1n2y and qjd6), those four don't fail.
  - theseus-core: the output golden's 30 s wait for "wake due" under CPU starvation (theseus-23wh*);
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb*);
    `tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`'s 3 s bound (theseus-vbju*);
    `tests_route`'s verdicts that come `late` under starvation (theseus-biy3*); `tests_lsp_edits::the_block_adds_no_frame`
    (theseus-xx6w*); `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia†: typed-ahead input can
    land on the prompt line; it fails alone too, about one run in two on this VM) and
    `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y†); `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is`
    (theseus-qjd6†: the exporter's retry counted as a fourth trace);
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs*);
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `job_approval::a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too` (theseus-cs71†: a 1.5 s wall
    bound on the cancel's round trip).
- A negative assertion ("nothing of X reached Y", "no process is left") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: theseusd's stop on a SIGTERM or a SIGINT, theseus-xbtr*, and a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at 4a449460 or later, with store format 22. Your clone has, besides v1's milestones:
- from the last three days: route.v1, the live rerank, security.v3's notices, replay, the ladder (`pack.mode`, `mode_for`, `pack_arm`), the Linux lanes, the refusal fallback; files (every surface accepts any file); speed (streamed Discord replies, a warm Jev connection, a confidence bar per routing mode); memory (retention, activation, consolidation into cited `Synthesis` nodes, tiering's stubs); the task board and the cockpit's tabs; the learning loop and `theseus judge prove`; the kernel's verified tree kills and caught nested locks; kernel-sim's crashes; the store's cut back to the last good sync and synced-only durability; one serialization per notification; `proc.run`'s steps; health's `web:`, 1-hour cache and disk lines; telemetry's error types, and each tool call counted once at its answer;
- **since batch 8 launched (60b43fb6):**
  - route's gaps: a routed session keeps the base it was moved from and follows a change of that base (the live profile, a place's bound profile, a pane's `-P`), and each loop records one `context.compiled` and one `loop.started` (store format 21);
  - situations: a turn's situation is a compiler input, and a check after each compile fails a turn whose context admits what its situation doesn't, enforcing from day one; the precedence line after the persona; testimony headers (the place, a reply's model, a summary's positions); volatile values shown "as of <date>, unverified" (store format 22);
  - the reader rule's tool-marker and same-name closures, and `theseus-index --version`;
  - memory: the adjacency projection's warm build waits between pages while the machine is busy and never past a clean stop (`startup::stop_has_begun`); tests of retention's shadow rule, activation's additions and a stub's kind; a synthesis's leading heading set aside before consolidation's checks and kept off its node, and a cluster rejected for its form proposed again, once;
  - bench/: the sampler's CPU counted once; both async arms measured end to end (the Theseus record from its ledger, the Claude Code arm on `MeasuredClaudeCode`); the recall bench's bulk reads sized by the compiler's rule, plain abstentions admitted, `--stale retracted` as an option, compactions counted by outcome.

**Other changes in flight.** About thirty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Batch 8's, merging into `main` one by one before any batch-9 branch (each reviewed on the owner's machine):
- timing-flakes and telemetry3 (accepted, joining next): four timing tests fixed at their causes (core term/tests.rs, telemetry/tests.rs; theseusd tests/job_approval.rs); the `theseus.cancel` metric, the index tender's gauges and restarts, and two resumed-span tests (core telemetry/, cancel.rs, tender/; one call in theseusd main.rs's `after_serving`);
- turn-stack, queue-frames, smalls: the turn's future boxed at `TurnRunner::run`; a late result's wake in the turn's end frame and a completion's `execution.queued` row (turn.rs, turn/end_step.rs, push.rs, the kernel's kernel.rs, both goldens, kernel-sim); the secret board's settle race, musl's `time_t`, the TUI's message order, a budget question's `loop.ended` (secrets.rs, wake.rs, fact/turn.rs, theseus-tui, theseusd main.rs);
- wal-mark-skip, crash-hold, durability-on: a start skips the WAL directory's sync when a mark vouches (theseus-store wal.rs, store.rs); a crashed call's held reservation booked as spent (theseus-kernel earlier.rs, kernel.rs; theseus-sim's fake model); the durability sessions list only their prefix, and health's durability line in every surface (core aws/durable*, the CLI's render/aws.rs, the cockpit's Systems cards);
- cli-tests, history-pages: health's 1-hour words, `judge prove`'s bytes, `watch`'s last line (crates/theseus cmd.rs and its goldens); `after` and `before` on the ledger and history reads, and a node's short id (the protocol, rpc/methods.rs, rpc/pages.rs, theseus-store's index, the CLI's history);
- learning-fixes: replay's yes-or-no rightness, the audit off the low thread, the prove's learned versions (learning/, rpc/judge_prove.rs);
- discord-live: the bindings file read live, and the courier's maps bounded (theseus-discord);
- soul-import, still being built: `theseus import`, imported sessions with their provenance, erase by tag, and recall's provenance (core import/, node.rs's bodies, recall.rs, rpc/, the index's tender and extract); it bumps the store format.

These are other cloud sessions like you, batch 9, each on its own branch:
- core-waits: four core tests that fail by the machine's speed or clock, fixed at their causes;
- route-tests: route's tests proof against load, a second switch's base, a routed turn's model in its metrics, a switched turn's recall drops;
- daemon-proofs: a stdio daemon's SIGINT stop, the store's page property test, a cancel row in the gate's jobs bench, the MCP sandbox's daemon watch;
- gate-tests: the gate's layers held through the gate (an edit's language-server start, an extension's floor, a batch's floor step, a ceiling's MCP tools, explain's L3 row);
- judge-tests: a reservation's sentences, a slow Jev at the compile point, the memory pass's links in a rerank, inbound and compile judgments' workload class;
- judge-reads: `judge.list`, the notices' brake and the learning ledger read what the answer needs, not all of history;
- judge-turn-cost: a judge-on turn bench, and the ladder's first read off the turn path;
- memory-tests: a search's own build unpaced, a search during a warm build answering at once, a recalled source read by position, the summary's profile and reservation;
- telemetry-tests: the daemon's telemetry path and the index gauges' changes held by tests;
- scrub-escaped: the scrubber matches a secret printed JSON-escaped;
- approvals-batch: a declined call ends its batch's waits, and a repeated tool-use id no longer reads as answered;
- bench-recall-plan: the recall bench's retraction rule and its plan's room for overhead;
- bench-hygiene: the async record's missing spend and calls, and bench tests that leave nothing behind and hold under load.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (22 on main today; soul-import takes the next at its merge), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,727) and crates/theseus-discord/src/render.rs (3,001); near it, crates/theseus/src/render.rs (3,098 of 3,100), crates/theseus-core/src/turn.rs (3,461 of 3,523), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-kernel/src/kernel.rs (3,008 of 3,030), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-discord/src/runtime.rs (3,453 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,799 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's toolrun.rs (2,494) and telemetry/tests.rs (2,484), theseus-sim's kernel_sim.rs (2,451), theseus-kernel's tests.rs (2,381) and theseus-store's wal.rs (2,352). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-voice-turns`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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

---
## Your task: a barge-in that pauses until words decide, so a laugh, a cough, an echo or a "yeah" no longer throws a reply away; and replies that wait for the speaker (theseus-9ln5; also theseus-kpa7)

Branch: `cloud/20261006-voice-turns`. Every commit's subject carries the id of the issue it builds. Deadline for the
report: 5 hours after you start.

**Background.** The voice engine (crates/theseus-voice/src/engine.rs) stops a reply when a listed speaker talks over
it for 300 ms, and `barge_in` then clears the whole queue: the rest of the reply and anything queued behind it (another
turn's reply, a report, the acknowledgment). The maintainer's first long call (4.6 minutes, 22 utterances, 13 replies)
went badly at exactly that point:
- **7 barge-ins, 5 of them set off by sounds with no words.** Each of the 5 overlapping utterances (0.36 to 1.82 s: a
  laugh, a cough, talk off-mic) came back from speech to text as an empty transcript, so it never became a turn; but it
  had already cut the reply, and everything queued was gone. 3 whole replies were never voiced, and of 166 sentences
  composed, 40 were heard whole.
- **A 0.28 s "Yeah." over a reply** was too short to stop it, became a turn of its own (with a tool call), and its
  reply queued behind the long one, to be thrown away at the next cut.
- **A reply began while the speaker was still mid-sentence**: `play_next` starts a clip whenever its audio is ready.
  Only the acknowledgment and reports wait for quiet (`advance`).
- **A thought split by a pause was answered as a fragment.** "Can you make yourself a" closed after 700 ms of silence
  and went out as a turn; "tool to order" began 0.71 s after its last word; the reply to the fragment ("your message
  cut off…") was spoken after the speaker had already finished the sentence.
- **In an earlier call, Theseus's own sentence came back through the speaker's microphone**, was transcribed, stopped
  the reply and was answered as a user turn.

This task makes a barge-in a pause until the overlapping utterance's words decide it, and makes replies wait for the
floor. It also emits, as engine events, the record of what was heard: a sibling task after this one (voice-heard)
carries that record into the next turn, writes its ledger rows, and shapes replies for speech.

**Read first:** the root AGENTS.md (FAST, EXQUISITE VISIBILITY, the reader rule) and crates/theseus-voice/AGENTS.md;
engine.rs whole (the module doc, `Config`, `Event`, `Utterance`, `Item`, `run`, `tick`, `transcribe`, `barge_in`,
`ended`, `done`, `reply`, `enqueue`, `advance`, `start_turn`, `synthesize_next`, `play_next`); vad.rs (`Closed`,
`is_open`, the pre-roll); speech.rs (`StandInSpeech`: each speaker's fixtures in order; an empty fixture is a sound with
no words; with no fixture it answers `[utterance N s]`, which counts as words); io.rs (`WavIo`, `Played`: a stopped
clip's `Ended` still comes); tests/pipeline.rs (`call`, `fixture`, the barge-in and report tests);
crates/theseus-discord/src/runtime/voice.rs: `pump`'s exhaustive match and the tests' `heard()`, the only places
outside the crate that must change; docs/design/m7-surface.md §2.8 (Receive and Send).

**What to build,** each a green commit:
1. **Hold, then decide** (theseus-9ln5).
   - **The stop keeps its trigger**: a listed speaker's 300 ms of speech while a clip plays stops it, as now. It no
     longer clears the queue: the queue is **held**. While held, no clip starts and no new synthesis starts (one in
     flight finishes and its audio is kept). Note the cut: the item that was playing and how long it had played. A
     stopped clip's `Ended` must not pop its item, as `ended` does today: a held item keeps its place and its audio,
     with its clip cleared so it can play again.
   - **The decision**, when an overlapping utterance closes and its transcript arrives (`done`). An utterance is
     **over speech** when its first speech frame (not its pre-roll) came while a clip played, during a hold, or in a gap
     between sentences still queued. Classify it by rules in a module of their own (`heard.rs`: pure functions over the
     transcript and the sentences concerned, unit-tested there). An utterance that began within 1.2 s after the last
     queued sentence ended is checked for echo only: otherwise it is a turn, because a "yeah" there answers the reply.
     - **wordless**: an empty transcript;
     - **echo**: at least 2 words, and at least 60% of its distinct words (lower-cased runs of letters, digits and
       apostrophes) are among the words of the sentence that was playing or of sentences that ended in the last 1.2 s;
     - **backchannel**: at most 3 words, all from a short list (yeah, yes, yep, yup, okay, ok, right, sure, uh-huh,
       mm-hm, mhm, mm, hm, hmm, cool, nice, alright, got it, I see);
     - **resume**: only a request to go on (go on, continue, keep going, carry on, go ahead, please continue, sorry go
       on, as you were saying);
     - **words**: anything else.
   - **Wordless, echo, backchannel and resume resume:** the cut sentence plays again from its start, from the audio its
     item holds (no synthesis), and the queue goes on. A cut acknowledgment isn't replayed. None of these becomes a
     turn: an echo is dropped; a backchannel or a resume request is still an `Utterance` event, with its class, for the
     runtime. Resume only when every utterance that overlapped the hold has been classified and no listed VAD is open.
   - **Words commit:** the held items are unsaid. Emit a `Cut` for each reply or report among them, then `BargeIn`
     with `dropped` as today; the utterance becomes the next turn, as today.
   - **The late cut:** an overlapping utterance that never reached 300 ms (a 200 ms "no") and whose transcript is words
     stops the clip and commits then. If the reply has already ended, it is just a turn.
   - **A failed transcription of an overlapping utterance commits** (a "stop" that wasn't heard must not be talked
     over).
   - **A report cut by words** goes back to the front of the report queue from its cut sentence. Keep queued reports
     as their sentences, so nothing is split twice.
   - **An echo-prone speaker:** after an echo verdict, that speaker's 300 ms stop is off for the rest of the call, and
     their cuts come at the transcript (the late cut). Otherwise a speaker on loudspeakers makes it stutter: stop,
     resume, stop.
   - **The facts** (engine.rs's `Event` and `Utterance`, each with its doc):
     - `Resumed { what, why, held }`: a stop that came to nothing, why it resumed (wordless, echo, backchannel,
       resume), and how long it was held;
     - `Cut { what, why, sentences, heard, into, last_heard, cut }`: of `what`'s `sentences`, the first `heard`
       played whole; the next was stopped `into` its audio, or never started; the texts of the last heard sentence and
       of the cut one. `why`: words (a barge-in), superseded (step 2), or the call's end (`Leave` or a dropped
       connection with anything unsaid);
     - `Utterance.over: Option<Over>`: what Theseus was doing when the utterance's first speech frame came: saying a
       sentence (the reply or report, the sentence's index and its text), or preparing the reply to a turn in flight
       whose reply hadn't begun (its `TurnId`);
     - `Utterance.heard_as`: words, wordless, echo, backchannel or resume.
     - `BargeIn` keeps its meaning: a cut that left `dropped` sentences unsaid, now emitted at the commit.
     - Items need their index within their reply and the reply's sentence count (today only the first item knows the
       count, and gives it up at its first play).
   - **theseus-discord:** add the new events to `pump`'s match as ignored arms (voice-heard writes their rows), and the
     new fields to its tests' `heard()`. Nothing else there.
2. **The floor** (theseus-kpa7).
   - `play_next` starts no clip, reply or report, while any listed speaker's VAD is open (the acknowledgment already
     waits so, in `advance`). When that utterance closes: not words, play; words, the waiting reply is superseded (a
     `Cut` with `heard` 0, why superseded), and the utterance becomes the next turn.
   - **A reply its speaker talked past is superseded:** when a reply arrives and a pending utterance by one of the
     turn's speakers began within 1.5 s of the end of that speaker's last speech in the turn (a thought split by a
     pause), the reply isn't played (`Cut`, heard 0, superseded) and the pending utterances become the next turn. If
     that utterance's transcript isn't in yet, the reply waits for it; if it isn't words, the reply plays.
   - Turns still start while a reply plays: don't serialize them.
3. **The module doc** of engine.rs says what the engine now does (its barge-in and turn bullets). Leave docs/ to the
   maintainer.

**Proof, offline:** new tests in a file of their own, crates/theseus-voice/tests/turns.rs, through the seam in virtual
time (`#[tokio::test(start_paused = true)]`, WAV fixtures, the stand-in speech), each asserting exact times:
- a 400 ms sound with an empty transcript over the first of 3 sentences: the clip stops 300 ms into it; the same
  sentence starts again from its beginning when the sound's utterance closes and is transcribed; all 3 sentences end
  played; 3 syntheses, none repeated; one `Resumed` (wordless); no `BargeIn`; no turn;
- the playing sentence's own words heard back: `Resumed` (echo), no turn; a one-word "stop" over the same sentence is
  never echo, and cuts;
- "yeah" for 600 ms over a reply: `Resumed` (backchannel), no turn; a 200 ms "mm-hm": nothing stops, no turn; both
  `Utterance` events carry `over` and `heard_as`;
- "yeah" 0.5 s after the last sentence of a reply that asked a question is a turn (backchannels count only over
  speech, never in the echo tail);
- "wait, which account" over the second of 4 sentences: `Cut` (sentences 4, heard 1, `into` about 300 ms, the two
  texts) and `BargeIn` (dropped 3), and the next turn carries the utterance with `over` naming sentence 2;
- "go on" after a stop: the rest plays from the cut sentence, and there is no turn;
- a 200 ms "no" over a reply: no stop at the VAD; at its transcript, a late cut;
- a failed transcription (a test-only `Speech` whose transcribe fails) commits;
- a report cut by words comes back at the next pause from its cut sentence;
- after one echo verdict, a second echo from that speaker doesn't stop it, and words still cut, late;
- a reply that arrives while the speaker is talking starts only after the utterance closes, and is never played when
  that utterance is words;
- a thought split by an 800 ms pause gets one answer: the first reply superseded, the continuation the next turn; a new
  question 3 s later doesn't supersede the answer.

The existing pipeline tests pass, with one update to `a_barge_in_stops_playback_within_300_ms`: the stop stays at 2.3
s, but its 200 ms "hm" now needs a backchannel transcript (the stand-in's default, `[utterance 0.2 s]`, is words and
would cut late), and its `BargeIn` comes at the commit, when `EDDIE`'s transcript is in. Keep what it proves, and say what
changed and why. If another existing test changes, say why. heard.rs's unit tests; the crate's suite 5 times under
load; theseus-discord's suite; the gate.

Planted reverts, each naming the test it breaks: the stop clears the queue again (the sound, echo, backchannel and "go
on" tests fail); every transcript classified as words (the same four); resume from the sentence after the cut (the
sound test, on order); the late cut removed (the "no" test); backchannels counted in the echo tail too (the
closing-question "yeah" test); `play_next` ignoring the VADs (the waiting-reply test); the 1.5 s window at 0 (the
split-thought test).

**FAST:** the stop is as fast as today. A committed cut makes the next turn at the moment today's code does (the
utterance's close and transcript), so no reply's first audio moves. A resume replays held audio: no synthesis. The
rules are a few string comparisons per utterance, off the turn path. Holding stops synthesis ahead, which saves what
today is synthesized and never played (7 of 54 sentences in that call). Say so in the report with the code paths; no
bench is needed.

**The live check is the maintainer's,** in the test voice channel after the join and an install:
1. Ask for a long answer; cough and laugh over it. It stops within 300 ms and starts the cut sentence again about a
   second after the sound ends. The `voice.barge_in` rows count only real cuts.
2. Say "yeah" or "mm-hm" over it: it goes on, and no new answer follows.
3. After a laugh, say "go on": it goes on from the cut sentence.
4. Say "wait, stop" over it: it stops; the next turn is his words.
5. Start a sentence, pause about a second, finish it: one answer to the whole.
6. Keep talking as an answer becomes ready: it waits for him.
7. On loudspeakers instead of headphones: no answer to its own words, and after the first echo no stutter.

**Leave alone:** the VAD's settings and vad.rs; deepgram.rs, songbird_io.rs and the examples (their matches have a
wildcard); sentences.rs (voice-heard adds a pass there); theseus-discord beyond `pump`'s arms and the tests' `heard()`;
the protocol's ledger kinds and `VoiceStatus` (voice-heard adds them); docs/.
