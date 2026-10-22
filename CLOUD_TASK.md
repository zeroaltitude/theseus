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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail3-discord`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `29f5dcad` (2026-10-22: install #69, which landed the patch1 tail lane `integ/tail` and the walk1 fixes); that is your base. Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
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
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,731 / 2,733 (two lines of room)
- `crates/theseus/src/render.rs` 3,122 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections below. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail3-discord, a read forum post leaves within the hour, the DM's stale task board, and the voice merge's bounds (theseus-wy8pt, theseus-ysp2k, theseus-we57k)

Branch: `cloud/20261021-tail3-discord`. Every commit's subject carries the Beads id of what it changes
(`theseus-wy8pt`, `theseus-ysp2k`, `theseus-we57k`). Deadline for the report: 3 hours after you start.

**Why.** Three follow-ups from rows that landed on `main` in install #69.
- **theseus-wy8pt (forum archive cadence).** The owner (verbatim): "tasks in a terminal state should be removed from
  the forum when they are 1) read, and 2) older than 24 hours". The forum-archive row archives a read post once it
  ended ≥ 24 h ago, but runs on the task channels' sweep, which runs at start and then once a **day**: a read post
  leaves 24 to 48 h after its end, not at 24 h. The owner set the 24 h, not the daily sweep. Also from that row's
  review (LOW): `TaskPosts::seen` marks only rows whose `ended_ms` is already set, so if the owner presses Clear done
  before the forum worker writes the end, that read is lost and the post stays (fail-safe, but wrong).
- **theseus-ysp2k (tail-discord follow-up).** In `ensure_task_home`, a task board drawn in the owner's DM before the
  state holds the home channel is redrawn at the home (`wake_places` via `board_lane`), but the earlier DM message is
  never removed: a stale board stays pinned in the DM. Also from that review, small: a comment at
  `Tasks::snapshot`'s re-apply site pinning "the snapshot carries no ended child" (finding 2), and a module-doc
  sentence on the core-side watch-scope race the ticket does not order (finding 3).
- **theseus-we57k (voice-knobs review, three lows).** In `theseus-voice/src/engine.rs`: (1) `awaits_its_rest`'s
  `Vad::is_open(last.speaker)` arm has no time bound, so an open VAD long after the merge window holds `start_turn`
  until that utterance closes and is transcribed (`goes_on` then refuses the join: only latency); (2) `join()` keeps
  `last.over`/`last.judged` and drops `next.over`, which is `None` today only because no report plays while a merge
  holds; (3) the hold gates all of `start_turn`, so another speaker's transcribed utterance waits up to `merge_ms`.

**Risk: low.** A timer, one read rule, one message cleanup, and three bounds on an engine hold that only ever adds
latency.

**Prerequisite: base `origin/main` at `29f5dcad`.** Your branch joins the new lane `integ/tail3`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail3-discord` | theseus-wy8pt, theseus-ysp2k, theseus-we57k | hourly archive tick + cleared-before-end read; the stale DM board; two comments; the voice hold's bounds | `crates/theseus-discord/src/runtime/forum/archive.rs` (+ tests), `runtime/task_channels.rs` (the spawn only, if needed), `runtime/task_home.rs`, `runtime/tasks.rs` (one comment, plus what the stale-board fix needs), the Discord tests for these (`tests_forum_archive`, the task-home tests), `crates/theseus-discord/AGENTS.md`; `crates/theseus-voice/src/engine.rs`, `crates/theseus-voice/tests/turns.rs`, `crates/theseus-voice/AGENTS.md` | M |

