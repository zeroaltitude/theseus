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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-turn-end-followups`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

This task was written against an older `main`. Your branch starts from `main` at `1d092ded` (2026-10-20). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (67 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,404 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- Google part 2 rows: gw-mail, gw-calendar, gw-drive, gw-sheets-slides (`crates/theseus-google/`, each its own
  service module, `fake/<service>.rs`, one registration line each).
- Harness rows: job-truths (job command lines, `theseus-kernel/src/job.rs`, fs/file read results), effort-on-evidence
  (turn checkpoints' effort), turn-end follow-ups and long blind waits, recall abstention and the `service` orphan.
- Threads rows: routing (`crates/theseus-core/src/thread_route.rs`, working sets, claims) and cross-stream reach
  (`theseus-memory/src/threads.rs`, `thread_recall.rs`, a place digest, Jev's near-miss check, thread links).
- Ontology follow-ups (`crates/theseus-ontology/`, the sweep, the dream's guidance, the replay), test-infrastructure
  rows (Discord task-thread tests, leaking shell loops), the China-origin model rows (`data_gate.rs`, session data),
  rough edges in progress words, detours and `theseus why`.
- On the maintainer's machine: task questions and reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI
  chat (`crates/theseus-tui`), herdr (`contrib/herdr`).

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: turn-end-followups, a silent job's long wait says what it sees, the end note's leaks close, and the bench counts the note (theseus-53ujg.5, theseus-53ujg.6)

Branch: `cloud/20261021-turn-end-followups`. Every commit's subject carries `theseus-53ujg.5` or `theseus-53ujg.6` (the
part it belongs to). Deadline for the report: 5 hours after you start.

**Why.** Two things a person's own work hits today, and one thing the next measurement needs:
- **A long wait on a silent job is blind.** On Terminal-Bench 2.0 (the v1 run, task `feal-linear-cryptanalysis`, trial
  8536YrE) the model compiled a 2^40 search, started it as a job with no progress output, and called
  `job_wait {"job":"j1","timeout_secs":1700}`. The wait returned after 1,637 s, only because the job hit its own
  timeout: `[timed out and killed] (no output)`. For 27 minutes nothing told the model the job had printed nothing,
  nor that it ran on one CPU (the container had 1 CPU; `nproc` said 32, so the model wrote 32-thread code). A sibling
  trial waited blind 590 s, then added a progress line, found the 1-CPU quota and finished in 48 s. A person's own
  silent long job (a training run without logs, a search) is waited on the same way: one `job_wait` may last 1,800 s
  (`[tools] job_wait_max_secs`), and its result says only "Still running after N s".
- **Two small in-memory leaks the turn-end facts row left** (its review's findings F3 and F4):
  - `remove_job_output` (`crates/theseus-core/src/toolrun/job.rs`) returns early while the job's wrapper lives, before
    `beside::forget(corr)`, and the spool's later sweep (`crates/theseus-core/src/sweep.rs` `sweep`) removes that
    job's raw output without calling it: that job's `before`/`said` entries in `toolrun/job/beside.rs`'s `BOOK` stay for
    the daemon's life.
  - `rewind.rs` `note_written` makes the execution's `WRITTEN` entry with `entry(..).or_default()`, so a file tool that
    settles after its turn ended (a cancelled turn's call finishing on the CPU pool) re-makes an entry that nothing
    removes. The same class was fixed for `beside` at the join (`Held::of` opens the turn's entry with
    `beside::begin_turn`, its `Drop` closes it; a launch after the turn only lists): mirror that.
- **The end note is unmeasured.** The turn-end facts row (joined) gives a turn that would end beside new build outputs,
  or with edits no command ran since, one factual harness note and one more loop, switchable with `[model] end_note`.
  Its review found that `left` also names caches and directories (`target/`, `__pycache__/`, `.pytest_cache/`, a new
  `Cargo.lock`) outside a git tree or where `.gitignore` doesn't list them, so an ordinary "write code, run the tests,
  stop" turn may get the note and one more model call. The owner decides that after measuring how often the note fires
  and whether the model acts on it; today the bench records neither, and no arm can run with the note off.

**Risk: low-medium.** The wait's beat changes when a `job_wait` on a silent job returns (it returns earlier, with
facts, and the model may wait again); `[tools] job_wait_beat_secs = 0` brings today's behaviour back byte for byte.
The leak fixes change no output. The bench part changes only bench code.

**Prerequisite: `main`.**

## Your row

| Part | Beads | Scope | Files |
|---|---|---|---|
| A, the wait's beat | theseus-53ujg.6 | a `job_wait` on a job that printed nothing for `[tools] job_wait_beat_secs` returns then, with facts; every still-running wait result carries the same facts | `crates/theseus-core/src/toolrun/jobs_tools.rs` (`wait`, and a new small module beside it for the facts, e.g. `toolrun/wait_facts.rs`, declared in `jobs_tools.rs`), `crates/theseus-kernel/src/cgroup.rs` (one read-only function: a job's CPU time by correlation id), `crates/theseus-core/src/config.rs` + `config/limits.rs` (the key, its default and range), the template `crates/theseus-core/config/theseus.example.toml`, tests |
| B, the leaks | theseus-53ujg.5 | close F3 and F4 | `crates/theseus-core/src/sweep.rs` (one call where a job's raw output is removed), `crates/theseus-core/src/rewind.rs` (`note_written` and a `begin_written`), `crates/theseus-core/src/turn/end_note_step.rs` (`Held::of` opens the written entry), tests |
| C, the note measured | theseus-53ujg.5 | the bench's trial record counts the note; an arm can run with it off | `bench/harbor/efficiency.py` (`theseus_record`), `bench/harbor/theseus_agent.py` (an `end_note` agent kwarg, as `effort_bounds` is one), `bench/harbor/theseus_bench.py` (`settings`), their tests, `bench/README.md` (one paragraph) |

## What exists (on your base, `origin/main` at `1d092ded` or later; search by name)

- **`job_wait`** (`toolrun/jobs_tools.rs`): `run_job_tool`'s `names::WAIT` arm clamps the asked `timeout_secs` (default
  `proc_sync_secs`) to `[tools] job_wait_max_secs` (1,800 by default) and calls `async fn wait(core, f, secs, asked)`.
  `wait` loops on the job's settle (`JobWaits::on_settle`), a person's message (`steers.watch`, returns early with meta
  `INTERRUPTED_BY: "message"`), a look every `waits::LOOK`, and the turn cache's keep-warm reads; at its end it returns
  `"Still running after {secs} s{cap}. job_read {name} shows its latest output; its result also arrives by itself
  when it ends."` with meta `{job, correlation_id, state: "running", warm_reads}`. Design and history:
  `crates/theseus-core/AGENTS.md`, the paragraphs "A job's handle" and "A wait lasts as long as it asked"
  (theseus-7m7w.3: the owner chose long waits because each early return costs a model call).
