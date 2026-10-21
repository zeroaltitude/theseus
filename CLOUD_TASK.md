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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-flakes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `6918e3c1` (2026-10-21, after the patch1 stage-1 landing), unless your task names another base (an integration branch). Where the
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
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,405 / 3,525
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

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: tail-flakes, five tests that go red under a loaded host are ordered by events, or the race they find is fixed (theseus-iubei, theseus-t0y6v, theseus-fumkt, theseus-4vgw1, theseus-f4tg1)

Branch: `cloud/20261021-tail-flakes`. Every commit's subject carries the Beads id of the test it fixes (`theseus-<id>`).
Deadline for the report: 5 hours after you start.

**Why.** Every lane gate on 2026-10-21 ran beside other lanes' gates and plants on one ARM host (load around 14 on 10
cores), and five tests went red there that pass alone. Each red costs a re-gate of the whole lane (20 to 25 minutes),
and a gate that is red for a known flake teaches people to wave reds through, which is how a real race gets merged.
The five, with what was seen:
- **theseus-4vgw1**, `crates/theseus-core/src/tests_reminders.rs`
  `with_no_dm_it_lives_in_a_reminders_conversation_and_every_surface_hears_it`: under a 6-lane gate it read
  `noticed()` as `[]` (left 0, right 1). The test waits until one `wake.fired` ledger row exists, then drains the event
  channel once with `try_recv` and wants one `wake.noticed`. The notification follows the frame that wrote the row, so
  a reader that sees the row can be ahead of it. The file already has the right tool: `next_noticed` (it awaits the next
  `wake.noticed` and its doc comment cites theseus-1b2zm for this exact reason); this one test was missed.
- **theseus-t0y6v**, `crates/theseus-core/src/tests_horizon.rs`
  `the_agenda_expands_a_series...` (search `"a paused series has no occurrences"`): twice in a full suite, right after
  `routine.pause` returns, the next `work.agenda` still listed `WorkKind::Routine` lines.
- **theseus-fumkt**, `crates/theseus/tests/herdr.rs` `a_line_sent_on_to_main_does_not_leave_the_pane_sending`: `timed
  out waiting for 3 × pane.report_agent at herdr` (its `PATIENCE`, 20 s) in three lane gates, each under load.
- **theseus-f4tg1**, `crates/theseus-core/src/judge/people/tests.rs`
  `a_shared_place_gets_nothing_and_a_private_one_proposes`: `until_rows` timed out at 20 s with `0 of 1 judge.call
  rows` (host load about 14, 4 test jobs); it passed in that lane's other five gates and alone.
- **theseus-iubei**, `crates/theseus-core/src/tests_task_checkpoint.rs`: under the repository's load recipe three tests
  failed in 2 or 3 of 3 runs at about 30 s each, on the base as well. The one named in a gate log is
  `a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes` (red at about 20 s in a full x86 run; all 15 tests of
  the file green 3 times alone). The other two were not named: find them under load (below).

**The owner's rule for this row:** fix the race in the test or in the code, **never by raising a timeout, a wait, a
`PATIENCE`, a `secs` argument or a sleep**, and never by adding a test to the flaky list in `.config/nextest.toml`
(it is empty today; keep it so). A test that waits should wait for the event it then asserts, not for a neighbour of it
and not for a length of time. A lost event (something that never arrives however long you wait) is a finding: fix the
code that loses it, with its own test.

