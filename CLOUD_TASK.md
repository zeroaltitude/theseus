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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-progress-words`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: progress-words, one progress line (step, $, time left, paused) on every Rust surface, and `paused` / `stopped` forum tags (theseus-ypnxt.2)

Branch: `cloud/20261017-progress-words`. Every commit's subject carries `theseus-ypnxt.2`. Deadline for the report: 5
hours after you start.

**Why.** Live monitoring is what the owner is most excited about, and today each surface shows a different subset of
the same facts. Seen live (2026-10-17):
- the Discord board row: "sleep 25 · turn 1 · $0.10 of $25", with no plan step and no time left;
- `theseus tasks --watch`: job lines ("sleep 8 [job 0:05 of usual 0:16]"), with no step change and no cost;
- `theseus tasks`: no `$` at all;
- the chat footer: doing, plan and `$`, but no time left;
- **paused is shown almost nowhere**, and the forum tag maps paused to running.

This row writes **one line** (contract C1) and puts it on every Rust surface. The cockpit's TypeScript mirror is
cockpit-tasks' (theseus-ypnxt.6).

**Prerequisite:** `main` (`40aa83cd` or later). You join **second**, after task-history.

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| 2 | `progress-words` | theseus-ypnxt.2 | A: the line and its golden; B: CLI `tasks` and `tasks --watch`; C: chat footer and task header, TUI rows; D: herdr; E: Discord board row and forum tags | A: a new `crates/theseus-protocol/src/work/progress_line.rs`, `work.rs` (`Progress::line`); B: `crates/theseus/src/render/task_tree.rs`, `crates/theseus/src/work.rs`; C: `crates/theseus-tui/src/chat/footer.rs`, `chat/task_view.rs`, `crates/theseus-tui/src/board.rs`, `work.rs`; D: `crates/theseus/src/herdr_up.rs`, `herdr_up/wire.rs`; E: `crates/theseus-discord/src/render/board_v2.rs`, `runtime/forum.rs`, `runtime/live_board.rs` | M |

Do A first and push it; then B, C, D, E in any order, each green and pushed before the next. A session that runs out
of time still lands A and whatever surfaces it reached.

## A. The line (contract C1, in the shared section below: follow it exactly)
- `pub fn progress_line(v: &WorkView, now_ms: u64) -> Option<String>` in a new
  `crates/theseus-protocol/src/work/progress_line.rs` (declare the module from `work.rs` or `work/`'s parent, not from
  `lib.rs`, which has 4 lines of room). `None` when no part has data.
- `Progress::line()` (`crates/theseus-protocol/src/work.rs:325`) keeps the model's words and takes the same
  `step N of M` count, so there is one wording. Pin its old outputs in a test first, then convert, and show the diff.
- **The golden:** `crates/theseus-protocol/src/work/golden/progress_line.json`, 8 cases, each the view fields used and
  the expected line, read by a test with `include_str!`. The 8 cases (the cockpit row writes the same 8, so keep these
  inputs and outputs exactly; take `now_ms` = `started_at_ms` + the elapsed minutes):
  1. nothing but a title → `None`;
  2. `cost {spent 0.40, limit 2}` → `$0.40 of $2`;
  3. `progress {done 2, total 5}`, cost as 2 → `step 3 of 5 · $0.40 of $2`;
  4. as 3, `forecast {min_p50 10, min_p90 20, n 4}`, 4 min elapsed → `step 3 of 5 · $0.40 of $2 · ~6 min left`;
  5. as 4, 15 min elapsed → `step 3 of 5 · $0.40 of $2 · ~5 min left`;
  6. as 4, 25 min elapsed → `step 3 of 5 · $0.40 of $2 · past its usual time`;
  7. as 4, `paused` true → `paused · step 3 of 5 · $0.40 of $2 · ~6 min left`;
  8. `progress {done 5, total 5}`, `forecast {.., n 0}`, no cost → `step 5 of 5`.
  If `coarse`'s rounding gives other words for 4-6, keep `coarse` and fix the golden's words, and say so: the cockpit row
  is told to take the golden as it lands.
- Plant: swap two parts' order; case 7 or 3 fails.

## B. The CLI
- `theseus tasks`: a row's cost is off today (`with_cost` false, `render/task_tree.rs:491-498`). Each task row ends with
  its progress line (or the parts of it the row does not already show: do not print `$` twice).
- `theseus tasks --watch`: `events()` (`render/task_tree.rs:681`) prints only start, ask, report, fail and cancel,
  though `crates/theseus/src/board.rs:6-7` says step changes print. Add three events: **step** (the plan step moved:
  `◐ <title>: step 3 of 5 · $0.40 of $2`), **past its usual time** (the forecast's `past` turned true, once), and
  **paused / resumed**. One line per change, never per frame; a frame that changes nothing prints nothing.
- Tests: the CLI's renderer tests (`render/task_tree_tests.rs`): a row with cost; a step change prints one line; a
  frame with no change prints none; paused then resumed print two. Plants: print every frame; drop the step event.

## C. The chat and the TUI
- The chat's task footer (`crates/theseus-tui/src/chat/footer.rs:19`, at most 5 rows: doing, plan and `$`) and the task
  view's header (`chat/task_view.rs`) show the progress line, so time left and paused appear there.
- The TUI's task rows (`crates/theseus-tui/src/board.rs`, `work.rs:60`) show it in the same place they show doing and
  `$` today.
- Keep each row's width rule: the line is clipped from its right end, never the `paused` part.
- Tests: the chat's `tests_tasks.rs` / `tests_task_view.rs` and the TUI's `tests_work.rs` / `tests_progress.rs`: paused
  and time left appear. Plants: footer without the line.

## D. herdr
- The board tab's lines come from `crate::work::view_line` / `change_line` (`crates/theseus/src/work.rs`, called from
  `herdr_up.rs:540` and `print_board`). Put the progress line in them (one change to `view_line` reaches the board tab
  and the follower).
- The pane tokens and the Space row (`herdr_up/wire.rs:233-256`) gain `paused` where they show a task's state. Check the
  herdr wire against its contract test (`crates/theseus/src/herdr_contract_tests.rs`); a token herdr's schema does not
  allow is a bug here, not in herdr.
- Tests: `herdr_up`'s tests: a board line has the step and time left; a paused task's token says paused. Plant: a
  `view_line` without the line.

## E. Discord: the board row and the forum tags (contract C4)
- The board row (`crates/theseus-discord/src/render/board_v2.rs:137-174`, `turn N · $x of $y · since …`) shows the
  progress line in place of its own `$` part: `⚙️ Task `…000001` · fs.edit · step 3 of 5 · $0.42 of $2 · ~6 min left`.
  Keep `turn N` only when there is no plan. A paused task shows `⏸️` and `paused` first. Update
  `render/golden/board_v2.txt` and show the diff.
- The live board's pace stays at most one edit per 2 s (`runtime/live_board.rs`): a time-left change alone must not
  cause an edit; it shows at the next edit that happens anyway, or once a minute at most.
- **Tags** (`runtime/forum.rs:79-97`): add `Tag::Paused` ("paused") and `Tag::Stopped` ("stopped"). `Tag::of` maps a
  paused view to `Paused` (it needs the view's `paused`, not only the state: change its input), `Cancelled` and
  `Expired` to `Stopped`, `Failed` to `Failed`. The tags are created where the forum's tags are created today; a
  forum made before this change gets the two new tags added on start, once (the fake Discord must show it).
- Tests: the board golden; a paused task's post gets the `paused` tag and gets `running` back on resume; a cancelled
  task gets `stopped`; an old forum gains the two tags once. Plants: map paused to running; map cancelled to failed.

## Overlaps
- **task-history** (.1) adds `--ended` and its own renderer in `render/task_history.rs`, and touches `tasks_tree.rs`'s
  args: stay out of both.
- **discord-tasks** (.5) owns Discord's `runtime/tasks.rs`, `start_task.rs`, `courier.rs` and `render.rs`; you own
  `render/board_v2.rs`, `runtime/forum.rs` and `runtime/live_board.rs`. Neither of you edits `runtime.rs` beyond a
  `mod` line, nor `task_channels.rs` / `war_room.rs` (review-fixes).
- **chat-start-pause** (.4) owns the chat's `commands.rs`, `keys.rs`, `task_picker.rs` and the TUI's key dispatch in
  `app.rs`; you own the footer, the task header and the TUI's row rendering.
- **pane-answers** (.3) owns `interactive.rs`, `herdr_setup.rs`, `herdr_keys.rs`.
- **cockpit-tasks** (.6) mirrors C1 in TypeScript from the 8 cases above; the joiner points its test at your golden.

## Report extras
- Before and after captures of each surface (CLI `tasks`, a `--watch` run, the chat footer, a TUI row, a herdr board
  line, the Discord board golden), from tests or the sim.
- The live board's edit count in a 10-minute scripted run, before and after (it must not grow).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at **`40aa83cd`**
(2026-10-17 22:21 MST, the trim batch's join) or later, with **store format 45**. The `file:line` references in this
task were read on `40aa83cd`. A line may have moved a little, so search by name.

`main` already holds every trim row. Use their names, never the old ones:
- the CLI: the everyday `theseus help` and `help all`; the `wake`, `model` and `memory` groups; `tasks --watch`;
  `answer ID snooze [WHEN]`; noun then verb for everything that is not an everyday verb;
- the config: `[routing.classes.<name>]` (was `modes`), `on_max_loops`, one `[cache]`, `[watch_for]`, `[route_bench]`,
  `[persona_import]`. An old key fails the load with "`<old>` was renamed to `<new>`".

**Still landing beside you. Build on `main` as it is; the joiner resolves.**
- **review-fixes** (theseus-oejh3, a cloud branch): `[privacy] sensitive_names`, and task channels that close to the
  operator alone (`crates/theseus-discord/src/runtime/task_channels.rs`, `war_room.rs`). No row of this batch edits
  those two files.
- **A local trim-remainder lane:** `theseus confirm …` folds into `theseus answer …`; the `executions` command folds
  into `sessions`; **`task.get` (`method::TASK_GET`) is deleted**, with other dead code. So:
  - add no new caller of `task.get`; read one task through `task.list` (see contract C2);
  - print `theseus answer ID …`, never `theseus confirm …`, in anything new;
  - use the wire names as they are on `main` (`execution.*`, `ExecutionView`): the lane renames CLI words, not wire
    methods. If you find otherwise, say so in the report.

Line ceilings at `40aa83cd` (`scripts/long-files.txt`, length / ceiling):
- `crates/theseus-protocol/src/lib.rs` **2,729 / 2,733**
- `crates/theseus-discord/src/runtime.rs` **3,497 / 3,500**
- `crates/theseus-kernel/src/kernel.rs` 3,026 / 3,030
- `crates/theseus/src/render.rs` 3,110 / 3,128
- `crates/theseus-core/src/turn.rs` 3,354 / 3,525
- `crates/theseus-core/src/tests_m3.rs` 7,962 / 8,050
- `crates/theseus-discord/src/render.rs` 2,926 / 3,009
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Never raise a ceiling. Put new code in new or small files: new wire types go in a protocol module (`tasks.rs`,
`work.rs` or a new one), not in `lib.rs`; Discord code goes in a `runtime/<name>.rs` submodule, not `runtime.rs`. A new
file over 2,500 lines is a shape failure.

## Where this batch comes from

The v1 feature review of 2026-10-17 (theseus-kkb04) walked task monitoring end to end on a scratch daemon with a fake
Discord, a scripted model and a fake Jev. Monitoring works on Discord, the chat, herdr and the CLI. What fails is
**finding a task later, starting one from where you are, and reading its progress in the same words everywhere**. It
ranked nine last features. The owner approved the list as ranked (Discord DM, 2026-10-17 23:34:35 MST: "(5) lgtm"),
with one change: **links stay on localhost.** No Tailscale or LAN serving, no change to `[web] bind`. A cockpit link
uses the configured `[web]` port on localhost (today the default link ignores it); it is fine that it opens only on the
host or through the owner's ssh tunnel. Where a Discord link's right target is the task's own forum post or channel,
it links that instead.

The nine items, and the row that owns each:

| # | Item | Row |
|---|---|---|
| 1 | Find any task later: an ended-task list with search, `task show`, a digest that names each task; Discord's `/tasks` done tasks from the store; the chat picker's search | task-history (core + CLI + digest); discord-tasks (Discord half); chat-start-pause (chat picker) |
| 2 | A cockpit Tasks page with a New task box; RUNNING 0 while a task runs fixed | cockpit-tasks |
| 3 | One progress line on every surface: step, $, time left, paused | progress-words (Rust surfaces); cockpit-tasks (the TS mirror) |
| 4 | Start a task from the chat; the offer chip that still shows when Jev is slow | chat-start-pause (chat); discord-tasks (chip) |
| 5 | Pause and resume from every surface | each surface's row (contract C3) |
| 6 | herdr panes that answer every question, choices included | pane-answers |
| 7 | Links: a task's Discord link opens its post or channel; cockpit links use the configured port on localhost | discord-tasks |
| 8 | One cross-surface journey test (theseus-h9tu1) | acceptance-journey |
| 9 | A task's model routed from its brief instead of inherited | task-model-by-brief |

## The eight rows (built in parallel today from main) and the join order

| Join | Slug | Beads | Owns (files) |
|---|---|---|---|
| 1 | task-history | theseus-ypnxt.1 | `task.list`'s filter params (`crates/theseus-protocol/src/tasks.rs`, core's `task.list` handler); `crates/theseus-protocol/src/work_digest.rs`, `crates/theseus-core/src/rpc/digest.rs`; the CLI's `tasks_tree.rs` args, `task_start.rs`'s `TaskCmd` (show, pause, resume), a new `render/task_history.rs` |
| 2 | progress-words | theseus-ypnxt.2 | the one progress line (a new `crates/theseus-protocol/src/work/progress_line.rs`); the CLI's `render/task_tree.rs`; the chat's `footer.rs` and `task_view.rs` header; the TUI's task rows (`board.rs`, `work.rs`); herdr's `herdr_up.rs` and `herdr_up/`; Discord's `render/board_v2.rs`, `runtime/forum.rs` (tags), `runtime/live_board.rs` |
| 3 | pane-answers | theseus-ypnxt.3 | the CLI's `interactive.rs`; `herdr_setup.rs`, `herdr_keys.rs`, `contrib/herdr/` |
| 4 | chat-start-pause | theseus-ypnxt.4 | the chat's `commands.rs`, `keys.rs`, `task_picker.rs`, `tasks.rs`; the TUI's key dispatch in `app.rs` and `new_task.rs` |
| 5 | discord-tasks | theseus-ypnxt.5 | Discord's `runtime/tasks.rs`, `runtime/start_task.rs`, `courier.rs`'s report post, `render.rs`'s footer and report link, `runtime/voice/tasks.rs`; core's `config/cockpit.rs`, `judge/inbound.rs` (`take_offer`), `turn/reply_step.rs` |
| 6 | cockpit-tasks | theseus-ypnxt.6 | `cockpit/src/` only (a new Tasks view, `Shell.tsx`, the nav, `DesktopNotices.tsx`) |
| 7 | task-model-by-brief | theseus-ypnxt.7 | core's `rpc/task_start.rs` (`target_of`) and the route call it makes |
| 8 | acceptance-journey | theseus-ypnxt.8 (and theseus-h9tu1) | `crates/theseusd/tests/acceptance_journey.rs`, `scripts/acceptance/`, `theseus-sim`'s `discord react` |

**Stay inside your row's files.** Where you must touch another row's file, keep the edit to the lines you need and
name it in the report under "Overlaps". The joiner merges in the order above, then review-fixes and the trim-remainder
lane resolve against whatever has joined.

## Contracts between the rows

- **C1. The progress line** (progress-words writes it in Rust; cockpit-tasks writes the same words in TypeScript). From
  a `WorkView`, its parts in this order, joined by ` · `, each left out when its data is absent:
  1. `paused` when `paused` is true (first, so it is never cut);
  2. `step N of M` from `progress` (`Progress { steps_done, steps_total }`, `work.rs:307`): `M` = `steps_total`,
     `N` = `min(steps_done + 1, steps_total)`; left out when `steps_total` is 0. `Progress::line()` (`work.rs:325`,
     "the one wording every surface shows", today `3/5 · <said>`) keeps the model's words and takes the same
     `step N of M` count, so there is one wording, not two;
  3. `$S of $L` from `cost` (`spent_usd` of `limit_usd`), in `theseus_protocol::push::usd`'s style (`$0.40 of $2`);
  4. from `forecast` (`WorkForecast { min_p50, min_p90, past, .. }`, `forecast.rs:20`) and the time since it
     started: `~T left` with `T` = `min_p50` minus the minutes elapsed while that is positive, else `min_p90`
     minus them, rounded as the CLI's `coarse` rounds
     (`render/task_tree.rs:305`; move it to protocol if you need it there): `~6 min left`, `~2 h left`; or
     `past its usual time` when `past` is true or the elapsed time passes `min_p90`; left out when `n` is 0.
  The line is a pure function of the view and `now_ms`. Example: `paused · step 3 of 5 · $0.40 of $2 · ~6 min
  left`. A golden of 8 cases lives at
  `crates/theseus-protocol/src/work/golden/progress_line.json` (input view fields and the expected line), written by
  progress-words. cockpit-tasks writes the same 8 cases into its own test from this list (the cases are in its task);
  the joiner then points the cockpit's test at the golden file.
- **C2. Reading tasks.** task-history adds optional params to `task.list` (`ended`, `since_ms`, `query` over title,
  brief and the report's first line, `id`, `limit`), all absent by default, so today's call returns today's result
  byte for byte. Every other row calls `task.list` as it is on `main` and filters on its side; the joiner may switch a
  caller to the params after task-history joins. Nobody calls `task.get`.
- **C3. Pause and resume.** Core's `task.pause` / `task.resume` exist (`crates/theseus-core/src/rpc/task_pause.rs`;
  today only Discord's ⏸ ▶️ reactions call them, `runtime/task_react.rs:284`). Each row adds its own surface's caller
  and nothing in core: the CLI `theseus task pause|resume ID` (task-history), the chat and TUI keys and `/pause`
  `/resume` (chat-start-pause), herdr's key (pane-answers), voice "pause it" / "go on" (discord-tasks), a Pause button
  on the cockpit's task row and page (cockpit-tasks). Showing `paused` is C1's.
- **C4. Forum tags.** progress-words adds `paused` and `stopped` tags (`runtime/forum.rs` `Tag::of`, today `:90-97`:
  paused maps to running and cancelled to failed). Cancelled and expired tasks get `stopped`, failed ones `failed`.
- **C5. Starting a task.** Every new start path calls `task.start` (no model turn), as `theseus task start` does
  (`crates/theseus/src/task_start.rs:112`), and prints or shows the task's short id and title.
- **C6. Links** (discord-tasks): the default cockpit address follows `[web] port` on localhost; nothing serves beyond
  loopback.
- **C7. Words.** One verb for an answer: `answer`. A task is a "task" everywhere (not a job or an execution) in
  anything a person reads.
- **C8. The journey test** (acceptance-journey) builds on `main`. Steps that need another row's feature are written
  against that row's contract above and marked `#[ignore = "lands with <slug> (theseus-ypnxt.N)"]`; the joiner removes
  each ignore at that row's join.

## Out of scope for every row of this batch

- Serving the cockpit beyond localhost: no `[web] bind` change, no Tailscale, no LAN (the owner's decision above).
- What a war-room guest keeps after close (owner decision D2, open).
- Nested tasks; the herdr board redrawn in place; voice "how's X going?" without a model call; speculation; remote
  runtimes; switching self-improvement on.
- Any config-key rename the trim rows did not make; route.v6's prompt wording and any Jev pack's text; the sealed
  holdout set; `PLAN_NOTE`; the LTO profile.
- review-fixes' and the trim-remainder lane's work (above).

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
