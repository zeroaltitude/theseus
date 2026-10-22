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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail5-discord`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `f1fc9b03` (2026-10-22: release stage 3, which landed `integ/tail3` and the threads
lane's round 4 on top of stage 2's `8260dbab`); that is your base. Line numbers quoted below are at `f1fc9b03` but may
still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `8260dbab`** (`git log --first-parent 8260dbab..HEAD` lists them):
- `814302a0` + `7cebad5b`: `integ/tail3` (Google fixes, the forum archive and the task home's DM board, test flakes).
- `066b1401` + `f1fc9b03`: the threads lane's round 4 (a held compile failure, quick-mode steers, thread moves and
  growth, the placed chat's filters).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail4` (`origin/integ/tail4`, `795afd3b` or later) and `integ/cn` round 5 (not pushed yet; its rows are the
  branches `origin/cloud/20261021-tail3-cn`, `-cn-config-guidance`, `-tail4-cn-draft` and `-tail4-cn-reply`) land as
  the next release stage. Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read
  their diff (`git diff origin/main origin/integ/tail4 -- <file>`, or `git diff origin/main...origin/cloud/20261021-<row> -- <file>`).
  Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail5-discord, rig tests for the archive tick and the DM board's removal, and an earlier run's DM board cleaned (theseus-bs8dl)

Branch: `cloud/20261021-tail5-discord`. Every commit's subject carries `theseus-bs8dl`. Deadline for the report: 3 hours
after you start.

**Why.** The tail3-discord row (joined at release stage 3 `814302a0`, now on `main`) added an hourly tick that archives
read forum posts, and made the task home's first standing remove the board the owner's DM drew before the home answered.
Its review found both held by no rig-level test, and its report left one gap:
1. Deleting `self.archive_tick();` in `runtime/task_channels.rs` (`sweep_task_channels`, near :428) fails no test: the
   tests drive `archive.rs`'s `every`/`due` helpers, not the spawn.
2. Removing the `LaneMsg::BoardMoved` send in `runtime/task_home.rs` (`ensure_task_home`, near :325-328) fails no test:
   `courier/tests_v2.rs` near :317 drives the lane directly. The author noted the daemon's `Shared` would need exposing to
   the task-home rig.
3. A DM board **pinned by an earlier run** of the daemon, with the home already recorded, is never cleaned: the lane
   removes only a board it made itself (its in-memory id), so after a restart the old pinned board stays in the DM.

**Risk: low.** Tests, plus one cleanup of a stale board.

