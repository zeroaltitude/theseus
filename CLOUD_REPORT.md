# CLOUD_REPORT: theseus-ler3c (hold-compile race)

**Result: NOT FIXED. A well-argued STOP with new findings. No production or test change is on this branch.**
Branch note: the brief names `cloud/20261027-p3y-hold-compile-race` in one place; the branch I was told to check out and used is `cloud/20261027-p3v-hold-compile-race`.

## Step 0: citations (base e461c62d, which is what the branch was cut from)
All exact, none wrong: route_step.rs:568-575 (the block is at 568-575 here, as the BASE UPDATE says), turn.rs:1909, judge/mod.rs:220 (field), :839-841 (pins_wait), :845-847 (setter), tests_route_effort.rs:294, fake.rs:246 and :251, MANIFEST_FORMAT = 52 at store.rs:692. I did not re-check tests_hold_compile.rs:155-162, :186-207 or :216-223 by number: I read the test file and the twin and steer tests are where described, with the sed ranges I used. origin/main at start was e461c62d (nothing landed after the base).

## Findings
1. **The twin is NOT clean.** The brief says `a_held_compile_failure_records_its_route` (no correction) does not flake. Measured here at 16 busy loops, test at nice -n 19, 40 repeats each, test binary run directly with `--exact`: steer 3/40 red, twin 4/40 red. Same panic in both: `context_overage` at tests_hold_compile.rs:224 (steer) / :157 (twin) with "estimated at 68,386 tokens (95,741 at the estimate's upper bound) ... 53,837 over". So the correction is not the discriminator. The twin waits up to max_wait_ms (200 ms) and also loses it under load; the steer test just has no wait at all, so it is more exposed. The race is "a 200 ms wall-clock wait for a verdict versus a loaded host".
2. At the brief's own recipe (4 busy loops), base is clean here: steer 0/40, twin 0/40 (a real result; the host measured in the brief was slower). At 8 loops base steer is 0/40. The race only shows at 16 loops, where each run takes 7 s to 500 s because nice 19 starves the process.
3. **Remedy 1 is not feasible.** The verdict is requested by the turn itself inside `run`, so it cannot be "in hand before run". `hold_when`/`release` delay a verdict but cannot make it arrive sooner.
4. **Remedy 2 alone does not remove the race.** `pins_wait()` makes `wait` true, but `beside` (route_step.rs:229-248) still bounds it by `max_wait` (200 ms by default, config-capped at 5000: `routing.max_wait_ms must be at most 5000`). Pinning only makes the steer test equal to its twin, which also flakes.
5. A wall-clock bound cannot make this deterministic. I tried both together (pin in the steer test, plus `c.routing.max_wait_ms = 5_000` in `rigged_with`, before `more(c)` so the wait test's 300 ms still wins). Results: 16 loops, 40 repeats: steer 8/40 red (same `context_overage`, runs took 144 s to 504 s: the process is starved, so even 5 s is lost); 8 loops: 0/40 with the fix, and 0/40 with it reverted. The fix is therefore **unproven**: no load level I found separates it from base. I did not run the twin cell with the fix (killed by a container restart, then superseded).
6. The production code argument stands: route_step.rs:570 (steering) -> :574 (no wait) -> turn.rs:1909 (context_overage escapes unless `t.route.held`). It is correct as designed; I recommend no production change.

## What I changed
Nothing on the branch except this report. The attempted test change (+7 lines in crates/theseus-core/src/tests_hold_compile.rs) is not committed because it is not proved. Its text:
```
// in rigged_with, before more(c):
c.routing.max_wait_ms = 5_000;
// in a_held_compile_failure_records_its_steer, after rigged(true):
r.core.runner.judge.pins_wait_for_verdicts();
```
## Recommendation
A real fix needs the wait not to be wall-clock: run these two tests on tokio's paused clock (as AGENTS.md's "prove waits for nothing on the paused clock" suggests), or give the rig a way to hold the compile's completion until FakeJev has answered. Raising reach() would still be the wrong fix.

## Proof commands
`cargo nextest list -p theseus-core -E 'test(a_held_compile_failure_records_)'` selects exactly: tests_hold_compile::a_held_compile_failure_records_its_route, ..._its_steer, ..._its_wait. Cells: /tmp/cell.sh (LOOPS busy `sh -c 'while :; do :; done'` at nice 0 by pid, test at nice 19, `--exact tests_hold_compile::<name>` on the built test binary). The discriminator (route.decided mode) was not captured: the failing runs panic at `run(...)` (the turn failed) before the rows are read.

## Live check for the maintainer
None needed; no behaviour changed.

## Keel / guard
`python3 scripts/keel-guard.py` output above was run on a tree with no changes beyond the task commit; no findings of mine (range is empty of code). cargo fmt --all --check: ok. Gate not run (nothing to gate). Known flaky lists not exercised. theseus-discord not touched.

## Integrator notes
- 62ab3625 "session started": empty, not for main.
- the cloud report commit: not for main.
- Nothing on this branch is for main.

CLOUD REPORT COMPLETE
