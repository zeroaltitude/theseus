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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail6-tests`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `49352e8a` (2026-10-22: release stage 4, install #72, which landed `integ/tail4` round 1
and `integ/cn` round 5 on top of stage 3's `f1fc9b03`); that is your base. Line numbers quoted below are at `49352e8a` but
may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f1fc9b03`** (`git log --first-parent f1fc9b03..HEAD` lists them):
- `6854991f`: `integ/tail4` round 1 (the TUI work view, thread routing by title, voice warm tracks, the Discord approval
  card and forums, the test temp-dir leak, the ontology sweep's renames and kind names).
- `a95ec41d` + `b046ca16`: `integ/cn` round 5 (the tool screen's redirects, the China-origin config guidance, reply data,
  the draft's first pass).
- `49352e8a`: a join fix to the approval card's why line.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (the lane beside you adds lines here: add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail5` (`origin/integ/tail5`, `b1cd38e9` or later) lands as the next release stage. Its rows are the branches
  `origin/cloud/20261021-tail5-effort`, `-tail5-threads`, `-tail5-discord` and `-tail5-t4fix`. Your task's "Rules for this
  row" names the files of theirs that sit near yours. Read their diff before you edit those files:
  `git diff origin/main...origin/cloud/20261021-<row> -- <file>` (three dots).
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail6-tests, the recall bench's x86 smoke red and the test dirs a read-only tree leaves behind (theseus-93lkm, theseus-kbfl0)

Branch: `cloud/20261021-tail6-tests`. Every commit's subject carries the Beads id of what it changes (`theseus-93lkm`,
`theseus-kbfl0`). Deadline for the report: 3 hours after you start.

**Why.** Two test-side issues are left after release stage 4 (`6854991f`, now on `main`):
- **theseus-93lkm, the x86 half.** tail4-tests fixed the recall bench's stand-in token count (`bench/recall/tokens.py`
  `tools_bytes` now mirrors `tools_defer::tools_bytes`) and the plan's recall reserve (`generate.fit_bound` reserves
  `RECALL_TOKENS = 1500`). On ARM, `bench/recall/test_drive.py` now runs 24/24. On x86 (debug binaries, python3.12) one
  smoke test is still red, deterministically (3/3), on `main` 49352e8a and on the old `integ/tail4` tip alone:
  `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing` fails `self.assertEqual(s["moved"], 0)` with
  `1 != 0`. The run's compactions came at turns `[11, 23]` against the plan's mark 10, and a "near" probe was compacted
  away by the second. So the x86 daemon compacts on a different turn than the plan predicts: some count the plan makes
  still differs from the daemon's on x86 (or in a debug build), and the cloud VM is x86, so you can reproduce it.
- **theseus-kbfl0, the read-only half.** A leak check (`TMPDIR` = a fresh dir, full `cargo nextest run -p theseus-core`)
  left 25 entries, all a self/mint store with read-only directories (`dr-xr-xr-x`): 11 of `scratch`'s own test dirs, because
  `scratch::remove`'s `remove_dir_all` fails with Permission denied, and 14 `.tmp*` dirs, because `tempfile::TempDir`'s
  drop fails the same way, silently. `-p theseusd` left one `.tmp*`.

**Risk: low.** Test and bench code only; no daemon behaviour changes (unless 93lkm's cause is a daemon count that is
wrong, in which case stop and say so in the report before changing daemon code).