**Risk: low.** Test-only changes except where a test shows a real lost event (then a small code fix in the file that
loses it, inside this row's files; see "Not in this row" for files you may not edit).

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| T | `tail-flakes` | theseus-iubei, theseus-t0y6v, theseus-fumkt, theseus-4vgw1, theseus-f4tg1 | each named test ordered by the event it asserts, or the race it found fixed; each shown red under load (or by a plant) before and green after | `crates/theseus-core/src/{tests_reminders,tests_horizon,tests_task_checkpoint}.rs`, `crates/theseus-core/src/judge/people/tests.rs`, `crates/theseus/tests/herdr.rs`; code only if a test finds a lost event: `crates/theseus/src/{herdr,herdr_slots}.rs`, `crates/theseus-core/src/push/work/horizon.rs`, `crates/theseus-core/src/rpc/agenda.rs`, `crates/theseus-core/src/test_support.rs` (a shared helper, additive) | M |

## How to show a race (do this first, for every test)
The repository's rule (`AGENTS.md`, "A race is reproduced under load before it is fixed"): **priority, not count,
makes the load**. Run the test binary at `nice -n 19` beside busy loops at nice 0. On this 4-core VM:

```
for i in 1 2 3 4 5 6; do sh -c 'while :; do :; done' & done      # note the pids; kill them by pid after
cargo nextest run -p theseus-core --no-run                        # build first, unloaded
nice -n 19 cargo nextest run -p theseus-core -E 'test(/tests_reminders::/)' --no-fail-fast
```

Run the test **beside its own module** (the module's other tests running at the same time is part of the load), five
rounds. Busy loops at the test's own nice do not starve it; loops at a higher priority do. Kill the loops by pid when
done (never `pkill -f`, which can match your own shell). If the load recipe cannot make a test fail on this VM, make it
fail deterministically with a plant instead (a delay injected at the racing point, e.g. a `std::thread::sleep` in the
fake or a `tokio::time::sleep` before the event is sent), and say in the report which way you showed it. Record for each
test: red count before (N of 5 under load, or the plant), green count after (5 of 5 under the same load).

## What exists, and what to do (lines at 6918e3c1; search by name)

### 1. theseus-4vgw1 (the key test): wait for the notice, not the row
`tests_reminders.rs`: `with_no_dm_it_lives_in_a_reminders_conversation_and_every_surface_hears_it` (near :285). Today:
two `wake.create` calls ("stretch" in 2 s, "water" in 3 s), then
`until("the first notice", 10, || rows(&r.core, "wake.fired").len() == 1)`, then `let n = noticed(&mut r);` (a
`try_recv` drain) and `assert_eq!(n.len(), 1)`.
- Use `next_noticed(&mut r).await` (near :195; it awaits the next `wake.noticed` notification) for the first notice
  and assert on it (`note == "stretch"`, `purpose == "remind"`, `posted_to == None`, the four answers). Keep every
  existing assertion; the keel guard counts assertions per test.
- Keep the test's meaning "one notice for the first fire": the second reminder ("water", 1 s later) will produce a
  second notice, so do not assert "no more notices ever" with a drain that races it. If you keep a "heard once"
  check, make it about the first wake's id (e.g. the next notice, if any, is for the other wake's id).
- **Key test proof:** under the load recipe the old test fails at least once in five rounds (or, deterministically,
  plant a `tokio::time::sleep(Duration::from_millis(200))` just before the core sends `wake.noticed`, in the wake's
  notice path: find it by `WAKE_NOTICED` in `crates/theseus-core/src/`); the new test passes with the same plant and
  load. Remove the plant before you commit.

### 2. theseus-t0y6v: read-after-write lag on the work board, not a lost pause (decide and show)
The code says which it is. `work.agenda` (`crates/theseus-core/src/rpc/agenda.rs` `work_agenda_rpc`) answers from the
**work board in memory**: `self.push.work_agenda(...)` -> `push/work/horizon.rs` `agenda` iterates `self.execs` and
skips a wake with `paused_at_ms.is_some()`. The board is a projection the push applies **after** a frame commits
(theseus-hanu). `routine.pause` returns once its frame is committed, so the next `work.agenda` can run before the push
has applied that frame. That is read-after-write lag, not a lost pause, and it is the same shape the reminder count had
in the same file: commit `677ee05e` (theseus-1b2zm) made `a_met_watch_ends_done_and_status_short_counts_the_future` wait
for `status_short()` to count the reminder, with the comment "nothing promises read-your-writes there". Follow that
precedent:
- **Confirm it** before changing anything: with a plant that delays the push's apply of frames (a short sleep where the
  push applies a committed frame to the board; find the apply loop in `crates/theseus-core/src/push/`), the old test
  fails at the paused assertion, and a later read (after the apply) shows the routine gone. If instead the pause never
  reaches the board (the wake's `paused_at_ms` stays `None` however long you wait), that is a lost pause: fix it in the
  push's handling of the pause frame, with its own test, and say so in the report.
- **Fix (lag):** after `routine.pause`, wait until the board shows the pause, then read the agenda once and assert as
  today. Use the existing `until(...)` helper on a condition (an event, not a clock): e.g. until `agenda(...)` has no
  `WorkKind::Routine` line, or until the board's routine view says `paused` (the same test file already does
  `until("waiting", 10, || of_kind(&w.held(), WorkKind::Routine)[0].state == WorkState::Waiting)` near :376 in
  another test). The keel guard compares assertions: keep `assert!(!paused.lines.iter().any(...), "a paused series has
  no occurrences")` as an assertion on the agenda read after the wait (a `until` on the same condition followed by the
  same `assert!` keeps it). Apply the same treatment to the `routine.resume` and `routine.delete` + `made_routine("every
  1m")` steps right below if the next read depends on them (`busy.lines.len() == AGENDA_MAX` reads the board after a
  delete and a create: the same lag can bite there).

