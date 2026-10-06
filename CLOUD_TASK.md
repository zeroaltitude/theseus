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
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report. Batch 9's branches, reviewed and joining main today, fix the ones marked *; batch 10's sibling sessions (below) work on those marked ‡; batch 8's timing-flakes is on main and fixed those marked † (their subjects name theseus-cs71, ynia, 1n2y and qjd6), so those four don't fail on your clone.
  - theseus-core: the output golden's 30 s wait for "wake due" under CPU starvation (theseus-23wh*);
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb*);
    `tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`'s 3 s bound (theseus-vbju*);
    `tests_route`'s verdicts that come `late` under starvation (theseus-biy3*); `tests_lsp_edits::the_block_adds_no_frame`
    (theseus-xx6w*); `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia†: typed-ahead input can
    land on the prompt line; it fails alone too, about one run in two on this VM) and
    `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y†); `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is`
    (theseus-qjd6†: the exporter's retry counted as a fourth trace);
    `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` can pass nextest's 120 s kill
    (theseus-0u6g‡); `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`
    (theseus-1g8j‡); `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`'s 20 ms bound
    (theseus-b38m‡); `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits` (theseus-9o2o‡); and
    `term::tests::a_close_leaves_no_child_behind` (theseus-d006‡, a negative assertion: keep its output);
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs*);
  - theseus-kernel's `children::a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` (theseus-r4hn‡: a count of
    how processes were classed, so its failure is a finding) and theseus-aws-catalog's
    `one_service_decodes_in_under_5_ms` (theseus-rnl3‡: a wall-clock decode bound in a debug build);
  - theseus-discord's tests_outbox: `a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1‡, sj0t‡) and
    `a_refused_pin_leaves_the_board_unpinned_and_edited` (theseus-o2tm‡);
  - theseusd's `job_approval::a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too` (theseus-cs71†: a 1.5 s wall
    bound on the cancel's round trip).
- A negative assertion ("nothing of X reached Y", "no process is left") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (daemon-proofs, on main since d279767f, emptied the list: theseusd's stop on a SIGTERM or a SIGINT, theseus-xbtr*, and a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at d279767f or later, with **store format 23**. Your clone has, besides v1's milestones and batch 9's base (4a449460: route's gaps, situations, the reader rule's closures, memory's paced warm build and consolidation's heading rule, and bench/'s measured arms):
- **batch 8's joins (2026-10-05 and 06):**
  - timing-flakes and telemetry3: four timing tests fixed at their causes; the `theseus.cancel` metric, the index tender's gauges and restarts;
  - cli-tests and history-pages: health's 1-hour words, `judge prove`'s bytes, `watch`'s last line; `after` and `before` on the history reads, and a node's short id;
  - turn-stack, queue-frames and smalls: the turn's future boxed at `TurnRunner::run`; a late result's wake in its turn's end frame and a completion's `execution.queued` row; the secrets board's settle race, musl's `time_t`, the TUI's message order, a budget question's `loop.ended`;
  - wal-mark-skip, crash-hold and durability-on: a start skips the WAL directory's sync when a mark vouches; a crashed call's held reservation booked as spent; the durability sessions list only their prefix, and health's durability line in every surface;
  - **soul-import (79be3213): `theseus import openclaw|list|erase`, imported sessions with their provenance, erase by tag, recall's provenance; store format 23** (core import/, node.rs's bodies, recall.rs, rpc/import.rs, the index's tender and extract);
  - learning-fixes (21bf5454): replay and the loop share one rule for a lean's rightness, the audit's requests run off its low thread, the prove names a learned loop version's placement (learning/, rpc/judge_prove.rs);
  - discord-live (6496ce34): Discord's bindings file read while the daemon runs (a place added, changed or removed binds, updates or unbinds with no restart), and a lane's message maps bounded (theseus-discord runtime/live.rs, runtime.rs, courier.rs);
