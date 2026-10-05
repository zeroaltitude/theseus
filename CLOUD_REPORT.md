# Cloud report: learning-fixes (theseus-m5az, theseus-bgg5, theseus-ag0t, theseus-vh67, theseus-u4t3)

Branch `cloud/20261005-learning-fixes`, from `b6fdde55` (main at `60b43fb6` plus the task commit). Started
20:22 UTC, report at about 22:10 UTC. Five commits, one per step, then this report. Store format unchanged (no
stored record changed). No new dependencies.

| step | issue | commit |
|------|-------|--------|
| 1. one `right` | theseus-m5az | `09378382` |
| 2. the audit's request off the low thread | theseus-bgg5 | `1e57fae4` |
| 4. the canary's control keeps its parent (test) | theseus-vh67 | `8d623cb5` |
| 5. a declined move keeps the window (test) | theseus-u4t3 | `0b38c8ee` |
| 3. learned loop versions in the prove | theseus-ag0t | `df4f73fa` |

Steps 4 and 5 are committed before 3 because they touch no code; 3 is the largest and shares tests_prove.rs with 5.

## Step 1: one `right` (theseus-m5az), `09378382`

**Found.** As the brief says: replay.rs's `right` took `graded`'s second value, which for a Noul is the label's
truth, not whether the lean met it. propose.rs had the correct rule (`Top::Noul(lean) => lean == x`) privately.

**Changed.** `report::right(a, t)`, beside `graded`, is the one rule; propose.rs's copy is gone (it imports it), and
replay.rs's `right` calls it. `graded`'s doc now says it returns the calibration pair, and for a Noul the label's
yes or no. propose's tests are unchanged.

**Proved.** `tests_replay::a_noul_that_leaned_against_its_label_is_an_error_the_candidate_fixes`: the `seeded`
fixture (now `seeded_with`, which takes extra labels) plus gull's `announced_unfinished` labeled yes (incumbent
0.2: an error) and tern's labeled no (incumbent 0.2: right). The fake answers gull 0.8, the rest 0.2. It checks
gull fixed on `announced_unfinished`, tern unchanged, totals fixed 2 and broken 1, and `errors: true` selects
exactly {heron, gull} with 2 fixed.
- Planted revert (replay's old `graded(..).map(|(_, r)| r)`): the test fails, `left: ([], [])`,
  `right: (["announced_unfinished"], [])`. Restored and touched.

## Step 2: the audit's request off the low thread (theseus-bgg5), `1e57fae4`

**Found.** `audit_run` polled `provider.stream_message` with `rt.block_on` on its `learning` thread at nice 19.
**The backfill:** it sends through replay's `Caller::ask` (backfill.rs line 248), which is already
`rt.block_on(rt.spawn(..))`. It needed no change.

**Changed.** A helper, `send_on_runtime(rt, &provider, request)`, clones the provider's `Arc`, moves it and the
request into a task on the runtime, and waits for it on the low thread. The task's `JoinError` becomes the
request's error, which is counted as `failed` and booked at its reservation, as any failed request is. Reading the
answer, the labels and the frame all stay on the low thread. I made it a helper because inline it took
`audit_run` past clippy's 100 lines. The module doc gained an "Off the low thread" bullet.

**Proved.** `tests_audit::an_audits_requests_are_polled_off_its_low_thread`. A stand-in provider (`Niced`,
wrapping `FakeProvider`) records `getpriority(PRIO_PROCESS, gettid())` of the thread that polls each request. A
3-request audit must record `[base; 3]`, where `base` is the test thread's own nice value (0, unless the test
itself runs niced; under `nice -n 19` both builds read 19, so the test proves nothing there, and its doc says so).
- Planted revert (`rt.block_on(async move { .. })` on the low thread): fails, `left: [19, 19, 19]`,
  `right: [0, 0, 0]`. I proved it twice, before and after moving the code into the helper. Restored and touched.

## Step 3: a learned loop version is not "never judged" (theseus-ag0t), `df4f73fa`

**Changed.**
- `learning/prove.rs`: `Input.learned`, the names of loop.v1's learned versions
  (`self.runner.judge.lineage().names_of_root(&self.store, LOOP_PACK)`, filled by `prove_input`). `build` collects
  the sessions a learned version judged, and `arm_of_task` leaves those tasks out as `learned_version`
  (`LEARNED_VERSION`). It checks after `cancelled` and before `never_judged`. A replay candidate such as
  `loop.v2`, which is not in the lineage, still says nothing (case g in the existing fixture is unchanged).
