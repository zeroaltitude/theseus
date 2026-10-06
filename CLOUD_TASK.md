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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-core-waits`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: four theseus-core tests that fail by the machine's speed or clock, each fixed at its cause: the golden's wait for its wake, the golden's time zone, a cancel's wait for four dispatched reads, and a failing Jev's turn bound (theseus-23wh; also theseus-ig6n, theseus-t2yb, theseus-vbju)

Branch: `cloud/20261005-core-waits`. Every commit's subject carries the id of the issue it fixes. Deadline for the
report: 4 hours after you start.

**Background.** Three of these fail under CPU starvation, one under a UTC clock. For you they are the task, not known
failures, and none may go on the flaky list. For each timing test: **reproduce the failure
first** under the load recipe (the test's binary built once, the one test with `--exact`, in a loop: the count of
failures in N runs, each failure's output kept; if it won't fail in 30 runs, say so); fix it at its cause, keeping
what it proves (no retry, no wider bound without saying what the test then proves); then at least 30 runs under load
with no failure, with their durations; and a planted revert of what the test guards still fails it.

**Read first:** theseus-core's AGENTS.md; tests_output.rs (`world`, `turn`, `drive`, `until`, `Mask`, `shapes`) and
core_output.txt's "a turn that sets a wake"; theseus-kernel's wakes.rs (`due_now`, `free`) and clock.rs; wake.rs (the
local time); tests_m3.rs's `mod parallel` (`Timing`, `rendezvous_after`, `Slowed`, the seven-calls test's
`cpu_cores`); tests_judge.rs (`rig_with`, `judged`, `until_judged`) and judge/mod.rs's `at_loop_end`; theseus-judge's
fake.rs (its modes: a request is recorded in `seen` before a `Slow` sleep).

**What the code says** (the code wins; report each difference):
- The kernel's clock is the real one (`Core::build` passes `RealClock`; `Parts` has no clock seam). tokio's paused
  clock (theseus-ksfu's fix in rpc/tests.rs) moves tokio's timers, never a wake's due time.
- turn-stack (batch 8) makes the golden's `conversation` `pub(super)` and runs it from a test of its own, on a runtime
  it builds: what drives the wake's wait must work there too.
- queue-frames (batch 8) moves a late result's wake (`late_result`, not `wake.at`'s) into the turn's end frame. If main
  has it when you clone (`git log --grep theseus-6qwr`), check it doesn't change this wait.

**The four,** each a green commit:
1. **theseus-23wh** (`tests_output::the_cores_output_matches_its_golden`). The conversation's last step, "Wake me in a
   second.", sets a wake about a second out; `until(out, "wake due", ..)` then polls `due_now` every 20 ms and panics
   after 30 s of wall time. On a 16-core machine, with 16 nice-0 busy loops and the golden alone at nice 19, main
   failed there 2 of 3 runs (67.9 s, 70.1 s) and passed once (44.4 s); unloaded it takes about 4 s.
   - First make the timeout say what it saw: the execution's `state` and `wake`, its pending wakes with their due
     times, and `now_ms`. Reproduce, and say which part was missing: the turn's own work, or the clock.
   - Drive the wait by the scenario's progress (the execution free and `Waiting` with the wake pending, as `w.exec`
     reads it, then the due time by the kernel's clock), with a guard for a real hang only: well past anything
     starvation does, under nextest's 120 s kill, its message what it saw. Don't read the bus tap there (the
     transcript drains it). Say what the test then proves.
   - **Plant:** a wake that never comes due (its due time an hour later): the test fails at the guard.
2. **theseus-ig6n** (the same golden). Its two `wake.at` lines (the tool's preview, the narrator's "Wake #N set for
   …") print the local time with its UTC offset. The mask turns digits into `#` and keeps the sign: the golden says
   `-#:#`, a UTC machine prints `+#:#`, so it passes only west of UTC (the gate line sets `TZ=America/Phoenix`).
   - Mask the sign in tests_output's normaliser. (Pinning TZ in the test sets an environment variable while other
     tests read the local time on other threads: if you choose it, say why that is safe.)
   - Rewrite with `THESEUS_GOLDEN=write`; commit only the two lines it moves.
   - The golden under `TZ=UTC`, `TZ=America/Phoenix` and `TZ=Asia/Tokyo` passes. **Plant:** the sign kept; a zone
     fails. Say in the report that the gate's `TZ=` can go once this joins.
