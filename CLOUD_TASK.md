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
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report. A batch-8 session (timing-flakes) is fixing the four marked *.
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs);
  - theseus-core's `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia*: typed-ahead input can
    land on the prompt line; it fails alone too, about one run in two on this VM) and
    `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y*);
  - theseus-core's `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is` (theseus-qjd6*: under
    load the exporter's retry is counted as a fourth trace);
  - theseus-core's `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb),
    `tests_lsp_edits::the_block_adds_no_frame` (theseus-xx6w), and `tests_route`'s tests that slow Jev's verdict past
    route's wait;
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `job_approval::a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too` (theseus-cs71*: a 1.5 s wall
    bound on the cancel's round trip).
- A negative assertion ("nothing of X reached Y", "no process is left") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: theseusd's stop on a SIGTERM or a SIGINT, and a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at 60b43fb6 or later, with store format 20. These joined main in the last two days, so your clone has them:
- route.v1 on the turn path, and its fix; the live rerank; security.v3's live notices; replay, audit and backfill of judgments; the ladder (`pack.mode`, `mode_for`, `pack_arm`, `theseus packs`); the tools smalls (runaway mode among them) and the tasks smalls; gliding with the place rule; the Linux lanes (spawn without a fork, a cgroup per L0 job where delegated, one sync per job completion, background passes that wait while the machine is busy); the bench fixes; rust-analyzer's errors after an edit; the refusal fallback; context-honesty;
- files: every surface accepts any file; PDFs, Office files, notebooks, EPUB, RTF, archives, recordings, video and the text in pictures reach the model;
- speed: a reply shows on Discord as its model streams, Jev's connection is kept warm, a verdict lands inside its wait, and each routing mode has its own confidence bar (trivial's is 0.4);
- memory: FSRS-6 retention and the `+retention` arm; spreading activation over memory's adjacency projection and the `+activation` arm; consolidation into cited `Synthesis` nodes and the `+synthesis` arm; tiering (transcript stubs that decode at first read, the node heat cache); a trivial detour sends no recall;
- the task board: claim leases, `/tasks` and the cockpit's task graph; the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop: a pack version's wording rewritten from the owner's labels, nightly and by `judge.learn`, placed through the ladder; and `theseus judge prove`, the prove's report from the daemon's ledger (learning/'s prove, the `judge.prove` method);
- the kernel: a deadline's stop verifies its whole tree's kill, an earlier process's provider calls settle unknown at the first tick, one late-after-cancel row, and nested kernel locks are caught; kernel-sim drives `/stop`, tasks, the outbox under crashes, and busy-turn wakes; a job result spooled during a restart's startup is taken at the harness loop's bind;
- the store: a failed WAL sync cuts its frames back to the last good sync (followers meet the cut as a rewind), and a reopened log syncs its last segment's name once; the durability tender ships only synced frames;
- each notification is serialized once and shared by every watcher; every operator act's `by` names its surface;
- `proc.run` takes up to 16 steps, judged as the strictest; `fs.patch` recounts hunk headers from their bodies;
- AWS: runaway mode ends on a raised line, and unknown `[policy.aws]` keys are named after serving;
- health's `web:` refusals and 1-hour cache writes, `theseus catalog`'s 1-hour price, the cockpit's `binary`, and one row and one notice per disk crossing;
- telemetry: a failed provider call's `error.type`, and the AWS calls counted and timed; each tool call counted once, at its answer, timed by its run, and traced in the turn that answers it (a confirmed call's run, a continuation's other answers, a late result);
- bench/: the harness sampler and the efficiency record, the async bench (bench/async/), and the incidental-recall bench (bench/recall/).

**Other changes in flight.** About ten other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Under review now, merging into `main` next:
- route's gaps: a routed session follows its base, and the route review's debts (turn/route_step.rs, turn/compile_step.rs, routing.rs, session.rs, rpc/methods.rs; store format 21);
- the reader rule's tool and same-name holes (tests_registry.rs's scan), and `theseus-index --version`.

Held on the owner's machine: situations, the precedence line and testimony (compiler.rs, turn/compile_step.rs, turn.rs, the store format, the core's golden).

