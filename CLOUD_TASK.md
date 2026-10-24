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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2f-turn`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the lane `integ/p2-route` at `fb721356` (2026-10-23, gate green: 8,067/8,067). That is your
base: **not** `main`. Where the release preamble above disagrees about `main`, ceilings or work in flight, this note
wins. Line numbers below are at `fb721356` and may be off by a few: search by name. Run the keel guard against
`fb721356`.

**What the lane holds** (`git log --oneline 23328545..fb721356`, on `main` `23328545`, release 1.0.1):
p2-effort-max (`ce01456d` + join fix `4c617395`), p2-route-data (`20ae9e85` + `5c324e1d`: route pack v8),
p2-xhigh-sure (`7e84b593`), and **p2-turn-budget** (`59239125`): a turn told its time left (`turn/clock_step.rs`,
`turn.submit.deadline_secs`, `theseus ask --deadline`), steered or stopped when nothing passes
(`turn/progress_step.rs`), and the session block's CPU line. Most of this row fixes review findings on that merge.

**Store format at the base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row should
need no bump and no new stored field. If a part truly needs one, STOP that part and say so in the report's first
section (personas take 50 in another lane). No protocol type changes either (`TurnSubmitParams` stays as it is).

**Line ceilings** (`scripts/long-files.txt`; Rust files only; an unlisted file caps at 2,500 lines):
`crates/theseus-core/src/turn.rs` 3,473 of **3,525**: keep your edit there at net zero lines if you can, never more
than +3. Unlisted and below: `provider.rs` 1,936, `rpc/mod.rs` 1,505, `rpc/tests.rs` 2,155 (put new RPC tests in a new
file), `turn/checkpoint_step.rs` 754, `turn/progress_step.rs` 382, `turn/clock_step.rs` 181,
`tests_turn_progress.rs` 294, `tests_failures.rs` 366. Never raise a ceiling.

**Other rows of lane `integ/p2-follow` firing now** (each from its own base): p2f-ops-act, p2f-ops-ui, p2f-ops-daemon
(cockpit Operations page, `theseus/src/client.rs`, calendar cache); p2f-caps (card drafting, guidance, forced-tool
calls: likely the provider request or its compile); p2f-threads (threads cards, families, `drive_read`); p2f-onto
(ontology names); p2f-bench (memory exam config, scale rig, bench/recall tests, Haiku tier); p2f-fanout (async
fan-out overlap: turn loop or tool runner). Files you may share: `provider.rs` (p2f-caps), `turn.rs` (p2f-fanout),
`bench/` (p2f-bench), core `AGENTS.md` (everyone). Keep those edits small and local; name them in the join notes.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (editor tools, never a shell heredoc; check it is not
empty after the commit), push, and end with a final message that starts `CLOUD REPORT COMPLETE`.

## Your task: p2f-turn, the turn-budget review's LOW fixes, the bench driver's deadline, and a long model call not cut at 600 s (theseus-rmjqr, theseus-5mkz1, theseus-5pg8u)

Branch: `cloud/20261024-p2f-turn`.

Every commit subject carries its Beads id (`theseus-rmjqr`, `theseus-5mkz1` or `theseus-5pg8u`). End each message
with the same `Co-Authored-By:` trailer the base's recent commits carry (copy it from `git log -3 --format=%B
fb721356`). You cannot sign here. Budget: about 4 hours.

## Your row

Read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` first (the section "A turn's time, its progress, and its
commands' CPUs", near line 2843).

### theseus-rmjqr: the turn-budget review's LOW findings (all IN except where marked)

The review read the p2-turn-budget branch; its line numbers are the branch's. At the base:

**(1) `since_pass` counts loops that ran no command.** `progress_step.rs` `TurnRunner::progress` (~200-220): `check`
is `None` for a loop with no `proc.run`, and `None => s.since_pass += 1`. So a deadline turn that never runs a check
(reads, edits, writing) is steered at 50% and stopped at 85% of its deadline.
- **OUT (owner call): changing which loops count.** Not counting no-command loops would make `since_pass` equal
  `failing` (a pass resets both, a failed check adds to both, nothing else adds), so the rule would fire only after 3
  *failing* checks: a stricter form of the owner's open question "gate the steer on `failing > 0`". The count as it is
  matches the module doc and `Signals::since_pass`'s doc ("all of them, with none yet"), which the design and review
  accepted. Leave `decide` and the counting alone; list it in the report as an OWNER CALL.
- **IN: the note's words.** With `failing == 0`, every loop since the last pass ran no check, yet `note()` says "no
  check has passed in its last N loops", which a model reads as checks failing. When `failing == 0` say "no check has
  run in its last N loops"; keep the `(f failed)` form as it is. The note is a harness note to the model, not a Discord
  message: the DM golden is untouched.

**(2) A failed read marks the loop's check failed.** `turn.rs:3166` in the `CallOutcome::Done` arm:
`t.checkpoint.failed |= status.is_error();` for every tool, and `progress` reads `t.checkpoint.failed` (~201). A loop
whose `proc.run` passed but whose `fs.read` errored counts as a failed check. Fix: the check's pass or failure comes
from the loop's **command** results only. Keep `Checkpoints::failed` exactly as it is for the checkpoint
(`checkpoint_step.rs` ~77, taken at ~333).

**(5) The background test compares counts.** `progress` ~205: `(ran && background == s.background)`; a loop that
finished one background job and started another reads as "no new background", and its command counts as a check.
Fix: a loop's command sent to the background (its own `CallOutcome::Background`) makes the loop no check, whatever the
count did.

**One change for (2) and (5)** (keeps `turn.rs` at net zero): in `run_tools`' result loop (`turn.rs` ~3158-3170), one
call per result before the `match`, for example `t.checkpoint.answered(&uses[r.index].name, &r.outcome);`
(`toolrun::Ran` has `index`, the call's place in `uses`), and move the `failed |=` line into it. The helper lives in
`checkpoint_step.rs` (or `progress_step.rs`), sets `failed` as today for any tool, and for a command (`is_command`,
`proc.run` or its wire name) records the loop's facts: a command answered, a command failed, a command sent to the
background. `progress` takes those facts (`mem::take`, every loop, including loops where `run_tools` did not run) and
computes `check` from them; the `State::background` count goes. Make the classification a small pure function and
unit-test it.

**(3) `deadline_secs` is untested end to end.** `rpc/methods.rs:987` sets `set_deadline(&session.session_id,
conn.arrived, p.deadline_secs)`, and `theseus/src/cmd.rs:103` passes `deadline_secs: a.deadline` (flag at
`theseus/src/main.rs:828-831`). Every test sets the deadline with `set_deadline` directly. Add one RPC-level test: a
`turn.submit` with `deadline_secs: Some(600)` through the core (see `rpc/tests.rs`: `submit()` at ~142 builds the
params, `serving_core` at ~106; reuse them) and assert the fake provider's first request holds the time line
("minutes left of this turn's 10"). Put it in a new `rpc/tests_submit_deadline.rs` with one `mod` line in
`rpc/mod.rs`. This test is green at the base (it covers wiring that works); its proof is the plant: pass `None`
instead of `p.deadline_secs` in `methods.rs`, and it fails.

**(4) A session's deadline is inherited by its later turns.** `clock_step.rs` keeps `DEADLINES` by session
(`set_deadline` ~74, `deadline_of` ~92), cleared only by a later `turn.submit`; `progress_step.rs` `with()` reads
`deadline_of(session)` for every fresh turn. A nudge, a cron wake, a task resume or an approval's continuation of that
session inherits it. Fix: the deadline belongs to the turn its `turn.submit` started. `turn_submit` passes the same
`conn.arrived` to `set_deadline` and to `TurnRequest.arrived` (`methods.rs` ~1063), and the turn keeps it as
`Turn.arrived` (`turn.rs` ~383, set in `Turn::start`; `progress_step` is a child module, so it can read it). Look the
deadline up by session **and** arrival (`from == t.arrived`): any other turn of the session has none. Drop the entry
when its turn takes it. A steer (a submit that joins a running turn) does not give the running turn a deadline: say so
in the module doc. Update `clock_step.rs`'s module doc and the core `AGENTS.md` paragraph ("kept in memory by session
... until it passes"). The existing rig in `tests_turn_progress.rs` (`Rig::turn`, ~61) sets the deadline with a start
in the past and runs with `arrived: None`: pass `arrived: Some(<that start>)` so its turns still match. In
`clock_step.rs`, keep the test `a_deadline_is_kept_by_session_until_it_passes` under its name with every assertion
(adapt its calls to the new lookup) and add the other-arrival case.

**(6) `with()` keeps a `State` per execution in a global map.** `progress_step.rs` ~120-161: `STATES` gets an entry at a
turn's first loop and loses it only on the progress path's end (`forget`); a turn ending by a `?` error leaves it.
Fix: hold the state on the turn, as a field of `checkpoint_step::Checkpoints` (already on `Turn`, `Default` per turn,
so `turn.rs` does not change), and drop `STATES`, `forget` and `held`. Each turn has its own id (`turn_id_for`), so
the lifetime is what the map gave. `clock()` and `shown()` then take `&mut Turn`. If a borrow makes that unworkable,
keep the map and remove the entry in `TurnRunner::fail` (one `turn.rs` line); say which you did. Update the module
doc's last paragraph.

### theseus-5mkz1: the bench driver passes `--deadline` (IN: the driver change only)

At the base, `bench/harbor/theseus_agent.py` `Theseus.run` (~186-250) runs `tb.run_script(...)`
(`bench/harbor/theseus_bench.py` ~247-298), whose `ask` line is `{spawn} ask --follow-for {follow_for} -`: no
`--deadline`. So in a benchmark only the spend steer can fire; the time steer and the stop-and-report never do. The CLI
flag exists at the base (`ask --deadline SECS`, `main.rs` ~828).
- **One helper, not two:** `bench/async/async_agents.py` `_task_timeout(environment)` (~206) reads `task.toml`'s
  `[agent] timeout_sec` (900 when absent; task dir = `environment.environment_dir`'s parent). Move it into a pure,
  standard-library `task_timeout(task_dir) -> float` in `theseus_bench.py` and call it from both agents.
- Add `deadline_secs(timeout) -> int | None` in `theseus_bench.py`: the task's timeout less `DEADLINE_MARGIN_S = 60`
  (a documented const: one minute for the stop note's report, the stop script and the history read), and none when
  that leaves under 60 s. `THESEUS_BENCH_DEADLINE=0` in the environment turns it off, as the other `THESEUS_BENCH_*`
  knobs do, so a later A/B can run without it.
- `run_script` gains `deadline: int | None = None`; with one, the `ask` line carries `--deadline <n>` (quoted like the
  rest). `Theseus.run` passes it for both arms (plain and routed). Update both docstrings and the bench README's driver
  paragraph in a sentence.
- Harbor's timeout multiplier, if a run sets one, is not read (the async agent does not read it either): the deadline
  then falls early, never late. Say so in the docstring.
- **OUT:** measuring the next bench run (spend on unsolved tasks, timeouts, `nr_throttled`, `turn.progress` rows per
  trial); pointing `Cpus::read` at the job's parent cgroup (the light-job-cgroup lane has not landed); a task's own
  deadline; and the OWNER questions (config keys for the progress consts, whether spend ever stops a turn, exit 8 or 0
  when a stopped turn reports). List them in the report; touch none.

### theseus-5pg8u: a max-effort call cut at 600 s (IN, with the STOP rule below)

The bench report: a regex-chess trial ran its turn at effort `max` and ended with `theseus: provider timeout (Total)
after 600001 ms [class=timeout, transient=true, code -32003]`, exit 1, after 4 answered model calls. What the base
shows:
- **The cut.** `Anthropic::stream_message_impl` (`provider.rs` ~963-1110) sets `deadline = started +
  timeouts.total_secs` (default 600, `Timeouts::default` ~381) and cuts the call there whatever it is doing; the stream
  loop's budget is `idle.min(deadline - now)` (~1044-1074). The client streams, and a silent stream is already cut at
  `stream_idle_secs` (60). A `Total` phase means events (thinking deltas or the API's `ping`s) kept coming: the call was
  alive when it was cut.
- **The retry exists.** `turn/retry_step.rs` `retry_after` retries a transient failure up to `[model.retries]
  transient` times a turn (default 0; the bench profiles set 4, `bench/theseus-bench.toml` ~44). The trial's history
  fits: its 4th answer came ~30 minutes after its 3rd, the failure ~34 minutes after that. So the turn most likely
  retried, each try cut at the same 600 s: the timeout is the bug, not a missing retry (no `model.retried` rows were
  kept, so say "most likely").

**What to build:**
1. **A live stream is not cut at `total_secs`.** Once the stream began, the call goes on while events keep arriving
   within `stream_idle_secs`, up to a hard bound of `LIVE_STREAM_TOTALS × total_secs`, `const LIVE_STREAM_TOTALS: u32 =
   3` documented beside `Timeouts` (a max-effort call can think for many minutes; the stream shows the provider is
   working; a stream that trickles forever must still end). Before the stream begins, `total_secs` bounds the call as
   today. Past the hard bound the error stays `Timeout { phase: Total }`. One method holds the bound (for example
   `Timeouts::whole_call() -> Duration`). No new config key; `Timeouts::default()` and the example toml's values stay;
   reword the example toml's `total_secs` comments (~77, ~284) to say what it bounds now.
2. **The kernel's action deadline follows it.** The model call's action is dispatched with
   `Some(self.cfg.model.timeouts.total_secs * 1000)` (`turn.rs` ~2407, `turn/compaction.rs` ~363); past that deadline
   the driver marks an action unknown. Use `whole_call()` at both sites (in place in `turn.rs`). Check `rpc/driver.rs`
   (~500-540) for other readers; fix the words of `telemetry/metrics.rs` ~434 if they now mislead (not `BOUNDS`).
3. **A whole-call timeout is retried at most once in a turn.** In `retry_after`, a `ProviderError::Timeout { phase:
   Total }` (downcast `failure.source`, as `turn.rs` ~1942 does for `overflow`) is retried once a turn, whatever
   `transient` allows: the same call would most likely run as long again. Other transient classes keep their count.
   With `transient = 0` nothing changes. If you need a second counter, make `retried` (`turn.rs` ~1803, `let mut
   retried = 0;`) a small `Default` struct defined in `retry_step.rs`, so the `turn.rs` edit stays in place.
4. Docs: the core `AGENTS.md` line on `[model.retries]` and provider timeouts, and `retry_step.rs`'s module doc.

**STOP rule.** If `python3 scripts/keel-guard.py --base fb721356 --head HEAD --quiet` flags any of items 1-3 (a
`cap-raised` or `budget-raised` finding), revert that item and write in the report: `OWNER CALL: theseus-5pg8u live
stream bound: 600 s whole call -> 600 s before the stream, 1,800 s while it is live` (with the finding's line). Do not
rename a const to get past the guard. The judge's own client (`theseus-judge/src/client.rs`) is not in this row.

## Tests (each red at the base, with a planted revert)

New-API tests that do not compile at the base show their redness by a plant: for each, revert the fix, run the test,
and put the failing test and line in the report.
- **(1)** `progress_step.rs` `the_notes_say_what_was_used_and_what_has_not_passed`: change its `failing == 0` case to
  expect "no check has run in its last 8 loops" (same number of assertions), and add a case that the `(3 failed)` form
  is unchanged. Red at the base (the base says "has passed"). Plant: the old arm.
- **(2)** `tests_turn_progress.rs` `a_passing_command_beside_a_failed_read_is_a_pass`: a deadline started at 0.6 used;
  every loop calls `proc_run` exiting 0 and `fs_read` of a missing file (one `Scripted::tools` with both calls); after
  four loops, no `turn.progress` row and no harness progress note. Red at the base (a steer row at loop 3). Plant:
  `failed |= status.is_error()` for every tool in the check.
- **(5)** a pure unit test of the classification: a command answered `Background` makes the loop no check even when the
  turn's background count is unchanged (one finished, one started); a command `Done` with an error is a failed check;
  no command is no check. Plant: compare counts again.
- **(3)** `rpc/tests_submit_deadline.rs` `a_submit_with_deadline_secs_tells_the_model_its_time`, and the same submit
  with `deadline_secs: None` has no time line. Green at the base; plant above.
- **(4)** `tests_turn_progress.rs` `a_later_turn_of_the_session_does_not_inherit_its_deadline`: turn 1 with a deadline
  (arrival matching) shows the time line; turn 2 on the same session with another arrival (as a nudge or a wake runs)
  has none in any of its requests. Red at the base. Plant: look up by session only.
- **(6)** No behaviour shows the leftover entry: with the state on the turn, `STATES` is gone (no test); with the map
  kept, a `#[cfg(test)]` count shows a turn failing on a 400 at its second loop leaves no entry.
- **5mkz1** `bench/harbor/test_bench.py`: `run_script(..., deadline=3540)` puts `--deadline 3540` in the stand-in's
  `ask-args` (the `STANDIN` script writes them, ~477) and no `--deadline` without one; `task_timeout` reads 3600 from
  an invented `task.toml`, 900 when absent; `deadline_secs(3600) == 3540`, `deadline_secs(90) is None`,
  `THESEUS_BENCH_DEADLINE=0` gives none. Red at the base. Keep the async agent's tests green.
- **5pg8u item 1** new `provider/tests_timeouts.rs` (one `mod` line beside `tests_warm` in `provider.rs` ~839; copy the
  loopback-server pattern of `provider/tests_warm.rs`): with `total_secs = 1`, `stream_idle_secs = 1`, a server that
  sends a valid message stream with `ping` events every 200 ms for about 2.5 s and then `message_stop` answers `Ok`
  (red at the base: `Timeout Total` at 1 s); one that pings forever is cut with `Timeout { phase: Total }` between 3
  and 4 s; one that goes silent after its headers is cut as `StreamIdle`. Plant: the loop's budget back to
  `deadline - now`.
- **5pg8u item 2** a unit test of `whole_call()` (3 × `total_secs`); plant: one call site back to `total_secs`.
- **5pg8u item 3** `tests_failures.rs` (next to `a_transient_failure_is_retried_inside_its_turn_as_the_config_allows`
  ~283, using its `rig_with` and `Scripted::Fail(ProviderError::Timeout { phase: TimeoutPhase::Total, elapsed_ms })`):
  with `transient = 4`, [Total, answer] answers with 2 requests; [Total, Total, answer] fails with 2 requests; [StreamIdle,
  StreamIdle, answer] still answers with 3. Red at the base (it makes a third request). Plant: drop the Total rule.

## Not in this row

The other `integ/p2-follow` rows (listed above) and their files: leave them. No cockpit, no protocol type, no
`cockpit/src/protocol.gen/`, nothing in Discord, no config key, no store field. The progress consts
(`MIN_LOOPS_WITHOUT_PASS`, `STEER_SHARE`, `STOP_SHARE`) and `decide` stay as they are. The judge's client timeouts stay.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical (`git diff
  --exit-code fb721356 -- crates/theseus-discord/src/golden/dm_flow.txt`). A change that would alter any message of that
  flow STOPS that part: write `OWNER CALL: <what and why>, before/after text` in the report instead.
- **Keel:** no removed or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py` or its
  tests, no renamed test. A cloud row cannot carry a `Keel:` trailer. Run `python3 scripts/keel-guard.py --base
  fb721356 --head HEAD --quiet` before the report and paste its output (it must be empty).
- **Public repo:** no person's name, organisation, address or email anywhere; fixtures use invented names and ids
  (`example.test`); write "the owner".
- Red first; real-world behaviour, never a benchmark special case (no benchmark task name in code or tests).
- Right over compatible; one helper, not two (reuse what the lane already has).
- **The gate for the touched areas:** `cargo nextest run -p theseus-core -E
  'test(/progress_step|clock_step|tests_turn_progress|tests_submit_deadline|tests_failures|tests_timeouts|retry_step|tests_stopping/)'`,
  then `cargo test -p theseus-core`, `cargo test -p theseus`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --check`, `python3 -m unittest discover -s bench/harbor` and `python3 -m unittest discover -s
  bench/async`, then once at the end `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (the known L1 reds
  on a VM without root: `theseus-sandbox::contract`, `spawn_100`, `theseusd::sandbox`; name any other red).
- No new dependency in any `Cargo.toml` or `Cargo.lock`.

## Your report

`CLOUD_REPORT.md`, committed, sections in this order:
1. **For the maintainer first:** store format (49, unchanged, or why not); protocol unchanged; `turn.rs` line count;
   the OWNER CALLs: (1)'s counting rule (failing > 0 gate), the 5mkz1 owner questions, and any 5pg8u STOP.
2. Per finding (rmjqr 1-6, 5mkz1, 5pg8u items 1-4): done or not and why, the files, the test, the planted revert with
   its failing test and line. For (6), which way you went.
3. The gate commands and their results.
4. The keel output (empty), and the DM golden diff result (exit 0).
5. Join notes for `integ/p2-follow`: the lines you changed in `provider.rs`, `turn.rs`, core `AGENTS.md` and `bench/`,
   for the siblings that may touch them.
6. A `docs/status.md` sentence for the integrator, and follow-ups (the OUT items above; the bench driver could keep
   `model.retried` rows beside `theseus-judge.json` so a trial shows its retries).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
