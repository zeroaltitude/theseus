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
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report.
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs);
  - theseus-core's `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y),
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb),
    `tests_lsp_edits::the_block_adds_no_frame` (theseus-xx6w), and `tests_route`'s tests that slow Jev's verdict past
    route's wait;
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker` (theseus-1n5f).
- One negative assertion failed once under load and is a finding, not a flake: theseus-kernel's `tree`
  `the_deadline_stops_the_whole_tree_too` (theseus-g11i; a batch-7 session fixes it). If it fails for you, keep its
  output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: the kernel sim's put-back check, theseus-81ig; theseusd's stop on a SIGTERM or a SIGINT; a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at e6f90af3 or later, with store format 16. These joined main last night, so your clone has them:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15), and its fix: a routed session keeps its move only while route.v1 acts for it, and `profile.use` moves it;
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the language servers' `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16);
- gliding with the place rule (`channel.post` and `channel.read`), and the cockpit Ship view's gentle roll;
- the Linux lanes: a job's wrapper and its L0 command spawn without a fork; each L0 job in a cgroup of its own where the daemon's is delegated, with a process cap and an exact stop; one sync per job completion; background passes that wait while the machine is busy; language servers that watch their own files;
- the bench fixes: the bench asks for its model's whole output cap, `[policy] private_addresses`, and `[model.retries]` (a transient failure's bounded retry inside its turn);
- rust-analyzer's compiler errors after an edit, a saved document waiting for the check after its save;
- the refusal fallback: a refused request goes once to its model's fallback (Sonnet 5.5's is Sonnet 5), and every surface says so.

**Other changes in flight.** About twenty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Under review now, merging into `main` one at a time over the next hours (your clone may hold some of them):
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop: labeled examples into packs, thresholds re-fit from calibration.

On the owner's machine:
- files: every surface accepts any file, and PDFs reach the model (the Discord attachments, `AttachmentContent`, `fs.read`, `web/fetch.rs`, the compiler's document blocks);
- speed: a confidence bar per routing mode, Jev's connection kept warm, and a reply that reaches Discord before its settle's sync (`config/routing.rs`, `turn/route_step.rs`, the judge client, the Discord outbox and streaming);
- context-honesty: the persona says how a request is assembled, and recall's notes say the harness chose them (`turn.rs`'s `PERSONA`, `recall/render.rs`).

These are other cloud sessions like you, each on its own branch:
- durability-fixes: the durability tender's missing-key read, only synced frames shipped, and two restore tests;
- aws-fixes: runaway mode after a raised line, unknown `[policy.aws]` keys in health, a network test, the hand image's build checks;
- push-once: one serialization per notification for every watcher, and every operator act's `by`;
- kernel-fixes: the deadline's stop of a whole tree, a late completion's one row, a restart's in-process calls, nested locks;
- sim2: kernel-sim drives `/stop`, wakes and the outbox under crashes;
- route-gaps: a routed session follows its base, and the route review's debts;
- bench-efficiency, bench-async and bench-recall: the benchmark program's efficiency track and two new benchmarks;
- proc-steps: `proc.run`'s steps, and `fs.patch`'s recount;
- health-words: health's web and 1-hour cache words in the CLI, `binary` in the cockpit, and disk crossings;
- prove-wire-in: `theseus judge prove` from the ledger.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (16 on main today; the changes under review take it to 17, 18 or 19, and the files change one more), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,709), crates/theseus-core/src/compiler.rs (2,560) and crates/theseus-discord/src/render.rs (2,928 of 2,930); near it, crates/theseus-discord/src/runtime.rs (3,453 of 3,500), crates/theseus/src/render.rs (3,062 of 3,100), crates/theseus-kernel/src/kernel.rs (2,992 of 3,030), crates/theseus-core/src/turn.rs (3,472 of 3,523) and crates/theseus-core/src/config.rs (2,855 of 2,910). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-kernel-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the kernel's fixes: the deadline's stop, a late completion's second taker, an earlier process's in-process calls, and a nested lock (theseus-g11i; also theseus-jnnj, theseus-m9iy, theseus-oqxw)

Branch: `cloud/20261005-kernel-fixes`. Every commit's subject carries the id of the issue it fixes. Deadline for the
report: 5 hours after you start.

**Background.** Four fixes in theseus-kernel, in this order. The first is a negative assertion that failed once, a
finding by this repository's rule: never weaken that test, retry it, or widen its waits.

**Read first:** theseus-kernel's AGENTS.md; tree.rs, job.rs (`run_l0`'s deadline, `stop_tree`, `linger`,
`WrapperEvidence`), spawn.rs, cgroup.rs, tests/tree.rs; kernel.rs (`completion_with`, `reconcile_with`,
`startup`, `lock`), tx.rs (`Kernel::frame`), locks.rs, outbox.rs, stops.rs; theseus-core's rpc/driver.rs.

**What changed since the issues were written** (the code and AGENTS.md win; report each difference):
- **Spawn without fork** (theseus-ypqg): the wrapper's `setsid` is the first act of `run_wrapper_process`, and the
  L0 command starts by the kernel's own spawn (spawn.rs), not std's.
- **A cgroup per L0 job where delegated** (theseus-a5nv): such a job is stopped by its cgroup, then what is left of
  its tree. tests/tree.rs runs undelegated, so its stops go by `tree::stop` (`scope: descendants`), as on this VM.
- kernel.rs is at 2,992 lines of its 3,030 ceiling (scripts/long-files.txt): there, only an arm, a field or a call.

