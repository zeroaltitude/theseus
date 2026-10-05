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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-sim2`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: kernel-sim drives `/stop`, report and scheduled wakes, and the outbox under crashes, and its put-back check is made deterministic (theseus-celu.35; also theseus-81ig)

Branch: `cloud/20261005-sim2`. Every commit's subject carries the id of the issue it serves: theseus-81ig for the
check, theseus-celu.35 for the rest. Deadline for the report: 5 hours after you start.

**Background.** `theseus-sim kernel-sim` is the kernel's simulator: a virtual clock, a real store and spool, seeded
crashes and faults, and invariants checked over the whole store after every step. v1.1's lane sim2
(docs/design/roadmap-v1.1.md, theme 6): its random operations never drive `/stop`, a scheduled wake, a report wake
or the outbox, and inject no crash around the outbox's transitions. Before V7 merges a plain turn's frames, the sim
must drive every transition a turn can take, under crashes and `--p-race` over many seeds. You change the sim,
never the kernel: a kernel bug the sim finds is reported with its seed, its flags and the invariant's message, and
the shortest `--steps` that shows it.

**Read first:** theseus-sim's AGENTS.md; src/kernel_sim.rs (its module docs list every invariant; `step`, the
race, `maybe_crash`, the TOTAL line), src/kernel_sim/wakes.rs, src/main.rs's kernel-sim flags, tests/sim.rs;
theseus-kernel's AGENTS.md, stops.rs (`stop_execution`, `stop_call`), wakes.rs (`fire_due`, a task's wakes),
tasks.rs (`take_reports`, a parent's `report_wakes`), outbox.rs, tx.rs (`Kernel::frame`), repeat.rs;
.config/nextest.toml.

**What changed since the roadmap's line was written** (the code and AGENTS.md win; report each difference):
- **37a's wakes are in the sim** (kernel_sim/wakes.rs): turns set wakes, half of them repeating, some with an
  `until`; turns take the due ones; the operator cancels some; and every check holds each execution's pending
  wakes to their rules. The heartbeat's reconcile fires due wakes (`fire_due`). Build only what is missing, and say
  what was there.
- **The step's picker** (`step`): an open (conversations, and tasks with no parent), a turn, a limit change, a
  cancel, a budget or confirm answer, a wake's cancel, input, and the heartbeat. So no task has a parent, and no
  report wake ever fires.
- **Held posts and the credential request are gone from main** (the place rule removed held posts; theseus-w5op
  the requests). An earlier attempt drove both: drive neither.
- **A turn's end stages its posts inside its frame** (`outbox_stage` in `Kernel::frame`).
- **kernel-fixes, a sibling batch-7 task**, changes the kernel beside you: a nested lock panics (theseus-oqxw), a
  cancelled job's late completion taken twice writes one row (theseus-jnnj), and an earlier process's in-process
  calls settle unknown after a restart (theseus-m9iy). Your sim runs main's kernel; write each invariant so it
  holds on both, and name in the report any count their fixes will move.
- src/kernel_sim.rs is 2,349 lines, and a Rust file past 2,500 fails the gate's shape phase unless
  scripts/long-files.txt lists it: each new operation goes in a module of its own under src/kernel_sim/, as
  wakes.rs is; in kernel_sim.rs only the `mod` line, the step's roll, the race's arm and the report's counts.

**What to build,** each a green commit:
1. **theseus-81ig first.** tests/sim.rs's `the_kernel_holds_its_invariants_under_seeded_faults` asserts
   `count(" series put back") > 0` over `kernel-sim --seed 1 --seeds 2 --steps 300`, a run whose second thread
   races turns (`--p-race`, 0.3 by default). A raced run reproduces from its seed only up to its first race, so
   whether a repeating wake is taken varies: under load about 1 run in 6 to 20 says "0 series put back" with every invariant
   held. Make the check deterministic: count it on a `--p-race 0` run (fully reproducible) or a scenario sure to
   take a repeating wake; check the test's other coverage counts the same way (any the interleaving can zero moves
   too); and take the theseus-81ig override off .config/nextest.toml in the same commit. Show 50 runs under load
   (the preamble's recipe) with no failure.
2. **`/stop`.** An operation that stops an open execution (`stop_execution`; a single call's `stop_call` too), now
   and then as a race's arm while a turn commits, and with crashes around it. Invariants, from stops.rs: the
   execution stays open, with its session, budget, spend, pending wakes and tasks; every job and call it had
   running is told to stop and settles; everything planned and unsent, and every open approval or budget
   question, is declined; a running turn plans nothing more (`KernelError::Stopped`), and its end parks the
   execution on input; its next input runs a turn. A stop is not a cancel: the "never runs again" check reads
   only `execution.cancelled`.
3. **Report wakes, and what scheduled wakes still lack.** Tasks opened under a parent (`open_task`, with and
   without `wake_parent`; depth one; the carve, DD7): the frame that ends one puts it on the parent's `reports`
   (`take_reports`); one opened with `wake_parent` that finishes or fails also asks for the parent's turn
   (`report_wakes`), which a busy parent keeps until the frame that frees it; reports that land together start one
   turn; a cancelled task wakes nothing; a task whose last wake is cancelled goes on to end and report (37b).
   Invariants: each report reaches its parent once, across crashes, never lost or twice; no session outspends its
   limit through its tasks, and the carve is released at the end but for what is in flight. For scheduled wakes,
   add what 37a's module misses (a wake that falls due while its execution is busy, kept and queued by the turn
   that frees it), and say what it is.
4. **The outbox under crashes.** Posts staged in a turn's end frame and outside a turn (`outbox_plan`), dispatched
   and settled by a fake binding that sends under each post's downstream key, with crashes after each transition:
   after the stage, after the dispatch before the send, after the send before the settle, after the settle.
   Invariants, from outbox.rs: every staged post is delivered once its binding runs; the fake channel keeps one
   copy per post (a re-send after a crash reuses its key); a second settle changes nothing; no execution waits on a
   post; neither a cancel, a stop nor its execution's end touches it; the reconciler never reads one.
5. **The gate's run.** tests/sim.rs asserts that each new operation ran, on its deterministic run, and its time
   stays near main's (measure both).

If a new invariant fails on a seed, that is a finding: report it as above and keep the check. If it fails on the
gate's fixed seeds, keep that operation out of the gate's run behind a flag the long runs pass, and say so.

**Proof, offline:** `theseus-sim kernel-sim --seeds 40 --steps 1000` at `--p-race 0` and at 0.3, every invariant
held, each new operation counted in the TOTAL line; the same command twice at `--p-race 0` with identical output;
tests/sim.rs 50 times under load. Planted bugs, each restored and never committed, each naming the invariant and
seed that catch it: in the kernel, `stop_execution` leaving a planned action authorized, a task's end that queues
no report for its parent, an `outbox_settle` that rewrites a settled post; in the fake binding, a re-send under a
new key.

**The live check is the maintainer's.** Write exact commands, release build, on a 16-core machine:
1. `theseus-sim kernel-sim --seeds 200 --steps 1000` at `--p-race 0` and at 0.3: all invariants held, and the new
   operations' counts.
2. The gate three times, the sim test passing with no retry (its override is gone).

**Leave alone:** theseus-kernel (kernel-fixes is changing tree.rs, job.rs, kernel.rs and locks.rs: never fix the
kernel here); theseus-sim's perf.rs, synth.rs and lifecycle.rs, and main.rs beyond kernel-sim's flags
(benchmarks and their rows); every other override in .config/nextest.toml.
