# CLOUD REPORT: precious few hard gates (theseus-lyxn7, theseus-nmod9)

Branch `cloud/20261026-p3p-gates-inventory`, base `78fcfb6d`. Commits: `6007f94b` (start), `1b182e81` (the inventory),
`a3cd26d3` (the one change). Store format and kernel SCHEMA untouched (51, 2). No dependency added. No AWS call, no
benchmark, no model call. `SELF_GATES_INVENTORY.md` belongs under `docs/design/`: the maintainer moves it at the join.

Note: the clone's HEAD was `e3c7d0e0` (install #89, which includes integ/p3-o); I branched from `78fcfb6d` as told.
`docs/design/o1h3f-design.md` does not exist at either commit; I worked from the code.

## Already done (found in code, not redone)
- **Confirmed your three.** (4) misroute guard: `toolrun/misroute.rs:6,9,261,263-264`, tests `:362,372,387` (theseus-g85jy, #88).
  nmod9 (1) `of_slow`: `rsi/backlog/read.rs:351-359`, only `turn.first_token` against `FIRST_TOKEN_BUDGET_MS` at `:22`;
  no `TURN_BUDGET_MS`/`turn.slow` anywhere. (1) depth: `DEPTH_REFUSAL` 8 refs on base, 0 on `p3n-task-depth` (depth 10,
  `[kernel] task_depth`, a chain walk bounded at 64; branch has no report, so I read its diff). Not joined.
- **Found done that the prompt did not list:** question TTL becomes a notice interval (`rpc/confirms.rs:550-595`,
  theseus-v2dnc, #88); a model-written task's layer 1 applies at once (`task_graph/tools.rs:312-352`,
  `theseus-protocol/src/tasks.rs:274`, `task_self.rs:88-92`) = lyxn7 (2); lyxn7 (7) task tools load on turns with no owner
  input (`turn/defer_step.rs`, theseus-o1h3f.3, #89 = on main, not in the base); lyxn7 (5) arrangement quote may be a tool
  result or the model's own line (`arrangement.rs`, #89) and is validation, not a wait: kept; `max_loops` default is
  notify (`config/limits.rs:67-77`, `turn/budget_step.rs:141`); proc.run 3,600 s (spntl); may-build wait (ccik8); wake count
  20 on main (o1h3f.8; base 5); admission for a person (`p3n-admission-notify`), halt-on-infra, door-fetch, timeout-switch2
  (all branches, not joined).
- **lyxn7 (3), a task cannot set up a routine: NOT settled by task-depth** (neither file is in its diff). Open; this row's change.

## Inventory
Commit `1b182e81`. Counts: section A, 15 rows done; section B: keep 25, to change 4, optional/owner call 2, unknown 0.
Cited anchors at base: `config.rs:804-805,874` (stuck 600), `:836,880` (deadline 600), `:883` (confirm ttl 900),
`:871` (ceiling 8), `:877` (heartbeat 60), `:451-452` (job_wait, defaults 1800/600 in `config/limits.rs:171,178`),
`:1109-1113` (max_loops 40, notify), `rsi/tree.rs:125` (GATE_LIMIT 2 h, used `:491,544`), `:131` (MAY_BUILD_WAIT 30 min, a
notice interval, `:590,598` region): all as cited. `default_deadline_secs` is not a cut: the reconciler probes past it
(`kernel.rs:2719`) and `toolrun.rs:1266` always passes the tool's own deadline.

## The change (`a3cd26d3`): a task may propose a routine
- Owner quote in the commit: "i want there to be precious few hard gates /anywhere/ in theseus" (2026-10-25 13:58).
- Before: `routine_tool::run` returned "a task cannot set up a routine" **after** the owner said yes, so a task raised a card it
  could never fulfil. Now the card stays (`routine.create` always waits for the owner) and the yes makes the routine.
- Red on the base: `tests_routine_task::a_task_proposes_a_routine_and_the_owners_yes_makes_it` failed:
  `assertion left == right failed: the owner's yes made no routine: []  left: 0 right: 1` (the base's own refusal, shown by running the
  new tests before the edit). Green after; `tests_routines` all pass with it (10/10).
- Guard tests: `a_task_s_routine_waits_on_the_owner_and_a_no_makes_nothing` (nothing exists before the answer, a no makes none).
- Not changed: `project/create.rs:184` (a task's project weekly review, same wording); say if the owner wants it too.
- Filter proof: `cargo nextest list -p theseus-core -E 'test(/tests_routine_task/)'` listed both tests.
- No planted revert beyond the base-red above (the base is the revert).

## Decided not to change, and why
`confirm_ttl_secs`, `max_loops`, progress steer: already notify / progress-based. `default_deadline_secs`: not a cut. `job_wait`
and `fan_out` caps: a return, not a stop. Model-call whole-call (1,800 s live stream), the 2 h self-build clocks, wake and task
admission failure, `proc.run`'s clock-only hour: clock-only stops that should become progress-based, but each is in a file another
row edits (`turn/retry_step.rs`, `rsi/tree.rs`, `turn/admission.rs`) or in the job wrapper the sandbox shares (L1 tests fail on this VM).
A task's repeating wake (`wake.rs:69`): left, OWNER CALL.

## Money backstops verified intact (kernel tests, 27 passed, `-E 'test(/tests_runaway/) | test(/tests_day_ceiling/) | test(/tests_overdraw/) | test(/tests_place_limit/)'`)
burn line (`tests_runaway::a_session_that_burns_its_line_in_ten_minutes_is_stopped_and_resumes_on_the_answer`), day ceiling
(`tests_day_ceiling::a_refusal_stops_the_day_until_midnight_or_a_higher_ceiling`), daily runaway
(`the_runaway_line_stops_the_day_and_holds_until_midnight`), notifying limit and hard multiple (`tests_overdraw::*`), AWS lines
(`an_aws_reservation_past_the_limit_is_still_refused_under_the_soft_settings`). I changed nothing near them.

## Could not verify / what unblocks
Joined state of `task-depth` (no report, not joined); admission, timeout-switch, halt-on-infra, door-fetch (under review): their
rows say "branch". Joining them lets the maintainer flip those rows to joined.

## Ceilings
`turn.rs`, `toolrun.rs`, `telemetry/tests.rs`: not touched (3,510 / 2,499 / 2,500 at base, same now). No ceiling raised, no
nextest flaky entry. `lib.rs` gained one `mod` line.

## Keel
`python3 scripts/keel-guard.py --quiet` printed nothing, exit 0, before each commit. **Keel findings expected: none.** (No cap,
test, assertion or ceiling changed; the owner quote is in the commit for the loosened refusal.)

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (after `cargo fmt` fixed the sorted `mod` line, which the first run
caught): door, keel, fmt, shape, recall tests, features, clippy, clippy-probe, cockpit, test build, reader rule all passed. Suite:
8,627 tests, 8,593 passed, **34 failed, all L1 on this root VM** (19 `theseus-sandbox` contract/bench `spawn_100`; 14 `theseusd::sandbox`;
one of them `a_host_beyond_the_list_once_approved_is_outside_text` took 30 s). No other failure; no negative assertion failed.
Phases after the suite: protocol types (no diff in `cockpit/src/protocol.gen`) passed; benches skipped (`THESEUS_GATE_NO_BENCH=1`).
Counted green per the preamble. Inventory commit was pushed before the full gate (Markdown only, keel guard clean); the gate ran
on the tree with the change.

## Live check for the maintainer
On a scratch daemon, in a task session ask the model to set up a routine: `theseus work list` should show a `routine.create` card
for the task; `theseus answer <id> 1` should then make it appear in `theseus routine list`.

## OWNER CALL
1. A task's repeating wake (`wake.rs:69-75`). 2. The project weekly review from a task (`project/create.rs:184`).
3. The live-stream 30-minute whole-call and `proc.run`'s one-hour wall: make them progress-based in a follow-up row.
