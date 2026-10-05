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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-bench-async`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the async bench: six families of local Harbor tasks where concurrency is the point, a driver that injects a message mid-trial, and a scorer (theseus-7gir.16)

Branch: `cloud/20261005-bench-async`. Every commit's subject carries its issue id. Deadline for the report: 5 hours
after you start.

**Background.** Theseus's async means: a `proc.run` that outlasts `[tools] proc_sync_secs` (60 by default) goes on as
a background job whose late result comes back as a continuation; `task.create` starts a child session that reports
back; `wake.at` sets a later turn; the daemon owns every wait (`theseus wait`), so nothing polls. Claude Code has
background commands and parallel subagents (Harbor's adapter sets `ENABLE_BACKGROUND_TASKS=1` and
`FORCE_AUTO_BACKGROUND_TASKS=1`); OpenClaw, a third arm not yet built (leave it room), background exec, subagents and
automations. This bench is our own containerised tasks, run through Harbor 0.23 by every arm with the same model,
limits, attempts and wall clock, each by its own async means. An instruction states the task, never how to run it
("in parallel", "in the background"): one text for every arm.

**Read first:** bench/README.md and bench/harbor/ (`test_bench.py`'s stand-in `theseus` is your tests' pattern);
crates/theseus's `main.rs` (`ask -s`, `sessions open`, `wait`, `stop`, `tasks`, `wakes`, `shutdown`); theseus-core's
`toolrun/job.rs`, `task.rs`, `tests_continuations.rs`; theseus-sim's `fake_model.rs`. If `pip install harbor==0.23.0`
works in a venv: its `agents/installed/claude_code.py`, `models/task/paths.py`, `verifier/verifier.py`.

**What changed, and what the code says** (the code wins; report each difference):
- **The adapter is headless:** `theseus --spawn theseusd --json ask -` is one turn on a stdio daemon that stops after
  it, so a job still running is never read (hence the profile's `proc_sync_secs = 900`). The async arm keeps a real
  daemon for the trial at the product's default; say what you set and why.
- **A message while a turn runs:** read what main does with a second `ask -s` (queued or refused), and inject as the
  CLI allows.
- **Tasks have their own sessions:** one session's history misses their calls. A trial's spend is every
  `provider.call` row in its daemon's ledger.
- **Jobs:** an L0 job gets a cgroup only where one is delegated, else its process group stops it: say which the
  container gave.

**What to build,** each a green commit:
1. **The tasks:** `bench/async/tasks/<family>/`, each a Harbor task (`task.toml`, `instruction.md`,
   `environment/Dockerfile` from a small public image with python3, `solution/solve.sh` the oracle, `tests/test.sh`).
   Its tools (`/opt/async/bin`, standard-library Python) append JSONL to a ledger outside the working directory:
   starts, ends, effects, violations, each with pid, wall and monotonic times. A step's duration is drawn at its
   start and logged; `ASYNC_TIME_SCALE` shortens it for tests. The tests check the ledger agrees with itself (an
   edited one fails) and copy it to the verifier's log directory. The plan's families:
   - **parallel:** K independent slow steps, then an aggregate (the ideal wall is the longest step);
   - **wait-tax:** one slow job ends at a random time, and its result is needed;
   - **interrupt:** a long job, then an injected second request whose answer goes to a file; the long job finishes;
   - **fanout:** six subtasks, two failing transiently; the aggregate complete, each effect once;
   - **cancel:** a long job cancelled by an injected message; the tests count its processes still alive;
   - **contention:** two concurrent subtasks on a no-lost-update ledger, and a tool allowing N calls at once.
2. **The driver** (`bench/async/driver.py`): each family's injection (a message, a trigger: a ledger event plus a
   delay, else a time; the trigger that fired recorded) and an injector per arm:
   - **Theseus:** `-a async_agents:TheseusAsync`, a subclass in `bench/async/` of the adapter's agent: `theseusd`
     for the trial, the instruction and the injection by `ask -s`; the trial ends when the session settled with no
     job, task or wake left, or at the task's timeout. At most one function joins `theseus_bench.py`: the
     daemon-mode script, beside `run_script`.
   - **Claude Code:** `-a async_agents:ClaudeCodeAsync`, a subclass of Harbor's `ClaudeCode` running its command with
     stdin from a FIFO in the CLI's stream-json input mode, where the driver writes the second message (check
     `claude --help` in the image; say what you found), the FIFO in one overridable method. If the CLI can't take a
     message mid-run, its interrupt and cancel cells read "not measurable": never a second process on the session.
3. **The scorer** (`bench/async/score.py JOB_DIR... --out DIR`, standard library): from each trial's `result.json`,
   ledger, ATIF `trajectory.json` and, if present, `agent/efficiency.json` (bench-efficiency's record, beside you:
   read, never require), `report.md` per arm and family: success; wall over the ideal (the ledger's durations); the
   wait tax (model calls and tokens with step timestamps inside the slow job's window); responsiveness (injection to
   the right answer's ledger line); orphans and duplicated effects; CPU and RAM.
4. **`bench/async/README.md`:** each arm's command (`harbor run -p bench/async/tasks ...`), the scores, and the
   public neighbours CooperBench (`cooperbench@1.0`) and BFCL's parallel categories (`bfcl@1.0`), run later,
   locally: build nothing for them.

If time runs out, leave contention, then cancel, and say so.

**Your suites** (the preamble's rule for bench/): `python3 -m unittest discover -s bench/async && python3 -m
unittest discover -s bench/harbor`, on the standard library alone.

**Proof, offline** (no Harbor, no Docker): each family's tests pass on its oracle's effects (`solve.sh` on this host
under a scratch root: paths from `ASYNC_ROOT`, tools on PATH, a small time scale) and fail on a planted wrong one per
family (a missing aggregate part, an abandoned long job, an effect twice, an orphan left running, a lost update, N
exceeded); the driver against a fake environment, and the daemon-mode script under a stand-in `theseus`; the scorer
over fixture trial directories against numbers worked by hand. If time allows, end to end: this workspace's
`theseusd` on `theseus-sim fake-model --rules` making the oracle's `proc.run` calls, the injection answered while
the job runs. Planted reverts, each naming the test it breaks: the ledger check skipped; an injection firing before
its event; the wait tax counting calls outside the job's window; the trial ending while a job runs.

**The live check is the maintainer's** (Docker, Harbor, an Anthropic key, a few dollars), from the root with
`bench/build.sh`'s export, `PYTHONPATH=$PWD/bench/harbor:$PWD/bench/async`, `HARBOR_TELEMETRY=0`:
1. `harbor run -p bench/async/tasks -a oracle -o jobs --job-name async-oracle`: reward 1 on every family.
2. `harbor run -p bench/async/tasks -a async_agents:TheseusAsync -m anthropic/claude-sonnet-5-5 -o jobs --job-name
   async-theseus -k 2`, then `-a async_agents:ClaudeCodeAsync --ak max_budget_usd=2.0 --ak max_turns=200 --job-name
   async-claude`: each ledger names the trigger that fired; Theseus's daemon stops at the trial's end.
3. `python3 bench/async/score.py jobs/async-theseus jobs/async-claude --out /tmp/async`: a row per family and arm,
   "not measurable" only where a CLI can't be reached mid-run.

**Leave alone:** bench-efficiency, beside you (the sampler, the record, `MeasuredClaudeCode`, `bench/report/`,
bench/README.md: say in your report what bench/README.md should link); bench-recall (`bench/recall/`); the `steps`
array `proc.run` is gaining (no family may need it); `bench/theseus-bench.toml`, docs/benchmarks.md, all Rust.