### 3. theseus-fumkt: a coalesced report, or a lost one
`crates/theseus/tests/herdr.rs` `a_line_sent_on_to_main_does_not_leave_the_pane_sending` (near :1652) waits for
`pane.report_agent` 1, types a line, waits for 2 `turn.submit`, then waits for **3** `pane.report_agent` (the first
view's, "working · sending your message", and the label back to the first), then releases, then asserts
`got[1] == ("working", "sending your message")` and `got.last() == got.first()`.
The adapter batches its jobs: `crates/theseus/src/herdr.rs` `coalesce` (near :727) drops a `pane.report_agent` that a
later report in the same batch replaces (by design: "A batch without the reports and metadata a later one replaces").
On a loaded host, the "sending" report and the "back to the label" report can land in one batch, so herdr hears only
the last: 2 reports, and the wait for 3 never ends. That is the leading hypothesis; confirm or refute it:
- **Show it:** the fake herdr already takes an answer closure (`FakeHerdr::answering`); a closure that sleeps (e.g.
  300 ms) on the first `pane.report_agent` holds the sender so the next two jobs queue into one batch. If the old test
  then times out with 2 reports whose last equals the first, the hypothesis holds.
- **If it holds (coalescing is correct):** the test is asserting an intermediate state the adapter is allowed to skip.
  Order it by the event it means: drop the wait for 3, wait for the release (`pane.release_agent`, which follows every
  report), then assert what the test is for: the last report before the release equals the first view's report (the
  pane is not left "sending"), and every report is either the first view's or "sending your message". Keep the
  "sending" check where it is guaranteed: if you can make it deterministic (e.g. the daemon's `turn.submit` answer for
  `main` held until the test has seen the "sending" report, using the fake daemon's answer closure), keep
  `got[1] == rep("working", "sending your message")`; otherwise move that check to a test where the adapter cannot
  coalesce it and say which.
- **If it does not hold** (a report is never sent, e.g. the change-only rule in `herdr_slots.rs` / `herdr.rs` compares
  against a report that was coalesced away and so never sends the label back): that is a lost report, a real bug a
  person sees as a pane stuck on "sending your message". Fix it in `herdr.rs`, with a test that fails on the old code.
- Under load: `nice -n 19 cargo nextest run -p theseus --test herdr` (the whole file) beside the busy loops.

### 4. theseus-f4tg1: the background judge call under load
`crates/theseus-core/src/judge/people/tests.rs` `a_shared_place_gets_nothing_and_a_private_one_proposes` (near :275
in the gate's numbering; search by name). After the private session's moorings it waits
`until_rows(..., "people.extracted", 1)`, then `until_rows(..., "judge.call", 1)` (`until_rows` near :268 polls the
ledger for 20 s). Red: 0 of 1 `judge.call` rows at 20 s. The extraction landed; the judging that follows it is a
background task.
- Find what starts the judge call after an extraction and what it waits on (a semaphore, a queue, a spawned task, the
  runtime's 2 worker threads: `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`). Under load, decide:
  **slow** (it does arrive later; then wait on the event that starts it, e.g. a notification or a counter the code
  already keeps, not on wall-clock), or **lost** (it never arrives: e.g. a task spawned on a runtime that is being
  starved by a blocking call, or dropped). A lost call is a finding.
- The same test asserts "a shared place gets nothing" with `tokio::time::sleep(Duration::from_millis(500))` followed by
  `assert!(rows(..., "people.extracted").is_empty())`: a negative assertion timed by a clock passes vacuously on a slow
  host. Order it by an event if the code offers one (e.g. the shared session's exchange end has been processed: a
  mark, a row or a counter that proves the extractor looked and chose nothing). If none exists, leave it and name it as
  a follow-up in the report.
- **Do not edit** `judge/people/extract.rs` or `judge/people/run.rs` (another lane edits both). If the fix belongs
  there, write the exact change in the report and stop; the test change alone must not hide it.

### 5. theseus-iubei: find the three, then order each by events
`tests_task_checkpoint.rs` was already converted once (`a2f8d276`, theseus-1b2zm: `fa14369c` "the slow-verdict
checkpoint test is ordered by events, not by a clock"). It still fails under starvation:
`a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes` (near :602) wraps `gate.reached()` in a 30 s
`tokio::time::timeout` ("the verdict held a loop: no seventh request") and uses `until_within(..., 30, ...)` twice.
Its timeout is a **negative assertion by clock** ("a held loop would never send the seventh request"): on a starved
host a slow loop and a held loop look the same.
- **Find the other two:** run the whole file under the load recipe five times
  (`nice -n 19 cargo nextest run -p theseus-core -E 'test(/tests_task_checkpoint::/)' --no-fail-fast`) and list every
  test that fails and at which wait. The tasks run on `FakeProvider { delay_ms: 60, .. }` and two worker threads.
- **For the slow-verdict test,** prove "the verdict never holds a loop" without a clock: the loop's progress is an
  event the test can observe (the seventh request reaching the `RequestGate`) while Jev's answer is still held
  (`jev.hold_when(ASKED)`); the property is that the seventh request arrives **while** the verdict is held, which the
  test can check by ordering (the gate is reached before `seen.release()` is called), with no timeout needed for the
  positive path. A hang must still fail the test: nextest's `slow-timeout` (60 s, terminate after 2) kills a hung
  test, so an unbounded `.await` is acceptable there; say so in a comment. Do not lengthen any timeout.
- For each of the other two, the same: wait on the event asserted. Where `task_with`'s 60 ms provider delay makes a
  test depend on how many loops fit in a period, that is a clock dependency: order it by requests seen instead.
- **Do not edit** `crates/theseus-core/src/turn/checkpoint_step.rs` (another lane edits it). If a test shows a lost
  event there, write it in the report and stop.

## Tests (each with a planted revert or a load proof; the first is the key test)
- **`with_no_dm_it_lives_in_a_reminders_conversation_and_every_surface_hears_it`** (theseus-4vgw1): the old version
  red, the new one green, under the same plant (a delay before `wake.noticed` is sent) and under the load recipe.
- `the_agenda_expands_a_series...` (theseus-t0y6v): red under a plant delaying the push's apply, green after; plus
  the load recipe's five rounds.
- `a_line_sent_on_to_main_does_not_leave_the_pane_sending` (theseus-fumkt): red under the slow first report (the
  fake's answer closure), green after. If you found a lost report, a new test for the code fix fails on the old code
  (plant: revert the fix).
- `a_shared_place_gets_nothing_and_a_private_one_proposes` (theseus-f4tg1): red under the load recipe or a plant that
  delays the background judge call, green after.
- The tests you find in `tests_task_checkpoint.rs` (theseus-iubei): each red under load before, green 5 of 5 after.
- Every converted test still fails when the behaviour it checks is broken: for each, a behaviour plant (e.g. for
  4vgw1, the wake posts no notice; for t0y6v, `agenda` ignores `paused_at_ms`; for fumkt, the adapter never sends the
  label back) still turns it red. A conversion that cannot fail on a broken product is a weakened test.

## Not in this row
- **theseus-h9jec** (`crates/theseusd/tests/judge.rs` `a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes`
  races the minute boundary): its file is in another row's planned files (a judge families row), so it waits for that
  row's join. Do not touch `crates/theseusd/tests/judge.rs`.
- `crates/theseus-core/src/turn/checkpoint_step.rs`, `crates/theseus-core/src/judge/people/extract.rs`,
  `crates/theseus-core/src/judge/people/run.rs`: another lane edits them. A fix needed there goes in the report.
- `.config/nextest.toml` (no flaky-list entry, no timeout change), any `PATIENCE`, `secs` or timeout value.
- A read-your-writes barrier on `work.agenda` or any board read (a product change; report it if you think it is
  needed).
- Other flaky tests: name them in the report; do not fix them here.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right helper over several near-copies: if two tests need the same "wait for this notification" helper, put one in
  `crates/theseus-core/src/test_support.rs` (additive) rather than copying it.
- **Ordered by events, never by clocks.** No new `sleep` used as a wait, no longer timeout, no retry. A bounded wait
  that remains (`until`, `until_rows`, `PATIENCE`) must be waiting for the exact thing the next assertion reads.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap, timeout or ceiling. Keep every
  assertion of every test you convert (the keel guard counts assertions per test); an assertion may move after a wait,
  never away. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and
  paste its output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the
  owner". Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md`, `crates/theseus/`'s),
  and the root `AGENTS.md`'s rules on races. Line ceilings: none of this row's files is listed in
  `scripts/long-files.txt`; keep every file under 2,500 lines or under its ceiling.
- Keep CLOUD_REPORT.md honest: what you ran, how many rounds, under which load, what passed. The VM has 4 cores; the
  lane gates that saw these reds ran on a 10-core ARM host at load around 14. If a test would not fail on the VM under
  load or a plant, say so plainly and give the maintainer the exact command to run it under load on a bigger host.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with:
- **Per test:** the cause found (lag, coalescing, lost event, clock-timed negative), the evidence (the plant or the
  load rounds: red N of 5 before, green 5 of 5 after), and the change.
- Each plant with the test that failed on it, the key test's first.
- For theseus-t0y6v, the decision (read-after-write lag or lost pause) and the line of code that decides it.
- What could not run on the VM, with the exact commands for the maintainer.
- The keel guard's output (empty).
- Follow-ups: any fix needed in a file this row may not edit (`checkpoint_step.rs`, `judge/people/{extract,run}.rs`),
  any other flaky test you saw, and whether `work.agenda` should promise read-your-writes.

Push, and end with a final message starting `CLOUD REPORT COMPLETE`.