These are other cloud sessions like you, each on its own branch:
- wal-mark-skip: a start skips the WAL directory's sync when a mark vouches for the found segment;
- turn-stack: the turn's future boxed, and a test that holds the stack's margin;
- memory-checks: activation's build paced by pressure, and tests for retention, activation and the stubs;
- synth-headed: a synthesis headed by a title passes the check, and a cluster rejected for its form comes back once;
- learning-fixes: replay's yes-or-no rightness, the audit's requests off the low thread, the prove's learned versions, two tests;
- timing-flakes: four timing tests fixed at their causes;
- telemetry3: two tests of resumed spans, the cancel metric, and the index tender's gauges;
- cli-tests: health's 1-hour words, `judge prove`'s bytes, and `watch`'s last line;
- discord-live: the bindings file read live, the courier's maps bounded, the disk notice's test;
- bench-async-measured: both async arms measured, and the async bench's missing tests;
- bench-recall-fixes: the mark's bulk sized by the compiler's estimate, and the scorer's admission and stale rules;
- bench-efficiency-fixes: the sampler's CPU counted once, and the sampler's cost measured per process;
- queue-frames: a late result's wake in the turn's end frame, and a completion's `execution.queued` row;
- history-pages: `after` and `before` on the ledger and history reads, and a node's short id;
- smalls: the secret board's settle race, musl's `time_t`, the TUI's message order, and two record nits.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (20 on main today; route's gaps take 21 at their merge), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,727) and crates/theseus-discord/src/render.rs (3,001); near it, crates/theseus/src/render.rs (3,098 of 3,100), crates/theseus-core/src/turn.rs (3,452 of 3,523), crates/theseus-core/src/compiler.rs (2,548 of 2,560), crates/theseus-kernel/src/kernel.rs (3,008 of 3,030), crates/theseus-core/src/config.rs (2,887 of 2,910) and crates/theseus-discord/src/runtime.rs (3,453 of 3,500). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's toolrun.rs (2,494) and telemetry/tests.rs (2,484), theseus-sim's kernel_sim.rs (2,451), theseus-kernel's tests.rs (2,381) and theseus-store's wal.rs (2,352). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-timing-flakes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: four timing tests fixed at their causes: a cancel's wall bound, two terminal tests that type ahead, and a trace counted twice (theseus-cs71; also theseus-ynia, theseus-1n2y, theseus-qjd6)

Branch: `cloud/20261005-timing-flakes`. Every commit's subject carries the id of the issue it fixes. Deadline for the
report: 4 hours after you start.

**Background.** The preamble lists these four as known failures under load, marked as this task's. For you they are
the task, not known failures. None is on .config/nextest.toml's flaky list, and none may go there. For each:
1. **Reproduce the failure first** under the preamble's load recipe (the test at nice 19 beside four busy loops at
   nice 0), with many runs of the one test: its test binary with the test's name and `--exact`, built once, in a
   loop. Keep each failure's output, and give the count of failures in N runs. If it won't fail in 30 runs, say so.
2. **Fix the test at its cause, keeping what it proves.** Add no retry, and widen no wait without saying what the test
   then proves.
3. **Run it many times under load** (at least 30) with no failure, and give the count and the durations.
4. **Plant a revert** of the behaviour the test guards, and show that the fixed test still fails on it.

Tests only. If a run shows a real bug, report it with its evidence; don't fix the kernel, the terminal code or the
exporter here.

**Read first:** theseusd's AGENTS.md and tests/job_approval.rs (its `Rig`); theseus-kernel's job.rs (`STOP_GRACE`,
`ANSWER_WAIT`) and tree.rs (`KILL_WAIT`, `FREEZE_LIMIT`, the stop); theseus-protocol's cancel.rs (`CancelVerdict`,
its `ms`); theseus-core's term/tests.rs (`read_until`, `marked`, `ctrl_c_interrupts_a_command`), term/tools.rs (how
a read's `until` matches) and term/keys.rs; telemetry/tests.rs (`Receiver`, `tuning`, `spans_of`, `core_with`) and
telemetry/export.rs (its one retry).

