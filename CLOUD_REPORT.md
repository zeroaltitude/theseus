# CLOUD_REPORT: a suite red must not hide the five phases after it (theseus-zsddv)

Branch `cloud/20261027-p3x-gate-suite-last`, base `5342a01c`.

## Headline: the keel guard is SILENT on this change

`THESEUS_KEEL_BASE=5342a01 python3 scripts/keel-guard.py` on the reorder (commit 1), and again on the reorder plus the notice (commit 2):

    keel: ok (5342a01..the working tree, 5342a01c05; 2 files changed, 0 findings acked, 0.0 s)

Reordering `machine_checks` removes no phase name and touches no keel/door line. **The belief that this row "needs the owner's words" is wrong for this change and can be retired.** No `Keel:` trailer is needed; **Keel findings expected: none.**

(Clone caveat: this clone's `origin/main` was stale at `7d39d57`, so the bare guard reported 85 findings from other people's merged work, none in gate.sh. I moved the local ref `refs/remotes/origin/main` to `5342a01` — local only, nothing pushed. The maintainer's own tree is unaffected.)

## Step 0: citations re-verified at `5342a01`

- `machine_checks` at `:671-680`: correct as the prompt states (notes' `:670-679` is off by one). `phase suite` is the second phase, at `:673`. Verified.
- `lifecycle_bench` `:340` and `jobs_bench` `:362` return early on `THESEUS_GATE_NO_BENCH`: correct.
- `turn_step` `:389` (only drops `--record`) and `size_step` `:402`, not `:403`: **off by one** (function opens at 402, its closing brace at 404). Neither skips.
- `protocol_types` `:241`: no bench character. Correct.
- `compiled_under_lock` `:659`: reads `registry.log` and `suite.log` with `2>/dev/null`. Correct.
- `keel_rule` at `~:1558` and `GATE_PHASE` at `:1555` of keel-guard.py: correct, including "a phase's command changed, or one added, is no finding".
- `MANIFEST_FORMAT` `crates/theseus-store/src/store.rs:692`: correct, 52. Not touched.
- Keel/door lines in gate.sh: `:31, :429-548, :740-749, :769`; none in `machine_checks`. Correct.
- Wrong or imprecise: only `size_step`'s line (402 not 403). Also the prompt says "`scripts/` may have a test harness for the gate"; the only one is `scripts/test_keel_guard.py` (judges the guard, not phase order), so I demonstrated on the real gate instead.
- `git log origin/main`: nothing had landed after the base that touches this (the clone only has the base).

## What I chose, and why

Commit `66ae37d`: `machine_checks` is now `registry, protocol types, lifecycle, jobs, turn, size, suite, compiled_under_lock`. Pass/fail is unchanged: `phase` still exits on the first red, so a red suite still fails the gate (asserted below: exit 100).

- **`compiled_under_lock`** counts `Compiling` lines in `registry.log` and `suite.log`. Left before the suite, it would read a `suite.log` that does not exist yet and report 0 as if that meant something. So it moves behind the suite with its log. Cost: on a red suite, the call after it is never reached, so I recover it in commit 2 (below) rather than lose the note.
- **Benches before the suite.** The suite now runs on a machine the benches just used, where before it ran before them. I think it does not matter: the benches stop their daemons, and the suite runs under any load by construction (it is its own parallel load, and its timing tests are on the known-flaky list). The benches get *better* conditions (no suite just before them); lifecycle does its own `sync` and `settle`.
- **Not built:** option (2) (continue past a named exception).

Commit `f18d00a` (option 3, separate commit): in the locked part's exit path, a failure prints `gate: NOT RUN, no verdict, after the failure in <phase>: <phases>` (from `machine_order`, kept beside `machine_checks`) and runs `compiled_under_lock`, so the compile note survives a red suite. Lines are all inside `on_exit` and beside `machine_checks`.

## Proof (real gate, same plant in both runs)

Plant: `assert_eq!(1 + 1, 3, "planted suite red (theseus-zsddv)");` inserted before line 137 of `crates/theseus-tools/src/change.rs` (test `an_edits_diff_names_its_file_and_counts_its_lines`). Restored with `git checkout` + `touch`, `git status` clean, after each run.

Command: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh`

**Base (unmodified gate.sh + plant), exit 100:**

        reader rule          5
        suite              536  <- failed here
        total             1864
    gate: FAILED in suite

No `protocol types`, `turn`, `size` (or lifecycle, jobs) row, and none of their lines in the log. The planted test failed (`theseus-tools change::tests::an_edits_diff_names_its_file_and_counts_its_lines`).

**Reordered (+ same plant), exit 100:**

        reader rule          4
        protocol types       0
        lifecycle            0      (log: "skipped (THESEUS_GATE_NO_BENCH ...)")
        jobs                 0      (log: "skipped ...")
        turn                15
        size                 0      (log: "size: skipped: no release build of 6b7858a+edits")
        suite              535  <- failed here
    gate: FAILED in suite
    exit 100

Protocol types, turn and size now each report a verdict, and the gate still fails overall. The planted assertion failed in the log. (`size` says "skipped: no release build" by its own design, a verdict from its own step.)

**Notice, with a planted failure in `theseus-core tests_registry::every_crate_is_read_by_a_binary_or_reserved` (`reader rule`), exit 100:**

    gate: NOT RUN, no verdict, after the failure in reader rule: protocol types lifecycle jobs turn size suite
    ...
      reader rule          4  <- failed here
    gate: FAILED in reader rule

Also checked in a bash harness that extracts `not_reached` and `compiled_under_lock`: failed in `turn` prints `size suite`; failed in `suite` prints nothing unrun; a `Compiling` line in `suite.log` prints the compile NOTE even when the suite failed.

**Clean tree on my change** (no plant), exit 100, only the known set (below); `protocol types`, `turn`, `size` ran:

    reader rule 3, protocol types 0, lifecycle 0, jobs 0, turn 15, size 0, suite 526 <- failed here
    FAIL: 19 theseus-sandbox::contract, 1 theseus-sandbox::bench, 14 theseusd::sandbox = 34, all L1/root (theseus-pv6i)

The first base run also showed `theseus-core tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` (a listed timing flake, theseus-b38m) once; it did not recur in the two later runs. The only reds in the clean run are the known 34, so the commit counts green per the brief (every phase after the suite ran).

## Things the maintainer should know

- **The door.** On a branch whose gate.sh differs from main's, the gate first runs *main's* copy over the tree (`phase door`), and only then the tree's own copy. At the join, main's (old-order) copy will therefore still stop in `suite` first. This change takes effect for gates started after it is on main. My demonstrations ran the tree's copy directly with `THESEUS_GATE_DOOR=own` (the state the gate is in after the door passes); a plain run was started and killed once (it was running main's copy).
- Not run here: the benches (lane-gate skip), cargo deny (the fetch finished at install but the phase is after the suite; not reached, because the suite red exits first).
- Name check: the diff has no names, no `/home/` paths. No spec, status, README or benchmark file touched. Cargo.lock untouched.

## Integrator notes

- For main: `66ae37d` (reorder) and `f18d00a` (notice). They touch `scripts/gate.sh` only.
- Not for main: `7e66bda` (task), `6b7858a` (session started), and this report's commit.
- Docs I would have written (not mine to edit): `scripts/AGENTS.md`'s "The gate" step 2 lists the locked part with the suite second; its order should read registry, generated TypeScript, lifecycle, jobs, turn, then "the whole suite, last, so its red hides none of them", plus one sentence on the `NOT RUN` line. The spec's item for this step belongs to the maintainer.
- OWNER CALL: none open.

CLOUD REPORT COMPLETE
