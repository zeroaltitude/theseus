<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2f-fanout`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `23328545` (release 1.0.1). That is your base. Nothing is merged on top of it for
this row: it is an investigation of 1.0.1 itself. Line numbers below may be off by a few: search by name. Run the keel
guard against `23328545`.

**Store format at the base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row should need
no store change. If a fix truly needs one, prefer a META key or a serde-default field. A row that must bump the format
STOPS that part and says so in its report's first section (format 50 is taken by personas in `integ/p2-onto`).

**Line ceilings** (`scripts/long-files.txt` at the base; an unlisted file caps at 2,500 lines). Never raise one; the
keel guard flags it.
| file | lines at base | ceiling |
|---|---|---|
| `crates/theseus-core/src/toolrun.rs` | 2,500 | 2,500 (unlisted): add **nothing** here |
| `crates/theseus-core/src/tests_m3.rs` | 7,981 | 8,050: at most one `mod` line; tests go in `tests_m3/` files |
| `crates/theseus-core/src/tests_m3/parallel/order.rs` | 246 | 2,500 |
| `crates/theseus-core/src/tests_m3/parallel/runs.rs` | 310 | 2,500 |
| `crates/theseus-core/src/toolrun/parallel.rs` | 336 | 2,500 |
| `crates/theseus-core/src/toolrun/operands.rs` | 254 | 2,500 |
| `crates/theseus-core/src/toolrun/calls.rs` | 244 | 2,500 |
| `crates/theseus-core/src/toolrun/steps.rs` | 333 | 2,500 |
| `crates/theseus-tools/src/proc.rs` | 458 | 2,500 |

**The other rows of lane `integ/p2-follow`** fire at the same time (table in "Not in this row"). None of them owns the
tool-dispatch files above. `order.rs` and `runs.rs` are declared in `tests_m3.rs` itself (`mod parallel { ... mod
order; mod runs; }`, near line 6139). Prefer adding your tests to `order.rs` or `runs.rs`; a new sibling file costs one
`mod` line there (7,982 of 8,050 lines: fine, but add only that line).

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit: it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`.

## Your task: p2f-fanout, the async bench's fan-out family lost its overlap at 1.0.1: prove or fix that one response's slow calls run at once (theseus-1kr2g)

Branch: `cloud/20261024-p2f-fanout`.

Every commit's subject carries `theseus-1kr2g`. Deadline for the report: about 4 hours after you start.

## Your row

### The finding (theseus-1kr2g)
The async bench ran at 1.0.1 (`23328545`) on the Theseus plain arm (Sonnet 5.5, `proc_sync` 60 s, k=2), against an
Oct 12 run of the same harness at `026dce3f` (confirmed: the Oct 12 trials' build-commit file reads
`026dce3fd1df84344113afdea284373e4633525f`, an ancestor of `23328545`, 1,669 commits back). The fan-out family:

| | 1.0.1 (n=2) | Oct 12 (n=2) |
|---|---|---|
| solved | 2/2 | 2/2 |
| wall (mean of the two) | 187 s | 143 s |
| wall / ideal | 4.89 | 3.55 |
| overlap (slow-step time / slow-phase wall) | 0.967 | 1.29 |
| calls per response | 1.0 | 1.5 |
| multi-call share of responses | 0 | 0.2 |
| first request's prompt tokens | 7,256 | 14,859 |
| tools in the catalog | 42 | 32 |

The other five async families are level or better (cancel responsiveness 182 s to 1.3 s; interrupt 157 s to 4.1 s).
The Claude Code arm on Oct 12 scored fan-out overlap 0.968 too.

