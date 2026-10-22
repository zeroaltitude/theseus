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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail5-threads`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail5-threads, the threads lane's last lows: a mid-turn move's growth, a held compile failure's rows (theseus-ddfe7, theseus-qiyxq)

Branch: `cloud/20261021-tail5-threads`. Every commit's subject carries the Beads id of what it changes (`theseus-ddfe7`,
`theseus-qiyxq`). Deadline for the report: 3 hours after you start.

**Why.** The reviews of two rows joined at release stage 3 (`066b1401`, now on `main`) left low findings the owner wants
in before the release closes:
- **theseus-ddfe7** (the tail3-threads review). (F1) A move before the origin turn's growth runs finds nothing in
  `take_grew`: growth runs after the turn ends, so when the owner says "that was for X" while the turn still runs, the
  growth then lands in the origin thread's working set and is never taken back. (F2) `thread_grow::added_by` attributes
  an entry to the message whenever the message named it, even when the turn's touched calls also added it, so a move
  then removes from the origin a file the turn really worked on, against the doc's "what the turn touched stays".
  (F3) `route.grew(...)` is pushed even when `added` is empty, so the `TURNS` ring holds no-op entries. (F4) The TUI's
  `TaskChanged` arm's `ours_in` filter (theseus-dos5f) has no test or plant.
- **theseus-qiyxq** (the tail2-route review). (F2) A compile failure in a private place whose verdict warns but does not
  hold now posts the DM warning and a `privacy.warned` row and still fails; `tests_hold_compile`'s private case should pin
  that row's `held: false`. (F3) A held compile failure records no `record_route`, `engage_verdict` or `note_routed` rows,
  so route stats undercount held turns. Plus the docs the row owed (below).

**Risk: low.** Bookkeeping and tests; no routing decision changes.

**Prerequisite: base `origin/main` at `f1fc9b03`.** Your branch joins the new lane `integ/tail5`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail5-threads` | theseus-ddfe7, theseus-qiyxq | moved turns grow nothing; `added_by` subtracts touched; no empty `grew`; the TaskChanged filter's test; the held compile failure's rows and the private case's assert; two AGENTS.md sentences | `crates/theseus-core/src/thread_flow.rs` (`take_grew`, `moved`, the state beside them), `crates/theseus-core/src/rpc/thread_grow.rs`, `crates/theseus-core/src/thread_grow.rs` (`added_by`), `crates/theseus-core/src/rpc/thread_move.rs` (only if the mark is set there), `crates/theseus-core/src/rpc/tests_thread_grow.rs` (or a new test file), `crates/theseus-core/src/turn/route_step.rs` (`fn held` near :673 only), `crates/theseus-core/src/tests_hold_compile.rs`, `crates/theseus-tui/src/chat/tests_thread_ask.rs`, `crates/theseus-core/AGENTS.md` (two sentences) | M |

