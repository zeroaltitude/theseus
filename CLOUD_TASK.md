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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-tests`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `8260dbab` (2026-10-22: release stage 2, which landed the threads lane rounds 1-3b and
the China-origin chain rounds 1-4 on top of install #69's `29f5dcad`); that is your base. Line numbers quoted below are
at `8260dbab` but may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `29f5dcad`** (`git log --first-parent 29f5dcad..HEAD` lists them):
- `bb265d6c` + `8260dbab`: the threads lane (a place's home splits into threads, thread routing with the directory rule
  and the which-thread ask, the place digest, thread moves and growth, the protocol nodes split).
- `38743708`: the China-origin chain (the data rule's open view, the open request, the tool screen, the draft review),
  with join fixes `c0d35275` (a session's data state is store format 49) through `30d6472e`.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail3` (`e29be1ed`) and `integ/threads` round 4 (`a52cac7b`) are being joined into the next release stage.
  Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read their diff
  (`git diff origin/main origin/integ/tail3 -- <file>`). Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail4-tests, the recall bench's two red smoke tests, and tests that leave their temp dirs behind (theseus-93lkm, theseus-5hr6i)

Branch: `cloud/20261021-tail4-tests`. Every commit's subject carries the Beads id of what it changes
(`theseus-93lkm`, `theseus-5hr6i`). Deadline for the report: 3 hours after you start.