**What to build,** each a green commit:
1. **theseus-g11i.** `the_deadline_stops_the_whole_tree_too` (marker 300.1802: a script starts a `setsid` sleeper
   and execs a second sleep; deadline 1 s) failed once in a gate at a load near 7: just after the completion could
   be read, its `/proc` scan found both sleepers running. It passed every other run. It scans before it reads the
   verdict, so the failure hid what the stop said. Tell three readings apart:
   - (a) the stop gave up: past `KILL_WAIT` (500 ms) or `FREEZE_LIMIT` it writes its survivors and the completion
     goes out, while a starved process with SIGKILL pending keeps its command line until it runs;
   - (b) the stop's scan missed them: `descendants` reads each task's `children` file, which proc(5) calls reliable
     only while the children are stopped (an exiting child can hide a live one), and a scan that finds nothing
     ends phase 1 at `live.is_empty()`, verified;
   - (c) the stop was right and the test's scan was wrong.

   First make a failure say which: the case prints the completion's `detail.stop` and, for each pid it found, its
   `/proc` state, start time, parent and pending signals. Reproduce under load (the case at nice 19 beside four
   busy loops; many runs of the binary with the case's name and `--exact`), recording what each failure printed.
   Then fix the stop so that no completion or cancel verdict is written while a process it reports stopped still
   runs: for (a), a kill's wait that ends when each frozen process has exited (its pidfd polled), bounded far past
   500 ms; for (b), an emptiness test that cannot miss (a child subreaper inherits every orphan of its tree, so in a
   wrapper process `waitpid` answering ECHILD after the reap means none are left). Fix what the runs show, and say
   why; the cancel's and the cgroup's stops share this code. Add a test that fails on the old code
   without load (an injected scan that omits a live process, or a tree kept off the CPU).
2. **theseus-jnnj.** A spooled completion has two takers since Tier 7.1, the drain and the turn waiting on its job,
   and `completion_with`'s take path reads only `Succeeded` and `Failed` as taken: when both read a cancelled job's
   late file, the action is written again and a second `completion.late_after_cancel` row appears (the budget is
   guarded: `completions_seen == 1`). Read a `Cancelled` action whose `completions_seen` is 1 or more as
   `Accepted::Taken`. Test: cancel a dispatched job, take its late completion twice with `take_completion_with`: one
   row, one write.
3. **theseus-m9iy.** A provider call in flight when the daemon dies stays `dispatched` after the restart until its
   deadline (`[model.timeouts] total_secs`, 600 s), when the heartbeat marks it unknown (`overdue_no_evidence`),
   and the requeued turn waits on it. An in-process call has no wrapper, spool file or evidence, and the process
   that ran it is gone. Startup's step 4 (`reconcile_with(evidence, false)`) already scans every dispatched action
   and skips the ones not overdue. In that scan, collect the in-process calls (at startup every one is an earlier
   process's) and mark them `outcome_unknown`, with a reason of their own, once the socket serves, as due wakes
   wait for the driver's first tick (DD8): nothing new on the start path beyond the scan, no frame before serving,
   one frame for all if you can. Tell an in-process call from a job (the action's `tool`, a provider call's
   `provider.messages`, or the core's `Evidence`) with no store format change, and say how. Jobs stay as they are
   (a wrapper has evidence). Check that what waits on such a call wakes when it is marked, and say what a budget
   reset (`reset_budget`) frees of it.
4. **theseus-oqxw** (v1.1's V4). A `Kernel::frame` closure that calls the kernel itself, not its view, on an
   execution the frame did not name takes that lock while holding others, out of id order: two such threads
   deadlock, and nothing catches it. Make `ExecLocks::lock_all` panic when the calling thread already holds any
   lock ("a lock taken while this thread holds another: name it in the one Kernel::frame"), releasing what it took,
   as it panics on the same id. First check that nothing nests on purpose (the outbox's transitions, `outbox_stage`
   in a turn's end frame among them, the observer, startup's and the reconcile's scans, a task and its parent's
   `locked_action`), and say what you found. The check is on every transition's path: O(1), a count per thread.

**Proof, offline:** for 1, the reproduction's failure counts before and after the fix at the same number of runs
under load, each failure's printout, the load-free test, and tests/tree.rs whole 20 times under load; for 2 and 4,
their tests (4's panics, writes nothing, and fails fast when no panic comes); for 3, a restart test (a
provider call and a job dispatched, the kernel dropped and reopened: the call unknown at the first tick after
serving, the job still dispatched, startup's frames unchanged). Then the kernel's suite, tests_frames.rs's golden
(say why any line moved), and `theseus-sim kernel-sim --seeds 40` at `--p-race 0` and 0.3, every invariant held.
Planted reverts, each naming the test it breaks: the stop's old ending; the `Cancelled` arm removed; the startup
collection removed; the held-lock check removed.

**The live check is the maintainer's**, on a 16-core machine. Write exact commands for:
1. `the_deadline_stops_the_whole_tree_too` 300 times at nice 19 beside one busy loop per core (`nproc`), on main's
   build and yours: main's failures, if any, print their verdict; yours none.
2. A scratch daemon (fresh state dir, Discord and the web off) whose profile's provider is a local listener that
   accepts and never answers (a few lines of python3): a turn, `kill -9` while its call waits, a restart: within
   seconds of serving the call is `outcome_unknown` with the new reason and the session ready; a job another session
   started before the kill (on the stand-in model, `theseus-sim fake-model --rules`) still completes.
3. `theseus-sim bench lifecycle --check` as main's.

**Leave alone:** crates/theseus-sim (sim2, a sibling, teaches kernel-sim `/stop`, wakes and the outbox: run it,
change none of it, and name any invariant your fixes move); the core's tool run and theseus-tools' proc.rs
(proc-steps); rpc/server.rs and bus.rs (push-once); disk.rs and the heartbeat's other work (health-words); the
Discord outbox and streaming (lane speed).