3. **theseus-t2yb** (`tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched`). Four `fs.read` calls,
   each slowed 600 ms by `Timing`; the test polls every 10 ms until all four are `Dispatched`, and fails "never
   dispatched" after 5 s. Seen once in a loaded join gate; alone it passed 5 of 5 in 1.1 to 1.6 s. Under load a read
   can end before the fourth starts (each takes a CPU-pool permit, as many as the host's cores unless
   `Parts::cpu_cores` sets them: theseus-i1i4's seam), or the poll can miss the window where all four run.
   - Fix it by order, as theseus-qh0u and i1i4 did with `rendezvous`: once started, the reads hold until the test
     releases them after its cancel; `cpu_cores = Some(4)`. The late completions must still come after the cancel
     (the test asserts "late completion after cancel"). The 5 s bound becomes a hang guard counting what was
     dispatched.
   - **Plant:** a cancel that leaves one running call dispatched (name the line): the test fails.
   - tests_m3.rs is 7,799 of its 8,050 ceiling: a latch in `Timing` and this test's lines only.
4. **theseus-vbju** (`tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`). For each fake Jev mode
   (`Down`, `Slow(10 s)`, `RateLimited`, `Malformed`) the turn must take under 3 s, and its request and result equal
   the judge-off turn's. It failed once at load about 18 (4.2 s; no message kept). The rig judges `loop.v1` alone:
   the turn's end (`after_turn`) calls `at_loop_end`, which spawns `judge_loop` and returns at once.
   - Reproduce under load, and read which mode's `took` failed.
   - Prove "waits for nothing" by order, never by speed. With `Slow`, a turn that waited returns with its judgment's
     row written; one that didn't returns before it (the client's `total`, 5 s here, ends the call). Make that margin
     structural (a delay and a `total_secs` no starvation reaches, then `until_judged`: say how long the test takes),
     or use the paused clock if you show the client's timers don't fire early while the runtime idles on the fake's
     socket. The other modes fail at once: keep their class and unchanged-turn checks, and say what their wall bound
     proved.
   - **Plant:** `at_loop_end` judging in place, `tokio::task::block_in_place(|| rt.block_on(..))` for its
     `rt.spawn`: the test fails.

**Proof, offline:** for each timing test the failures before, the runs after and the plant; ig6n's three zones and its
plant; five runs under load of `tests_output::`, `tests_m3::parallel::` and `tests_judge::`; theseus-core's suite
whole once under load.

**The live check is the maintainer's.** Exact commands, on a 16-core machine:
1. Each timing test 200 times at nice 19 beside one nice-0 busy loop per core (`nproc`), on main's build and on
   yours: main's failures as the issues say; yours none.
2. The golden under `TZ=UTC` and the machine's own zone, and the workspace suite with `--retries 0` at the gate's
   load: none of the four fails.

**Leave alone:**
- turn-stack (batch 8): tests_output.rs's `pub(super)` line, tests_stack.rs, turn.rs; queue-frames and smalls (batch
  8): the golden's rows near a background job's completion and in the budget scenario;
- history-pages (batch 8): its line in tests_m3.rs; timing-flakes (batch 8): term/tests.rs, telemetry/tests.rs,
  theseusd's job_approval.rs;
- judge-tests (batch 9): judge tests, maybe in tests_judge.rs: change only this one test there;
- theseus-judge's fake.rs (route-tests, batch 9, may change it); the kernel, wake.rs and the judge's code, except for a
  plant you restore.
