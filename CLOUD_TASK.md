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

**What main holds.** You clone main at faaa9df6 or later, with store format 17. These joined main last night, so your clone has them:
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
- the refusal fallback: a refused request goes once to its model's fallback (Sonnet 5.5's is Sonnet 5), and every surface says so;
- files, part one: every surface accepts any file, and PDFs reach the model (the Discord attachments, `AttachmentContent`, `fs.read`, `web/fetch.rs`, the compiler's document blocks; store format 17);
- context-honesty: the system header says how a request is assembled, and recall's notes say the harness chose them (`turn.rs`'s `ASSEMBLY`, `recall/render.rs`'s preamble);
- FSRS-6 retention and the `+retention` arm.

**Other changes in flight.** About twenty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Under review now, merging into `main` one at a time over the next hours (your clone may hold some of them):
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop: labeled examples into packs, thresholds re-fit from calibration.

On the owner's machine:
- files, part two: notebooks, Office documents, EPUB, archives, audio and video reach the model (the attachment readers beside part one's);
- speed: a confidence bar per routing mode, Jev's connection kept warm, and a reply that reaches Discord before its settle's sync (`config/routing.rs`, `turn/route_step.rs`, the judge client, the Discord outbox and streaming);

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
- prove-wire-in: `theseus judge prove` from the ledger;
- telemetry-resumed: a confirmed call's run and a background job's end counted once as tool calls;
- telemetry-calls: a failed provider call's `error.type`, and the AWS calls' metrics;
- reader: the reader rule's tool and same-name holes, and `theseus-index --version`;
- wal-sync: a failed fdatasync's frames cut back, and a reopened log's directory synced.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (17 on main today; the changes under review take it to 18, 19 or 20), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-telemetry-resumed`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: a confirmed call's run and a background job's end, timed and counted once as tool calls, with an approved AWS call's request spans (theseus-8pei; also theseus-0zm4)

Branch: `cloud/20261005-telemetry-resumed`. Every commit's subject carries its issue id. Deadline for the report: 5
hours after you start.

**Background.** v1.1's lane telemetry2 (docs/design/roadmap-v1.1.md, theme 2) makes OTel tell the
ledger's story. Its first three steps are on main (a failed turn's spend, every call's first token, a failed tool
span's ERROR status). This is its last, with a gap on the same path:
1. **theseus-8pei.** `theseus.tool.calls` and `theseus.tool.duration_ms` (telemetry/metrics.rs, `tool_calls`) read
   the `tool <wire name>` spans that `trace_calls` (turn.rs) writes for the calls `run_tools` ran. A call is traced
   once, in the turn whose model proposed it, with its outcome there: `awaiting_confirm` when it waits for the
   operator, `background` for a proc.run that outlived `proc_sync_secs`. The confirmed call's run (the continuation,
   `ToolRuntime::resume`) and a job's end (its late result, `ToolRuntime::absorb`) make no span, so their result and
   time never reach the metrics, and proc.run's histogram misses every background job.
2. **theseus-0zm4.** An AWS call that runs after an approval gets its `aws.called` rows but no request spans:
   `Aws::bind` keeps a call's requests until `trace_calls` takes them by tool_use id (`Aws::spans`). Nothing takes a
   resumed call's, so they fall off the bounded list (`TRACED`, 256 calls).

**Read first:** the roadmap's theme 2; the spec's §3.23 (docs/spec/); crates/theseus-core/src/
telemetry/metrics.rs (`tool_calls`, `turn`, `failure`), telemetry/spans.rs (`tool_calls`, `ToolCall`, `failed`),
telemetry/tests.rs (`core_with`, a whole core where proc.run waits for the operator; the tool-metrics test of
`awaiting_confirm` and `background` points), telemetry/tests_files.rs (a small test file's form);
turn.rs (`catch_up`, `run_tools`, `trace_calls`, `call_result`); toolrun.rs (`ResumeOutcome`, `Batch`, `Ran`,
`CallOutcome`); toolrun/resume.rs (`resume`, `run_fresh`, `run_confirmed`, `run_authorized`, `check_dispatched`,
the answers); toolrun/late.rs (`absorb`); fact/mod.rs and fact/turn.rs (`CaughtUp`); aws/mod.rs (`bind`, `spans`);
tests_continuations.rs; the AGENTS.md files.

**What changed since the issues were written** (the code wins; report each difference):
- **C2 is on main:** a fact's `span` writes into its turn's trace, and the runtime's recorder (`TurnCtx::rec`)
  carries none, so nothing in resume.rs or late.rs can make a span. Bring the calls back to the turn, as
  `run_tools` hands `batch.ran` to `trace_calls`: `ResumeOutcome` and `absorb`'s return carry each answered call
  (its tool_use, outcome and times), and the turn builds the spans.
- **A third gap:** `run_fresh` runs, in the continuation, the calls after the one that asked (never gated in their
  turn). They have `Ran`s and no span: trace them too.
- **The continuation's span** (`continuation`, kind `tool`, `CaughtUp`'s) is a leaf. Its calls' spans go under it,
  each built as `trace_calls` builds one (tool, family, backend, result; the call's AWS and language-server spans
  and its judged marks as children). That is 0zm4's fix.
- **A late result's time.** The job ran before this trace began, and a span can't start before the trace's origin.
  Its action keeps `dispatched_at_ms` and `settled_at_ms`. Recommended: a `tool <wire>` span per late result at its
  absorption, with `late: true` and the job's run (`run_ms`), which `tool_calls` takes as the call's time. Or count
  late results without a span, and say why.
- **Counted once.** Decide whether a call counted `awaiting_confirm` or `background` is counted again at its answer
  (two outcomes for one call) or moved. Recommended: moved. Each call counts once, at its answer: in its own turn
  when it ran there; in the continuation when it waited for the operator (approved, declined, superseded, unknown
  after a restart); at its late result for a background job, timed by its run. The proposing turn's span keeps its
  outcome, so the trace still says what happened then. Say which you chose, and the line §3.23 then needs.
- **Batch 6's tiering** (under review, maybe on your main) changes `late_results`' search for the placeholder and one
  filter in resume.rs. Keep your change to `absorb`'s return and to resume's outcomes, away from those lines.
- **turn.rs is near its ceiling** (3,499 of 3,523). Move `trace_calls` and `call_result` to a module under turn/,
  called from `run_tools` and `catch_up`, so turn.rs ends shorter. telemetry/tests.rs has 2,482 of the 2,500 lines an
  unlisted file may: new tests go in a file of their own; edit tests.rs only where an expected point moves.
- **FAST:** the spans are built after the calls ran, from what they returned: nothing waits on them or reads the
  store.

**What to build,** each a green commit:
1. **The continuation's calls traced** (8pei, 0zm4). Every call `resume` answers (run, declined, superseded,
   reported unknown, run fresh) is in `ResumeOutcome` with its outcome and times; `catch_up` writes their spans under
   the continuation's, each with its AWS and language-server spans. A test through `core_with`: a proc.run that
   waits, approved, then the next turn: its continuation holds the call's span, result ok, timed by its run.
2. **Late results traced.** `absorb` returns each late result's call (tool, tool_use id, result, the run from its
   action); the turn writes its span. A test: a proc.run with a `proc_sync_secs` shorter than its sleep, its late
   result taken by the next turn: one span, `late: true`, the job's run time.
3. **Counted once,** by your rule: `tool_calls` counts each call at its answer, by its result, timed by its run. The
   existing tool-metrics test's points change only where the rule says. A test: a confirmed call and a background
   job, across their turns, give one `theseus.tool.calls` point each, outcome ok, and the duration holds their runs.
4. **0zm4's test:** an AWS read under `[policy.aws] read = "approve"`, approved: its request span is under its
   call's span in the continuation, and nothing is left in `Aws`'s list for it.

**Proof, offline:** the tests above; telemetry's tests and tests_continuations.rs as before, but the points your rule
moves (name each); the core's output golden unchanged, or moved only where a continuation's spans show (say where);
`a_plain_turn_stays_within_its_frame_budget` and `theseus-sim bench turn --check`. The new tests 5 times under load.
Planted reverts, each naming the test it breaks: the continuation's spans dropped; a late result timed by its
absorption, not its run; a moved call counted in its proposing turn too; the AWS spans not taken.

**The live check is the maintainer's,** on the stand-in model (`theseus-sim fake-model --rules`, no key: a rule's
`calls` names `proc_run` and its input). Write the rules file and exact commands for a scratch daemon: fresh state
dir, Discord and the web off, `[telemetry] otlp_endpoint` at a loopback OTLP/HTTP sink (python3's http.server,
saving each POST), `metrics_interval_secs = 5`:
1. A turn whose proc.run waits; `theseus confirm <correlation id> --approve`; the turn it resumes.
   `theseus --json ledger --kind turn.trace -n 1`: the continuation span holds the call's span, result ok. The
   sink's next metrics: one `theseus.tool.calls` point for the call, outcome as your rule says.
2. A proc.run of `sleep 8` with `proc_sync_secs` at 2: after its late result's turn, `theseus.tool.duration_ms` holds
   about 8 s for it, once.

**Leave alone:** telemetry-calls (beside you: `provider_calls`, the provider and AWS instruments, and the
`INSTRUMENTS` list in metrics.rs: add no instrument); batch 6's tiering (`late_results`' search, the stubs) and the
memory rows' metrics; proc-steps (toolrun.rs's plan and job path, toolrun/job.rs, order.rs); route-gaps (one
call in turn.rs, turn/route_step.rs); lane files (toolrun.rs's `ToolRuntime` fields, `read_file`, `build_runtime`);
the kernel and the store.
