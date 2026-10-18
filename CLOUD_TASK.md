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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261018-task-checkpoint-routing`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: task-checkpoint-routing, Jev re-judges a running task at checkpoints and moves its model up, or back down to its base, by routing's own rules (theseus-ypnxt.9)

Branch: `cloud/20261018-task-checkpoint-routing`. Every commit's subject carries `theseus-ypnxt.9`. Deadline for the
report: 5 hours after you start.

**Why.** The owner (Discord DM, 2026-10-18 09:07 MST, verbatim): "Yes I think that tasks having dynamic range in model
selection will make them more match benchmark results which have been strong both on completion and improving
iteratively on price". Today Jev's route pack judges a person's message at `inbound` (`judge/inbound.rs:265`
`at_inbound`) and, since theseus-ypnxt.7, a task's brief once (`judge/inbound.rs:280` `at_brief`,
`turn/brief_step.rs:36` `brief_point`, `routing/brief.rs:18` `brief_base`). After that a task's model never moves: a
task is mostly one long turn of many loops (`turn.rs:1764`, `while run_model`), and the turn routes once
(`turn/route_step.rs:790`, "a turn routes once"). A task stuck on a cheap model stays stuck; a task Jev moved up stays
up. The benchmark runs that did well on completion and on price escalated and came back down: give tasks that range.

**Risk: medium-high.** This is a model switch in the middle of a turn, which no routing code does today. Keep it in new
files, small, on by default, and measured. Routing changes cost and quality; the benchmark rerun measures it.

**Prerequisite:** `main` (`b95a2558` or later).

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `task-checkpoint-routing` | theseus-ypnxt.9 | A: today, pinned; B: the checkpoints; C: the verdict's input; D: the rule; E: the switch at a loop boundary; F: config, records and words; G: Jev's cost | new `crates/theseus-core/src/routing/checkpoint.rs` (the pure rule), new `crates/theseus-core/src/turn/checkpoint_step.rs` (the turn's side), `judge/inbound.rs` (one `at_checkpoint` beside `at_brief`), `config/routing.rs` (`[routing.task_checkpoints]`), a call or two in `turn.rs`, the record (`fact/route.rs`), `task show` and the progress line's model part; tests beside each | M-L |

## A. Pin today first
Before any change, commit two tests on the rig of `tests_task_brief.rs` (the driver runs tasks; `FakeJev` scripts the
route pack; the scripted provider answers loops):
- a task on a cheap base whose loops fail many times in a row runs every loop on its base, and the route pack is asked
  once (the brief);
- a task started with `--profile` runs every loop on that profile, and the route pack is asked nothing.
Then convert the first to the new rule (show the old and new assertion); keep the second as it is.

## B. The checkpoints (a "step" here is one loop: one model call and its tool calls)
A running task asks a checkpoint verdict, at a loop boundary (where `steer_in` and `paused_at` run, `turn.rs:1764-1784`),
on any of these, whichever comes first:
1. **Failing:** the last `failing_streak` (K) loops in a row each had a failed tool result: a tool result that is an
   error as the tool runner already marks it (a test or build command's non-zero exit the exec tool reports as an error
   counts; do not invent a new failure test, read what the runner marks).
2. **No progress:** `quiet_loops` (N) loops in a row with no progress signal. A progress signal is one of: a plan step of
   the task marked done (the task graph's steps, `push/work/settle.rs:399-433`, `WorkKind::Step`), the task's progress
   words changed (`Progress::said`), or a loop with no failed result after a failing run (recovery).
3. **Phase change:** the task's current step changes (`Progress::current`, the first open step, moves to another step),
   for example a plan step done and implement begun. A task with no plan has no phase change.
4. **Before the report:** the loop at which the task's last open plan step is marked done, so the loop that writes the
   report runs on the checkpoint's model. A task with no plan has none of these.

**Which tasks.** A task whose base came from its brief or its parent (no profile asked). A task started with a profile
is pinned: no checkpoint, no Jev call. A conversation's turns are untouched: no checkpoint outside a task, and a plain
turn's frames and requests stay byte-identical.

**Never waited on.** A checkpoint is asked at a loop's end and its answer applied at the first later loop boundary that
finds it come; the loop never waits for it (FAST). At most one checkpoint in flight per task. A late answer older than
the next checkpoint's ask is dropped.

## C. The verdict's input (bounded)
The checkpoint asks route.v6 the same way the brief does (`at_brief`'s route-only path, `inbound.rs:284`
`inbound_early_of(msg, true)`), with `task: true`, and with a text that is the task's recent state, not its history:
- the brief's first 600 characters;
- the plan's steps with their states, at most 12;
- the last 6 loops' own lines (`say`, `turn/advance_step.rs:15` `say_of`, already on `loop.ended` rows), each with
  its failed results' first line when it had any;
- the trigger, in one plain line (`failing: 3 failed loops in a row`, `no progress for 8 loops`, `phase: plan →
  implement`, `before the report`).
Bound the whole text at **1,500 tokens** (cut the oldest loops first) and test the bound. Do not change route.v6's
prompt or any pack's text: the text is the message the pack judges.

## D. The rule (routing's own, in `routing/checkpoint.rs`, pure)
Write it as one function beside `routing::decide` (`routing.rs:432`) and `step_down` (`routing.rs:388`), reusing them:
- **Only Jev elevates.** The verdict's class picks its first usable profile under the place's cap
  (`routing::pick`, `routing.rs:268`), as a conversation's switch does.
- **The base** is the task's base: its brief's pick or its parent's (the session's `last_target` after the first turn;
  `routed.from` is empty for a task, `route_step.rs:813`). Moving up records the base as `routed.from`, so the step
  back down works from it.
- **Step back down needs two agreeing verdicts** (`STEP_DOWN` hold, `routing.rs:360`): the first checkpoint verdict whose
  class's pick is the base or cheaper sets the hold, the second makes the switch. A verdict of another class, or under
  its bar, breaks the row.
- **Never below the task's base.** A verdict whose pick is cheaper than the base steps down to the base, no further.
- **The cold-switch hold** (`[routing] cold_switch_tokens`, `config/routing.rs:43`): a switch up at an estimate at or
  above it waits for a second agreeing checkpoint verdict (a `Hold`), with the warm prefix subtracted as
  `decide_route` does (`route_step.rs:668-680`, theseus-rr15).
- **Detours do not apply in tasks.** A `trivial` or `quick` verdict at a checkpoint moves nothing, and counts toward a
  step down when its pick is no dearer than the base.
- **The confidence bar** is the class's (`RoutingConfig::confidence_for`, `config/routing.rs:359`); under it nothing moves.
- **The data floor** (`[privacy] sensitive_names`, theseus-oejh3) applies as on any turn: route through
  `route_profiles(sensitive)` and `data_floor` (`turn/data_step.rs:44`), never around them.
- **The cap on switches:** at most `max_switches` moves per task (up or down; a step down after the cap is still
  allowed, so a task is never stuck above its base). Past it, verdicts are recorded and move nothing up.

## E. The switch, at a loop boundary
- The next loop runs on the new target: its spec rebuilt as `route_to` builds one (`route_step.rs:780-830`:
  `request_spec`, `session_block`, `walk`, `stand`, `thread_spec`, `defer_tools`), its compile made on the new profile,
  thinking stripped as a routed switch strips it, and the session's `last_target` and `routed` written in the loop's
  own frame. The provider is resolved as `compile_routed` resolves `routed_to` (`turn.rs:1762`).
- The model that moves mid-turn already exists once: a refusal's fallback (`turn/fallback_step.rs:32`, `fall_back`,
  a `OnceLock<Target>` slot per turn). Take that shape: one slot per allowed switch, bounded by `max_switches`
  (validated at load against the slots), never a leak. Keep "a turn routes once" true for the inbound route; the
  checkpoint has its own slots.
- The reservation, the effort (`routing::effort`, the class's level for the new profile) and the trace root
  (`route_to`'s `set_root`) follow the switch, as for an inbound route.
- `turn.rs` is at 3,359 of its 3,525 ceiling: add at most a call line or two there; the work lives in
  `turn/checkpoint_step.rs`.

## F. Config, records and words
- `[routing.task_checkpoints]`, on by default, in `config/routing.rs`, the template
  (`crates/theseus-core/config/theseus.example.toml`, `[routing]` at :1493) and the owner-shape fixture
  (`crates/theseus-core/fixtures/owner_shape.toml`): `enabled = true`, `failing_streak = 3`, `quiet_loops = 8`,
  `min_loops_between = 4` (M, below), `max_switches = 2`. `deny_unknown_fields` as its neighbours; a value of 0 for a
  count is an error at load, not a silent off (use `enabled`).
- **The record:** each checkpoint verdict is one `route.decided` row (`fact/route.rs:22`) with `source` =
  `"task checkpoint"`, plus optional fields `trigger` (`failing`, `quiet`, `phase`, `report`) and, on a switch,
  `cold_tokens` (the compile's estimate less the warm prefix on the new model) and `cache_usd` (what writing those
  tokens cold costs at catalog prices). No new ledger kind; old rows read as before. `theseus why` and the footer's
  `routed:` read it: `routed: deep_coding (task checkpoint: failing)`, or your clearest equivalent in the
  existing style.
- **The words:** `theseus task show` prints the task's current model and each move (`model: sonnet (from haiku, loop 7:
  failing)`), from the rows. The progress line (`crates/theseus-protocol/src/work/progress_line.rs`, its golden
  `work/golden/progress_line.json`) adds an `on <profile>` part only for a task a checkpoint has moved off its base: an
  unmoved task's line is byte-identical, the 8 golden cases unchanged, and you add cases for a moved task. The
  `WorkView` field you need is optional with a default (`crates/theseus-protocol/src/work.rs:141`).

## G. Jev's cost, bounded and counted
- **At most one checkpoint verdict per `min_loops_between` (M) loops** per task, whatever the triggers say; a trigger
  inside the window waits for it (the failing and quiet counts keep running).
- **Jev's spend is the task's.** A checkpoint's settled cost is charged to the task's execution, so the task's `spent`
  (and the parent's, by the kernel's carry, `crates/theseus-kernel/src/tasks.rs:531`) shows it. It still draws from the
  judge's day budget as every judgment does (`judge/mod.rs:645`, `judge/spend.rs`); say in the report how the day's
  total avoids counting it twice. A task whose budget cannot cover the checkpoint's reservation skips it (recorded,
  reason `budget`). The breaker open, the judge paused or off: no checkpoint, the task runs on.

## Tests (the rig of `tests_task_brief.rs` and `tests_route_down.rs`; `routing/checkpoint.rs`'s own unit tests)
Each with a planted revert: break the behaviour, see the named test fail, restore.
- a task on Haiku (a `haikuhi`-style base in the rig) whose loops fail K times in a row moves to Sonnet at the next
  loop, the switch recorded with its `cold_tokens` and `cache_usd` (plant: never ask at a failing streak);
- a task that recovers steps back to its base only after two agreeing verdicts; one agreeing verdict then a
  disagreeing one leaves it up (plant: step down on the first);
- never below the base: a `trivial` verdict on a task whose base is Sonnet leaves Sonnet (plant: let the pick go under
  the base);
- the cold-switch hold: at an estimate above `cold_switch_tokens`, the first agreeing verdict holds and the second
  switches (plant: switch at once);
- the cap: after `max_switches` moves up, a further elevating verdict moves nothing, and a step down still works
  (plant: no cap);
- at most one verdict per M loops: a task failing for 12 loops asks at most 3 (plant: ask every loop);
- Jev's cost lands in the task's spent (plant: leave it out);
- a pinned task asks nothing; a conversation turn's frames and requests are byte-identical with checkpoints on;
- the input bound: a task with 40 long loops gives a checkpoint text of at most 1,500 tokens, the trigger line kept.
The turn bench and the owner-config fixture test pass; quote their numbers.

## Overlaps
- **A local morning lane (theseus-31pgz)** is changing the task model by brief (routine_coding briefs pick Sonnet,
  trivial ones Haiku), the progress line (it keeps the cost forecast `[$x of ~$p50]` beside the time part), and Discord
  forum fixes. Build on `main` as it is; touch `routing/brief.rs`, `turn/brief_step.rs` and `progress_line.rs` only in
  the lines you need, and list each in "Overlaps". **For the joiner:** resolve against theseus-31pgz's commits (it may
  land first); keep both: its brief picks and its progress-line words, and this row's checkpoint rule and `on <profile>`
  part.
- **After this lands,** routine_coding task briefs may go Haiku-first again, because a failing Haiku task now moves up.
  Do not make that change: name in the report the one line of the routing table (the brief rule or the class list,
  wherever theseus-31pgz puts Sonnet for routine_coding briefs) that would do it.
- The RSI rows in flight (mint-validate, sealed-hold) touch no file of yours.
- The China-origin data rule (theseus-a1wvq) builds on `[routing]` later: name your functions plainly and add two lines
  on checkpoints to `crates/theseus-core/AGENTS.md`'s routing section and `routing.rs`'s module doc.

## Report extras
- A cost note on the stand-in: three scripted tasks (one that fails three loops then passes, one that stalls, one clean)
  on a Haiku base, the model each loop ran on, the checkpoints asked, and Jev's cost per task. Not a benchmark.
- The one place to switch the rule off (`enabled`) and the one place its code joins the turn (file:line).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at **`b95a2558`**
(2026-10-18 05:16 MST, the night join of the last-features batch and RSI rows) or later, with **store format 45**. The
`file:line` references in this task were read on `b95a2558`. A line may have moved a little, so search by name.

`main` already holds the last-features rows (task history, the one progress line, pane answers, chat start and pause,
Discord tasks, the cockpit's Tasks page, a task's model from its brief, the acceptance journey) and the trim rows. Use
their names, never older ones:
- the config: `[routing.classes.<name>]` (not `modes`), `on_max_loops`, one `[cache]`. An old key fails the load with
  "`<old>` was renamed to `<new>`";
- the words: a task is a "task" everywhere a person reads; one verb for an answer, `answer`;
- nobody calls `task.get`; read one task through `task.list`.

**Still landing beside you. Build on `main` as it is; the joiner resolves.**
- **A local morning lane (theseus-31pgz):** the task model by brief (Haiku only for trivial briefs, routine_coding briefs
  on Sonnet), the progress line keeping the cost forecast, every CLI task getting a forum post, questions never silent
  under `ping_window`, herdr acceptance tests. See your task's "Overlaps".
- **Two RSI cloud rows** (mint-validate, sealed-hold): self-improvement's mint and the sealed scorer. No file of yours.

Line ceilings at `b95a2558` (`scripts/long-files.txt`, length / ceiling):
- `crates/theseus-core/src/turn.rs` **3,359 / 3,525**
- `crates/theseus-protocol/src/lib.rs` 2,706 / 2,733
- `crates/theseus-core/src/tests_m3.rs` 7,973 / 8,050
- `crates/theseus/src/render.rs` 3,108 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,497 / 3,500
- `crates/theseus-discord/src/render.rs` 2,963 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030

Never raise a ceiling. Put new code in new or small files: `routing/checkpoint.rs`, `turn/checkpoint_step.rs`, a new
`tests_task_checkpoint.rs` (not `tests_m3.rs`). A new file over 2,500 lines is a shape failure.

## Out of scope

- Route.v6's prompt wording and any Jev pack's text; a new pack; the sealed holdout set.
- Checkpoints in a conversation (not a task); re-routing a pinned task; nested tasks.
- Changing the routing table itself (the classes' profile lists, the brief's picks, the bench's routed config): name
  the line, do not make it.
- Any config-key rename the trim rows did not make; `PLAN_NOTE`; the LTO profile.
- theseus-31pgz's and the RSI rows' work (above).

If your work shows one of these is needed, write it in the report and stop there.
## Rules for this row
- **Model roles.** You write the code. Keep it minimal and elegant: reuse what exists (the reads, the renderers, the
  RPCs named above), and let every line you add earn its place.
- **Nothing lost.** Behaviour, wire JSON, stored records and user-visible output stay the same except where this task
  changes them. Where it changes them, pin the old output in a test first, then change it, and show the diff in the
  report. New wire fields are optional with a default, so an old client still reads a new answer.
- **The first-request budgets hold.** No new model tool, and no always-loaded text:
  - the first request as sent stays at or under **8,000 tokens in cli and task places** and **15,000 in the owner's
    DM**. The turn bench and the owner-config fixture test must pass, and the report quotes their numbers;
  - FAST: nothing new on a plain turn. A plain turn stays 5 frames. A new read runs only when a person asks for it
    (a command, a key, a page), never per turn.
- **Every behaviour change gets a test and a planted revert**: break the behaviour, see the named test fail, restore.
  The report lists each plant, the test that caught it, and its failure line. A pure move needs no plant: say so.
- **Keel.**
  - Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Converting a test to a new
    wording that this task orders is fine; show the old and new assertion.
  - Before your last commit, run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its
    output in the report. It must be empty. If it is not, give the finding and the exact `Keel:` trailer the guard asks
    for, and say why the change is the task's; the joiner decides and signs.
  - Do not edit `scripts/keel-guard.py` or its test.
- **No store format change. No new dependency.** Cargo.lock and the package-lock.json files gain no package. A new
  META key or a new ledger kind is not a format change; a new table or a changed record shape is (stop and report).
- **Right over compatible** (the owner, 2026-10-17): one right name for each thing, no aliases. When you rename a word a
  person reads, fix every caller (scripts, bench, `contrib/herdr`, docs, the cockpit) in the same row.
- **Live, not shadow.** What you build is on by default and works on the owner's daemon after install. No feature flag
  that defaults off.
- **Discord stays lively.** Do not silence anything that pings today. A new post or edit follows the existing ping
  policy and the live board's pace (at most one edit per 2 s).
- **The cockpit loses no datum.** Anything you move names its new home in the report.
- **No person's or organisation's name** anywhere in the repo, in commit messages, or in the report:
  - write "the owner";
  - use obviously made-up names in tests (Examplecorp, Acme, Widgetco; harbour and quay titles are fine);
  - if you edit a file that already carries a person's name, replace it with "the owner" in the lines you touch.
- **Read the AGENTS.md/CLAUDE.md of each crate you touch, first**, and keep each AGENTS.md true for what you change (a
  line or two).
- **Keep CLOUD_REPORT.md honest:** what you ran, what passed, what you could not run in the cloud. It has these
  sections: "What changed" (file:line), "Tests and plants" (a table), "Budgets" (the turn bench's and the
  owner-config fixture's numbers), "Overlaps" (every line you touched in another row's files, and every name you used
  that a landing lane renames), "Proof nothing was lost", and "For the joiner".