## What exists (lines at `29f5dcad`; search by name, a line may have moved)
- **The archive.** `crates/theseus-discord/src/runtime/forum/archive.rs` (163 lines): module doc (read = Clear done or a
  dismissed failure; old = `ended_ms` ≥ `ARCHIVE_AFTER_MS` = 24 h; archived and locked once, `archived_ms`; "It runs on
  the task channels' sweep (`Shared::sweep_once`)"); `kept`, `due(p, now, read)`; `TaskPosts::seen(tasks, store)` :49
  sets `read_ms` only where `p.ended_ms.is_some()`; `Shared::archive_read_posts` :67 (loads the list, filters `due`,
  `update_thread(..).archived(true).locked(true)`, 404 = gone, marks `archived_ms`, saves). Pure tests near :117-160.
- **The sweep.** `runtime/task_channels.rs` :426 `sweep_task_channels` spawns a loop: `sweep_once()`, then
  `sleep(DAY)` or the outbox's `stopped()`. `sweep_once` :441 deletes old task channels, then `archive_read_posts()`.
- **Clear done.** `Tasks::clear_done` in `runtime/tasks.rs` returns the cleared task ids (search `clear_done`), which
  reach `TaskPosts::seen`. Whatever the board keeps as "cleared at" per place/session (search `cleared`) is what the
  review means by `cleared[session] >= ended_ms`.
- **The task home.** `runtime/task_home.rs` (612 lines): `ensure_task_home` :302 (find or make the home; on success
  `d.state.send_replace(Some(channel))`, `register_task_homes()`, `wake_places()` when it changed); `board_lane` :575
  (a place's board goes to the home's lane while the home stands, else its own lane). `runtime/default_forum.rs` :304
  `wake_places`. The DM board message the place drew before the home answered is the one left behind: find where a
  place's lane keeps its board message id (search `LaneMsg` and the lane's idle/clear path in `live_board.rs` /
  `task_home.rs`).
- **`Tasks::snapshot`** `runtime/tasks.rs` :199 (`snapshot(ticket, r: WorkWatchResult) -> HashSet<String>`), its
  re-apply site.
- **Line ceiling:** `crates/theseus-discord/src/runtime.rs` is 3,499 / 3,500. Do not add a line to it; put new code in
  the child modules above.
- **Voice.** `crates/theseus-voice/src/engine.rs` (1,841 lines): `join(last, next)` :157; `goes_on` :1731 (same speaker,
  `last` unfinished and not said over, `next.started <= last.closed + merge`); `awaits_its_rest` :1743 (the run at the
  head of `pending` ends in an unfinished utterance and the window has not passed, or the same speaker has another
  begun, or `self.vads.get(&last.speaker).is_some_and(Vad::is_open)`). Tests in `crates/theseus-voice/tests/turns.rs`
  (`an_unfinished_utterance_waits_for_its_rest` and its helpers `ms`, `turns`, `call_with`, `first`, `Lines::say`,
  `OWNER`, `ROBIN`).

## What to build
1. **theseus-wy8pt.**
   - The archive gets its own hourly tick (a constant `ARCHIVE_TICK` of one hour in `archive.rs`, spawned beside the
     sweep, stopping on the same `stopped()` future); `sweep_once` stops calling `archive_read_posts`, or keeps it and
     the tick only adds runs: pick one and say why. A read post now leaves at most an hour after its 24 h, never
     48 h. Update the module doc's "It runs on the task channels' sweep" sentence and the `sweep_task_channels` doc.
   - The read rule: a post also counts as read at sweep time when the board's clear for its place/session happened at
     or after the post's `ended_ms` (the review's `cleared[session] >= ended_ms`), so a Clear done pressed before the
     forum worker wrote the end is not lost. Prefer computing it at sweep time over storing more.
   - No config key. If one seems needed, do not add it: say so in the report.
2. **theseus-ysp2k.** When `ensure_task_home` first settles the home and the owner's DM place had already drawn a board
   there, that DM board message is removed (or edited to the one-line "tasks moved to <home>" form the code already
   uses for a DM whose tasks live at the home, if one exists: search before inventing words). Take whichever of the
   review's two options is smaller: the lane's idle/clear path edits the stale DM board, or the first draw is held
   until `ensure_task_home` first answers. Add the comment at `Tasks::snapshot`'s re-apply site (finding 2) and one
   module-doc sentence in `task_home.rs` on the core-side watch-scope race the ticket does not order (finding 3).
