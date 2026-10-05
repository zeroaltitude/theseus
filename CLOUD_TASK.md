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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-queue-frames`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: every queue writes its row in its frame: a late result's wake rides in the turn's end frame, and a completion that queues its execution writes `execution.queued` (theseus-6qwr; also theseus-2xep)

Branch: `cloud/20261005-queue-frames`. Every commit's subject carries its issue id. Deadline for the report: 5 hours
after you start.

**Background.** v1.1's step V3 (docs/design/roadmap-v1.1.md §2): every queue writes its row, in its frame.
1. **theseus-6qwr.** theseus-core's turn.rs, `TurnRunner::run`: the turn ends in one frame
   (`frames.kernel.end_turn_with`, parked on input), and later, after the session hold drops and the failure
   bookkeeping, `if rewake && stopped.is_none()` it calls `self.kernel.wake(&exec_id, "late_result")`, a frame of its
   own. Between the two, every surface reads the execution waiting on input, which is settled: a `session.wait`
   until settled, parked before the end, returns there, before the late result is read (theseus-jj9f's second shape;
   its first, the answer's, is one frame now).
2. **theseus-2xep.** A completion (kernel.rs's `completion_with`, behind `accept_completion`,
   `take_completion_with` and `mark_unknown`) that moves a waiting execution to queued writes only the action's row
   (`action.succeeded`, `.failed`, `.outcome_unknown`, `.resolved`) with `execution_state: "queued"`. Every other
   queue writes `execution.queued` with its why. Readers patch around it: theseus-core's push.rs board takes the why
   `result` from the action row, and theseusd's tests/push.rs `row_state` reads action rows' `execution_state`.

**Read first:** theseus-kernel's AGENTS.md; tx.rs (the module docs: staged frames; "Nested, it joins": a failure takes
back only what it staged); kernel.rs (`end_turn_with`, `end_turn`: its stop arm and its `queued_results` arm, `wake`,
`completion_with`); tests_frames.rs and tests/golden/kernel_frames.txt; theseus-core's turn.rs (`TurnRunner::run`
from `run_inner`'s answer on; the end of `run_inner`, where `again` is decided), fact/turn.rs (`WokenAgain`,
`Parked`), harness.rs (the driver: a 500 ms tick or `admission`), push.rs (the board's `why`), tests_push.rs
(`a_settled_wait_parked_before_an_answer_returns_after_its_continuation`, your test's model), tests_continuations.rs
(`a_late_result_that_lands_during_a_turn_is_answered_by_the_next`), tests_m3.rs
(`a_plain_turn_stays_within_its_frame_budget`); theseusd's tests/push.rs (`row_state`,
`every_execution_row_has_its_event_and_every_event_its_row`); cockpit/src/lib/timemachine.ts (states and whys from
execution.* rows); theseus-sim's kernel_sim (kernel_sim/stops.rs and tasks.rs read execution.* rows).

**What changed since the issues were written** (the code wins; report each difference):
- **Both flags are known before the end:** `rewake` comes from `run_inner`, and `stopped` is read from the
  execution before `end_turn_with`; `end_turn` itself takes the stop (`e.stopped.take()`, parked on input, why
  `stopped`). Compose the end and, when `rewake` and not stopped, a nested `k.frame(&[id], |k| k.wake(id,
  "late_result"))` whose error is logged, not returned, so a wake that can't happen takes back nothing else. Say
  whether a stop that lands between today's read and the end's lock changes the decision, and read it inside the
  frame if that closes a window.
- **The wake now lands before the rest of `run`:** the posts' `posted`, `judge.after_turn`, `pass.after_turn`, the
  `Parked` line, `admission.notify_waiters()`. The driver may admit the next turn at its next tick. Check that
  nothing there must come before the same execution's next turn, and say what you found. `WokenAgain` is recorded
  only when the wake was staged.
- **Queues with no why:** `end_turn`'s queue for results already waiting (`queued_results`) and `TurnEnd::Requeue`
  write `execution.queued` with no `why`. Say whether they should say `result` too, and do it if it is a word.
- **The long files:** turn.rs is 3,452 of 3,523: there only the call, the
  composition in a module beside it (crates/theseus-core/src/turn/). kernel.rs is at 3,008 of 3,030: there only the
  row's lines in `completion_with`.
- **No format change:** `why: "result"` is a new value of an existing row's field, not a field of a stored record.
  Check that against AGENTS.md's store rule and say so.
- **FAST:** a turn that took a late result writes one frame fewer; nothing else on the turn path changes.

**What to build,** each a green commit:
1. **theseus-2xep.** In `completion_with`, when the completion moves a waiting execution to queued, add
   `execution.queued { execution_id, why: "result" }` to its frame after the action's row (which keeps its
   `execution_state`). Then read execution.* rows alone: push.rs's why comes from `execution.queued` only, and
   `row_state` no longer reads action rows. A test that a completion queueing a waiting execution shows why
   `result` from the new row (no test pins it today). Rewrite kernel_frames.txt and the core's output golden with
   `THESEUS_GOLDEN=write`, and commit only the lines this moves (completions' frames gaining the row). If kernel-sim
   can check it cheaply, add the invariant that every move to queued writes `execution.queued` in its frame.
2. **theseus-6qwr.** The late-result wake in the end's frame, as above. Test, after the answer's test in
   tests_push.rs: a background job's result lands while a later turn runs; a `session.wait` until settled, parked
   before that turn ends, returns only after the late result's turn ends; every view the push sends in between reads
   the execution queued or running, never waiting; the end's frame holds `execution.queued` with why
   `late_result`. A stopped turn still queues nothing.

**Proof, offline:** the tests above; the frame budget tests (a plain turn's 5); tests_frames's golden (say why each
moved line moved); theseusd's tests/push.rs whole; the kernel's and the core's suites; `theseus-sim kernel-sim
--seeds 200` at `--p-race 0` and at `--p-race 0.3`, before and after, every invariant held (quote both totals lines);
tests_push.rs and tests_continuations.rs 10 times under load. Planted reverts, each naming the test it breaks: the
completion's row removed (the push prove, with the new `row_state`, and your why test); the wake back after the end,
in its own frame (the settled-wait test); the nested wake's error returned instead of logged, if a test can reach a
failing wake (a kernel no longer accepting, a terminal execution): say which.

**The live check is the maintainer's.** Write exact commands for a scratch daemon (fresh state dir, Discord and the
web off) on the stand-in model (`theseus-sim fake-model --rules`):
1. A turn whose `proc.run` of a short sleep outlives its wait and parks on the job: when the job ends,
   `theseus ledger -k execution.queued -s SESSION` shows why `result`, right after its `action.succeeded`.
2. A job's result landing while a later turn of the same session runs: `theseus watch --all --json` beside it shows
   no `waiting` view between that turn's end and the late result's turn, and a `theseus wait SESSION --until settled
   --after POSITION` started before the end returns after the late result's turn.
3. `theseus-sim bench lifecycle --check` and `theseus-sim bench turn` as main's.

**Leave alone:** turn-stack (a sibling: boxes the turn future at `TurnRunner::run`'s entry: keep your turn.rs edit to
the end's call); smalls (a sibling: fact/turn.rs's loop rows, the core golden's budget scenario: commit only your
lines there); situations (held on the owner's machine: turn.rs, compile_step.rs, compiler.rs, the core golden);
route-gaps (under review: turn/route_step.rs, turn/compile_step.rs, rpc/methods.rs); telemetry's continuation spans
and `CaughtUp` (joined: read only); theseus-sim beyond running kernel-sim and the one invariant.
