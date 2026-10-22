# CLOUD_REPORT: tail6-tests (theseus-93lkm, theseus-kbfl0)

Branch `cloud/20261021-tail6-tests`, based on `49352e8a`. Commits are signed with the VM's ssh key; the VM has no
`allowedSignersFile`, so `git log --show-signature` cannot verify them here. Both commit messages end with the
`Co-Authored-By: Claude (cloud session, Sonnet 5.5)` trailer the brief gives (the base's own commits carry
`Tabitha/Claude`; the brief names both, and I used the first).

Python tests: `python3 -m unittest discover -s bench/recall` runs 100 tests; the only red is
`Budget.test_the_prices_are_the_catalogs` (the known separate one).

## Step 1: theseus-93lkm, the x86 smoke (commit 58f65aac)

### What I found

The smoke did **not** go red on this VM (x86, python 3.11 and 3.12, debug binaries): `test_drive` ran 24/24 and the
smoke test passed 3/3 before any change. So I reproduced the failure by cause instead.

The count that differs is **the recall notes in the tail**, not a token count of the turns:

- Every turn of a session that can recall (the second session of the smoke; earlier ones have nothing outside their
  own context) writes a `Recall` node into the session once the index tender is up. It stays in the tail until the
  next recompile, up to `[memory] session_recall_cap_tokens` (12,000; `recall_step.rs`: a note is `paused` only when
  `in_tail + used > cap`). The plan reserved one note (`RECALL_TOKENS = 1500`, the per-request budget).