**The four,** each a green commit:
1. **theseus-cs71** (theseusd, `a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too`). It asserts `took < 1500
   ms` on `execution.cancel`'s round trip. In loaded suites that took 2.077 s and 1.507 s, with a verdict that was
   right: verified by the tree, 3 killed, 0 survivors. On a 16-core machine the round trip was about 0.1 s, even
   beside 64 busy loops, and the wrapper's own stop took 10 to 32 ms in every run. That stop is the verdict's `ms`,
   from the stop's signal to its verdict. So only the machine's load pushes the round trip past 1.5 s.
   - **The fix** (the issue's first option): bound the stop's own time, the `ms` of the answer's `verdicts[0]`, and
     keep only a loose wall bound at the stop's own deadline. The daemon waits `STOP_GRACE` (2 s) plus `ANSWER_WAIT`
     (3 s) for the wrapper before it reads the cancel uncertain.
   - **The `ms` bound** comes from what it means and from your numbers. A stop that waits out its grace can't be under
     `STOP_GRACE`. Print `took` and `ms` for every run under load.
   - If a cancel reads uncertain under the recipe, that is the product's own deadline under starvation: report it
     with its numbers, and leave the deadline alone.
   - **Plant:** a stop that waits out its whole grace before its kill (in the kernel's stop, for the plant only). The
     `ms` bound must fail.
2. **theseus-ynia** (theseus-core, `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`, through
   `ctrl_c_interrupts_a_command`). After Ctrl-C ends `sleep 4242`, the helper types `echo back` at once. The tty
   echoes it before the shell draws its next prompt, the shell then prints `back` after that prompt (`ok> back`), and
   the wait for `"\nback\n"` times out at 15 s. It failed 3 of 6 runs alone on a 4-core VM like yours, so reproduce
   it unloaded first, then under load.
   - A read's `until` matches the whole screen, and `ok> ` is already on it. So wait for what only the interrupt's
     prompt draws (look at a failing screen), then type.
   - Keep the assertions that `after` never ran and that the shell took the next command.
   - The issue's other option, matching `back` wherever it lands, keeps the typing ahead: say why you chose as you
     did.
   - The same test's earlier fix (theseus-y6zr: read until the prompt after the output) is the precedent.
   - **Plant:** Ctrl-C that interrupts nothing (its key sent as nothing). The test must fail at its own 10 s
     "did not interrupt" bound.
3. **theseus-1n2y** (`term::tests::python3s_repl_computes_on_the_screen`). It failed once in a full suite at 5.9 s,
   and passed 3 of 3 alone. Its last steps:
   - it types `while True: pass` and two newlines as soon as `15150` shows;
   - it sleeps 300 ms and sends Ctrl-C;
   - after `KeyboardInterrupt` it sends Ctrl-D, then reads with a 5 s quiet wait.

   5.9 s fits that quiet read, with Python still in its loop or still reading, better than a 15 s `until`. Under load,
   each key can land before Python is ready for it. Reproduce it, find which step it is, and fix it as in 2: send each
   key at the state it is meant for, never after a sleep. Those states are the `... ` prompt, the loop running
   (Python's CPU time rising in /proc), and the fresh `>>> ` after the interrupt. **Plant:** Ctrl-C that interrupts
   nothing; the test must fail at its `KeyboardInterrupt` wait.
4. **theseus-qjd6** (`telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is`). It asserts 3 POSTs to
   `/v1/traces` ("each turn's trace"), and under load it got 4. The test's tuning has a 2 s post timeout, and the
   exporter retries once: a batch whose answer comes after 2 s is posted again, and the receiver counts both. A cloud
   session reproduced it 3 of 3 under the recipe.
   - **The fix:** count distinct trace ids (the `traceId` of `spans_of`'s spans): three turns, three traces, however
     often each was posted. Or give it a tuning the load can't reach. Say which you chose, and why.
   - A duplicate post after a late answer is the exporter's at-least-once retry. Say in the report whether that is a
     bug (receivers don't deduplicate), but don't change export.rs.
   - telemetry/tests.rs has 2,484 lines, and an unlisted file fails past 2,500: stay under, or move this test to a
     tests_<subject>.rs of its own.
   - **Plant:** a turn that exports two traces (the continuation's turn split across two trace ids). The test must
     fail.

**Proof, offline:** for each test, the failure count before the fix, the runs after it, and the plant. Then, under
load, five runs each of theseusd's job_approval tests, `term::tests`, and `telemetry::tests`; and the gate.

**The live check is the maintainer's.** Write exact commands, on a 16-core machine:
1. Each of the four tests 200 times at nice 19, beside one busy loop per core (`nproc`), on main's build and on yours.
   Main's failures, if any, should print what the issue said; yours should be none.
2. The whole workspace suite with `--retries 0`, at the load the gate meets: none of the four fails.

**Leave alone:**
- telemetry3, a sibling: its new telemetry test files and instruments, and tests.rs's other tests;
- the kernel's stop code (job.rs, tree.rs, stops.rs), the terminal code (term/ but tests.rs) and export.rs, except
  for a plant you restore;
- the flaky list.
