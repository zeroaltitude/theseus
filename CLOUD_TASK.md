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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-bench-recall-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the recall bench's mark sized by the compiler's estimate, its admission and stale checks, and its compactions counted by outcome (theseus-523y)

Branch: `cloud/20261005-bench-recall-fixes`. Every commit's subject carries its issue id. Deadline for the report:
4 hours after you start.

**Background.** bench/recall/ joined main. Its first live smoke (seed 7, the Theseus arm at `baseline`, Claude Sonnet
5.5, 30 turns) showed three things the offline tests couldn't:
1. **The mark's bulk read is a context overage.** Turn 10, the one bulk turn, exited 1: "its request is estimated
   at 24,338 tokens (34,074 at the estimate's upper bound) against the 22,154 the window leaves after the output
   cap, 11,920 over, with every earlier turn dropped. Nothing was sent." The generator sizes the log at four bytes a
   token (6,944 tokens); the compiler read the turn at about 12,300 beside 12,042 of system prompt and tools. The
   run compacted at turn 11 and the probes kept their buckets, but the mark is an error turn where Claude Code
   would answer: the arms differ at the very turn the bench is built around.
2. **The admission regex misses plain abstentions.** p003's right abstention scored wrong: "I can't tell. A
   case-insensitive search for "lanyard" across the 20 workspace files found no matches, so nothing there pins a
   lanyard version for veery." `ADMIT` has no `tell`, and its `no …` branch wants `no` before the verb.
3. **Strict stale fails a right answer.** p001: "The jacana archiver is on port 38013. … Ignore the "27340" I
   mentioned in my first answer." The old value is named only to retract it: scored wrong, stale, confident-wrong.

Read as a person would, the smoke's arm is recall 6/6 and abstention 2/2, where the scorer said 83% and 50%. A
second compaction at turn 25 had outcome `ring`, and the driver counts any `context.compacted` row as a compaction.

**Read first:** bench/recall/README.md; generate.py (`_window`, `bulk_log`, `est_tokens`, `OVERHEAD_TOKENS`,
`PER_TURN`, `MARGIN`, `estimate`, `PRICES` and its test); progression.py (`request_budget`, `HEDGE`, `ADMIT`,
`kind_check`, `value_check`, `bucket_of`); checks.py; score.py; drive.py (`compactions`); test_generate.py (`Window`,
the pinned digest); test_drive.py (`rules_for`, the stand-in smoke). The Rust estimator, read only: theseus-core's
provider.rs (`Census`, the framing), catalog.rs (`TokenRates`), compiler.rs (`estimate`, `MARGIN_PERCENT`,
`request_budget`, the ring), fact/compaction.rs.

**What the code says** (the code wins; report each difference):
- **The estimate.** A tool result's text is `json` bytes at 2.4 a token for Claude's current family
  (`TokenRates::CLAUDE`, or the catalog row's `bytes_per_token`), text 3.3; each message 3 tokens, each block 1, each
  tool id 15. From a compilation's second call on, the provider's count of the last request is `counted` and only
  what was written since is estimated; a ring's candidates are estimated whole by bytes. The upper bound is
  `counted` + estimate × 1.4 (`MARGIN_PERCENT` 40); the ring runs when it passes `request_budget` (the window less
  the output cap and 4,096) and keeps turns while the estimate is under 60% of it; a turn whose newest exchange alone
  passes it fails as an overage.
- **So the bulk owes two bounds:** alone beside the system prompt and tools, estimated whole, its upper bound under
  the budget with a margin (the live overhead, 12,042, is 16,859 at the bound: the window must grow); and the turns
  before the mark (counted) plus the bulk's bound past the budget.
- **The checks live in the progression:** `kind_check` writes `ADMIT` into each abstention's `check`, `value_check`
  the stale line. Changing either moves the generated bytes and the smoke's pinned digest; an old run keeps its own.
- **`context.compacted`'s outcome** is `compaction` (a summary in the cut's place) or `ring` (the cut kept, no
  summary, and why). Both drop leading turns; the row carries the cut's message count and first and last positions.
- **The README's Stale bullet states the strict rule as the bench's** ("It moved from 27340 to 38013" is stale),
  after theseus-exam's superseded items; the bench's first report called it the owner's choice.
- **The stand-in smoke** reads the bulk with `proc_run cat` and passes on main where the live mark failed: find why,
  and make it reproduce the overage on main's sizing if it can (an `fs_read` of the whole log, as a model reads).

**What to build,** each a green commit:
1. **The bulk sized by the compiler's rule.** Mirror it in Python (the rates, the framing, the margin, the budget),
   each constant named with its Rust source, and a test that reads them from those files, as `PRICES`' test does.
   Size each bulk so its turn alone, estimated whole with the system prompt and tools, keeps its upper bound under
   the budget with a margin, while the turns before the mark plus the bulk cross it; grow the window where both
   can't hold. Calibrate the overhead once against the compiler's own figure in a scratch daemon's ledger
   (`context.compiled`, on the stand-in model) and say it. `est_tokens` and the budget's dollars follow the same
   rule. Test both bounds for the smoke and the full at seed 7 and two other seeds; re-pin the smoke's digest,
   showing only the bulks, the window and the estimates moved.
2. **`ADMIT` widened:** "can't tell", "no match(es)", "found no …", "nothing … says/pins/shows", "not able to find",
   "unable to …"; `HEDGE` where it misses them. Test the two live sentences, their short forms, and replies that
   must not read as an admission. Decide how an old run's stored checks meet the new rule (the scorer re-derives an
   abstention's check from its kind, or new progressions only), and say which.
3. **The retraction-aware stale rule, as the scorer's option** (`score.py --stale retracted`; `strict` the default,
   as today): a reply giving the new value and naming the old one only where it retracts it ("ignore", "no longer",
   "was … before", "moved from", "replaced", in the same sentence) is right, and counted in a column of its own
   (the old value named), never stale or confident-wrong. Tests: p001's reply right under the option, wrong by
   default; "The archiver is on port 27340." stale under both; "It's 27340, or maybe 38013." wrong under both. The
   README's Stale bullet names the option.