- **The mixed case** (both loop.v1's and a learned version's judgments in one task) is left out as
  `learned_version` too. Why: the prove compares loop.v1's canary with its control, and a mixed task's stops were
  not all loop.v1's. `false_completion` and `success` read "the last judgment", which would be loop.v1's even
  where the learned version judged the actual last stop. So the record would attribute to loop.v1's arm an outcome
  another version shaped, the same reason `both_arms` is left out. A mixed task arises when a placement changes
  mid-task (a learned shadow placed, rolled back, a canary started).
- `rpc/judge_prove.rs`: `prove_window` now reads `pack:loop`'s `pack.mode` rows once, whatever the branch (before,
  the `since` branch read none). The default window is chosen exactly as before (the latest loop.v1 `canary` row
  not declined, `rfind` over the same order). Then `learned_placed` appends, when a learned version in the lineage
  was placed (`shadow`, `canary` or `live`, not declined) inside `[since, until]`:
  `; a learned version stood in loop.v1's place: loop.v101 moved to shadow on 2026-10-05`, with
  `, the latest of N placements` when there were more. With none, the line is byte-for-byte today's.
- theseus-protocol `judge_runs.rs`: `left_out`'s doc lists `learned_version`; `cockpit/src/protocol.gen/
  JudgeProveResult.ts` is regenerated (that doc comment only).

**Proved.**
- `tests_prove::a_task_a_learned_version_judged_is_left_out_as_learned`. A `pack.version` row for `loop.v101`
  (helper `learned_loop`, then `lineage().forget()`). A loop.v101 judgment for ses_e moves e from `never_judged` to
  `learned_version`, and every other record and reason is as before. Then a loop.v101 judgment for ses_g, already
  judged by loop.v1 in its canary (the mixed case), moves g out of the records: `learned_version` 2, through
  `judge.prove` too.
- `tests_prove::the_window_names_a_learned_version_placed_inside_it`. With no placement the line is exactly
  today's; a declined loop.v101 row changes nothing; a shadow placement is named with its day; a canary placement
  after it is named, with "the latest of 2 placements"; and once loop.v1's own canary move comes after both, the
  window opens there and the line names neither.
- Planted reverts: build with learned judgments dropped (`false && learned.contains(..)`): the first test fails at
  its `left_out` assert. The window's append removed: the second fails. Both restored and touched.
- Today's outputs hold: `the_ledger_gives_exact_records`, `the_method_is_the_generator_over_its_records` and the
  core's golden pass unchanged.

## Step 4: the canary's control arm keeps its parent (theseus-vh67), `8d623cb5`

In `a_live_parents_candidate_below_the_minimum_goes_to_the_canary`, once loop.v101's canary is placed, the test
finds `ses_dock<i>` ids, one in each arm (`learn::arm(id, "loop.v101", 0.2)`). It asserts `placed` gives loop.v101
for the canary and loop.v1 for the control, and that the loop end's dispatch, `JudgeService::plan_loop_end` (what a
turn's end calls to choose the version and mode), names the matching pack.
- **A choice to note:** I used `plan_loop_end` for "a turn in each session" rather than whole turns. A real turn
  makes its own session id, so landing one in each arm would mean running turns until both arms appear. The rig's
  scripted provider holds a fixed script that the writer also reads, so that would have been brittle.
  `plan_loop_end` is the decision the turn's end makes (the existing shadow-parent test uses it the same way).
- Planted revert (`if s.rung == Rung::Canary && false` in `placed`): this test fails (at the control's `placed`
  assert, line 390). The other learn-loop, ladder and lineage tests still pass, as the issue said. Restored and
  touched.

## Step 5: a declined move keeps the window (theseus-u4t3), `0b38c8ee`

In `the_default_window_is_the_canarys`, after the accepted move and the late task, the test writes a loop.v1
`pack.mode` row with `mode: "canary"` and `declined: true`, then checks that `since_ms`, the window line and the
task count (1) are unchanged.
- Planted revert (`!m.declined` removed): the test fails at that assert. Restored and touched.

## Proof overall

- Targeted suites (replay, audit, prove, learn loop, ladder, backfill, learning, `learning::`, lineage): 63 of 63
  passed.
- **Under load**, tests_learn_loop, tests_prove, tests_audit and tests_replay (30 tests) at `nice -n 19` beside four
  busy loops at nice 0, three runs: 30/30, 30/30, 30/30 (23.6 s, 25.9 s, 27.1 s). An earlier attempt at the same
  runs failed 8 tests at once after about 13.7 s each in its third run. That was this VM's disk filling: a second
  target directory I had made to check the intermediate commits, about 26 GB, put the session past its disk
  allowance. That target dir and the main target's `incremental/` were deleted, and the three clean runs above came
  after. Not a finding in the code.
- Each intermediate commit passes `cargo clippy -p theseus-core --all-targets -- -D warnings` on its own tree. The
  full gate ran on the final tree, which is identical to `df4f73fa`'s.
- Python under bench/: untouched.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree: fmt, shape, features, clippy,
cockpit, test build and the reader rule passed. The suite ran 2,793 tests: 2,760 passed, 21 skipped, and 33 failed,
all of them the known L1 failures of a root VM (theseus-pv6i): 20 theseus-sandbox `contract` tests, theseus-sandbox
`bench spawn_100`, and 12 theseusd `sandbox` tests. No other failure, and none passed only on a retry. I then ran
the phases after the suite myself:
- protocol types: clean once `cockpit/src/protocol.gen` was staged, which the commit does;
- `cargo deny --offline check`: advisories, bans, licenses and sources all ok;
- the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`): frames_plain 5 against a budget of 5,
  frames_tool 9 against 9, ok;