## What exists (at `f1fc9b03`; search by name)
- **Growth.** `crates/theseus-core/src/rpc/thread_grow.rs` (200 lines): after a turn ends it grows the thread's
  working set, and near :120 calls `self.runner.threads.route.grew(turn_id, &t.slug, added)` whatever `added` holds;
  near :135 `take_grew(turn)` gives a move the growth to take back. `crates/theseus-core/src/thread_flow.rs` near :145
  `pub fn take_grew(&self, turn: &str) -> Option<(String, WorkingSet)>` and the `TURNS` ring. `crates/theseus-core/src/thread_grow.rs`
  (290 lines) near :98 `pub fn added_by(current, next, named) -> WorkingSet` and its module doc ("what the turn touched
  stays").
- **Moves.** `crates/theseus-core/src/rpc/thread_move.rs` (366 lines): `moved_by_words`, near :174 `state.moved(&routed.turn)`
  (the move's in-memory state), near :145 the `take_grew` use the review names.
- **Tests.** `crates/theseus-core/src/rpc/tests_thread_grow.rs` (263 lines) drives growth and moves; read it for the rig.
- **The held compile failure.** `crates/theseus-core/src/turn/route_step.rs` `fn held` (near :673; its doc ends "the turn
  loop then ends the turn held (theseus-cljlz)") reads the verdict and runs `personal_warn`; `crates/theseus-core/src/turn.rs`
  near :1876 takes the held exit. The rows a normal routed turn writes: `record_route` (route_step.rs near :837),
  `engage_verdict` (`turn/persona_engage.rs` near :156), `note_routed` (`turn/route_step/correct.rs` near :468).
  `crates/theseus-core/src/tests_hold_compile.rs` (155 lines): `a_held_turn_whose_compile_fails_is_still_held` (asserts
  `held: true`) and `a_compile_failure_in_a_private_place_still_fails` (near :146; no `privacy.warned` assert yet).
- **The TUI's filter.** `crates/theseus-tui/src/chat/events.rs` near :485 `Event::TaskChanged(t) if self.ours_in(&t.session_id, t.turn_id.as_deref())`;
  `crates/theseus-tui/src/chat/tests_thread_ask.rs` near :253 `a_placed_threads_other_turn_does_not_ask_in_this_chat`
  (covers `ConfirmRequested` only); `chat/tests.rs` near :946 builds a `TaskChanged`.

## What to build
1. **ddfe7 F1.** A turn already moved grows nothing in its origin: when growth runs for a turn the move state holds
   (`state.moved`, or a mark you add beside `take_grew`'s ring), skip the origin's growth (or grow the destination
   instead, if that is one line and the module doc's rule supports it: decide and say which). Bounded memory: the mark
   lives in the same bounded ring as the growth, not an unbounded set.
2. **ddfe7 F2.** `added_by` subtracts what the turn's touched calls added: an entry both named and touched is the
   turn's, and stays in the origin on a move. Update its doc.
3. **ddfe7 F3.** `route.grew` only when `added` is not empty.
4. **ddfe7 F4.** One more case beside `a_placed_threads_other_turn_does_not_ask_in_this_chat`: a `TaskChanged` with
   `turn_id: Some(<another turn>)` is not shown in this chat; one with `turn_id: None` still is (the background worker
   case). The review notes this narrowing is intended.
5. **qiyxq F2.** In `a_compile_failure_in_a_private_place_still_fails`, or a new test beside it, a private-place compile
   failure whose verdict warns: one `privacy.warned` row with `held: false`, and the turn still fails.
6. **qiyxq F3.** A held compile failure records the route rows a turn that ran would (`record_route` with the verdict,
   `engage_verdict`, `note_routed`), or the subset that makes sense for a turn that never called the model: decide, and
   say why in `held`'s doc. No double rows when the turn later runs normally.
7. **Docs** (core `AGENTS.md`, one sentence each): under Privacy warnings: "A compile that fails (`context_overage`) in a
   shared place still reads the verdict and is held, not failed (theseus-cljlz); in a private place or for a task it
   fails as before, and a private place's warning still posts." Under Routing: "`context.detoured` and `ContextDetour`
   carry `recall` (whether the detour's request held the turn's own recall); `kind` is `steered` for a steer that names
   no mode (theseus-nfrlj)." Search first: skip either sentence if `main` already says it.

## Tests (each with a planted revert; the first is the key test)
- **`a_turn_moved_before_its_growth_grows_nothing_in_the_origin`**: start a turn in thread A, move it to B while it runs
  (or call the move before the growth step, as the rig allows), let growth run: A's working set is unchanged. Plant:
  remove the moved check: A grows.
- `a_named_and_touched_entry_stays_on_a_move` (`added_by`). Plant: drop the subtraction.
- `an_empty_growth_pushes_no_grew` (the ring holds no entry for it). Plant: drop the guard.
- The TUI `TaskChanged` case (F4). Plant: drop `ours_in` from the `TaskChanged` arm.
- The private `held: false` assert (qiyxq F2). Plant: make `personal_warn` mark `held` in a private place.
- `a_held_compile_failure_records_its_route` (qiyxq F3). Plant: remove the new record call.
- `cargo nextest run -p theseus-core thread`, `-p theseus-core hold_compile`, `-p theseus-tui thread_ask` pass, then
  both crates whole.

## Not in this row
- Thread routing's scores and the pre-router (`thread_route.rs`, `thread_title.rs`): `integ/tail4`'s tail4-route edits
  them; do not touch them.
- The row's own proposal that `confirm.list` and the budget question carry `turn_id`: a wire change; list it as a
  follow-up only.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main:** `integ/tail4` edits `crates/theseus-core/src/thread_route.rs`,
  `crates/theseus-core/src/rpc/tests_thread_route.rs` (+ `tests_thread_route/title.rs`), `turn/compile_step.rs`,
  `rpc/tests.rs`, `rpc/mod.rs`, `lib.rs`, the TUI's `app.rs`, `chat/tasks.rs`, `chat/tests_tasks.rs`, `main.rs`, and
  core `AGENTS.md` (`git diff origin/main origin/integ/tail4 --stat`). Stay out of all but `AGENTS.md`, where your two
  sentences go in their own sections. `integ/cn` round 5 edits `turn/compaction.rs`/`compaction/gate.rs` and
  `turn.rs`'s neighbours (`git diff origin/main...origin/cloud/20261021-tail3-cn --stat`); do not edit `turn.rs` unless
  the held exit truly needs it, and then keep it to the held arm.
- **Rows beside you on `main` f1fc9b03, joining `integ/tail5` with you:** tail5-effort (`routing/effort.rs`,
  `config/routing.rs`, the template, `route_step.rs`'s effort call only) and tail5-discord (Discord `runtime/task_home.rs`,
  `runtime/task_channels.rs`). In `route_step.rs` you edit only `held` (and a helper beside it, if needed).
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field (the mark is in memory).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-core/AGENTS.md` (threads, routing, privacy warnings) and `crates/theseus-tui/AGENTS.md` first.
  Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id and finding; the moved-turn rule you picked (skip, or grow the destination)
and why; which route rows a held compile failure now writes and why; each test and its plant with the failing line; the
keel guard's output; the store format (unchanged); a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
