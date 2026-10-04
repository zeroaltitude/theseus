# Cloud report: the hands' cancels, budgets, and watching (step 40 part 2, theseus-mgw.11)

Branch `cloud/20261004-hands-cancel`, from `main` at `4bb5aac` (the task's own commit `5d1da21` on top). Started
09:22 UTC, done at 10:45 UTC (deadline 14:22). All six sub-steps are built, the grid included.

Commits, oldest first:

| commit | what |
|---|---|
| `00f8482` | steps 1–3: cancels per backend, reservations, the hour's meter and the day's budget (one commit: they share the group's frame and the poller's pass) |
| `db57a58` | step 4: overdue hands asked about first; reaped hands failed with the reaper's reason; the reaper's failures read |
| `0187611` | step 5: quotas, and waves |
| `51ba161` | store: the older-store format test reads 8 (the first gate's output was misread; this test was failing at `00f8482`) |
| `1ffc8cf` | theseusd: the kill -9 prove, a real daemon against a fake AWS |
| `b11d097` | step 6: `hands.list`, Discord's one line per group, the cockpit's grid |
| `989eb0b` | the proofs hold under load; theseus-core's AGENTS.md names part 2 |

The store's format goes 7 → 8 (a new `VerifiedBy::Ecs`, and the hour's alert mark `aws.hour.alerted.<account>`), with
the format-7 sample of a hand a cancel reached in `tests_layouts.rs`. Renumber at the merge as needed. Two tests pin
the number and moved with it: theseusd's `tests/versions.rs` (8, and a "newer" store of 9) and theseus-core's
`store::tests::a_write_moves_an_older_store_to_this_builds_format`.

No new dependency; `Cargo.lock` and the package-lock files are untouched.

---

## Step 1: cancellation per backend (`00f8482`)

**Found.** Part 1 left running hands to their TTL: a cancel or `/stop` of the `aws.hands.run` call went through
`terminate_all`'s generic path, which marked each hand `unsupported` ("runs in process"), and `until` met only
cancelled the hands never launched.

**Changed.** `aws/hands/cancel.rs`:
- **Fargate:** `StopTask` (reason `theseus: cancelled`), the cancel `acknowledged`, then `DescribeTasks` for up to 4 s;
  STOPPED (or MISSING) settles it `termination_verified` with a new `VerifiedBy::Ecs`. A stop not yet seen is left
  acknowledged (the hand stays open, so the poller keeps polling). The poller's next pass verifies it (`cancel::verify`),
  and so does ECS's own task-state change on the queue (`cancel::stopped_event`). A verified stop books the time the
  task ran at its size's rate, through a new `Kernel::cancel_verified_costing`. Plain `cancel_verified` would have
  booked $0 for a task that ran.
- **Lambda:** `cancel_unsupported` with "a Lambda invocation cannot be stopped; its function's timeout (the hand's TTL)
  ends it". Its reservation is held. Its envelope, when it comes, is `LateAfterCancel`, which books the real cost.
  `open_groups` keeps such a group open for the poller until the hand's TTL plus 5 minutes have passed, or its late
  envelope has come.
- **`until` met or unmeetable:** `group::step` stops the hands still running before it settles the group. Their cancel
  facts are written as rows. The aggregate counts them as `cancelled` (new in `Tally`, the `aws.hands.settled` row and
  the result text: "2 cancelled", each hand's verdict in words).
- **A cancel or `/stop` of the call** reaches the group through the one stop. `ToolRuntime::terminate_all` now takes
  the store, and routes `aws.hand` and `aws.hands.run` actions to `stop_group` and `stop_hands` before the generic
  path. The call's own verdict sums its hands':
  - verified when every hand that ran is STOPPED;
  - `unsupported` ("N Lambda hands cannot be stopped…") while Lambda hands run on;
  - uncertain while a task has not shown STOPPED. Each hand's own verdict follows on its own action.

  The group's record says `settled: "cancelled"`, with one `aws.hands.settled` row.
- **A late envelope after any cancel** is recorded as late and settles nothing (the kernel's `LateAfterCancel`).

**Proved.** `aws/hands/tests_part2.rs` on part 1's stateful fake, which now has `StopTask` and `DescribeTasks`, and
tasks that can be STOPPED, MISSING, or still running:
- `fargate_until_met_stops_the_running_hands_and_verifies_them`: 2 `StopTask`s, each verified `ecs`, nothing held,
  one settle, a late envelope recorded as late;
- `a_fargate_stop_is_verified_later_by_describe_tasks_or_ecs`: acknowledged, then verified by the poller's
  `DescribeTasks` for one hand and by ECS's event for the other; the poller then goes quiet;
- `lambda_until_met_marks_the_running_hands_unsupported`: no `StopTask`, two late rows, never settled twice;
- `a_cancel_of_the_call_stops_its_group`: Fargate, the call `termination_verified`, `cancelled: 2, not_launched: 1`,
  3 `action.cancel_verified` rows;
- `a_stop_of_a_lambda_group_says_its_hands_run_to_their_timeout`: `/stop`, the call `unsupported`, "2 Lambda hands".

Part 1's `a_duplicate_settles_once_and_a_late_one_settles_only_its_hand` changed with the behaviour. Its second hand
is now cancelled when the group's `first_success` is met, so that hand's envelope is late, not a settle.

**Planted revert.** I skipped `StopTask` when `until` is met (`if false && !running.is_empty()` in `group::step`).
Four tests failed:
- `fargate_until_met_…`: "a StopTask for each running hand: []", left 0, right 2;
- `lambda_until_met_…`: `(Dispatched, None)`, where `(Cancelled, Some(Unsupported))` was wanted;
- `a_fargate_stop_is_verified_later_…`: `(Dispatched, None)`, where `(Dispatched, Some(Acknowledged))` was wanted;
- with the second planted revert below, the reservation test.

I restored the file from a copy and touched it; `git status` was clean of the plant.

## Step 2: reservations (`00f8482`)

**Changed.** `toolrun/hands.rs`: each hand's action reserves its worst case, its TTL at its size's rate
(`ceil(hand_max_usd × 1e6)` micros), in the group's one frame.
- It settles at the envelope's cost, through the kernel's existing path.
- A hand that never started now completes with `cost_micros: Some(0)` instead of `None`, so nothing is held.
- `max_usd` stays the group's cap.
- A group whose total worst case passes what the session has left is not run. Its result says why, with the figures,
  and it hands the turn an `OverBudget` (`Hands::over_budget`). The turn reads it at the top of its next loop
  (5 lines in `turn.rs`) and asks the budget question with the same `ask_budget` a model call uses, card included.
  It then ends waiting on `Wake::Budget`.
- A reservation that lost a race inside the frame (`KernelError::OverBudget`) takes the same path.

**Proved.**
- `each_hand_reserves_its_worst_case_and_settles_at_its_real_cost`: two Lambda hands each reserve 20,001 µ$. After the
  envelopes, `reserved_micros` drops by exactly 2 × 20,001, `spent_micros` rises by exactly the two completions'
  costs, and nothing is held.
- `a_group_over_the_sessions_budget_meets_its_question`: 20 Fargate hands at 4 vCPU and 1 h each, against a $1.40
  limit. Nothing is planned or launched, the result says "over the session's budget… 20 hands", one `budget.asked`
  row with `needed_usd > 3`, and the execution waits on `Wake::Budget` naming the question.

**Planted revert.** I dropped a hand's reservation (`reserve` → `0` in `plan_action`). The reservation test failed:
`reserved_micros` left 0, right 20,001, with the hand's action printed. The over-budget test still passed, because
its check runs before the frame; that is expected. Restored and touched.

## Step 3: the hour and the day (`00f8482`)

**Changed.**
- **Config:** `[aws.accounts.<id>] hourly_alert_usd` (default 1.0, must be > 0) and `daily_budget_usd` (optional, not
  0), in `config/aws.rs`, the template, and the validation.
- **The hour** (`aws/hands/watch.rs`): every poller pass meters, per account, what the AWS actions dispatched this
  clock hour reserve (still running) or spent (a completion's cost; the reservation when there is none). Today only
  hands reserve, so only hands count.
  - Past the line it alerts once that hour, all in one frame: an `aws.hour.alert` row (new `LedgerKind`, with its fact
    and narrative line), the mark `aws.hour.alerted.<account>` (META, so a restart that hour stays quiet), and a
    `notice` to the operator through the outbox.
  - Health's new `AwsAccountStatus.hands` (`AwsHandsStatus` in `theseus-protocol/src/aws.rs`) carries it, and so do
    the CLI's health lines ("aws: … hands: … · this hour $0.04 of its $0.01 line (PAST IT: alerted)").
  - It alerts only. Whether it should refuse is the owner's open question.
- **The day:** `infra/aws/theseus-foundation.yaml` gains the `DailyBudgetUsd` parameter (default 0, meaning none), a
  `HasDailyBudget` condition, and `DailyBudget` (`theseus-daily`, COST, DAILY, alerts at 80% and 100% to the alerts
  topic, no action).
  - `tend.rs`'s reconcile is now per budget (`reconcile_budget(account, Which::{Monthly, Daily})`): the same change set
    of the old template with one parameter changed. For the day, `Add` and `Remove` of `DailyBudget` are allowed too.
  - A stack whose template predates the parameter stops, saying the template comes first.
  - The tender reconciles both after serving; the daily row carries `"budget": "daily"`.
  - The bootstrap passes `DailyBudgetUsd`, from the config, else the stack's, else 0.

**Proved.**
- `the_hours_alert_fires_once_an_hour`: the line at $0.01 and two Lambda hands give one row (`line_usd: 0.01`,
  `usd > 0.03`), the mark, the notice ("past its $0.01 line"), and health's block (2 Lambda running, reserved > 30,000
  µ$, `alerted_hour_unix_ms` this hour). A second group in the same hour adds no row; the test skips that check if the
  clock crosses an hour.
- `aws::tests_c2::the_daily_budget_reconciles_as_the_months_does`: 0 → 5 changes only `DailyBudget` (Add) and keeps
  the month at 50; then equal; a stack without the parameter stops with "predates the daily budget" and no change set.
- The bootstrap's two tests with stacks "as the bootstrap leaves them" gained `DailyBudgetUsd = 0`.

## Step 4: overdue and reaped hands (`db57a58`)

**Found.** The heartbeat's reconciler, which never waits on the network, would mark an overdue hand unknown with
`overdue_no_evidence`. Nothing read what the AWS-side reaper did, and its Lambda's failure records went unread to the
dead-letter queue.

**Changed.**
- `aws/hands/overdue.rs`: `overdue::Evidence` wraps the wrapper's evidence at the heartbeat (`rpc/driver.rs`) and at
  startup (`rpc/mod.rs`), and probes a hand as still running.
- The hands' own pass (in the poller, `overdue::pass(ctx, rec, now)`) reads each hand past its deadline:
  - **Fargate:** `DescribeTasks`. Still RUNNING is left for the reaper. STOPPED with the reaper's reason (`theseus ttl
    reaper…`) settles **failed**, `producer: hands:reaper`, `error: "stopped by the TTL reaper: …"`, at the time ECS
    says it ran. STOPPED otherwise, or MISSING, is unknown with what ECS said.
  - **Lambda:** unknown ("no envelope and no failure record came by its TTL and grace"); its envelope resolves it.
- ECS's state change with the reaper's reason settles the hand failed the same way, never unknown.
- The reaper's failure records (`requestContext.functionArn` of `theseus-reaper`) are read, then deleted: an
  `aws.reaper.failed` row (new `LedgerKind`), a narrative line, and health's `reaper_failures` with the last one's
  words.

**Proved.**
- `the_heartbeat_leaves_hands_to_their_own_reconciler`: the probe.
- `an_overdue_fargate_hand_is_asked_about_before_it_is_unknown`:
  - nothing is asked before the deadline;
  - two hours on, one `DescribeTasks` for four hands: RUNNING left, reaper → failed with its reason, "Essential
    container" → unknown, MISSING → unknown.
- `an_overdue_lambda_hand_is_unknown_until_its_envelope`: unknown, then resolved by its envelope (one
  `action.resolved`).
- `a_reaped_hand_fails_with_the_reapers_reason_and_its_failures_are_read`: the ECS event gives failed with "TTL
  reaper"; the reaper's failure record gives a row (`RetriesExhausted`, "AccessDenied…"), its message deleted, and
  health's count 1.

## Step 5: quotas (`0187611`)

**Changed.** `aws/hands/quota.rs`:
- Before a wave the group reads its account's quota for its backend: Fargate's On-Demand vCPU quota (Service Quotas
  `GetServiceQuota`, `L-3032A538`), or Lambda's `UnreservedConcurrentExecutions` (`GetAccountSettings`). It is read
  once an hour per account, region, and backend, and kept (`Hands::quotas`).
- `group::next` takes the cap (quota over the hand's size, at least 1) beside `concurrency` and `max_usd`, so a bigger
  group launches in waves as hands settle.
- A quota that cannot be read is logged and caps nothing; one smaller than a hand runs one at a time. It never fails
  for a quota.

**Proved.**
- `a_group_bigger_than_its_quota_launches_in_waves`: room for 3 of 5. 3 RunTasks at once, never more than 3 running,
  all 5 in the end, met, and the quota read once.
- `lambdas_concurrency_caps_a_group_and_a_tiny_quota_runs_one`: Lambda capped at 2; Fargate 2 vCPU hands under a
  1-vCPU quota run one at a time.

## Step 6: watching (`b11d097`, health in `00f8482`)

**Changed.**
- **Health's hands line:** groups open; running by backend; the oldest; reserved; the hour; the reaper (step 3).
- **`hands.list`:** a protocol read, in a new `theseus-protocol/src/hands.rs` (`HandsListParams`, `HandsGroupInfo`,
  `HandsListResult`), with one dispatch arm. The generated TypeScript is regenerated. Each group comes with a cell per
  hand (waiting, running, stopping, succeeded, failed, unknown, cancelled, not_launched), spent and reserved against
  its cap (`max_usd`, else its worst case), and its line.
- **Discord, one line per group** (`aws/hands/grid.rs`): the poller's pass posts "🖐️ 37/100 done, 2 failed, … $1.84
  of $5" to the group's session's place as a `hands` post when the line changes, and once more as the group settles.
  The binding writes it under the key `hands:<group>` (a new arm in `courier.rs`), so each new state edits the one
  message.
- **The cockpit:** the Systems view's AWS card shows the account's hands block. A new `components/HandsGrid.tsx`
  panel ("AWS hands") reads `hands.list` every 3 s while the view is open: a row per group with coloured cells, and
  spend against cap.

**Proved.**
- `a_group_is_read_as_cells_and_one_line_that_changes_in_place`: the cells `["running","running","waiting"]`, cap
  $5, the line "🖐️ 0/3 done, 0 failed, 2 running, $0.00 of $5…"; at most 4 posts for 3 hands, each different, none
  after the settle; the final "3/3 done … until met".
- Discord's `tests_outbox::a_hands_groups_line_is_one_message_edited_in_place`: four posts, two groups, two messages,
  the first edited to its last state.
- The cockpit: `npm run lint` (no new warnings), `npm test`, and `npm run build` (tsc) pass.

## The kill -9 prove (`1ffc8cf`, `989eb0b`)

`crates/theseusd/tests/hands.rs`:
- **The rig:** a real `theseusd`, the stand-in model calling `aws_hands_run` for 3 Lambda hands, and a stateful fake
  of AWS over TCP: STS, the three stacks, `Invoke`, and the queue, with SQS's visibility timeout.
- **The run:** the first hand settles, then the daemon is `kill -9`'d (its `Daemon` dropped: SIGKILL). The other
  envelopes (one twice) arrive while it is down, and the daemon restarts on the same state dir.
- **The asserts:**
  - each hand: exactly one `action.succeeded`;
  - the group: exactly one, and one `aws.hands.settled`;
  - one or two `completion.duplicate`;
  - nothing launched twice;
  - every message the restart found deleted.

**Under load** (AGENTS.md's recipe: four `while :; do :; done` loops at nice 0, the tests at nice 19, the loops killed
by their pids):
- The first runs showed two test-timing faults (fixed in `989eb0b`):
  - **The fake queue.** A kill between the first hand's settle and its delete left the message in flight. Real SQS
    redelivers it, but by then every group has settled and the poller is idle, so it waits for the next group's
    poll. The fake now redelivers, and the test says what is expected.
  - **The hour test** read health a moment before the block was set; it now waits for the block.
- After the fix:
  - `aws::hands` (35 tests): 5 of 5 rounds green under load, about 40 s each (6.5 s unloaded);
  - the daemon test: 6 of 6 under load, about 10 s each (0.3 s unloaded);
  - part 1's `a_lambda_group_…` alone: 3 of 3 under load.

---

## The live check (the maintainer's; it spends real money and waits for the owner's go)

On a scratch daemon of this build, never the operator's.
- **The config:** a copy of the operator's config with `[discord] enabled = false` and `[web] enabled = false` (never
  a copy of the bindings file).
- **The account:** in the account's config table, `[aws.accounts.<the home account>]`, keep `owner_role`, and add
  `hourly_alert_usd = 0.01`.
- **The stacks:** the hands stacks applied. Lambda needs `theseus-hands` with `HandImageUri` set.

```sh
S=~/scratch/hands-live; mkdir -p $S
# config: as above, at $S/config.toml
theseusd --config $S/config.toml --state-dir $S/state --socket $S/sock > $S/theseusd.log 2>&1 &
D=$!          # the scratch daemon's pid: kill only this one
T="theseus --socket $S/sock"
```

**1. A twenty-hand Lambda group, `until: first_success`, `max_usd: 1`.** About $0.40 reserved at worst (20 × 600 s ×
2 GB); likely under two cents spent.
```sh
$T ask 'Call aws.hands.run once with argv ["sh","-c","sleep $((THESEUS_HAND_INDEX % 7 + 5)); test $THESEUS_HAND_INDEX -eq 3"], count 20, until "first_success", max_usd 1. Then say you are waiting.'
$T rpc hands.list '{}'           # the cells: running, then failed, succeeded, cancelled
$T health                        # "aws: … hands: 20 running (20 Lambda, 0 Fargate), $0.40 reserved · this hour …"
$T ledger --kind action.cancel_unsupported -n 30   # the hands still running when #3 succeeded: "…its function's timeout…"
$T ledger --kind aws.hands.settled -n 5            # one row: met, succeeded 1, cancelled N
$T ledger --kind completion.late_after_cancel -n 30  # their envelopes, late; none settled twice
```
- **Expect:** the group settles `met`; every hand that ran on shows `cancel_unsupported`; its late envelope books its
  cost (`action.cancel_unsupported`'s count equals the late rows' count, eventually).
- **The hour's alert:** `$T ledger --kind aws.hour.alert` shows one row past the $0.01 line, health says "(PAST IT:
  alerted)", and a second group in the same hour adds no row.

**2. Fargate cancelled mid-run.** Only once Fargate hands run on the account's existing network, the other change;
never a NAT gateway of Theseus's own. A few cents.
```sh
$T ask 'Call aws.hands.run with argv ["sleep","600"], backend "fargate", vcpu 0.25, memory_mb 512, ttl_secs 900. Then say you are waiting.'
$T rpc hands.list '{}'           # wait for its cell to be "running" (20–60 s)
$T executions                    # the execution's id
$T cancel <execution id>         # or: $T stop <session>
$T ledger --kind aws.called -n 10                 # StopTask, then DescribeTasks
$T ledger --kind action.cancel_verified -n 5       # verified_by "ecs" (within a minute: the poller's pass, or ECS's event)
```

**3. The daily budget.** Show the change set; apply only with the owner's go.
- **Price first:** AWS Budgets may charge for a budget past the account's first two; check its price.
- **The template comes first.** The foundation must carry the current template, which has the `DailyBudgetUsd`
  parameter. `theseus aws bootstrap` shows the foundation's change set: the new parameter and its condition. With
  `DailyBudgetUsd = 0` it makes no budget.
- **Setting the key is the go.** The tender's reconcile applies a budget-only change set without asking, as the
  month's does (§3.7). So putting `daily_budget_usd = 5` in the account's table *is* the go. Keep it out of the scratch
  config until the owner says so.
- **Once set,** after a restart the daemon's log shows `aws: the budget's reconcile budget="daily" … changed the budget
  from $0 to $5 a day`. Health says "the day's: …", and an `aws.budget.reconciled` row carries `"budget": "daily"`.
- **Check:** `infra/aws/check.sh` (cfn-lint and the rules) on the template. I could not run it here: no cfn-lint on
  this VM.

**4. kill -9 mid-group, and a restart.**
```sh
$T ask 'Call aws.hands.run once with argv ["sleep","45"], count 5. Then say you are waiting.'
$T rpc hands.list '{}'           # all five running
kill -9 $D
theseusd --config $S/config.toml --state-dir $S/state --socket $S/sock >> $S/theseusd.log 2>&1 & D=$!
$T ledger --kind action.succeeded -n 20   # each hand's correlation id once, and the group's once
$T ledger --kind aws.hands.settled -n 5   # one row
$T shutdown
```

---

## Left, uncertain, and choices the owner should hear about

- **The hour only alerts.** Whether it should also refuse is the owner's open question; nothing refuses now.
- **A group over budget** asks the session's budget question through an in-memory hand-off read at the turn's next
  loop (`Hands::take_over_budget`). If the turn ends some other way first, the hands call's own result still says it
  did not run and why.
- **A group's call verdict while a Fargate stop is unseen** is `outcome_uncertain`, since a settled action is never
  upgraded. Each hand's own `termination_verified` follows on its own action.
- **The hour's meter** counts a hand at its completion's cost, else its reservation. A Fargate hand whose stop was
  verified has its real cost only in the session's budget (`cancel_verified_costing`), so the meter counts its
  reservation. That is conservative for an alert.
- **Scans that grow with history.** The poller's pass scans the `aws.hands.group.*` META prefix:
  - for health and the meter, groups open or begun in the last 13 hours;
  - for late-listening (a Lambda hand cancelled `unsupported`);
  - for `hands.list`.

  It runs after serving, only while hands are open, and is small today. An index of recent groups would bound it.
- **The reaper's failure records** are read only while a group is open, since the poller polls only then. Until then
  they wait in the queue, which keeps messages 14 days.
- **The quota caps each group by itself**, not by the account's other running groups.
- **Discord's line** is an outbox post per change: up to about one per settled hand, but always one message, edited
  in place. A time-based coalescing could cut the posts.
- **No `theseus hands` CLI command.** `theseus rpc hands.list '{}'` works, and the cockpit is the reader.
- **Docs for the maintainer to write:**
  - `docs/design/aws-toolset.md` §3.3:
    - the Observatory's grid is now the cockpit's Systems view;
    - the reconciler: the heartbeat defers hands to their own reconciler, which asks `DescribeTasks`;
    - a reaped hand is failed, not unknown;
  - §3.7: the daily budget, and the hour's meter is Theseus's;
  - §5 "Step 40": done;
  - the spec's Part III item and `docs/status.md` (step 40 complete; the format number).

## The gate

The gate ran three times on the way and once on the final head, `989eb0b`.
- **Before the suite, all ok:** fmt, shape, features, clippy, the cockpit (lint, test, build), and the test build.
- **The suite:** 2025 tests, 1991 passed, 34 failed, 17 skipped.
  - **33 are the root VM's** (theseus-pv6i, known): 20 in `theseus-sandbox` (its contract tests and its bench's
    `spawn_100`) and 13 in `theseusd::sandbox`. The same 33 failed in every gate run here.
  - **1 is this VM's clock:** `theseus-core tests_output::the_cores_output_matches_its_golden`. Its only diff is a
    wake's time offset, `+#:#` here (UTC) against `-#:#` in the golden. With `TZ=America/Los_Angeles` it passes, 2 of 2.
- **After the suite, run by hand:**
  - protocol types: clean; the generated TypeScript is committed;
  - deny: advisories, bans, licenses, and sources ok, offline, after `cargo deny fetch`;
  - benches: off (`THESEUS_GATE_NO_BENCH=1`).
