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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-prove-wire-in`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: `theseus judge prove`: the prove's records built from the ledger, and the command that runs the generator over them, row 50 (theseus-0j2.18)

Branch: `cloud/20261005-prove-wire-in`. Every commit's subject carries `theseus-0j2.18`. Deadline for the report: 4
hours after you start.

**Background.** Roadmap row 50, L3's join (design m5-judgment.md: L3, and §2.9's "The prove"). The prove asks whether
JUDGE_STOP (`loop.v1`, at `loop_end`) helps tasks: canary against control at equal total budget, rates per task and
per dollar. The generator is on main: theseus-judge's prove.rs (pure: the same records give the same report byte for
byte) and the `theseus-judge prove <records.jsonl>` binary. Nothing builds its records from the store, and the CLI
has no `theseus judge prove`: this step builds both. Until JUDGE_STOP acts on tasks under canary (row 45, step 26b,
not built), the report says "insufficient" with its counts: the honest answer, and the live check's.

**Read first:** design m5 §2.4 (JUDGE_STOP), §2.9 (the learning ledger, "The prove") and L3's entry; theseus-judge's
prove.rs (its module docs define every record field: read only), learn.rs (`Arm`, `arm`, `similarity`,
`NEAR_IDENTICAL`), bin/theseus-judge.rs and fixtures/prove/; theseus-core's learning/ (mod.rs, labels.rs's
`resolve`, system.rs's `false_completion` and `continuation` rules and `FALSE_COMPLETION_MS`, report.rs),
judge/mod.rs (`LOOP_PACK`, `ask_mode`), judge/ladder/, fact/judge.rs (`JudgeCall`), the tasks' rows
(`task.created`, the kernel's `task.ended`, `task.closed`), rpc/learning.rs and rpc/judge_runs.rs (runs over the
ledger); crates/theseus's main.rs (`JudgeCmd`), cmd.rs, judge_runs.rs; theseus-protocol's judge_runs.rs; the AGENTS.md
files.

**What changed since the design was written** (the code and AGENTS.md win; report each difference):
- **The ladder joined** (26a): a judgment of a pack in canary or shadow records its session's arm in its context
  (`pack_arm`: `canary` or `control`, written by `ask_mode`); arms are sticky per session (`learn::arm`), so a task
  is in one arm. A `judge.call` row is the judgment whole: answers, cost, context, scoped `judge:<pack id>`.
- **No nudge is recorded.** The canary's nudge is 26b's; nothing on main writes one (`NudgedTurnEnded` is built only
  in tests). `nudges` and `unnecessary_nudges` stay 0 until then: say so in the report's words, not as data.
- **Labels already hold most answers:** system labels (`false_completion`, a near-identical task by
  `learn::similarity` at `NEAR_IDENTICAL` within `FALSE_COMPLETION_MS`; `continuation`), operator labels, audit
  labels (weight 0.5), resolved heaviest first by `labels::resolve`. One rule, never a second copy: the records read
  these and re-derive nothing.
- **learning/ is changing beside you** (the learning loop, under review: packs at run time, report.rs, labels.rs,
  tender.rs). Put your code in a module of its own (`learning/prove.rs`) with only the `mod` line in mod.rs, and use
  their functions as they are.
- **theseus-protocol's lib.rs is at its ceiling** (2,709): the method's name joins an existing line of the method
  list (the `JUDGE_REPLAY` line), and its types go in a module file (judge_runs.rs, or one of their own). The
  protocol depends on no crate of ours: carry the report as the generator's JSON, and its markdown.
- **FAST:** nothing on the start path. The prove reads through the store's pages by kind, on the blocking pool;
  give its time on a store of 10,000 tasks.

**What to build,** each a green commit:
1. **The records** (`learning/prove.rs`, pure over the rows it is handed): one per finished task (`task.ended`; say
   how `task.closed` counts), each field as §2.9 and prove.rs's docs define it:
   - `arm`: the `pack_arm` of the `loop.v1` judgments in the task's session; a task with none (never judged), or
     with both, is left out and counted by reason;
   - `success`: false on a near-identical task within 24 hours, an operator label saying the task failed, or an
     audit saying it isn't done; true once the 24 hours have closed with none of those; `null` while open. Say what
     "the audit says done" needs where no audit ran, and which label you read as the operator's "wrong";
   - `false_completion`: the resolved `false_completion` label of its judgments (`null` where nothing called it
     complete);
   - `spend_micros`: the task's whole spend as its budget counts it, judge calls included; `judge_micros`: the
     `judge.call` costs whose context names its session; say where you read each;
   - `turns`: its session's ended turns (say whether its child tasks' count);
   - `stops`: each `loop.v1` judgment: the decision that acted in its arm (the canary's Jev answer, the control's
     baseline), and `should_stop` from its resolved `work_state` label (`complete`: true; otherwise false;
     unlabeled: `null`).
2. **The method:** `judge.prove { since?, until?, min_tasks?, min_labeled? }`, in an rpc module of its own, with one
   dispatch line in rpc/server.rs: it builds the records, runs `theseus_judge::prove::prove` with `ProveMinimum`
   (its defaults unless given), and answers the report's JSON, its markdown, the records counted by arm and the
   tasks left out counted by reason. It writes nothing. Its default window: since `loop.v1`'s latest move to canary
   (its `pack.mode` row), else everything; say if that is wrong.
3. **The CLI:** `theseus judge prove [--since <day>] [--until <day>] [--min-tasks N] [--min-labeled N] [--records
   <path|->] [--json]`: the markdown by default; `--records` writes the JSONL records, so that `theseus-judge prove`
   over that file gives the same report byte for byte. `JudgeCmd` gains the variant (main.rs: a few lines), cmd.rs
   its dispatch line, and the code a module of its own. TypeScript regenerated.
4. **Classification, if time allows:** §2.9's decision quality for `classify.v1` against its baseline (the model's
   own `task.create` and slash commands) on audit- and operator-labeled messages, from the nightly report's numbers
   or the labels. If not, say what it needs.

**Proof, offline:** a store written with invented tasks, judgments, labels and spend gives exact records (every
field, both arms, a never-judged task left out, an open 24-hour window giving `null`); the records through the
generator equal `theseus-judge prove` over the `--records` file, byte for byte; fewer than the minimum per arm says
"insufficient" with its counts; canary and control cohorts with known outcomes give the generator's exact rates per
task and per dollar; judge spend counts in `spend_micros`; the method writes no frame. Planted reverts, each naming
the test it breaks: judge calls left out of `spend_micros`; an open window read as success; the arm read from a
judgment of another pack.

**The live check is the maintainer's,** with a GLM key and Jev's. Exact commands for a scratch daemon: fresh
state dir, Discord and the web off, `[judge] enabled = true`:
1. `theseus judge prove` on the empty store: "insufficient", canary 0 and control 0, saying why.
2. `theseus packs promote loop.v1 --canary 0.5` (short of the bar, its row says `forced`), then three short tasks
   the model creates from `theseus ask` messages: `theseus judge prove` counts them by arm, still "insufficient",
   with its counts.
3. `theseus judge prove --records /tmp/prove.jsonl`, then `theseus-judge prove /tmp/prove.jsonl --markdown -`: the
   same report.

**Leave alone:** theseus-judge's prove.rs and its binary; learning/'s existing modules beyond calls (the learning
loop); the ladder and its rules; rpc/server.rs beyond your line (push-once changes its serialization); cmd.rs beyond
your dispatch line (health-words changes `catalog` there); the task graph (task-board, under review); the cockpit
beyond regenerated types (say what its Judgment view should show).