**Prerequisite: base `origin/main` at `49352e8a`.** Your branch joins the new lane `integ/tail6`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail6-tests` | theseus-93lkm, theseus-kbfl0 | find and fix the x86 compaction-turn difference in the recall plan; remove read-only test trees | `bench/recall/generate.py`, `bench/recall/tokens.py`, `bench/recall/drive.py`, `bench/recall/test_drive.py` (new assertions only), `bench/recall/test_generate.py`; core `src/scratch.rs` (`remove` and a helper), the core tests that make read-only stores with a bare `tempfile::TempDir` (search `0o555` and `set_readonly` in test code), `crates/theseusd/tests/*` (only the one leaking fixture) | M |

## What exists (at `49352e8a`; search by name)
- `bench/recall/test_drive.py` near :924 the smoke test, near :974 `assertEqual(s["moved"], 0)`;
  `test_a_run_plans_at_the_overhead_it_measures_and_records_it` (green now on both arches).
- `bench/recall/generate.py` `fit_bound` and `RECALL_TOKENS`; `bench/recall/tokens.py` `tools_bytes` (and the test that
  reads the constants from `crates/theseus-core/src/tools_defer.rs`); `bench/recall/drive.py` the driver; the README.
- The daemon's side: `crates/theseus-core/src/tools_defer.rs` near :157-171 `tools_bytes`; the compaction trigger in
  `crates/theseus-core/src/turn/compaction*.rs` (search the budget it compares against); `config/memory.rs` near :200
  `recall_budget_tokens`'s default.
- `crates/theseus-core/src/scratch.rs`: `dir`, `file`, `tempdir` (near :33), `remove` (near :51: `remove_dir_all`, else
  `remove_file`), `keep_track`, the `atexit` sweep, `prune`, and its tests.
- Read-only trees in core: `extend/freeze.rs` (near :253-256 sets `0o555`), `rsi/sealed/runner.rs` (near :367, :387),
  `binary.rs` (near :119-124), `flow/load.rs` (near :188). The tests that build those stores are the leak's source.

## What to build
1. **93lkm.** Reproduce on the VM: build debug binaries as the bench README says and run
   `python3 -m unittest bench.recall.test_drive` (or the README's command). Find the count that differs: compare the
   plan's predicted context tokens per turn with what the daemon's ledger records for the same turns (the run's
   `provider.call`/compaction rows). Fix the plan (the stand-in's counting or the plan's reserve) at the root, the way
   tail4-tests did, so the first compaction falls on the plan's mark on x86 and ARM alike. No assertion, mark or
   tolerance in `test_drive.py` moves; add a `test_generate.py` case that pins the corrected count.
2. **kbfl0.** `scratch::remove`: on a permission error, make the tree writable (`u+w` down the tree, directories first)
   and remove again. The tests that make read-only stores under a bare `tempfile::TempDir` use `scratch` (or a guard
   that does the same chmod before the drop). After the fix, the leak check above leaves 0 entries from core and
   theseusd; measure it and paste the counts.

## Tests (each with a planted revert; the first is the key test)
- **The x86 smoke**: `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing` passes 3/3 on the VM, and
  `test_drive` runs 24/24. Plant: revert your plan fix: the smoke goes red with `moved 1 != 0`.
- The new `test_generate.py` case. Plant: the same revert.
- `a_read_only_tree_is_removed` (in `scratch.rs`'s tests: make a dir, a file in it, chmod the dir `0o555`, remove,
  the path is gone). Plant: drop the chmod retry.
- The leak count (before and after, `TMPDIR` = a fresh dir): paste both.
- `cargo nextest run -p theseus-core scratch`, then `-p theseus-core` and `-p theseusd` whole; the bench's Python tests.

## Not in this row
- The host's `/tmp` sweep and free-space pre-check (local tooling).
- Regenerating the recall progression (a measurement step after install).
- `test_generate.Budget.test_the_prices_are_the_catalogs` (a known separate red).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main (integ/tail5):** tail5-t4fix edits `scratch.rs` (the test `a_made_dir_is_swept`: `sweep()`
  becomes `remove_all(paths)`). Read `git diff origin/main...origin/cloud/20261021-tail5-t4fix -- crates/theseus-core/src/scratch.rs`
  first; edit only `remove` and a helper beside it, and add your test after the module's existing tests, so the merge is
  clean. This overlap cannot be avoided: the fix belongs in `remove`.
- **Rows beside you on `main` 49352e8a, joining `integ/tail6`:** tail6-cn (core data gate, compaction's gate,
  `consolidate/run.rs`, `keep_warm.rs`, `own_bench/replay.rs`, `judge/reply_data.rs`) and tail6-onto (`judge/onto/*`,
  the cockpit). Stay out of their files; if 93lkm's cause is in compaction code, report it instead of editing.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner".
- Read `bench/recall/README.md` and `crates/theseus-core/AGENTS.md` (tests, scratch dirs) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the count that differed on x86 and the evidence (plan versus ledger, per turn); the fix; the smoke's
3/3; the leak counts before and after; each test and its plant with the failing line; the keel guard's output; a
`docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