- the lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.

No compile ran under the lock.

## The live check (the maintainer's)

On a scratch daemon of this branch's build, then again of main's, on a fresh state dir. Discord and the web off, the
judge on (the Jev key, a few cents, with the owner's go), and the model on the stand-in at a host name.

```sh
S=/tmp/lf-scratch && mkdir -p $S
cat > $S/rules.json <<'EOF'
[{"when": "Reply with one JSON object.", "text": "{\"work_state\": \"progressing\", \"announced_unfinished\": false}"},
 {"when": "", "text": "Done: the herons are counted."}]
EOF
theseus-sim fake-model --addr 127.0.0.1:9448 --rules $S/rules.json &   # note its pid
theseusd example-config > $S/theseus.toml
# Edit $S/theseus.toml: [model] api_base = "http://localhost:9448" (a host name: its lookup goes through the
# blocking pool); [secrets] anthropic_api_key = "env:FAKE_KEY" and the Jev key as the owner keeps it;
# [discord] enabled = false; [web] enabled = false; [judge] enabled = true.
FAKE_KEY=sk-fake-0000 theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &   # note its pid
for i in 1 2 3 4 5 6; do theseus --socket $S/sock ask "Count the herons on pier $i."; done   # loop.v1 judgments
```

Check 1, the audit's threads:

```sh
theseus --socket $S/sock judge audit loop.v1 --profile <the live profile, e.g. sonnet> --sample 5
ps -L -o tid,ni,cls,comm -p <daemon pid>      # during the audit (run it in a loop) and after it
```

The audit should ask 5 and write labels. No thread but `learning` shows NI 19 at any point. Main's build can show a
tokio pool thread at 19 for the 10 s of its idle life after the audit.

Check 2, the prove on main's build and on this branch's, over the same state (stop one daemon, start the other on
the same `--state-dir`):

```sh
theseus --socket $S/sock judge prove > $S/prove.<build>.out 2> $S/prove.<build>.err
diff $S/prove.main.out $S/prove.branch.out                                   # empty
diff <(grep -v 'built in' $S/prove.main.err) <(grep -v 'built in' $S/prove.branch.err)   # empty
```

No learned loop version exists there: no `learned_version`, and the window line is the same. Stop each daemon with
`theseus --socket $S/sock shutdown`, and kill the fake model by its pid.

## Left, uncertain, and for the owner

- **The mixed case** (step 3) is my decision: left out as `learned_version`, for the reason given above. The other
  choice, building from loop.v1's judgments alone, would keep more tasks but attribute to loop.v1 outcomes another
  version shaped.
- **The window line names only the latest placement and a count**, not each one. A nightly loop could place many
  versions, and the line stays one line.
- A learned version in **shadow** stands in loop.v1's place (`placed` returns it), so loop.v1 judges nothing in
  those sessions. While one is placed, the canary's prove loses those tasks to `learned_version`. This is as the
  design has it, but the owner may want to know that placing a learned shadow during a canary thins the prove.
- **Docs to change (the maintainer's):**
  - theseus-core AGENTS.md, "The prove" bullet: a task a learned loop version judged is `learned_version`, and the
    window line names a learned placement.
  - docs/design/m5-judgment.md §2.9 (the prove's left-out reasons): add `learned_version` and the mixed rule.
  - The audit bullet in AGENTS.md ("Replay, audit, and backfill"): its requests run on the runtime, waited for on the
    low thread (theseus-bgg5).
- `tests_audit`'s nice test proves nothing when the test process itself runs at nice 19 (both builds read 19
  there); the gate does not nice it.
