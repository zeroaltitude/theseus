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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-bench-measure`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

---
## Your task: bench-measure, the measurements the next run depends on: the routed profile mirrors the live table, a plan can cap the effort, the recall driver reads every history page, and the kill family measures four harnesses (theseus-53ujg.4)

Branch: `cloud/20261019-bench-measure`. Every commit's subject carries `theseus-53ujg.4`. Deadline for the report: 4
hours after you start.

**Why.** The next benchmark run measures v1 patch1's harness changes. The v1 run found places where the bench itself,
not Theseus, got the measurement wrong or could not make it. Each is Python under `bench/` plus the Rust tests that
read the bench profile; no product code changes in this row.

**Risk: low.** Bench code only. The benchmark's own task sets, graders and any held-out or sealed material are out of
bounds: never read or change them.

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| F4 | `bench-measure` | theseus-53ujg.4 | the routed profile level with the live table; a plan's effort bound through the adapter; the recall driver pages history; the kill family's Pi kill and the Pi/OpenCode restarts | `bench/theseus-bench-routed.toml`, `bench/harbor/{theseus_agent,theseus_bench,plan,run}.py`, `bench/harbor/test_bench.py`, `bench/README.md`, `bench/recall/{drive,test_drive}.py`, `bench/async/{async_agents,driver,test_kill}.py`, `crates/theseusd/tests/bench_profile_routed.rs`, the `resolved_profiles` golden (`theseus-core/src/config/tests_inherit.rs`) | M |

## What to build
1. **The routed profile mirrors the live table.** The v1 run's operator patched `bench/theseus-bench-routed.toml` by
   hand to match the table the owner runs. Apply exactly those differences:
   - `effort = "medium"` on `[profiles.sonnet]` and `[profiles.opus]`, and `effort = "high"` on `[profiles.fable]`
     (the owner's table since 2026-10-11); fix the comment above the profiles (it says those three name none) and the
     stale "route.v4" in the file's comments;
   - a new profile, and add `opushi` to `ROUTED_PROFILES` in `bench/harbor/theseus_bench.py` (near :33, :168-171) so
     the adapter caps its `max_loops` like the others (update `test_bench.py` near :168-183 and `bench/README.md`'s
     "the five profiles", near :11):
     ```toml
     [profiles.opushi]
     provider = "anthropic"
     model = "claude-opus-5-5"
     max_loops = 200                         # set by the adapter
     on_max_loops = "end"
     effort = "high"
     ```
   The `review` class (`["opushi", "fable"]`) and the bounds `["low", "max"]` are already the config's defaults
   (`config/routing.rs`); add them to the file only if a test needs them spelled out, and say which.
   **Rust tests read this file:** update `crates/theseusd/tests/bench_profile_routed.rs` (near :64-68 it asserts
   sonnet, opus and fable have no effort: make them Medium, Medium, High, and add `opushi`), and rewrite the golden with
   `THESEUS_GOLDEN=write cargo test -p theseus-core tests_inherit`, checking that only those profiles' lines moved.
   These edits follow the file; they are expected, not keel findings. `route_owner.rs` and `bench_routed_live.rs` load
   the file too: they must pass.
2. **A plan can cap the effort.** The HIGH tier capped Jev's effort at `high` by editing `effort_bounds` in a copy.
   Today a plan never reaches the adapter beyond attempts (`bench/harbor/plan.py`; `run.py` emits `harbor run` lines;
   the adapter reads `--ak` and `THESEUS_BENCH_*`: `theseus_agent.py` near :135-141, :181-210). Add an
   `effort_bounds` kwarg to the Theseus agent's `__init__` (`--ak effort_bounds=low,high`), written into the daemon
   config it builds as `[routing] effort_bounds` through `tb.settings` (`("routing", "effort_bounds")`); `plan.py` and
   `run.py` add that `--ak` when a plan has the key, and `plan.check` validates it (two known levels, low ≤ high).
   Create no tier plan; test with a fixture plan.
3. **The recall driver reads every page of history.** `bench/recall/drive.py` (near :616) runs
   `theseus --json history --full <session>`, which returns the newest 200 nodes (the CLI's page). Every history file
   the v1 run saved holds exactly 200 nodes, so the delivery check missed 57 facts that were delivered. Page with
   `--before <nodes[0].position>`, then each page's `older`, until a page has no `older` or no nodes (the first page
   carries no `older`: `rpc/pages.rs` near :255-257, `theseus-protocol/src/history.rs` near :45-50). Keep every node of
   every page, the assistant's included, and still write one `raw/history-{i+1}.json` per session (`test_drive.py`
   near :955 reads it).
4. **The kill family measures Pi.** In every Pi trial of the v1 run the kill never fired: the record reads only
   `{"arm": "pi-async", "family": "kill-bisect", "ended": "settled", "wall_s": 49.4}`, with no `kill`. Three defects:
   - `report["kill"]` is set only in `_killed` (`async_agents.py` near :68-82), which runs only on a non-zero exit
     (near :414-417): record the killer's result on every exit, 0 included;
   - Pi's run has no `pipefail`, so `wait` returns `tee`'s status (`driver.py` near :665-669): add it to `pi_stdin`'s
     run;
   - the kill finds Pi with `--cmd '\bpi\b.*--mode rpc'` (`driver.py` near :293), but Pi sets its process title (the
     sampler finds comm `pi`, `efficiency.py` near :51), so the pattern never matches: find it by `--comm pi --root`.
5. **The restarts find their binaries.** Every OpenCode restart printed `sh: 1: opencode: not found`, and Pi's restart
   has the same `PATH` bug (`driver.py` near :291; `test_kill.py` near :313-320). For OpenCode and Pi, resolve the CLI
   once in the run's own shell (`bash -lc '<prefix> command -v <cli>'`) and restart with `PATH=<dir>:$PATH <absolute
   path>`.

## Tests (each with a planted revert; the first is the key test)
- **`recall_delivery_reads_every_history_page`**: a fake CLI honouring `--before`, with three pages (200, 200 and 37
  nodes; no `older` on the first, an `older` on the second, none on the third), and a fact marker only on the oldest
  page: it counts as delivered. Plant: read one page.
- `a_plans_effort_bound_reaches_the_daemon_config`, and a plan without one leaves the profile's. Plant: ignore the key.
- `bench_profile_routed.rs` with the new efforts and `opushi`; the golden rewritten (only those lines moved).
- `a_pi_kill_trial_records_its_kill` (a fake procs entry whose cmdline is just `pi`, and an exit 0). Plant: the old
  `--cmd` pattern.
- `the_restarts_name_absolute_binaries` (OpenCode and Pi). Plant: the bare name.
- `python3 -m pytest bench/async bench/recall bench/harbor -q` and the touched Rust tests pass (say the counts).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `bbc65530` (2026-10-18
21:32 MST, the v1 cut's last join fix, tagged as v1) or later, with **store format 45**. The `file:line` references
below are at `bbc65530`; on your base a line may have moved a little: search by name.

Line ceilings at `bbc65530` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-core/src/turn.rs` 3,384 /
3,525, `crates/theseus-protocol/src/lib.rs` 2,706 / 2,733, `crates/theseus/src/render.rs` 3,108 / 3,128,
`crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030, `crates/theseus-discord/src/runtime.rs` 3,497 / 3,500 (full: never
add a line there). **`crates/theseus-core/src/toolrun.rs` is 2,489 lines and unlisted**: a file past 2,500 needs a
ceiling entry, which this batch may not add, and another row of this batch adds one registration line there. So **add
no line to `toolrun.rs`** in these rows. Never raise a ceiling. Put new code in new modules; in a long file add only the
`mod` line, the field or the call.

## v1 patch1: the harness-effectiveness rows

Theseus's v1 is cut and benchmarked. On Terminal-Bench 2.0 it is level with Claude Code on the same model and effort,
and it is cheaper per solved task. The rows below come from reading every failed trial of that run for causes a real
user would also hit; none is tuned to a benchmark task. The owner's words (Discord DM, 2026-10-19 00:21, verbatim
excerpt): "Want to add a full analysis of last benchmark results, which were very generally encouraging, and do another
pass of analysis to see what we could do to be a more effective harness. Analyze, plan and implement in v1 patch1."
His standing rules that bind these rows: benchmarks are a lens, never a target ("we // do // want to take those
occasions to make our tools faster, more useful, more sophisticated ... but we won't ever tune our stuff just for the
benchmark itself", 2026-10-07); minimal prompting: the API contract, then tool shape, then descriptions, then result
metadata, and model-facing prose only as plain facts (2026-10-09); features ship on and measured, with a named
rollback (2026-10-06).

| Row | Slug | Beads | Prerequisite | Area |
|---|---|---|---|---|
| F1 | `turn-end-facts` | theseus-53ujg.1 | main | `toolrun/job/beside.rs` (new, declared by `mod beside;` in `toolrun/job.rs`), `toolrun/job.rs` (the `mod` line, one call at `launch`, one line where the cap and uncapped lines are joined, one line in `remove_job_output`), `rewind.rs` (a turn's written paths, in memory), `turn/nudge_step.rs` (a shared `may_continue` and `Nudge::sent`), `turn/end_note_step.rs` (new), `turn.rs` (the `mod` line, one call in the stop chain, the field and its result), `fact/turn.rs` + `fact/mod.rs` + `theseus-protocol/src/ledger.rs` (one fact), `theseus-protocol/src/files_left.rs` (new; one `pub mod` line and one field in `lib.rs`), `theseus-protocol/src/nudge.rs` (`harness_lines`), `config` (`[model] end_note`), the template |
| F2 | `job-truths` | theseus-53ujg.2 | main | `theseus-kernel/src/job.rs` (`spawn_detached`, the wrapper's `parse`, `spawn_l0`: argv out of band, the runner at exec), `theseus-kernel/src/job_l1.rs` (the runner at exec for L1), `toolrun/job.rs` (the `ResultStatus::Unknown` arm only), `rpc/driver.rs` (`wrapper_signalled`'s reason), `theseus-tools/src/fs.rs` (the binary branch) + `theseus-tools/src/magic.rs` (new), `theseus-core/src/file_read.rs` (the miss message), and the test helpers that find a job by its text in `/proc` (`theseusd/tests/gone_jobs.rs`, `job_handles.rs`, `tests_term.rs`) |
| F3 | `effort-on-evidence` | theseus-53ujg.3 | **the local routing lane's join** (its conversation-turn checkpoints are this row's base) | `turn/checkpoint_step.rs` (the effort-only raise, the `Off` state), `routing/effort.rs` (`Runs.floor`, `decide`), `turn/route_step.rs` (`route_effort`), `turn/effort_step.rs` (`stick`'s exception for a checkpoint) |
| F4 | `bench-measure` | theseus-53ujg.4 | main | `bench/theseus-bench-routed.toml`, `bench/harbor/{theseus_agent,theseus_bench,plan,run}.py` (a plan's effort bound through `--ak`), `bench/recall/drive.py`, `bench/async/{async_agents,driver}.py`, their tests, `crates/theseusd/tests/bench_profile_routed.rs` and the `resolved_profiles` golden (they read the profile), `bench/README.md` |

F1, F2 and F4 run in parallel now. F3 launches once the local routing lane has joined `main`. Joins are plain
merges, one at a time: F2, then F1, then F4, then F3.

**Other work in flight tonight** (stay out of their areas): the Google Workspace rows (`gw-*`: `crates/theseus-google/`,
`theseus-core/src/google/`, one registration line in `toolrun.rs`, `broker.rs`, `data_gate.rs`, the route's data step);
the speed rows (`speed-*`: the census and first request, the turn's path before the request leaves, idle memory and
binary size); a local routing lane (the provider-origin allowlist for private data, routing hard tasks away from the
cheapest model, Jev's warnings: `routing/data.rs`, `data_gate.rs`, `catalog.rs`, `routing.rs`, `turn/route_step.rs`,
`turn/checkpoint_step.rs`, `turn/data_step.rs`, `config/privacy.rs`); a Discord test hang; the README; a voice fix.

**Contracts between these rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **`toolrun/job.rs` is shared by F1 and F2 at different places.** F2 edits only the `ResultStatus::Unknown` arm of
   the header in `job_result_of`; F1 adds its `mod beside;` line, one call where a job is launched (`launch`) and its line only where the
   cap and uncapped lines are joined (the `format!("{header}{}{}", …)` near the end of the header's build), and one
   line in `remove_job_output` that drops its entry. Neither touches the other's.
3. **One harness line a turn.** F1's end note and the existing stop nudge (`turn/nudge_step.rs`, `NUDGE_TEXT`) never
   both fire in one turn: if the nudge sent its line, F1's note stands down for that turn (its facts still reach the turn result and the footer).
4. **The store format:** none of these rows changes a store layout. A new ledger fact kind (as `turn.nudged` is one)
   is fine. F1's per-turn facts ride on the turn result (protocol) and its own fact row; if a row finds it must store something, it says why in the
   report and bumps by one from its base (the joiner renumbers).
5. **No new model-facing instruction.** Any text the model reads (a result line, an end note) states facts: what
   exists, what changed, what happened. It never tells the model what to do, never says "must", and never gives
   examples. Tool descriptions change only to state a tool's contract.

## Out of scope for every row

The owner's config, his daemon and his machine. Any benchmark run (the next run measures these rows; a row never
does). `docs/status.md`, the README, `docs/design/` and the spec (the maintainer writes those; say what should change).
Routing choices of which model a turn starts on (the routing lane's), and Jev's packs' prompts. If your work shows one
of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the
  rollback: a test shows that with it off the old behaviour is byte-for-byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). A new model tool is deferred by
  default (these rows add none). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the path before a request leaves, and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as the shared contracts say.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
