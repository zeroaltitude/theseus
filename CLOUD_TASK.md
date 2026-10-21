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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-rough-edges`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: rough-edges, progress words that tell the truth, a TUI board that says where a task goes and draws clean under tmux, a herdr pane that asks once, and a forum post for every task (theseus-ztkhg, theseus-ikw4p, theseus-dowi0, theseus-hzdlf)

Branch: `cloud/20261021-rough-edges`. Every commit's subject carries the id of the piece it fixes (`theseus-ztkhg`,
`theseus-ikw4p`, `theseus-dowi0` or `theseus-hzdlf`). Deadline for the report: 6 hours after you start.

**Why.** A dry run of the release walkthroughs (2026-10-19: `theseus chat`, `theseus tui` and herdr on a 180x50 tmux
terminal, against a live 1.0.0 daemon) found these, each seen on screen:
- **Progress words that mislead (theseus-ztkhg).** `theseus tasks` showed `$0.04 of ~$0.02 (limit $1) · ~<1 min left`
  on a task that ran 6 minutes: the median of similar tasks reads like a cap the task blew through, and "left" kept
  counting after the task was past its usual. The chat footer said `$0.0029 of ~$0.01 (limit $25)`. A row said
  `model 1:48` while a 25 s `proc.run` ran (the time was the whole turn's, and the tool was not named). A row's words
  stayed on `Round 1 done. Continuing.` for 2 minutes while the task did other things. A single task's row ended
  `(0 need you, 1 working, 0 done, 0 failed)`.
- **TUI board rough edges (theseus-ikw4p).** At start and after a `/` filter the cursor is on no row, and `N` then
  starts the task under Home (the owner's DM) while its prompt `new task: the brief (enter starts it)>` does not say
  where. Under tmux 180x50 the open-session header read `n  session open: en er  p ns the on  under the cursor`
  (meant: `no session open: enter opens the one under the cursor`), a sidebar row read
  `This:isoapscratchlconversationHforoanoinstallstest…` (two strings drawn over each other), and the chat's digest
  line began `◆ ◆`. About twenty one-turn install-check conversations ("Reply with the single word: ok") crowd the
  board. Failed tasks the owner dismissed show `◆ done` in the tree.
- **herdr pane rough edges (theseus-dowi0).** A task pane prints `[answer] [history] [open]` and
  `approve? [y/N/t/note]` twice for one question. `docs/herdr.md` says `herdr plugin log theseus`, which herdr 0.9.1
  answers with `unknown option: theseus` (`herdr plugin log` with no id works, and shows each action's command, exit
  code, stdout and stderr). herdr's `prefix+n` runs `theseus task start` with no `--in`, so the task goes under Home:
  fine, but undocumented. `prefix+shift+a` jumps to the longest-waiting question, which on a running install is often
  a self-improvement question (a self `fs.edit`): also undocumented.
- **A fast task can miss its forum post (theseus-hzdlf).** By reading (a review, not reproduced yet):
  `crates/theseus-discord/src/runtime/threads.rs` `next()` returns `Next::Nothing` for `(None, true)` (no place made,
  task already ended), and `crates/theseus-discord/src/runtime/forum.rs` `one()` returns early on
  `if ended || homed { return None; }` for a task with no slot. The sync reads the latest cached view, so a task that
  finishes in seconds while its place's actor is slow under load can end with no post. The owner's rule is
  "threads === tasks": every task has its post.

**Risk: low to medium.** Wording on every surface (CLI, TUI, Discord board, cockpit) changes in two places
(over-the-median spend and time), so goldens move; the TUI's drawing of outside text changes; one Discord sync path
gains a post for an ended task.

**Prerequisite: `main`.**

## Your row
Work in this order; if time runs short, finish and test the earlier pieces and list the rest as follow-ups.

| Piece | Beads | Files (as on `main`) |
|---|---|---|
| A | theseus-ztkhg | `crates/theseus-protocol/src/work/progress_line.rs`, `crates/theseus-protocol/src/forecast.rs` (`spent_of`), `crates/theseus-protocol/src/work/golden/progress_line.json`, `cockpit/src/lib/progressLine.ts` and `cockpit/test/progressLine.test.ts`, `crates/theseus-core/src/push/work/views.rs` (`doing_of`), `crates/theseus/src/work.rs` (`view_line`, `system_line`), `crates/theseus/src/render/task_tree.rs`, `crates/theseus-tui/src/work.rs` (`form_of`, `doing`) |
| B | theseus-hzdlf | `crates/theseus-discord/src/runtime/threads.rs` (`next`), `crates/theseus-discord/src/runtime/forum.rs` (`one`), a test file of its own |
| C | theseus-ikw4p | `crates/theseus-tui/src/new_task.rs`, `crates/theseus-tui/src/ui.rs`, `crates/theseus-tui/src/board.rs`, `crates/theseus-tui/src/app.rs` (the filter's cursor), `crates/theseus-tui/src/chat/welcome.rs` (one line) |
| D | theseus-dowi0 (items 1, 3, 4, 5) | `crates/theseus/src/interactive.rs`, `docs/herdr.md` |

### A. Progress words (theseus-ztkhg)
The one progress line is `theseus_protocol::work::progress_line` (a pure function of the view and the clock), and the
cockpit writes the same words in TypeScript from `golden/progress_line.json`. Change the words there once; every
surface follows.
1. **Spend past its usual.** `cost_part` / `forecast_cost` / `WorkForecast::spent_of` say `$0.40 of ~$0.60 (limit $2)`.
   Keep that exactly while the spend is at or under the median (the owner, 2026-10-18: keep `[$x of ~$p50]`). Past the
   median the words are `$0.04, past its usual ~$0.02 (limit $1)`. The `$x of ~$p50` form must never show a spend
   above its `~` figure.
2. **Time past the median.** `time_left` counts `~6 min left` to the median, then today counts to the 90th
   percentile, which is how a 6-minute task read `~<1 min left`. From now: the countdown runs to the median only;
   past the median, or once the spend is past its median, the time part says `past its usual time` (the words
   already used past the p90). Update the golden case "past the median, time left from the p90" to the new words
   (rename it), and add cases: spend past its median before the time is; time past its median with spend under it.
   The TypeScript `progressLine.ts` and its test follow the golden; `cockpit` tests run with
   `cd cockpit && npm ci && npm test` (if the VM cannot install the packages, say so and give the command).
3. **`model 1:48` while a tool runs.** `doing_of` in `crates/theseus-core/src/push/work/views.rs` returns
   `DoingKind::Job` for an open job, `DoingKind::Tool` for another open action, else the view's own `doing`, whose
   `Thinking` time starts at the turn's start (`ei.turn_started_ms`). A 25 s foreground `proc.run` showed as
   `model 1:48`. First write the test (below) that runs a foreground `proc_run` of a few seconds in a task and reads
   the task's view while it runs; then find why the action is not seen. Suspects to check in order: the filter
   `a.a.state != ActionState::Planned || a.a.confirm.is_some()` drops a dispatched action still marked `Planned`; the
   action is not in `acts_by_exec` until it settles; the TUI/CLI form maps it wrong. Then make `Thinking`'s time start
   at the latest of the turn's start and the end of the turn's last settled action (the model has been thinking since
   the last result came back, not since the turn began), from data the view already holds; no new stored field.
4. **Words older than what the task does.** `Progress.said` (the model's last words) has `Progress.at_ms`. Where a
   surface shows `said` (`crates/theseus/src/work.rs` `view_line`, `Progress::line` in
   `crates/theseus-protocol/src/work.rs`, the TUI board row), words older than the task's current `doing.since_ms`
   and older than 60 s carry their age: `Round 1 done. Continuing. (2 min ago)`. One helper in the protocol
   (beside `progress_line`), called by each surface.
5. **The rollup.** `(N need you, N working, N done, N failed)` in `view_line` and `system_line` shows only its
   non-zero parts: `(1 working)`, `(2 done, 1 failed)`; nothing when all are zero.

### B. A forum post for every task (theseus-hzdlf)
- Decision (the owner's rule, threads === tasks): a task that ended before any sync saw it running is owed its post.
  At the first sync that sees it, make the post in its final state (the ended mark in its name as `task_name`/`post_name`
  make it, the done/failed/stopped tag), with the report's first line where a running post would have its card. Only
  for a task that **ended since this binding started**: an upgrade must never back-fill posts for old tasks.
- Find which path loses it before changing both: write the test first, holding the place's sync until the task has
  ended (the stand-in REST can be slowed: `FakeDiscord::set_delay_ms` in `crates/theseus-sim/src/fake_discord.rs`; a
  400 ms delay on every answer did not lose a post in the review, so hold the sync itself rather than only slowing
  REST; the task-thread tests show how a test holds a task, `gate()` in `tests_task_threads.rs`). If the homed path makes the post on request at task start and the test cannot lose it, keep
  the fix to the path that can (`forum.rs` `one()`'s index-card path for a shared forum, `threads.rs` `next()`'s
  `(None, true)` for a channel's thread), and say which in the report.
- Exactly one post per task, ever: an ended task that already has a post (`adopt`, the `made` list) gets none.
- `crates/theseus-discord/src/runtime.rs` has **one line of room** under its ceiling (3,499 / 3,500): do not add lines
  there. New code in `runtime/forum.rs`, `runtime/threads.rs` or a new module under `runtime/`.

### C. The TUI board (theseus-ikw4p)
1. **Where `N` puts a task.** The prompt names the conversation the task goes under:
   `new task under Home: the brief (enter starts it)> ` or `new task under <title>: …` (`new_task_prompt` in
   `new_task.rs` knows `home`; the prompt is drawn in `ui.rs`, near line 708). After a `/` filter, the cursor moves to
   the first row that matches (`app.rs`), so `N` right after a filter goes under the conversation shown first. The
   default limit stays the daemon's (a quarter of the parent's: `crates/theseus-core/src/task.rs`); the start's flash
   already names it.
2. **Clean drawing under tmux.** Reproduce first: draw the board and an open-session header through ratatui's
   crossterm backend into the `vt100` emulator (already a dev-dependency) at 180x50 and compare each row read back with
   what was meant; a sidebar title carrying an emoji with a variation selector (task names begin `⚙️ `, U+2699 U+FE0F,
   `crates/theseus-discord/src/runtime/threads.rs` `task_name`) and `◆` are the first suspects: when the TUI's width of
   a glyph differs from the terminal's, every cell after it on the row lands one off, and ratatui's diff then leaves
   stale cells (the overdraw seen). If `tmux` is installed on the VM, also check a real run
   (`tmux -L scratch new-session -d -x 180 -y 50 …` and `tmux -L scratch capture-pane -p`); it is optional. Fix where
   the TUI draws outside text (titles, names, labels): make the width the TUI counts match what terminals draw (for
   example, drop U+FE0F and draw such glyphs as their one-cell form), in one function every draw of outside text calls.
3. **One-turn conversations fold.** Top-level conversations that are idle, have one turn or none, no tasks, no
   question, and were last active over an hour ago fold into one row at the foot of the sidebar,
   `▸ N one-turn conversations`, which the existing fold key opens (`board.rs` already folds idle children: reuse its
   rule and key). Nothing that needs the owner ever folds.
4. **A dismissed failure is not done.** A failed task the owner dismissed shows `✗` dimmed with `dismissed`, never
   `◆ done` (`board.rs` rows, the `done` closure from `App::is_done`, `ui.rs`'s `DONE` glyph).
5. **The chat's digest line.** `chat/welcome.rs` (near line 49) formats `◆ {line} · theseus digest`, and the line
   already starts with `◆`: draw one. This is the only edit in `crates/theseus-tui/src/chat/`, which another session is
   working on: keep it to that line.

### D. The herdr pane, items 1, 3, 4, 5 (theseus-dowi0)
1. **One question, one block.** `crates/theseus/src/interactive.rs` prints a question's card, its links line
   (`crate::herdr_keys::links_under`, called near line 733) and `approve? [y/N/t/note]`. In a herdr pane it printed
   both twice for one question. Find the second print (two events for one question, or a redraw), and print a
   question's block once per question id. The test rig in `crates/theseus/tests/herdr.rs` already counts the prompt
   (`wait_prompt`): add a test that one question gives one prompt line and one links line.
2. **`docs/herdr.md`:** the troubleshooting line says `herdr plugin log` (no id; on herdr 0.9.1 an id is
   `unknown option`), and what it shows; the keys table says `prefix+n` starts the task under Home (the DM) and that
   `--in` is not passed; `prefix+shift+a` says it goes to the longest-waiting question, which can be a
   self-improvement question.
- **Not here:** Tab in the answer card and the order it walks (item 2: `crates/theseus/src/herdr_answer.rs`), and the
  keys (`crates/theseus/src/herdr_keys.rs`, `contrib/herdr/`): another session is fixing herdr's keys in those files
  now. Do not edit them; item 2 is a follow-up filed for after that work joins.

## Tests (each with a planted revert; the first is the key test)
- **`a_spend_past_its_usual_never_reads_as_under_it`** (key test, `progress_line.rs` tests, through the golden): a
  view with spend $0.04 and a forecast median of $0.02 reads `$0.04, past its usual ~$0.02 (limit $1)` and its time
  part `past its usual time`; at $0.01 it reads `$0.01 of ~$0.02 (limit $1)`. Plant: restore `spent_of` for every
  spend; the test and the golden must fail. The cockpit's `progressLine.test.ts` reads the same golden.
- `a_running_tool_is_named_with_its_own_time` (core, `crates/theseus-core/src/push/work/`): a task whose step is a
  foreground `proc_run` of 3 s reads, while it runs, `DoingKind::Tool` (or `Job` if the run became a job) with the
  tool's label, never `Thinking`; after it, `Thinking` counts from the tool's end. Plant: return the view's own `doing`
  first.
- `old_words_carry_their_age` (protocol): words at 10:00 under a doing since 10:01, read at 10:02, end `(2 min ago)`;
  words newer than the doing show bare. Plant: drop the age.
- `a_rollup_shows_its_non_zero_parts` (CLI `work.rs` tests): `(1 working)`; all zero shows nothing. Plant: the old
  format.
- `a_task_that_ended_before_its_first_sync_gets_one_post` (Discord, a new test file declared beside the others): the
  post exists, tagged done, named with the done mark, and there is exactly one. Plant: restore the early return
  (`(None, true) => Next::Nothing` or `if ended || homed`, whichever path the test exercises). Also
  `an_old_ended_task_gets_no_post_at_start`: a task that ended before the binding started gets none.
- `n_says_where_the_task_goes` and `a_filter_puts_the_cursor_on_its_first_row` (TUI): the prompt names Home with no row,
  and the conversation's title with one. Plant: the old prompt.
- `outside_text_draws_cell_for_cell` (TUI, through `vt100`): a board whose session title holds `⚙️` and the
  no-session header read back exactly as meant at 180 columns. Plant: remove the normalisation.
- `one_turn_conversations_fold_and_a_question_never_does` (TUI) and `a_dismissed_failure_never_reads_done` (TUI), each
  with a plant.
- `one_question_one_prompt_in_a_pane` (`crates/theseus/tests/herdr.rs`). Plant: the second print restored.
- Every golden and test that changes because of the new words changes in the same commit as the words, and the
  report lists them.

## Not in this row
- Item 2 of theseus-dowi0 (Tab in the answer card, its order) and herdr's keys: the herdr session's files
  (`herdr_answer.rs`, `herdr_keys.rs`, `contrib/herdr/`).
- The TUI chat beyond the one digest line (`crates/theseus-tui/src/chat/`: another session's), task questions and
  reports (`owner_ask`, `turn/join_step.rs`, `task.close`), job command lines (`theseus-kernel/src/job.rs`), the Google
  rows (`crates/theseus-google`), the threads rows (`thread_route.rs`, `thread_recall.rs`), the Discord task-thread
  test rig (`tests_task_threads.rs`'s `gate`: the test-infra row's).
- The default task limit (a quarter of the parent's): a design choice for the owner; say in the report if you think it
  should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code).
  Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. These are wording and drawing fixes: no
  switch; the rollback is the revert of the commit, named per piece in the report.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). FAST: nothing new on a plain turn (a
  plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not
  move), nothing on the path before a request leaves, and nothing on the start path grows with history. The work
  view's `doing_of` runs on every board change: keep it O(the task's open actions).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A golden updated to new
  words is not a weakened test when the words are this task's; say which goldens moved and why. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless it names only goldens this task changes on purpose; explain each.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-tui/AGENTS.md`,
  `crates/theseus/AGENTS.md`, `crates/theseus-discord/AGENTS.md`, `crates/theseus-protocol`, `cockpit/`); where they
  describe the words you change (the progress line's examples), update them in the same commit. Keep CLOUD_REPORT.md
  honest: what you ran, what passed, what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup:
  about 33 L1 tests fail on this VM; none of this row's tests should need L1).

## Your report
Commit `CLOUD_REPORT.md` at the repo root (written with your editor tools, never a shell heredoc; check its size after
the commit, it must not be empty), push the branch, and end with a final message that starts `CLOUD REPORT COMPLETE`.
The report holds:
1. Any file left over its line ceiling, with the reason (first section; "none" if none).
2. What changed, per piece (A-D) and item, with before/after words for every wording change.
3. Each test and its plant, and the test that failed on it; the goldens that moved.
4. What hzdlf's test showed (which path lost the post) and what `model 1:48`'s cause was.
5. What could not run on the VM (tmux, the cockpit's npm tests), with the exact commands for the maintainer.
6. The keel guard's output.
7. Follow-ups: every item not done, with what a person still hits.
