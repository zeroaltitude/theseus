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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-telemetry3`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: two tests of the resumed calls' spans, cancels as a metric, and the index tender's gauges and restarts (theseus-kxyc; also theseus-6xwq, theseus-qdk5, theseus-gfi4)

Branch: `cloud/20261005-telemetry3`. Every commit's subject carries its issue id. Deadline for the report: 5 hours
after you start.

**Background.** v1.1's lane telemetry2 (docs/design/roadmap-v1.1.md, theme 2). Its last step joined just before you
start: each tool call counts once, at its answer, timed by its run (`run_ms`), and is traced in the turn that answered
it. Four gaps:
1. **theseus-kxyc.** A late result that settles while a turn of its execution still runs is taken by that turn's
   `finish` (`TurnRunner::take_late`, turn/calls.rs), not by a later turn's `catch_up`, and traced at the trace's top
   level: the only place such a job counts. With `take_late` pushing no span, every test passes.
2. **theseus-6xwq.** Calls [A, B, C] where A waits for the operator (theseus-a60): the continuation answers A, then
   runs B and C through `run_fresh`, and `ResumeOutcome::ran_batch` (toolrun.rs) shifts their indices and groups past
   A's. With `index: r.index`, B's span is named for A, C's for B, C has none, and `theseus.tool.calls` counts A twice
   and C never; every test passes. The group shift is untested too.
3. **theseus-qdk5.** The M4 design (m4-boundaries.md §2.11) names `theseus.cancel{backend,state}`. Health counts
   cancels by backend and state (`HealthResult.cancels`, `Stops::count` in cancel.rs); no metric exists.
4. **theseus-gfi4.** M6 §2.13: the index's lag, documents and RSS as metrics. Health shows the tender (`index`,
   `children.tenders`); nothing is exported.

**Read first:** theseus-core's AGENTS.md (a call's span, and when it counts); turn/calls.rs; toolrun.rs (`Ran`,
`ResumeOutcome`), toolrun/resume.rs (`run_fresh`), toolrun/late.rs (`absorb`); turn.rs (`catch_up`, `finish`);
telemetry.rs (`record_*`); telemetry/metrics.rs (`Kind`, `INSTRUMENTS`, `tool_calls`, `node_cache`);
telemetry/tests_resumed.rs whole and the helpers it imports; cancel.rs (`Stops`, `Ended`, `job_backend`);
tests_cancel.rs; tender.rs (`status`, `health_block`, `HEALTH_DEADLINE`, `start`), tests_tender.rs (`FakeOs`,
`stand_in`); theseus-protocol's index.rs (`IndexStatus`, `TenderStatus`); theseusd's main.rs (`after_serving`); the
spec's §3.20 (docs/spec/).

**What changed since the issues were written** (the code wins; report each difference):
- **A count outside a turn has a path already.** qdk5 says the core's metrics are a projection of a turn's end. Since
  then `record_push` (push.rs), `record_push_lost` (rpc/server.rs), `record_judgment` (judge/sink.rs) and
  `record_durability` (aws/durable.rs) add outside turns to the cumulative aggregate posted every
  `metrics_interval_secs`. So the cancel needs a `record_*` call where it counts, not a new export path. `Stops` (in
  `ToolRuntime`) holds no telemetry handle: give `Stops` one, set once as the core is built, or count in the core
  from the `Ended`s a cancel returns (`written` is health's rule); say which. Either way toolrun.rs gains no line, and
  health's counts and the metric agree by construction.
- **A label to check:** an L0 job stopped by its own cgroup (a delegated daemon, theseus-a5nv; theseus-kernel's
  `stop_tree`) records `VerifiedBy::Cgroup`, which `job_backend` names `l1`, a name meant for L1's older records. If
  the action tells its level, name it `l0` there, with a test; else report it.
- **Gauges exist** (`Kind::IntLast`), but nothing samples on the interval: the node cache is read as a turn ends.
  Recommended for the tender: a task started after serving beside it (`after_serving`), only while telemetry has an
  endpoint and `[index]` is on, asking `health_block()` (no socket before a tender runs; 100 ms) every interval. Or a
  sampler the exporter's tick calls: say which, and why.
- **Restarts:** `TenderStatus.restarts` counts since the start; a counter fed by its rise between samples, as
  `node_cache`'s counters are, needs no hook in the supervisor. Say what the gauges show while the tender is down or
  late (`health_block` gives its last answer with its age, or none).
- **`INSTRUMENTS`** is `[&Instrument; 34]`: add at its end; the maintainer sums the length at the merge.
- **Line counts:** telemetry/tests.rs is 2,484 lines (2,500 fails) and a sibling edits it: add nothing
  there. toolrun.rs is 2,494: nothing there but the plant.
- **FAST:** nothing on the start path or the turn path; the sampler asks at most once an interval, after serving, and
  nothing runs while telemetry is off.

**What to build,** each a green commit:
1. **kxyc.** In tests_resumed.rs: a job that goes to the background (`proc_sync_secs = 1`) and settles while a turn
   of its execution still runs, so that turn's `finish` takes it (for example the model's next answer held past the
   settle with `FakeProvider`'s `delay_ms`, while a task drains the spool with `core.heartbeat`, as `wait_queued`
   does). Make it deterministic, and say how. Assert one `tool proc_run` span, `late: true`, `result: ok`, `run_ms` at
   least the job's run, at the trace's top level (no `continuation` parent), and `theseus.tool.calls` counting it once.
2. **6xwq.** Three scripted calls, the first asking (proc.run under `approve`), then two fs.read (open); approve the
   first. Assert three `tool` spans under `continuation`, each named and `tool_use_id`'d for its own call, in order,
   A's alone, the reads under one `tools` span if they ran together; `theseus.tool.calls` counts proc.run once and
   fs.read twice.
3. **qdk5.** A `theseus.cancel` counter (`theseus.cancel.backend`, `theseus.cancel.state`, valued as health names
   them), fed where health counts. Tests in a file of their own, a whole core posting to the receiver: an async tool's
   cancel (as tests_cancel.rs makes one) and a job's give two series of 1, equal to health's `cancels`.
4. **gfi4.** Gauges of the tender's lag (bytes, ms), documents and RSS, and a restarts counter, under
   `theseus.index.*` (named as §3.20's others). Tests with tests_tender.rs's stand-in and fake OS: a sample records
   the stand-in's numbers; a kill and restart raise the counter by one; a hung stand-in keeps each sample within the
   deadline; nothing asks before the tender runs, and nothing samples with telemetry off. If (4) outgrows the time,
   build (3) and report (4)'s shape.

**Proof, offline:** the tests above; every telemetry test, tests_cancel.rs and tests_tender.rs; the conformance test
(`testdata/old-exporter.json`) as it was; the core's output golden unchanged; the telemetry and tender tests 3 times
under load. Planted reverts, each naming the test it breaks: `take_late` pushing no span; `index: r.index`, then
`group: r.group`, in `ran_batch`; the cancel's feed removed; every cancel fed as `verified`; the restarts counter fed
the total instead of its rise; `documents` read from `nodes`.

**The live check is the maintainer's.** Exact commands for a scratch daemon: fresh state dir, Discord and the web off,
`[telemetry] otlp_endpoint` at a loopback OTLP/HTTP sink (python3's http.server saving each POST),
`metrics_interval_secs = 5`:
1. `[tools] proc_sync_secs = 1`, a stand-in model rule (`theseus-sim fake-model --rules`) calling `proc_run` with
   `sleep 60`, then `theseus executions cancel <id>`: the sink's `theseus.cancel` has one point (`l0`, `verified`),
   and `theseus health`'s `cancels since the start:` line agrees.
2. `[index] enabled = true`, theseus-index beside theseusd: each post's `theseus.index.*` gauges match `theseus index
   status`; `kill -9` the tender (its pid in health's `children:` line): after its backoff, restarts reads 1.

**Leave alone:** timing-flakes (qjd6's fix in telemetry/tests.rs); turn.rs, turn/calls.rs, toolrun.rs and toolrun/
(the joined counting rule: plants only); queue-frames (turn.rs's end) and turn-stack (`TurnRunner::run`'s entry);
tender.rs's supervision (a hook only if the rise can't serve; say so); cancel.rs's stops and verdicts.
