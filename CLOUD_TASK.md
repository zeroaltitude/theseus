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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-bench-async-measured`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: both async arms measured as the efficiency track measures them, and the async bench's missing tests (theseus-z5ty; also theseus-6xre)

Branch: `cloud/20261005-bench-async-measured`. Every commit's subject carries its issue id. Deadline for the report:
5 hours after you start.

**Background.** The async bench (bench/async/) and the efficiency track (bench/harbor's sampler and record) joined
main together but don't meet, so the async report's CPU and RAM stay empty:
1. score.py's `efficiency()` reads top-level `cpu_s` and `peak_rss_mb` (and such); the record (`bench-efficiency/1`)
   nests them: `harness.cpu_s`, `harness.peak_rss_kb`, `work.cpu_s`, `wrappers.cpu_s`, `container`.
2. TheseusAsync runs no sampler: its `run` drives `daemon_script`, and the inherited `populate_context_post_run`
   writes a record with `sampler.status: missing` and the spend of `theseus-turn.json`, the first ask only. That
   leaves out the injected turn, the job's continuation and any task's session, which `theseus-calls.json` (the
   daemon's `provider.call` rows) holds; Harbor's counters already read it. In a live cancel trial the record said
   $0.0168 and 2 model calls, and the ledger said $0.0423 and 8.
3. ClaudeCodeAsync is on Harbor's `ClaudeCode`, not `MeasuredClaudeCode`.

Then theseus-6xre: two untested driver paths, a stand-in race, and stand-ins left running.

**Read first:** bench/README.md, bench/async/README.md; bench/async's async_agents.py, driver.py (`Theseus.settle`,
`finish_script`, `spend`, `claude_stdin` and its commands), score.py, test_driver.py (the stand-ins, `TheseusTrial`,
`EndToEnd`); bench/harbor's efficiency.py (`record`, `theseus_record`, `claude_code_record`, `result_event`),
sampler.py (`start_script`, `stop_script`), theseus_bench.py (`run_script`, `daemon_script`), theseus_agent.py,
claude_code_agent.py; bench/report/efficiency.py (`load_trial`'s `sampled`).

**What the code says** (the code wins; report each difference):
- **Harbor 0.23.0 needs Python 3.12 or later.** Run the standard-library suites on python3, and Harbor's in a venv
  (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`), where tests that import Harbor run
  with `ASYNC_HARBOR=1`.
- **A `provider.call` row's data** carries `model`, `usage` (Anthropic's four keys) and `cost_usd`: the ledger bills
  by model. Tool calls are no ledger kind but each history's `tool_call` nodes, and `finish_script` reads only the
  conversation's history: say what that misses of a task's session.
- **The driver's own CLI calls are harness by name** (settle's `executions`, `wakes` and `wait`; the finish's reads):
  all run `theseus`; Claude Code's driver reads its log with grep and wc, which land outside. Keep the polls out of
  the harness (for instance through a link of another name, so their `comm` differs), or count them and say what
  they add. The two asks are the arm's own client and stay harness.
- **The record reads the stream's last `result`.** In stream-json input the CLI writes one per turn; a message taken
  mid-turn joins the running turn (seen live: one result, `num_turns` 4). Whether each result's `modelUsage` and
  `total_cost_usd` is the session's so far or the turn's needs a live two-message trial.