4. **Compactions by outcome:** run.json keeps each row's outcome and the cut's span; the bucket rule says which
   outcomes move a probe (a ring cut drops the same turns without a summary: likely yes; decide); the report counts
   the two apart. Test with a fixture run holding a `ring` row.

**Your suites** (the preamble's bench/ rule): `python3 -m unittest discover -s bench/recall`, on the standard library
alone and with this workspace's binaries (test_drive.py finds `target/debug`).

**Proof, offline:** the stand-in smoke end to end: every turn exits 0, the first compaction at the mark's turn or
the next. Planted reverts, each naming the test it breaks: the bulk at four bytes a token again; the margin dropped
from the alone bound; `ADMIT` without `tell`; the retraction rule accepting an old value with no retraction; a
`ring` row read the way you didn't choose.

**The live check is the maintainer's** (a key, a release build of `theseus`, `theseusd` and `theseus-index`, a
throwaway container). Write exact commands for:
1. `python3 bench/recall/generate.py --seed 7 --size smoke --out /tmp/rf-smoke`: the window, each bulk's estimate
   and bound, the budget.
2. `python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir target/release --model
   anthropic/claude-sonnet-5-5 --progression /tmp/rf-smoke --out /tmp/rf-th`: every turn exits 0, the mark's turn
   compacts, each outcome counted, nothing left running.
3. `python3 bench/recall/score.py /tmp/rf-th --out /tmp/rf-report`, and again with `--stale retracted`.
4. The full on Theseus alone, before Claude Code's.

**Leave alone:** bench/async, bench/harbor, bench/report (two sibling sessions); `bench/theseus-bench.toml`;
crates/theseus-exam (read only); all Rust.