- When the tender comes up is timing: `recall.ran` says `outcome: unavailable, why: the index tender is pending: it
  starts 2 s after serving` until then. On this VM the first note came at turn 26 of 30 (`raw/history-2.json`:
  four `recall` nodes, turns 26 to 29); on the owner's x86 it came earlier, so the second session accumulated
  more notes, passed the budget and compacted again at turn 23 (the red run's `[11, 23]`).
- Plan versus daemon, per turn, in session 2 (first-call count less the 7,345 overhead, against the plan's
  cumulative estimate): turns 15 to 25 agree within -223 to +34 (the counts sit under the plan). From turn 26 the
  difference is +1,090, +2,367, +3,154, one note of about 1,100 to 1,300 tokens a turn. The plan's 1,500 covered
  the first of them. Window 33000 (budget 20,654); the last turn ended at a ring (`a summary of up to 4,096 tokens
  would not fit beside the 17,051 the kept turns take`) at turn 29.

Reproduction on the old plan: a throwaway script held the driver 8 s before turn 0 (the tender up from the start) and
ran the smoke: compactions `[11, 23]`, the same as the red run. With the fix: `[11]`.

No daemon code is wrong: the daemon counts what it renders. The plan's reserve was.

### What I changed

`bench/recall/generate.py`: `RECALL_TOKENS = 12000` (the tail's cap, read from `memory.rs` by a test) and
`recall_for(session)`: none for the first session (nothing to recall before it compacts, so its marks keep their
bounds), the cap for every later one. `fit_bound` takes the recall as an argument (defaulting to the cap); the
unmarked-session fit, the mark fit, `bounds_of` and `plan_misses` pass `recall_for(session)`.

Cost, which the owner should hear: the plan's windows grow. At the VM's measure (7,345) the smoke of seed 7 goes
from 33000 to 47000; at 13,943 it is 56000 (the README says 46000 at 13,943; the README is not in my files, see
"For the integrator"). The full of seed 7 at 13,943 is 101000 (the README says 94000). The arms' dollars per run
rise by about the share the window adds. The alternative, setting the scratch daemon's
`session_recall_cap_tokens` to the 1,500 budget, would let recall run once per recompile window and so change what
the bench measures; I did not do that.

Pins that moved with the plan (all in `test_generate.py`; none in `test_drive.py`):

- `SMOKE_7` digest `5b54c6e94b52c3ba` to `8d60136d336766a8` (the window is in the digest).
- `test_a_thin_plan_is_planned_again_until_its_written_bounds_hold`: its inputs were seed 7 at 13,599 (first plan
  44000 missed). Under the new reserve that plan holds first time, so it now takes seed 11 at 13,700 (first plan
  50000 misses, kept 51000). Same assertions on new inputs.
- `test_a_plan_holds_at_the_cushion_over_and_under_its_overhead`: the seed 8 at 13,600 case (41000 rejected, 42000
  kept) no longer exists; replaced by seed 11 at 13,686 (50000 rejected, 51000 kept, misses only at 13,736). The
  seed 11 at 13,679 case is unchanged.
- Docstrings say so. These are changed inputs of the same checks, not a loosened check; the keel guard is empty.

New tests:

- `test_generate.TheRustRule.test_a_later_session_reserves_the_recall_notes_the_tail_may_hold`: reads the cap from
  `config/memory.rs` and the pause rule from `recall_step.rs`, checks `recall_for`, `fit_bound`'s difference, and
  that the smoke's second session fits whole with the cap beside it.
- `test_drive.TheseusDriver.test_the_smoke_compacts_once_however_early_the_index_comes_up`: holds the driver 8 s
  before the first turn (the tender is up from the start of session 2), runs the smoke on the counting stand-in,
  and asserts one compaction, at the mark or the turn after, with more than three recall nodes in the sessions.
  This is the deterministic form of the owner's red; the plain smoke cannot show it on this VM.

### How I proved it

- Before: `python3 -m unittest bench.recall.test_drive`: 24 tests OK; smoke 3/3 OK (python 3.11 and 3.12).
- Diagnostic (old plan, tender up early): `[11, 23]`; (new plan): `[11]`.
- After: `python3 -m unittest bench.recall.test_drive`: all pass; the smoke 3/3 OK again; `discover -s bench/recall`:
  100 tests, 1 failure (the prices, known).
- Plant (generate.py put back as `HEAD` had it, then restored and `touch`ed, `git status` checked):
  - `test_the_smoke_compacts_once_however_early_the_index_comes_up` fails:
    `AssertionError: [11, 23] not found in ([10], [11])`.
  - the new test_generate case fails: `AssertionError: 12000 != 1500`.
  - `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing` stays green under the plant on this VM
    (no timing here makes the tender early); the new drive test above is what fails in its place. I could not
    show `moved 1 != 0` on this machine.

## Step 2: theseus-kbfl0, read-only test trees (commit 3a22a7af)

### What I found

The tree is a loaded flow: `flow/load.rs` freezes `self/mint/<name>/<digest>/` with `0o555`. As root, `rm` ignores
that, so this VM shows no leak at all; I measured as `nobody` (`setpriv`), one process per test, over every core
test, with `TMPDIR` a fresh directory (the leak check of the brief, but not through nextest).

- Before: **26 entries**: 11 `theseus-test-*` (a scratch dir whose `remove_dir_all` fails) and 15 `.tmp*`
  (the shared rig's bare `tempfile::tempdir()`, whose drop fails silently), all holding `self/mint/..` read-only
  directories (the owner counted 25).
- After: **0 entries** from core.

### What I changed

- `scratch.rs`: `remove` retries after `thaw(path)`, which gives every directory at and under the path the owner's
  `rwx`, parents first (a file is deleted by its directory's mode). Only `remove` and a helper beside it changed,
  and the new test is last in the module, so the merge with tail5-t4fix's `remove_all` is clean (I read
  `integ/tail5`; `cloud/...-tail5-t4fix` is not on the remote).
- `tests_places.rs` `rig_defer`: the dir is `crate::scratch::tempdir()`, so the exit sweep removes it after the thaw.

### How I proved it

- `scratch::tests::a_read_only_tree_is_removed` (a dir tree of three `0o555` levels, `remove`, the path is gone).
  Run as `nobody` against the built test binary: passes; with `thaw(path);` commented out it fails
  (`test result: FAILED. 0 passed; 1 failed`). As root it would pass either way, so it needs a non-root runner to
  guard; the nextest runs on the owner's machine are non-root.
- `cargo nextest run -p theseus-core scratch flow:: own_bench::whole`: 61 passed (as root).
- Leak count over every core test as `nobody`: 26 before, 0 after, as above. In that run 3 tests failed for
  environment reasons (the disk-floor refusal when the disk filled, and the L1 ones), none leaving a directory.
  An earlier "after" run (2 entries) was taken with the disk nearly full and killed two tests at my 120 s timeout:
  `tests_m3::a_stop_during_a_jobs_launch_stops_it_once_its_pid_is_written` hangs as `nobody` here and leaks its
  writable `.tmp*` when killed. That is a killed process, which the prune handles once six hours old, not this
  issue; I mention it because it showed up.

### What I could not do

- **theseusd.** The brief says `-p theseusd` left one `.tmp*`. I ran its integration binaries as `nobody` (with
  `TMPDIR` fresh): only `acceptance_journey` left one, a writable dir holding `xdg-state` only (the test needs tmux
  and ran into my 240 s timeout). `tests/extend.rs` already thaws in its `Drop`. I found no read-only fixture in
  theseusd to change, so I changed nothing there. The owner's one entry may be this one or another that needs a
  tool this VM lacks (the L1 tests fail here as root-or-nobody).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, at 3a22a7af: door, keel, keel tests, fmt, shape,
features, clippy, clippy-probe and cockpit all passed. The suite phase ran 7919 tests: 7884 passed, **35 failed**:

- 34 are the known L1 failures (the VM runs as root): 20 of `theseus-sandbox::contract`, `theseus-sandbox::bench
  spawn_100`, and 13 of `theseusd::sandbox` (`a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace`,
  `a_deadline_ends_a_job_mid_tunnel...`, `a_granted_program_in_l1_gets_its_grant_at_launch`, `the_jobs_bench_l1_row`,
  `a_job_with_no_list_has_no_proxy`, `l1_argv_routes_a_call_to_l1`, and the rest of that file).
- 1 is a flake under the gate's load: `theseus-discord tests_task_home::the_task_home_holds_the_board_reports_and_one_pointer_per_task`
  (panicked at `tests_task_home.rs:328`, a Discord post's text); rerun alone it passes (`tests_task_home`: 7/7). Not
  in the known list and not in my files; the owner may want it on the flaky list.

The first gate attempt died in its test build on "No space left on device" (the writable allowance is about 11 GB);
I freed `target/debug/incremental` and ran it again; the numbers above are the second run. The phases after the
suite were not reached because the suite failed on the known cases; `fmt` and `clippy --workspace --all-targets
-D warnings` passed inside the gate, so I count the commit green on the brief's terms. No test in `theseus-core`
failed, and no `scratch`, `flow::` or `own_bench::whole` test.

## Keel guard

`python3 scripts/keel-guard.py --base 49352e8a --head HEAD --quiet` printed nothing (exit 0). (The clone's
`origin/main` ref is older than `49352e8a`, so I gave the commit; run against `origin/main` it is empty too.)

## For the integrator

`docs/status.md` sentence: "tail6-tests: the recall bench plans a later session at the recall tail's cap (12,000, not
one 1,500 note), so a machine whose index tender comes up early no longer compacts a second time (smoke window
33000 to 47000 at a 7,345 overhead); a read-only test tree (a loaded flow's frozen copy) is thawed and removed by
`scratch::remove`, and the shared test rig's dir is a scratch one."

Docs to change (not mine to edit): `bench/recall/README.md` quotes 46000/94000 (seed 7 at 13,943) and the budget
of 30,404 and $0.54/$12.02; they are 56000/101000 now (run `generate.py` for the numbers). Its "Tests" list can add
the early-index smoke. `docs/spec` Part III's item for 5hr6i/93lkm can name the thaw.

## Follow-ups

- `theseus-core` `tests_m3::a_stop_during_a_jobs_launch_stops_it_once_its_pid_is_written` hangs when the runner is not
  root and has no cgroup: worth a look on the owner's machine (it did not hang as root).
- The recall reserve is the worst case; a measured `recall_tokens_in_tail` per turn from a live run could size the
  window tighter later.
- Disk: the VM has about 11 GB of writable space; the debug workspace build (about 20 GB with the gate's second
  profile) fills it. I deleted `target/debug/incremental` twice and lost the gate's first test build to it.