**Why.**
- **theseus-93lkm.** `bench/recall/test_drive.py`'s `TheseusDriver` fails 2 of 24 on `main` (seen on `29f5dcad` and on
  the release branch; ARM, python3, debug binaries): `test_a_run_plans_at_the_overhead_it_measures_and_records_it`
  (`run.compactions[0]` is 5, expected the plan's mark 10 or 11: the first compaction now comes at turn 1-5) and
  `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing` (the same compaction-turn assert on ARM; on
  x86 the fact `f001` is also not delivered). Pre-existing on `main`, not from the latest lanes. Likely a compaction or
  ring change since the bench's plan was set: the ring's "the ring dropped nothing new to summarize" row follows a
  compaction at turn 1-5. (The long-known `test_generate.Budget.test_the_prices_are_the_catalogs` red is separate: not
  this row.)
- **theseus-5hr6i (the leak half).** The ARM gate host's `/tmp` (a 16 GB tmpfs) filled with 12 GB of leftovers from
  every lane's gates: **20,312 `/tmp/<PREFIX>t_*` dirs (~9 MB each; PREFIX below means the literal dir prefix that
  `test_core_with` in `crates/theseus-core/src/rpc/tests.rs` writes: read it there) and 5,171 `/tmp/.tmp*` (tempfile) dirs**, and a
  gate went red with ENOSPC (`tests_window.rs:29` write, `index_memory` cache bound `:89` apply). Tests must clean up
  what they make. (A host-side sweep before gates is the maintainer's local tooling: not this row.)

**Risk: low.** Test support and a bench; no product behaviour changes unless 93lkm's cause is a real compaction bug
(then that fix is product code: say so plainly).

**Prerequisite: base `origin/main` at `8260dbab`.** Your branch joins the new lane `integ/tail4`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-tests` | theseus-93lkm, theseus-5hr6i | find why the smoke's first compaction moved and fix the side that is wrong; make test dirs go away | `bench/recall/` (`drive.py`, `generate.py`, `test_drive.py`, `README.md`) if the bench is stale; the compaction code (`crates/theseus-core/src/turn/compaction*.rs`, `fact/compaction.rs`) only if the daemon is wrong, + a core test; `crates/theseus-core/src/rpc/tests.rs` (`test_core_with` and the other PREFIX makers), `crates/theseus-core/src/rpc/tests_voice_pick.rs`, a small shared helper in core's test support (search `test_support`), and each crate's tests that leak `tempfile` dirs (`into_path`, `keep`, `persist`, `TempDir::new` held in a leaked `Arc`, or a `std::mem::forget`) | M |

## What exists (at `8260dbab`; search by name)
- **The bench.** `bench/recall/`: `drive.py` (the driver; `processes_naming`, `PROBE_TEXT`), `generate.py` (the
  progression: `build(seed, size, overhead)`, `marks()`), `standin.py` (the counting stand-in model), `score.py`,
  `test_drive.py` (the two tests near :924 and :976; `self.measured()`, `self.drive(..., counting=True)`), README. The
  driver uses the workspace's `target/debug` binaries (`THESEUS_RECALL_BIN_DIR` overrides): run
  `cargo build --workspace` first, then `python3 -m unittest bench.recall.test_drive -v` (check the README for the
  exact invocation).
- **Compaction.** Where the daemon decides to compact and where the ring drops turns: search `ring dropped nothing`,
  `compaction_rows`, `fn compact` in `crates/theseus-core/src/turn/`. `git log --first-parent --oneline -- crates/theseus-core/src/turn/compaction* crates/theseus-core/src/fact/compaction.rs`
  lists the joins that moved it; the bench's plan last changed at `git log --oneline -- bench/recall/generate.py`.
- **The leaks.** `crates/theseus-core/src/rpc/tests.rs`: `test_core_with` (near :26) makes
  `std::env::temp_dir().join(format!("<PREFIX>{}", new_id("t")))` and never removes it; the same pattern at :123,
  :853, :1065, :1202, :1466, :1613, :1887, :1925 and `rpc/tests_voice_pick.rs` :130 (`git grep -n 'temp_dir().join(format!'`).
  `test_core` returns an `Arc<Core>` that tests often leak into spawned tasks, and nextest runs each test in its own
  process, so a `Drop` alone may never run. `.tmp*` dirs are `tempfile`'s default prefix: find the tests whose
  `TempDir` never drops (`git grep -n 'into_path()\|\.keep()\|TempDir::new' -- crates`), including `theseusd` and
  `theseus` integration tests that kill a child daemon.

## What to build
1. **theseus-93lkm.** Run the two tests on your base and read the run's `compaction_rows`, `turns.jsonl` and the
   daemon's ledger for the first compaction: name the row's `outcomes`, the turn's request tokens against the
   compaction mark, and which join moved it (`git log -S` / one bisect step per suspect join if needed, a debug build each).
   Then fix the wrong side: if the daemon now compacts early by mistake, fix the daemon with a core test that pins the
   mark; if the daemon's new rule is right and the bench's plan predates it (its marks assume an overhead or a ring
   rule that changed), update the plan in `generate.py` / `drive.py` so the mark is where the daemon now compacts, and
   keep the test's assertion (mark or mark+1) as it is. The x86 `f001` not delivered: find whether it follows from the
   same cause (a compaction that dropped the turn that said it) and fix it with the same change, or say why not.
2. **theseus-5hr6i.** Every PREFIX dir a test makes is removed when the test ends, even when the `Core`
   leaks: the smallest sound way, for example the helper makes the dir with `tempfile::Builder::new().prefix(PREFIX)`
   and keeps the `TempDir` in a guard the test holds, or registers it in a process-wide list that a `#[ctor]`-free
   mechanism removes (nextest: each test is its own process; a process-exit hook is not guaranteed). If a test cannot
   own a guard, the helper prunes PREFIX dirs older than 6 hours under `temp_dir()` once per process
   (`OnceLock`), which bounds the leak whatever kills a test. Never prune `.tmp*` (other programs use that prefix):
   instead give this repo's `tempfile` uses the PREFIX where they leak, so the prune covers them.
   Count what is left: run `cargo nextest run -p theseus-core` once with `TMPDIR` set to an empty dir, and report the
   count of dirs left before and after your change.

## Tests (each with a planted revert)
- 93lkm: the two `test_drive` tests pass on your branch (paste their output). If you changed the daemon: a core test
  that the first compaction comes at the mark, with a plant (your fix reverted: it fails).
- **`a_test_core_leaves_no_dir_behind`** (core): a `test_core` made and dropped (or leaked, then its guard dropped)
  leaves no PREFIX dir under a scratch `TMPDIR`. Plant: drop the cleanup.
- `the_prune_removes_only_old_theseus_test_dirs` (if you add the prune): a fresh one and a `.tmpX` stay, an old one
  goes. Plant: prune by prefix alone.
- `cargo nextest run -p theseus-core` must pass, and each crate whose tests you changed.

## Not in this row
- The ARM host's own `/tmp` sweep and a gate pre-check of free space: the maintainer's local tooling.
- `test_generate.Budget.test_the_prices_are_the_catalogs`: a separate known red.
- Any bench re-run on real models: the conductor's.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test (a looser tolerance or a moved mark in an assertion is a weakening;
  a plan that predicts the daemon's real rule is not), and do not raise a budget, cap or ceiling. A cloud row cannot
  carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- **Near you, not yet on main:** `integ/tail3` changed `crates/theseus-core/src/test_support.rs`, `rpc/mod.rs`
  (`Core::build` gained a clock seam), `tests_task_brief.rs` and `tests_wait_beat.rs`. Read
  `git diff origin/main origin/integ/tail3 -- crates/theseus-core/src/test_support.rs crates/theseus-core/src/rpc/mod.rs`
  first and keep your edits off its hunks (prefer a new helper file over editing `test_support.rs`).
- **Rows beside you on `main` 8260dbab, joining `integ/tail4` with you:** tail4-voice, tail4-discord, tail4-tui,
  tail4-route (core `thread_route.rs`, `turn/compile_step.rs`, a new `rpc/tests_thread_title.rs`), tail4-onto (core
  `judge/onto/`). Stay out of their files.
- **Dependencies.** None new (`tempfile` is already a dev-dependency: check each crate's `Cargo.toml`; adding it as a
  dev-dependency to a crate that lacks it is fine and is not a new third-party package). **Store format** 49, unchanged.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-core/AGENTS.md` (tests) and `bench/recall/README.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: 93lkm's cause (the first compaction's evidence, the join that moved it, daemon or bench at fault) and
the fix; the `f001` finding; the leak sources you found (file:line each) and the cleanup's shape; the dirs-left count
before and after; each test and its plant with the failing line; the keel guard's output; the store format
(unchanged); follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