3. **theseus-we57k.**
   - (1) The open-VAD arm counts only when that VAD opened within `last.closed + merge` (its start, not "now"): a VAD
     that opened later is a new utterance, which `goes_on` would refuse anyway. Find how `Vad` exposes when it opened;
     add an accessor if needed.
   - (2) In `join`, `next.over` is not dropped silently: either carry it (`last.over = last.over.or(next.over)`) or
     `debug_assert!(next.over.is_none())` with a comment saying why it holds. Pick the one that cannot fold a barge-in
     away, and say which.
   - (3) The hold delays only the held speaker's turn when it can do so without reordering turns; if a correct bound
     needs more than a small change, leave the code, document the bound (`merge_ms` at most) in the engine's doc and
     `crates/theseus-voice/AGENTS.md`, and say so.

## Tests (each with a planted revert; the first is the key test)
- **`a_read_post_leaves_within_the_hour_after_its_day`** (Discord, fake Discord; tokio paused time if the rig allows):
  a post read and ended 24 h + 1 min ago is archived by the next tick, without a daily sweep running. Plant: tick
  daily again: it fails.
- `a_clear_before_the_end_still_counts_as_read`: Clear done at t, end written at t + 1 s, sweep after 24 h: archived.
  Plant: drop the cleared-at rule.
- `the_dm_board_drawn_before_the_home_answers_does_not_stay`: the DM draws a board, then `ensure_task_home` answers;
  the DM's board message is gone or replaced. Plant: skip the cleanup.
- Voice (in `tests/turns.rs`): `an_open_vad_after_the_window_does_not_hold_the_turn`: an unfinished utterance closes,
  the same speaker opens a new VAD after `merge` has passed: the first turn fires at its own close + merge, not at the
  new utterance's end. Plant: restore the unbounded arm. For (2), a test only if you carry `over`; for (3), a test only
  if you change the code.
- The existing `an_unfinished_utterance_waits_for_its_rest`, the forum-archive tests and the task-home tests pass
  unchanged. Do not edit their assertions; if one must change, stop and say why in the report's first section.
- `cargo nextest run -p theseus-discord` and `cargo nextest run -p theseus-voice` must pass.

## Not in this row
- The live check (`/thread link a: b:` in a private channel; a pair staged twice → DM card) and the spec Part III item
  for theseus-98bz.12 (ysp2k): the owner's and the maintainer's.
- An owner message in a post counting as read: Discord tells a bot nothing of views. Not this row.
- `docs/status.md` and `crates/theseus-core/AGENTS.md`: tail3-google owns them in this wave. Put a status sentence in
  your report for it.
- `crates/theseus-core`, `google/*`, the TUI.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Line ceilings** (`scripts/long-files.txt`): `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (add nothing);
  `engine.rs` 1,841, `tests/turns.rs` about 2,054, both under 2,500. Never raise a ceiling.
- **In-flight rows beside you on `main` 29f5dcad, joining `integ/tail3` with you:** tail3-tests (`theseus-core` test
  helpers, `rpc/mod.rs`, `config/cockpit.rs`, `crates/theseus/`) and tail3-google (`google/*`, learning, docs). Stay
  out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** on your base is 48. The forum list is a META value with serde-default
  fields; this row should add no stored field. If you must, a bump goes to 49 with the old layout's sample in
  `crates/theseus-core/src/tests_layouts.rs`.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-discord/AGENTS.md` and `crates/theseus-voice/AGENTS.md` first. Run the two crates' tests,
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; the tick's shape (and whether the daily sweep still archives); the read
rule; which stale-board option you took and why; each voice choice ((2) carry or assert, (3) code or doc); each test and
its plant with the failing line; the keel guard's output; the store format (unchanged); a status.md sentence for
tail3-google; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