**What to build,** each a green commit:
1. **z5ty, the scorer.** `efficiency()` reads the record's own fields (`agent/efficiency.json`, else result.json's
   `metadata["efficiency"]`): harness CPU, peak harness RSS in MB, and work CPU beside them, only when the sampler's
   status is `ok` or `running` (bench/report's rule); the columns named for what they hold. Test over a fixture
   record of the real shape, made by `efficiency.record`. Update the async README's CPU and RAM line.
2. **z5ty, TheseusAsync measured.** The sampler starts before the daemon and stops after the finish's clean stop
   (on Harbor's timeout path too: the shielded finish), with theseusd's job wrappers apart as `run_script` has them
   (`ARMS["theseus"]`), detached so it outlives the first command. Recommended: `daemon_script` and `finish_script`
   take the sampler as `run_script` does, so the stand-in tests and EndToEnd run it. The record from the ledger: a
   function of its own at efficiency.py's end, reading theseus-calls.json's rows by model (`spend_from: "ledger"`,
   the calls its rows, the cost their sum), the sampler's summary and the trial's wall, written after the inherited
   record, replacing efficiency.json and `metadata["efficiency"]`. Say what the rows leave out that a turn's totals
   count (a call a stop cut, a failed call), and whether another row holds it.
3. **z5ty, ClaudeCodeAsync on `MeasuredClaudeCode`** (`class ClaudeCodeAsync(cca.MeasuredClaudeCode)` and the
   import). Its sampler starts through `environment.exec`: check the FIFO rewrite in `exec_as_agent` is untouched.
   For the results, code for one reading, say which and why, keep the other one function away, and have the record
   count the results it saw; test with a fixture stream of two.
4. **6xre:** (a) the stand-in `theseus`'s `wakes` lists one wake due about a second ahead until it is due, and settle
   must not return `settled` before it; (b) a test that runs `ClaudeCodeAsync.run` itself with a fake environment and
   the stand-in `claude`, Harbor's `ClaudeCode.run` patched to issue `HARBOR_RUN`, asserting both messages were
   answered before the input closed (with `ASYNC_HARBOR=1`); (c) the stand-in's `health` exits 1 until `daemon.env`
   exists, and the stand-in daemon takes a start delay from the environment, so the race is a test; (d) the stand-in
   `theseusd` exits once its directory is gone, and `TheseusTrial`'s tearDown waits for its pid (`theseusd.pid`)
   after `shutdown`; (e) the FIFO tests stop their CLI, its tee and the FIFO's holder by pid, pass or fail.

**Your suites** (the preamble's bench/ rule): `-s bench/async`, `-s bench/harbor`, `-s bench/report`, under python3
and under the venv with `ASYNC_HARBOR=1`; say each count and what skipped.

**Proof, offline:** EndToEnd on this workspace's build (`ASYNC_E2E_BIN=$PWD/target/debug`, from bench/async) with
the sampler around the trial: the sampler `ok` with theseusd among the harness's processes, the record's
`spend_from` `ledger` and its calls equal to theseus-calls.json's rows, and health's cgroup line. The suite 5 times
under the load recipe. After every run, plants too, no stand-in left (list processes whose command line names one
of the run's temp dirs): one left is a finding. Planted reverts, each naming the test it breaks: A3, settle's
`if not live and not pending:` made `if not live:` (breaks (a)'s test); A4, ClaudeCodeAsync closing the input
right after the injection (breaks (b)'s); score.py's top-level keys back; TheseusAsync's record from
`theseus-turn.json` again; the health fix reverted (the delay set); the tearDown's wait and the own-dir check
removed.

**The live check is the maintainer's** (bench/async/README.md's environment). Write exact commands for:
1. `.venv/bin/harbor run -p bench/async/tasks -i interrupt -a async_agents:TheseusAsync -m
   anthropic/claude-sonnet-5-5 -o jobs --job-name am-theseus`: efficiency.json's `spend_from` `ledger`, sampler `ok`,
   calls equal to theseus-calls.json's rows, cost equal to result.json's.
2. The same with `-a async_agents:ClaudeCodeAsync --ak max_budget_usd=2.0 --job-name am-claude`: settled well inside
   its timeout, the record sampled.
3. `python3 bench/async/score.py jobs/am-theseus jobs/am-claude --out /tmp/am`: every CPU and RSS cell filled.
4. The CLI in stream-json outside Harbor (a scratch directory and `CLAUDE_CONFIG_DIR`), two messages needing no
   tool, the second after the first result: the second result's `modelUsage` against the first's and the session
   log's per-message sum.

**Leave alone:** bench-efficiency-fixes beside you (`machine()`, `theseus_spend`, test_efficiency.py's `SUMMARY` and
`Machine`, sampler.py, test_sampler.py, bench/report/): add at efficiency.py's end, change none of theirs, test in
a file of your own; bench-recall-fixes (bench/recall/); the families' tasks; `bench/theseus-bench.toml`; all Rust.