**The owner locked the Discord DM task flow** (2026-10-22: "do not change how it is now, all of the little touches are so
good"). Item 3 only removes a stale board, which the flow already means to do ("a board the DM drew before the home
answered is stale"); no message's words, layout, order or emoji change. If the fix would change anything the owner sees
beyond that board going away, stop, do items 1-2 only, and describe the change in the report instead.

**Prerequisite: base `origin/main` at `f1fc9b03`.** Your branch joins the new lane `integ/tail5`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail5-discord` | theseus-bs8dl | a rig test for the archive tick's spawn; a rig test for the home's `BoardMoved`; an earlier run's pinned DM board removed when the home stands | `crates/theseus-discord/src/runtime/task_channels.rs` / `runtime/forum/archive.rs` (a test hook only if needed), `crates/theseus-discord/src/runtime/task_home.rs`, `crates/theseus-discord/src/courier.rs` / `courier/board.rs` (`board_moved`), `crates/theseus-discord/src/tests_task_home.rs` (or a new test file with one `mod` line), `crates/theseus-discord/src/tests_forum_archive.rs`, Discord `AGENTS.md` (one sentence) | S-M |

## What exists (at `f1fc9b03`; search by name)
- **The tick.** `crates/theseus-discord/src/runtime/forum/archive.rs` (module doc: "It runs on its own hourly tick
  (`ARCHIVE_TICK`, `Shared::archive_tick`)"; near :94 `pub(crate) fn archive_tick(self: &Arc<Self>)`; the test near :216
  `a_read_post_leaves_within_the_hour_after_its_day` tests the helpers). `runtime/task_channels.rs` near :427
  `sweep_task_channels` calls it, and is called from `revive_task_threads` (`runtime/threads.rs` near :590).
  `crates/theseus-discord/src/tests_forum_archive.rs` is the rig-level suite.
- **The home.** `runtime/task_home.rs` (621 lines) near :307 `ensure_task_home`: on the home's first standing
  (`was != Some(channel)`) it sends `LaneMsg::BoardMoved` to the owner's DM lane. `courier.rs` near :97 (the variant),
  :338 (the lane's board id to remove), :483 (`LaneMsg::BoardMoved => self.board_moved()`). `courier/board.rs` draws the
  board. `crates/theseus-discord/src/tests_task_home.rs` (475 lines): `rig`, `home_record`, `home_up`, `boards(r, channel)`,
  `cards`, `the_task_home_is_made_once_and_found_after_a_restart` (near :214; the restart rig to reuse for item 3).
- **The fake.** `crates/theseus-sim/src/fake_discord.rs` and `fake_discord/*.rs` (pins: `git grep -n pin crates/theseus-sim/src`).

## What to build
1. **The tick's rig test.** A test that fails when `self.archive_tick()` is removed from `sweep_task_channels`: for
   example a counter of tick spawns (or runs) on `Shared`, read by a test in `tests_forum_archive.rs`, or the rig's own
   observable (a read post archived without calling the helper). Prefer an observable over a hook; a hook is `#[cfg(test)]`
   or a plain counter health already shows. The author's note: with a paused clock and real sockets the rig jumps to the
   daily timer, so assert the spawn, not the hour.
2. **The home's rig test.** A test through the task-home rig: the DM draws a board before the home answers; the home
   stands; the DM's board is gone. Expose what the rig needs from the daemon (`Shared` or a narrow accessor) under
   `#[cfg(test)]` or `pub(crate)`, not a new public API.
3. **An earlier run's board.** When the home stands and the DM lane has no board of its own, find the bot's own pinned
   board message in the DM (the board's own marker: read `courier/board.rs`/`render/board.rs` for how a board is
   recognized; never remove a message that is not the bot's board) and unpin and delete it, once. Test: start, draw a DM
   board, stop; restart with the home recorded: the old board is removed, nothing else in the DM changes (assert the DM's
   other messages are byte-identical). Plant: skip the lookup.

## Tests (each with a planted revert; the first is the key test)
- **`an_earlier_runs_dm_board_goes_when_the_home_stands`** (item 3). Plant: skip the pinned lookup.
- `the_home_standing_removes_the_dms_board` (item 2). Plant: drop the `BoardMoved` send.
- The archive tick's test (item 1). Plant: drop `self.archive_tick();`.
- `cargo nextest run -p theseus-discord task_home`, `-p theseus-discord archive`, `-p theseus-discord board`, then the
  whole crate. The Discord task-flow goldens pass unchanged (`git grep -ln golden crates/theseus-discord/src`).

## Not in this row
- The approval card (`render/approval.rs`), forum creation (`runtime/default_forum.rs`, `runtime/forum.rs`): `integ/tail4`'s.
- The archive tick and the daily sweep's benign race (review finding 3) and the per-call spawn (finding 4): no change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main:** `integ/tail4` edits Discord `runtime.rs` (3,499 / 3,500 on your base: add nothing
  there), `render.rs`, `render/approval.rs`, `runtime/voice.rs`, `tests_default_forum.rs`, `tests_gateway.rs`,
  `tests_outbox.rs`, Discord `AGENTS.md`, and `crates/theseus-sim/src/fake_discord.rs` + `fake_discord/forums.rs` +
  `fake_discord/tests/reread.rs` (`git diff origin/main origin/integ/tail4 --stat`). Stay out of all of them except
  Discord `AGENTS.md` (your one sentence in the task-home section). If the fake lacks pins, add them in a new
  `fake_discord/pins.rs` with one `mod` line, not in `fake_discord.rs`'s body.
- **Rows beside you on `main` f1fc9b03, joining `integ/tail5` with you:** tail5-effort (core routing and config) and
  tail5-threads (core threads, TUI `chat/`). Neither touches Discord.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-discord/AGENTS.md` (the task home, the board, the forum archive) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per item; how an earlier run's board is recognized (and why nothing else can match);
what the owner sees change (only the stale board going away); each test and its plant with the failing line; the keel
guard's output; the store format (unchanged); a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