- **batch 9's joins (2026-10-06):**
  - the bench stack: the async record's spend and calls, the recall bench's retraction rule and its overhead plan (bench/ only);
  - voice-turns (f33f0eca): a barge-in held until the words over it decide, a reply waiting for the floor, `Cut` and `Resumed` (theseus-voice);
  - scrub-escaped, gate-tests, approvals-batch (2b342594, bd3eb769, 25b0578f): a secret printed JSON-escaped is withheld (core scrub/escaped.rs); the gate's layers held by tests and `policy.explain` naming L3; a declined call ends its batch's waits, and a call is found by its own response (core toolrun/calls.rs, `resume::calls_of`, the core golden);
  - judge-tests and judge-reads (fb1133dc, d28bfc4b): judge tests; `judge.list` pages back from the newest judgment with a `more` floor (the CLI prints `M+`), the notices' brake reads today's rows, the learning rules skip judgments whose windows closed (core rpc/judge.rs, judge/notice.rs, learning/system.rs);
  - memory-tests and telemetry-tests (a8f4b27c, 2bf9e7d3): a search during the paced warm build answers `building` at once, a compaction summary's call reserved on the estimate's upper bound, `Adjacent::paces`; tests of the daemon's own telemetry path through `install_telemetry`, and the index gauges by the tender's latest answer (core recall/activation.rs, turn/compaction.rs, provider.rs's `Scripted::BilledBy`, tests_activation_*.rs, tests_compaction.rs, telemetry tests).
  - core-waits, route-tests, daemon-proofs (2e592804, 857ab09a, d279767f, joined 11:59 on 10-06): four core tests fixed at their causes (the golden's wake set 20 s past its turn and its UTC offset's sign masked, so the gate line's TZ can go), route's tests proof against load and a routed turn's trace root naming the model it ran on, a `--stdio` daemon's pipes relayed by threads so its stop drops the runtime (theseusd stdio.rs), the lifecycle bench's `cancel` phase, the flaky list emptied;

**Other changes in flight.** A few other changes are reviewed and merging into `main` while you work, one at a time on the owner's machine. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.
- judge-turn-cost (batch 9, in a fix round with batch 10's judge-sink): the judge's sink writing between turns and flushing at a stop, the ladder's first read off the turn path, a judge-on turn bench (core judge/ladder/, judge/lineage.rs, judge/mod.rs, judge/sink.rs, rpc/packs.rs, theseus-sim perf).

These are other cloud sessions like you, batch 10, each on its own branch:
- imported-skip: session lists skip imported sessions without reading them (theseus-7087);
- voice-heard: the next voice turn says what was heard, cut and never said; replies shaped for speech; a failed voice turn said aloud;
- voice-echo: an echo and a closing "yes" told apart from a real answer in a voice call;
- judge-sink: judge-turn-cost's sink keeping its clock across a backlog and writing its batch at a stop (theseus-s1am);
- bench-pi: Pi as the benchmarks' fourth arm (theseus-jp9p; bench/harbor/pi_agent.py, bench/report/, bench/README.md);
- discord-bound: a lane forgets a turn's message ids when the renderer drops the turn, and the renderer's own maps are bounded the same way (theseus-6809; theseus-discord courier.rs, render/, runtime.rs's place actor);
- discord-watch: the bindings file's watch retries a failed bind, keeps the start's note, waits out a torn save and keeps the owners' DM order (theseus-u6v6; runtime/live.rs, tests_live.rs);
- discord-tests: theseus-discord's load flakes at their causes, a binding's event loop ended at the stop, `refuse_unbound` under the lanes' lock (theseus-yduk; tests_outbox.rs, runtime.rs);
- daemon-stops: a `--stdio` daemon stops on the `shutdown` method, a restart in place drops its runtime first, an import or erase stops between frames, the manifest's upgrade syncs the log's directory first (theseus-yg1y; theseusd main.rs, core rpc/import.rs and import/write.rs, theseus-store store.rs);
- cancel-fast: `execution.cancel`'s round trip, its stop waits event-driven and its frames fewer (theseus-dwoj; theseus-kernel job.rs and tree.rs, core cancel.rs and rpc/driver.rs);
- flake-causes: five tests that fail under load fixed at their causes (theseus-d006; term/tests.rs, tests_push.rs, learning/tender.rs's tests, theseus-kernel's children test, theseus-aws-catalog's decode test);
- learned-shadow: `pack.promote` warns while a learned version stands in shadow, the prove's window line names one standing from before it, the learning cut's clock (theseus-nwa5; rpc/packs.rs, rpc/judge_prove.rs, learning/system.rs);
- voice-holds: the hold and the floor bounded, two turns.rs gaps, a waiting report's `Cut` at the call's end (theseus-aq4t; theseus-voice engine.rs, tests/turns.rs);
- scrub-encodings: the scrubber's twice-escaped JSON, YAML and repr escapes, and base64 inside an escaped string (theseus-nlvx; core scrub.rs, scrub/escaped.rs);
- budget-upper: a turn's own call reserved on the estimate's upper bound, memory's telemetry in `Core::build`, the warm build's stop test (theseus-ps9i; turn.rs's two reservation lines, rpc/mod.rs, tests_activation_pace.rs);
- core-gaps: a failed routed turn counted where it ran, and four core test gaps (theseus-udzb; rpc/methods.rs's `count_failed_turn`, tests_route*.rs, tests_m3.rs, tests_audit.rs, tests_stack.rs);
- bench-bounds: bench/'s sampler bounds, the recall scorer's retraction rule, and a daemon under its planned overhead (theseus-ufe5; bench/ only).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,715) and crates/theseus-discord/src/render.rs (3,001); near it, crates/theseus/src/render.rs (3,099 of 3,100), crates/theseus-kernel/src/kernel.rs (3,025 of 3,030), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-core/src/turn.rs (3,496 of 3,523), crates/theseus-discord/src/runtime.rs (3,418 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,800 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's telemetry/tests.rs (2,491) and toolrun.rs (2,418), theseus-sim's kernel_sim.rs (2,465), theseus-store's wal.rs (2,430) and theseus-kernel's tests.rs (2,384). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-voice-dave`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. Older text in the tree still names people while a pass removes it: never copy a name from it into new text.
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
## Your task: a voice call that joins but never hears is noticed, rejoined, and said (theseus-d93y)

Branch: `cloud/20261006-voice-dave`. Main must hold voice-heard's join (theseus-discord's `runtime/voice/notes.rs`, the
`voice.cut` and `voice.resumed` rows); if it doesn't, stop and say so in the report. Every commit's subject carries
`theseus-d93y`. Deadline for the report: 4 hours after you start.

**Background.** Discord's voice channels use DAVE, end-to-end encryption over an MLS group: the bot must be welcomed
into the group before it can decrypt anyone's audio. The owner's daemon joined a voice channel twice on one evening:
- **The first join failed.** The join itself worked: a `voice.joined` row, and health read "joined … (one listed
  speaker)". But no audio reached the engine: 0 utterances, 0 sentences, $0 of transcription. The journal at the
  connect showed songbird's runner with `dave_session { protocol_version: 1, is_ready: false, status: INACTIVE }`
  and one `ERROR songbird::driver::tasks::udp_rx: Illegal RTP message received`. The person spoke for minutes to a
  call that heard nothing, and nothing anywhere said so.
- **The second join, 30 minutes later, worked.** davey logged "Processing welcome", then "Welcomed to group
  successfully" (a leaf and an epoch), and decryptors appeared for both people. A 4.6-minute conversation followed.

So the failure is intermittent: sometimes the group's welcome never arrives or is never processed, and the bot stays
INACTIVE. Earlier live checks only ever played a greeting with the bot alone in the channel, where no welcome is
needed.

**Read first:**
- the root AGENTS.md (FAST, EXQUISITE VISIBILITY);
- theseus-voice's and theseus-discord's AGENTS.md;
- `crates/theseus-voice/src/songbird_io.rs` (DAVE's silent start, the speaking map, the receive path);
- `crates/theseus-discord/src/runtime/voice.rs` (the join's timeout, `pump`, health's voice line, `voice.failed`);
- `crates/theseus-voice/examples/join.rs`;
- songbird's and davey's public APIs as vendored in `Cargo.lock`'s versions: what state they expose (session
  status, ready, epoch), and whether a reconnect or a fresh key package can be asked for without leaving the call.

**What to build,** each a green commit:
1. **Notice it.** Call a voice connection deaf when, `deaf_after` (default 10 s) after the join, a listed speaker is in
   the channel, and at least one of these holds:
   - DAVE's session is not ready;
   - packets arrive for a listed speaker but none decrypts.

   A silent channel is not deaf. A bot alone in the channel is not deaf.
   - Make the rule one pure function over a snapshot (the session's status, per-speaker packet and decrypt counts,
     the speaking map, and the elapsed time), unit-tested on its cases.
   - Read the snapshot from songbird and davey where they expose it, and from your own counters where they don't.
2. **Recover.**
   - On a deaf verdict, rejoin once, as a leave and a join of the same channel, after a short wait.
   - If the rejoin is deaf too, stay joined, mark the call deaf, and stop trying. Never loop.
   - Write `voice.deaf` (`why`, `elapsed_ms`, `attempt`) and `voice.rejoined` rows on the place's session, as
     `voice.joined` is.
   - Add a metric: `theseus.voice.deaf` (IntSum, attribute `theseus.voice.why`), following voice-heard's
     `theseus.voice.cuts` pattern.
3. **Say it.**
   - Health's voice line reads `deaf since …, rejoined once`, or `deaf (rejoin failed)`.
   - The place's text channel gets one notice: "I can't hear the call (Discord's voice encryption didn't finish
     setting up). Rejoining…", then "Rejoined: I can hear you now", or "Still can't hear: try /leave and /join".
     Use the outbox's notice path, not the voice engine.
   - Nothing is spoken.
4. **Config:** `[voice] deaf_after_secs` (default 10), with a default in `theseusd example-config`'s template, as the
   preamble's config rule says.

**Proof, offline:**
- unit tests of the rule:
  - INACTIVE past the bound with a listed speaker: deaf;
  - alone: not deaf;
  - silent but ready: not deaf;
  - ready but nothing decrypts while packets arrive: deaf;
- a runtime test through a fake connection seam that drives one deaf join, one rejoin that works and one that
  doesn't. Check the rows, health's line, the two notices, and that no third attempt happens;
- theseus-voice's and theseus-discord's suites, and the whole workspace suite (`TZ=America/Phoenix`);
- `scripts/gate.sh` before every commit;
- **planted reverts,** at least three, each failing a test:
  - the bot alone counted deaf;
  - a second rejoin allowed;
  - the notice sent twice.

**FAST:** the check runs on a timer after the join, never on the audio path's hot loop. A rejoin costs the call a few
seconds only when it was already deaf. Nothing touches the start path.

**The live check is the maintainer's** (a voice channel on the owner's daemon, after an install):
- `/join` with a person speaking. The usual case is a welcome processed, no `voice.deaf` row, and no notice.
- To force the deaf path, give the exact steps if you find a way, for example joining before the person's client
  finishes connecting. If you find none, say so.
- `theseus ledger -k voice.deaf` and `-k voice.rejoined`, and health's line.

**Docs:** don't edit `docs/`. In the report, give the text for m7-surface.md's voice section and the spec's Part III
item.

**Leave alone (in flight):**
- `crates/theseus-voice/src/{engine.rs,heard.rs,sentences.rs}` and `tests/turns.rs` (voice-holds);
- `runtime/voice/notes.rs` (voice-heard, joined before you start).

In `runtime/voice.rs`, keep your edits to the join, the health line and a new module of your own, for example
`runtime/voice/deaf.rs`. The Discord tasks (discord-bound, discord-watch, discord-tests) are in flight in
`runtime.rs`, `live.rs` and `outbox.rs`: stay out of them, and if you must touch one, keep the hunk small and say so.

## Also in this task: three small fixes from voice-heard's review (theseus-b6vz, theseus-ved2, theseus-nthu)
voice-heard's reviewer found these in the voice runtime you are already in. Each is a green commit of its own, with
its issue id in the subject.
- **theseus-ved2 (pre-existing):** a rebind after a failed turn returns before `voice_next`. A voice turn queued
  behind the failed one is never submitted, and the call takes no more turns until someone types. Fix the one line.
  Test: a failed turn, then a queued voice turn, and the second is submitted.
- **theseus-b6vz:**
  - A voice turn held for the operator (a budget question, an approval) says nothing aloud today. Have the call say
    one constant sentence, "I need your answer in the text channel.", once per held turn, through the same reply path
    `FAILED_TURN` uses.
  - Find where a `/stop` is known, so a stop that a step refuses (`StoppedAtStep`) is never spoken as "Sorry, that
    didn't work". If you can't provoke that case, prove it by a seam.
  - Tests for both.
- **theseus-nthu:** pin three rules the branch's tests don't hold, and cap `Notes.waiting`:
  - an old call's late events never reach the next call's notes;
  - `FAILED_TURN` is not spoken for a turn that was stopped;
  - the first-words map keeps only the newest 32 turns;
  - `Notes.waiting` gets a cap of its own: only 3 are read, so keep at most 8.

  Each test must fail on a planted revert.

`runtime/voice/notes.rs` and the reply path in `runtime/voice.rs` are yours for these three. This replaces "Leave
alone" for notes.rs.