**The task the model was given** (the bench's text; an invented upstream, no real service): "Ingest the six parts 1, 2,
3, 4, 5 and 6, each with `ingest PART`; each takes a while. The upstream is flaky: a transient error says so, and the
same part tried again works. A part ingested twice has its rows duplicated, so ingest each part successfully exactly
once. Then write /app/ingest-summary.txt: one line per part, ..." Each `ingest N` sleeps 15-30 s; some attempts fail
with exit 75 and "transient error: 503 upstream unavailable".

### The first pass (done on the host; you cannot read the run data, so here it is)
Per trial: each agent response (seconds from the prompt), its tool calls, and the ingest ledger.

**1.0.1, trial A (wall 183 s, overlap 0.963): one call per response, every response.**
- 1.5 s: `proc_run {command: "which ingest; ingest 1"}`; ingest 1 runs 1.7-22.7 s.
- 25.2 s: one `proc_run {steps: [argv ingest 2, ingest 3, ingest 4, ingest 5, ingest 6]}`. Step 1 (ingest 2) failed
  at 38.8 s; the result said `[step 2 of 5: ingest 3 ...: not run]` for steps 2-5 (steps stop at a failure, by design).
- 40.5 s: `proc_run {command: "ingest 2; echo ---; ingest 3; echo ---; ingest 4; ..."}` (one serial shell line):
  "Still running as job j1 after 60 s"; the parts ran back to back: 2 at 40.6-58.0, 3 at 58.1-86.0, 4 at
  86.1-115.9, 5 at 116.0-137.8, 6 at 137.9-150.0 (failed).
- 102.5 s: `job_wait j1`. 151.5 s: `proc_run ingest 6` (151.6-175.0). 176.5 s: `fs_write` the summary.

**1.0.1, trial B (wall 191 s, overlap 0.971): one call per response.**
- 1.6 s: `ingest 1` (1.8-31.8). 34.6 s: `proc_run {command: "for p in 2 3 4 5 6; do ingest $p; echo \"exit $?\"; done"}`
  (serial loop; went to job j1 at 60 s). Parts 2 (fail), 3 (fail), 4, 5, 6 ran one after another 34.6-127.7 s.
- 96.2 s: `job_wait j1`. 129.6 s: one shell `until` loop retrying 2 then 3 (129.8-184.8). 186.9 s: write the summary.

**Oct 12, trial C (wall 110 s, overlap 1.60): the only trial that batched.**
- 1.6 s: `ingest 1` (1.7-31.3). 33.3 s: two calls in one response (a malformed `argv` string, then `ingest 2`, which
  failed at 41.7).
- **43.9 s: five `proc_run` calls in one response, `{command: "ingest 2"}` ... `{command: "ingest 6"}`. All five
  started at 44.0-44.1 s** and ended 55.8-73.9 s; each result said `[ran at once with 4 other calls of the same
  response]`. 75.1 s: retry `ingest 5` (75.1-105.0). 106.7 s: write the summary.

**Oct 12, trial D (wall 176 s, overlap 0.98): one call per response.** 1.6 s `ingest 1` (failed at 9.4); 12.3 s one
shell loop `for p in 1 2 3 4 5 6; do until out=$(ingest $p 2>&1); ...` that ran every part serially 12.4-170.6 s,
two `job_wait`s, then the summary.

So the mean of 143 s at Oct 12 is (110 + 176) / 2: one trial batched, one did not. At 1.0.1 neither batched (183,
191). Trial D at Oct 12 and both 1.0.1 trials have the same shape and the same wall. **The overlap fell because the
model did not put the parts in one response, not (on this evidence) because Theseus ran a batch serially.** P(0 of 2
batch | a 50 % batching rate) is 25 %: n=2 cannot tell a change in the model's habit from luck.

**Live evidence that 1.0.1 still runs one response's programs at once** (same wave, the 20 other async tasks):
- a response of `proc_run {argv: ["bench-batch","32"]}`, `{argv: ["bench-batch","64"]}`, `{argv:
  ["bench-batch","128"]}` (the fan-out shape: one program, a bare number): all three started at the same tenth of a
  second, ran 16 s / 34 s / 38 s, and the next response came 41 s after: at once;
- `proc_run {command: "build-config debug"}` / `release` / `minsize`: all three started at the same instant;
- 15 results across 5 trials carried "ran at once with 2 other calls of the same response". One said "started after
  call 1, which names `run-tests` too": a `sed ...; which run-tests` call followed by `run-tests` in the background,
  a correct hold.
- Layer 1 on the stand-in model: N calls of `sleep 2` in one answer take 2.24 s for n=1 and 2.44 s for n=16.

### Suspects in the code between `026dce3f` and `23328545` (read each at the base; rule each in or out with a test)
From `git log --oneline 026dce3f..23328545 --` the tool-dispatch, parallel, operand, steps, job, defer and proc paths:
1. **`01bb34bd` toolrun: a program that shares an operand with an earlier call of its response starts after it has
   ended** (`toolrun/operands.rs`, `toolrun/parallel.rs`, `toolrun/calls.rs`; `[tools] parallel_reads` 32,
   `parallel_writes` 16 beside `parallel_runs`). The operand rule drops the program, flags, bare numbers, a few common
   words, redirects and the leading shape words both calls begin with (`SHAPE_WORDS` = 2), and splits a script
   (`bash -c "a && b"`) on `;|&\n(){}`. Check the shapes a model writes for independent parts: `ingest 2` vs `ingest
   3` (argv and `command`); `cd /app && ingest 2` vs `cd /app && ingest 3`; `ingest 2; echo "exit $?"`; `ingest 2 2>&1
   | tee part-2.log`; `ingest part-2` vs `ingest part-3`; a `timeout 600 ingest 2` prefix; a mixed pair where one
   call has a `cd /app &&` prefix and the other names `/app` as an argument. Any false share there serializes a real
   fan-out: that is a bug to fix (the rule errs toward waiting by design, so fix only what is plainly no shared thing).
2. **`c816b01b` `parallel_runs` defaults to 16, up from 8.** Not a serializer; confirm the default and the per-class
   caps read as `parallel.rs` says.
3. **The job path when a group's call passes `proc_sync`** ("Still running as job jN after 60 s"): do the other calls
   of the group still start at once, and does a call that turns into a job hold the group's results back? Read
   `toolrun/job.rs`, `toolrun/job/beside.rs`, `toolrun/late.rs`, `toolrun/waits.rs`.
4. **Deferred tools and tool search** (`2e10af94`, `b87e3109`, `fbda205b` three tiers for tool admission, `c2d757e1`
   a trivial detour carries `tools_find` only, `5d63b5d2` a shorter tools note: it dropped the note's lines "Prefer
   fs_read ... over proc_run" and "proc_run calls that take longer than N seconds continue in the background"). These
   halved the first request (14,859 to 7,256 tokens). They change what the model reads, not how calls run. Check that
   no request at the base sets the Anthropic `disable_parallel_tool_use` flag (`git grep` found none at either commit:
   confirm) and that `proc_run`'s description still says "The proc_run calls of one response run at once, each its
   own job, ...; programs that must run in order go in one call as `steps`" on the first request of a turn in the
   default place (the test `four_sleeps_of_one_response_end_in_about_one_sleep` in `tests_m3/parallel/runs.rs`
   asserts this on its rig: check the bench's place class gets the same).
5. **`steps`** (`toolrun/steps.rs`, existed at `026dce3f`): runs in order and stops at the first failure. Trial A used
   it for independent parts. That is the model's choice of tool shape, not a dispatch fault; do not change steps'
   semantics.

### What to do
1. **IN: a wall-clock-free fan-out test** (the row's core). In `tests_m3/parallel/order.rs` (it already has a paused-
   clock rig through the product's grouping: `Prog`, `prog(id, line)`, `run(calls, tweak)`, `most_at_once`) or a new
   sibling file, a turn whose one response issues N (use 5) slow program calls in the fan-out shapes of suspect 1
   (argv and `bash -c` lines), each held long (seconds of paused time), and assert all N were in flight at once
   (`most_at_once() == N`, or a rendezvous/barrier of N: a serial dispatch never reaches it) and that each result
   carries `[ran at once with 4 other calls of the same response]`. Add a case where the held time passes the rig's
   `proc_sync` so the calls become jobs (suspect 3), if the rig's fake tool can take that path; if only the real
   launcher can, add one real-`proc_run` test whose N calls meet at a file barrier (each call `touch`es its own marker
   under the test's temp root and waits until N markers exist, with a bound that fails the test instead of hanging):
   it passes only if all N run at once, with no wall-clock threshold.
2. **IN: rule each suspect in or out.** For suspect 1, a table test of operand pairs (in `operands.rs`'s own tests)
   for the shapes listed: which share nothing (must run together) and which truly share (stay in order).
3. **IN if found: fix the cause, red first.** A false operand share, a job transition that blocks the group, or
   anything else that serializes independent calls of one response: the test above goes red at the base, the fix
   turns it green. Keep `toolrun.rs` at 2,500 lines: put code in `toolrun/*.rs`.
4. **IN if nothing serializes: prove it and explain the drop.** The test passes at the base; show it discriminates with
   a planted revert (for example: make `parallel.rs`'s grouping put every program in its own group, or make
   `operands::shared` always return a word): the test goes red. In the report, explain the measured drop with the
   per-trial evidence above (the model's call shape, n=2).
5. **OUT (report only): any change to what the model is told** (`proc_run`'s description, the tools note, the persona)
   to make independent parts more likely to be batched. Its effect needs live runs and the owner's call. If you think
   a wording would help, put the before/after text in the report as a proposal, with the evidence. Do not edit it.
6. **OUT: rerunning the bench, a larger n, any live model call.** Name the rerun the maintainer should do (fan-out
   family, k=4 or more, at the tip of this lane) as a follow-up.

## Tests (each red at the base, with a planted revert)
- `a_fan_out_of_five_slow_programs_runs_at_once` (name it your way; the fan-out test of item 1), in
  `tests_m3/parallel/order.rs` or a new sibling: asserts five calls of one response in flight together and each
  result's "ran at once with 4 other calls". Red at the base only if a cause is found; otherwise green at the base, and
  red with the planted revert of item 4. Name the plant and its failing line.
- The `proc_sync` / job case of item 1, if built: same assertion past `proc_sync`.
- The operand table test of item 2 in `toolrun/operands.rs`'s tests: every listed independent shape shares nothing;
  the true shares still share. If a fix changes the rule, the cases it fixes are red at the base.
- If you fix something, each fix's own red-first test and planted revert.
Paused clock (`#[tokio::test(start_paused = true)]`) or a barrier: no assertion on elapsed wall time.

## Not in this row
The other rows of `integ/p2-follow` fire at the same time. Leave their files alone:
| slug | issues | base | its files |
|---|---|---|---|
| p2f-ops-act | floor greying from the daemon's answer, model menu / Join pane nits | integ/ops | cockpit `ops/floor.ts`, ModelMenu, Join pane, palette, SessionDeck, new protocol methods |
| p2f-ops-ui | Timeline, Graph, ops-shell follow-ups | integ/ops | cockpit Timeline, Graph, Board, Bridge, parity, shell |
| p2f-ops-daemon | ops verbs, calendar cache | integ/ops | `crates/theseus/src/client.rs`, rehome / session.profile handlers, `calendar_cache.rs` |
| p2f-turn | turn-budget findings, bench driver `--deadline`, provider timeout retry | integ/p2-route | the turn budget, the bench driver, the provider client |
| p2f-caps | card drafting, guidance waiting reason, forced-tool calls | integ/p2-caps | caps / card drafting, forced tool choice |
| p2f-threads | threads cards, apart_by_dir, families fixtures, drive_read | integ/p2-threads | threads, families, Google Drive read |
| p2f-onto | stale kept name pick | integ/p2-onto | ontology names |
| p2f-bench | memory exam config, scale rig ceiling, bench tests in the gate | integ/p2-misc | `bench/`, the exam, the scale rig |
None of them touches `toolrun/`, `tests_m3/parallel/` or `proc.rs`. p2f-turn and p2f-caps work near the turn and the
provider request: do not edit `turn.rs`, the provider client or the request builder. If a fix truly needs one of
them, make the smallest change and name it as a join note in the report.

## Rules for this row
- **Red first; real-world behaviour, never a benchmark special case.** No bench task name (`ingest`, `fanout`,
  `bench-batch`, ...) in code; tests use invented programs (`load-part 2`, `fetch-shard 3`, hosts under
  `example.test`).
- **Right over compatible; one helper, not two:** reuse the rig, `Prog`, `most_at_once` and `results` that the
  parallel tests have.
- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run `git diff
  --exit-code 23328545 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A change that would alter
  any message of that flow STOPS: write `OWNER CALL: <what and why>, before/after text` in the report.
- **Keel:** no removed or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py` or its
  tests, no renamed test. A cloud row cannot carry a `Keel:` trailer. Before the report run `python3
  scripts/keel-guard.py --base 23328545 --head HEAD --quiet` and paste its output (it must be empty).
- **Data floor:** never loosened. This row does not touch routing or data marks.
- **Public repo:** no person's name, no real organisation, address, calendar id or email anywhere: in code, tests,
  commits or the report. Write "the owner".
- **No new dependency** in any `Cargo.toml` or `Cargo.lock`.
- **The gate for the touched areas:** `cargo test -p theseus-core` (at least the `parallel`, `operands`, `steps` and
  `jobs` tests by filter, then the crate), `cargo test -p theseus-tools` if `proc.rs` changes, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo fmt --check`; then once, at the end, `THESEUS_GATE_NO_BENCH=1
  scripts/gate.sh`. Read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` first. No protocol type changes are
  expected; `cockpit/src/protocol.gen/` stays untouched.
- **Commits:** the Beads id in each subject. End each message with the `Co-Authored-By:` trailer line the base's recent
  commits carry: copy it from `git log -3 --format=%B 23328545`; do not retype it. You cannot sign here.

## Your report
Commit `CLOUD_REPORT.md` (editor tools, not a heredoc; check it is non-empty after the commit), push the branch, and
end with a short final message that starts `CLOUD REPORT COMPLETE`. Sections:
1. **For the maintainer first:** the verdict in one paragraph (a cause found and fixed, or the code overlaps and the
   drop is the model's call shape plus n=2), store format (unchanged at 49, or why not), any ceiling near its limit,
   any OWNER CALL.
2. **Each suspect (1-5):** in or out, the evidence (the test, the code line), and for a fix: the change, its test, the
   planted revert and the failing line.
3. **The fan-out test(s):** names, files, what they assert, the planted revert that turns each red and its failing
   line.
4. **The measured drop explained:** the per-trial shapes above, set against what your test proves.
5. **Proposals (OUT):** any wording for `proc_run` or the tools note, before/after text, why; the bench rerun to run.
6. The gate commands and their results; the keel guard's output; the DM golden diff result.
7. Join notes for `integ/p2-follow` (files shared with sibling rows, expected none); follow-ups.