- **`job_read`'s one-job view** (`read_one`, same file) builds "running N, M lines so far; the last K" through
  `toolrun/peek.rs` `peek` (bounded, 64 KiB by seek). Reuse it; do not read the file another way.
- **A job's CPU time**: `crates/theseus-kernel/src/cgroup.rs`. Where the daemon's cgroup is delegated, each L0 job is
  born in `<the daemon's cgroup>/job-<correlation id>`; `Job::usage()` reads `cpu.stat`'s `usage_usec`, but a `Job` is
  held by the wrapper, not the daemon. `ready_jobs()` gives the daemon's `Jobs` (the parent directory) where cgroups
  are on. Without delegation (`cargo test`, a daemon under another unit) a job has no cgroup. L1 jobs run in the
  sandbox; give them a CPU fact only if the same path exists for them, else none.
- **The end note's books**: `toolrun/job/beside.rs` (`before`, `begin_turn`, `forget`, `forget_turn`; `pub(crate) mod
  beside` in `toolrun/job.rs`), `rewind.rs` (`WRITTEN`, `written`, `forget_written`, `note_written`),
  `turn/end_note_step.rs` (`Held::of` / `Drop`). The test `beside::tests::a_job_outliving_its_turn_adds_to_no_turn`
  is the pattern for both leak tests.
- **The spool's sweep**: `sweep.rs` `sweep` walks the spool's results and removes those whose `fate(..).removes()`;
  each entry's file stem is the job's correlation id.
- **The bench**: `bench/harbor/efficiency.py` `theseus_record(logs, …)` reads the trial's `theseus-turn.json` (the turn
  result) into `record(...)`. The turn result's `files` is `theseus_protocol::files_left::FilesLeft`: `changed`,
  `left` (each a `PathList`: `paths`, `more`), and `end_note: Option<EndNote>` (`left`, `unrun`, `loop`, `acted`).
  `bench/harbor/theseus_agent.py`'s `__init__` takes `effort_bounds` from `--ak effort_bounds=…` and passes it to
  `tb.settings(...)`, which writes `(section, key)` values into the trial's config.

## What to build

### A. A silent job's wait returns at a beat, with facts (theseus-53ujg.6)
1. **The key.** `[tools] job_wait_beat_secs`, default **600**; `0` turns the beat off. Valid: 0, or from
   `proc_sync_secs` to `job_wait_max_secs` (the config check names both on a bad value, as `job_wait_max_secs`'s check
   does). One commented line in the template under `job_wait_max_secs`, stating the contract.
2. **The beat.** Inside `wait`, a job is *silent* when its spool result file has not grown for the last
   `job_wait_beat_secs` of this wait (compare the file's size at the wait's start and at each look; a growth restarts
   the window). When the window passes silent and the asked wait has not ended, the wait returns now with the facts
   below and meta `{…, "beat": true, "silent_secs": N}`. A job that prints keeps the wait as long as asked, exactly as
   today. The job's end, a person's message, and a stop still end the wait at once, before the beat. Measure with
   `tokio::time::Instant` (the paused clock drives the tests).
3. **The facts, as facts, in one place** (a small function in its own module; used by the beat and by the plain
   "Still running after N s" result, so every still-running wait says the same things):
   - how long the job has run (from its dispatch), and how long this wait lasted;
   - its CPU time where the job has a cgroup, and the share of one CPU it used over the run (`CPU 1,630 s in 1,637 s,
     about 1.0 CPU`); no CPU line where it has none (never a guess, never a zero);
   - its output: "no output since it started", or "no new output for N s" and its last lines (at most 10, through
     `peek`), or, when it printed during the wait, its line count so far and the last lines;
   - the beat's own sentence: `[the wait returned at {N} s of {asked} s asked: the job printed nothing in that time]`.
   Example: `Job j1 (bash -c ./attack) still running after 10 min (this wait 10 min of 28 min asked; it printed
   nothing in that time). CPU 598 s in 600 s, about 1.0 CPU. No output since it started.` Exact words are yours; the
   content is these facts and nothing else: no advice, no "you should", no example of what to do.
   The tool's description is not changed except by the existing `say_wait_cap` line, if you extend that line with the
   beat's contract (`A wait on a job that prints nothing returns after N s.`), keep it one sentence.
4. **The CPU read.** One function in `theseus-kernel/src/cgroup.rs`, e.g. `pub fn job_cpu_usec(correlation_id: &str)
   -> Option<u64>`: `ready_jobs()`'s directory joined with `job-<correlation id>`, its `cpu.stat`'s `usage_usec`; read
   only, no allocation beyond the read. Reuse the `field` parsing `Job::usage` already has (lift it to a private fn).
5. **What does not change:** the clamp, the max cap's sentence, the message-arrived return, `delivered`, the keep-warm
   reads, `job_read`, `job_stop`, and every path when `job_wait_beat_secs = 0`.

### B. The leaks (theseus-53ujg.5, F3 and F4)
1. **F3.** Where `sweep.rs` removes a job's raw output, call `crate::toolrun::job::beside::forget(<correlation id>)`
   (the file stem). Do not move `remove_job_output`'s early return: while the wrapper lives, its late result may still
   read the cached line.
2. **F4.** `rewind.rs`: a `pub(crate) fn begin_written(execution_id)` opens the execution's `WRITTEN` entry;
   `note_written` adds only to an entry that exists (`get_mut`), so a write that settles after its turn ended adds to
   nothing. `turn/end_note_step.rs` `Held::of` calls `rewind::begin_written` beside `beside::begin_turn`; `Drop` already
   calls `forget_written`. Any other caller of `note_written`'s path that runs outside a turn (search `Rewind::settle`
   callers) keeps working: it simply records nothing for the end note, which only a turn reads.

### C. The note measured (theseus-53ujg.5, F5)
1. `efficiency.py` `theseus_record` adds an `end_note` block to the trial's record from the turn result's `files`:
   `{"noted": bool, "acted": bool|None, "left": n, "unrun": n, "changed": n}` (`left`/`changed` are the `PathList`s'
   `len(paths) + more`); a trial whose turn result has no `files` gets `{"noted": false, "acted": null, "left": 0,
   "unrun": 0, "changed": 0}`. Keep the record's other fields byte for byte; bump the record's `SCHEMA` only if the
   module's own rule says a new field needs it (read the module's docstring).
2. `theseus_agent.py`: an `end_note` agent kwarg (`--ak end_note=off`; default on), passed to `tb.settings(...)`,
   which writes `[model] end_note = false` into the trial's config when off and nothing when on (the daemon's
   default is on).
3. `bench/README.md`: one paragraph saying how to run the off arm and where the counts are.

## Tests (each with a planted revert; the first is the key test)
- **`a_silent_jobs_wait_returns_at_the_beat_with_what_it_saw`** (`crates/theseus-core/src/tests_wait_beat.rs`, new,
  on the paused clock with the `Gated` launcher pattern of `tests_wait_cost.rs`): a job that prints nothing, a
  `job_wait` asked for 1,500 s, `job_wait_beat_secs = 600`: the wait returns at 600 s (599-603 s on the clock), its
  text says the job printed nothing and its meta has `beat: true`; the turn's next request carries it. Plant: never
  take the beat. The test must fail.
- `a_job_that_prints_keeps_its_wait`: a job that writes a line to its spool result every 100 s keeps a 900 s wait to
  900 s. Plant: count any time as silent (ignore growth): it must fail.
- `the_beat_off_is_todays_wait`: with `job_wait_beat_secs = 0`, the 1,500 s wait goes the whole way and its text and
  meta equal today's exactly. Plant: a beat at 600 regardless of the key.
- `a_still_running_wait_says_its_output_and_cpu`: the "Still running after N s" result names the job's last lines and
  "no new output for N s" (from a seeded spool file); with a CPU reading given (inject it: no cgroup in `cargo test`)
  it says the CPU share; with none, no CPU words at all. Plant: drop the CPU line's guard (say 0 s).
- `job_wait_beat_secs_has_a_default_and_a_range` (config tests, as `job_wait_max_secs_has_a_default_and_a_range`).
- `a_swept_jobs_book_entries_go` (B, F3): a job's `before`/`said` entries exist, the sweep removes its raw output,
  the entries are gone. Plant: drop the `forget` call.
- `a_write_settling_after_its_turn_adds_nothing` (B, F4): after `Held` dropped, a settle for that execution leaves no
  `WRITTEN` entry. Plant: back to `entry(..).or_default()`.
- The end-note tests already on main (`tests_end_note`, `beside::tests`, `tests_stop_nudge`) stay green unchanged.
- Bench (Python, `python3 -m unittest` in `bench/harbor`): `test_theseus_record_counts_the_end_note` (a turn JSON with
  `files.end_note` and one without), `test_end_note_off_writes_the_switch` (the trial config has `[model] end_note =
  false` with `--ak end_note=off`, and no such line by default). Plant each.
- **The existing 7m7w.3 wait tests** in `tests_wait_cost.rs` (a 900 s wait on a running job, the cap, the keep-warm
  reads; `a_wait_asked_for_900_s_on_a_running_job_goes_the_900_s`, `job_wait_max_secs_caps_the_wait_and_the_description_says_it`,
  `a_wait_past_the_cache_lifetime_reads_the_cache_before_it_lapses`, `a_five_minute_job_costs_fewer_calls_and_dollars_with_the_longer_wait`)
  wait on `Gated` jobs that print nothing, so the beat changes what they see. Their subject is the wait's length with
  nothing to say, so set `job_wait_beat_secs = 0` once, in that file's `config(..)` helper; change no assertion. Name
  the line in the report as an expected keel finding. Add one test of your own in `tests_wait_beat.rs` that a wait
  returned at the beat still had its keep-warm reads before it (the beat must not starve the cache).

## Not in this row
- A turn deadline or "time left in the turn": Theseus has no such deadline today; do not invent one.
- A job's CPU *quota* (`cpu.max`) or `nproc` inside a job (a separate follow-up on theseus-53ujg).
- Whether caches and directories count as `left` (`beside.rs`): the owner decides after part C measures it. Do not
  change `beside.rs`'s listing or the note's wording.
- `toolrun/job.rs` beyond reading it: **job-truths** (theseus-53ujg.2, in flight on another branch) owns
  `theseus-kernel/src/job.rs`, `job_l1.rs`, `toolrun/job.rs`'s `ResultStatus::Unknown` arm, `rpc/driver.rs`
  `wrapper_signalled`, `theseus-tools/src/fs.rs` + `magic.rs`, `file_read.rs`, and the test helpers
  `theseusd/tests/{gone_jobs,job_handles}.rs`, `tests_term.rs`. **effort-on-evidence** (theseus-53ujg.3, in flight)
  owns `turn/checkpoint_step.rs`, `routing/effort.rs`, `turn/route_step.rs`, `turn/effort_step.rs`. **recall-abstain**
  (theseus-53ujg.7/.8, in flight) owns `toolrun/resume.rs`, `toolrun/handles.rs` and `bench/recall/`. Stay out of all
  of them; never find a job by its command text in `/proc` (job-truths moves that text off the command line).
- `docs/status.md`, the README, `docs/design/`, the spec: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The beat ships on by default (600 s). Its rollback is
  `[tools] job_wait_beat_secs = 0`: `the_beat_off_is_todays_wait` shows the old behaviour byte for byte.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool and at
  most one sentence to `job_wait`'s description through `say_wait_cap`. FAST: nothing new on a plain turn (a plain turn
  stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move),
  nothing on the path before a request leaves, and nothing on the start path grows with history. The facts are read
  only when a wait returns still running.
- **Facts only.** Every word the model reads states what exists or happened (how long, how much CPU, what it printed,
  why the wait returned). Never what to do, never "must", never an example of an action.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty except for the `tests_wait_cost.rs` rig lines this task names; explain each line it prints.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  (this row stores nothing new).
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented.
- Line ceilings: `scripts/long-files.txt`. Put new code in new modules; in a long file add only the `mod` line, the
  field or the call. Add no line to `crates/theseus-core/src/toolrun.rs`.
- Read the AGENTS.md of every crate you touch first (`crates/theseus-core/AGENTS.md`, `crates/theseus-kernel/AGENTS.md`,
  `bench/README.md`), and update the paragraph on `job.wait` in `crates/theseus-core/AGENTS.md` with the beat (two
  or three sentences). Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud. A
  job cgroup's CPU read needs a daemon under a systemd unit with `Delegate=yes`, which the VM lacks: test the read
  against a temporary directory shaped like the cgroup (a `cpu.stat` file), and give the maintainer the exact command
  for a live check on a delegated daemon.

## Your report
Commit `CLOUD_REPORT.md` at the repository root with: what changed (per part A, B, C, with the commits); each test and
its plant, and whether the plant failed it; the full test commands you ran and their counts; what could not run on
the VM and the exact commands for the maintainer (the live CPU line on a delegated daemon; a silent job of 11 minutes
under `theseus ask` showing the beat); the keel guard's output, each line explained; the store format (unchanged);
follow-ups (anything you found in another row's area, named, not changed). Then push the branch and end with a final
message that starts `CLOUD REPORT COMPLETE`.
