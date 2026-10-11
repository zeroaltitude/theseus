# CLOUD_REPORT: money (theseus-3p36, theseus-l9on, theseus-jse4)

Branch `cloud/20261010-money`, cut from main at e57216c (store format 26; config-split had joined). Four commits on top
of the task commit, each gated and pushed as it went green. Started 04:50 UTC, report written 06:55 UTC.

| Step | Commit | Issue |
|---|---|---|
| A cut call booked at its cache split | 2f39275 | theseus-3p36 |
| The day ceiling: a sticky stop, and words that say what happens next | 962afc3 | theseus-l9on (a, b) |
| A hard stop at 10x a limit that notifies; a multiple noticed at the settle that crosses it | 304adb1 | theseus-jse4 |
| Each bench driver sets its daemon's day ceiling | abf0f38 | theseus-l9on (c) |

No store format bump: no stored record changed. The `provider.cut` row and the completion's `detail` gain fields, but
those are ledger JSON, not a record layout. No protocol types changed (protocol.gen untouched). No new dependencies.

---

## 1. A cut call is overcharged (theseus-3p36) — 2f39275

**What I found.** `TurnRunner::settle_cut` booked a cut call's whole `est_input` as `input_tokens`, which is plain input
at the uncached price, capped at the reservation. The turn already holds what's needed to do better: `t.last` (the
previous answered call's `Usage`, with its cache read and write) and `compiled.request.cache_control` (the conversation's
top-level breakpoint, with its TTL).

**What I changed.**
- New `crates/theseus-core/src/turn/cut_step.rs`. `Prefix::of(t.last, compiled)` takes the cached part to be the
  previous call's `cache_read + cache_creation`, since this request repeats that prompt through its conversation
  breakpoint. That part is zero on a new compilation (a recompile repeats nothing) and in a turn's first loop.
  `Prefix::split(est)` then books `min(cached, est)` as a cache read. The rest is a cache write at the breakpoint's TTL
  (1h tokens counted too) when the request carried a breakpoint, else plain input. With nothing cached it is
  `uncached`: exactly today's figure.
- `turn.rs`: the `Cut` struct gets a `prefix` field, filled where the cut is built, and `settle_cut` uses the split.
  The cap is still `min(reserved)`. These are the only lines added in turn.rs for this step; the file is at 3,449 of
  its 3,525 ceiling.
- The `provider.cut` row (`fact/turn.rs`) and the completion's `detail` carry `cache_read_input_tokens`,
  `cache_creation_input_tokens` and `split` (`uncached` | `cache_read` | `cache_read_write`), beside `estimated: true`.
- crates/theseus-core/AGENTS.md: the cut trap's sentence now says how the estimate is split.

**How I proved it.**
- `turn::cut_step::tests` (2 unit tests): the split's arithmetic for both TTLs, with and without a breakpoint, with a
  cache larger than the estimate, and with nothing cached.
- `tests_money::a_cut_call_whose_prefix_was_cached_is_booked_at_the_split_price`: first call answers billed
  `cache_read 20,000 + cache_write 3,000`, second call held until `stop_execution` cuts it. The split is `cache_read*`,
  the cost is under a fifth of the old figure (the same estimate as plain input), and the cost is ≤ the reservation.
- `tests_money::a_cut_with_no_cache_read_prices_as_before`: the first call is cut. The split is `uncached`, cost ==
  old figure capped at the reservation, and the budget's spend equals it.
- **Planted revert:** `prefix: cut_step::Prefix::default()` at the cut site. Only the cached-prefix test failed; the
  other 11 cut-related tests passed. File restored and `touch`ed, `git status` clean.

**Live check, run on this VM.** Scratch daemon (bench profile, `api_base` pointed at a Python stand-in of the Messages
API that bills the first call as `cache_read 20,000 / write 3,000` and holds the second), then `theseus ask "CUT …"`,
then `theseus stop <session>`. The `provider.cut` row:
`{"split":"cache_read_write","cache_read_input_tokens":23000,"cache_creation_input_tokens":102,"input_tokens":0,"cost_usd":0.004855,"reserved_usd":1.326244,"estimated":true,"sent":true}`.
The old figure for those 23,102 tokens as plain Sonnet input is about $0.069.

---

## 2. The daily ceiling: sticky, and true words (theseus-l9on a, b) — 962afc3

**What I found.** `DayCeiling::hold` and `fits` compared spend + held + need against the limit on every call, so a
smaller call after a refusal could pass. `Today::reached()` already read the day as stopped once a refusal had been
recorded, so the counter and health disagreed. The refusal said "raise … or wait until midnight", which reads as though
the work waits.

**What I changed (kernel `day_ceiling.rs`).**
- `State.reached_limit`: the ceiling the day was stopped under. `refuses()` is now `stopped || spend+held+need >
  limit`, where `stopped` means a refusal today under a ceiling no lower than the current one. It applies to provider
  holds, background `hold_call`s and `fits` (speech).
- The stop lifts at local midnight (`roll` resets it) or under a higher ceiling. The start already ignored a
  `spend.ceiling` row written under a lower ceiling. A start under the same ceiling that reads the row back stays
  stopped: `seed` sets `reached_limit` to the current limit.
- `Reached.since`: a refusal caused by the stop says so ("the $X daily ceiling stopped model calls at HH:MM local time
  today … so this call was not made either"). `Today.reached_at_ms` is shown only while the day is stopped.
- Words, in `REFUSAL_ENDS`, used by both forms of `Reached`'s Display: "a refused turn or task ends, it does not wait
  for the day to turn; a higher `[kernel] daily_spend_ceiling_usd` applies at the daemon's next start".
  `notice_text` (the owner's DM) and the shared-place note in `ceiling_step.rs` say the same.

**Tests.**
- New `a_refusal_stops_the_day_until_midnight_or_a_higher_ceiling` (kernel). A smaller provider call, a background
  hold and a fit are all refused after a refusal. The next local day is free. `set_limit` higher lifts the stop, and a
  refusal at the new limit stops the day again. A seeded start keeps the stop.
- Two kernel tests changed meaning with the rule. I rewrote them with no fewer assertions; the keel guard is clean.
  - `a_reservation_holds_the_day_and_one_past_the_ceiling_is_refused_unwritten` used to show "the call that fits is
    still made". It now shows that call refused (`since` set), and checks the words.
  - `a_background_hold_settles_or_lets_go` now makes its refusal last, and checks that a fit is refused after it.
- Core `a_turn_at_the_ceiling_makes_no_call_and_ends_with_the_words`: the assertion is rewritten to the new words.
- **Planted reverts:**
  - `refuses()` without `Self::stopped(..) ||`: three kernel tests failed (the new one and the two rewritten ones).
  - `REFUSAL_ENDS` back to "the owner can raise … or wait until midnight": the core words test and the kernel
    first-refusal test failed.
  - Both files restored and `touch`ed, `git status` clean.

**Live check, run on this VM.** Scratch daemon with `daily_spend_ceiling_usd = 0.01`, then two `theseus ask` runs,
both exit 1, class `daily_ceiling`, with one `spend.ceiling` row:
- First: "today's spend reached the $0.01 daily ceiling ($0 spent today, $0 held …, and this call needs $1.319266),
  so the call was not made, and no model call will be until the day turns at 2026-10-12 00:00 local time; a refused
  turn or task ends, it does not wait for the day to turn; a higher `[kernel] daily_spend_ceiling_usd` applies at the
  daemon's next start".
- Second: "the $0.01 daily ceiling stopped model calls at 2026-10-11 06:50 local time today ($0 spent today), so this
  call was not made either, …".

The second live call was the same size as the first. The kernel test is what shows a *smaller* call refused.

---

## 3. A hard stop at a multiple (theseus-jse4) — 304adb1

**What I found.**
- Under notify nothing automatic bounded a stuck loop.
- `spend_noticed` ran only after an answered call (`Called::Answered`), comparing spend before and after that call. A
  cut or failed call's settle that crossed a multiple was therefore never noticed; the next notice came at the next
  multiple. The same was true of anything settled between two loops.

**What I changed.**
- The key: `[kernel] spend_limit_hard_multiple`, default 10, 0 for never. The field is in config.rs, its default in
  `config/limits.rs` (from `theseus_kernel::default_hard_multiple`), and the template has the key plus a comment
  paragraph.
- The kernel (`KernelConfig.spend_hard_multiple`): `overdraw.rs::hard_stop`, called in `plan` beside the over-budget
  check. Under a limit that notifies (provider calls only), a call whose `spent + reserved + need` would pass
  `multiple × limit` is refused with `KernelError::HardStop(HardStop)`, and nothing is written. This is one
  comparison; no new frame.
- The core: `turn/budget_step.rs::hard_stopped` fails the turn with class `spend_hard_stop`. The class is lasting,
  so the driver retries nothing. A `LoopCut` row is written, and the failure's words come from `HardStop`'s Display:
  multiple, limit, cap, spent, held, needed, and that a higher `spend_limit_usd` or multiple lets the session go on
  from the next start.
- Each multiple's notice now ends "At 10 times the limit, $5, its calls stop (`[kernel] spend_limit_hard_multiple`)."
- Crossing: `spend_checked` replaces the old `spent_before`/`spend_noticed` pair.
  - It runs at each loop's start and right after the call's settle, whatever the outcome (answered, failed, cut).
  - It compares against `SpendMarks`, an in-memory map of execution → the spend its last check saw. A restart starts
    empty, and an execution's first check only marks it.
  - It makes the same two execution reads a loop made before.
- `TurnRunner.spend_marks` is new (one line in turn.rs; the constructor line is in rpc/mod.rs). `budget_step` is now
  `pub(crate) mod`.
- AGENTS.md (core: "Limits that notify"; kernel: `overdraw.rs`) describe both.

**Tests.**
- `tests_overdraw::a_notifying_limit_stops_at_its_hard_multiple` (kernel), with a $1 limit:
  - $8.50 spent and $1 held; a call needing $0.60 is refused, nothing written, and the words are checked.
  - A call needing exactly the remaining $0.50 fits.
  - aws.hand still goes to over-budget.
  - Multiple 0 never stops.
  - A limit that asks refuses at the limit, never at the multiple.
- `tests_money::a_notifying_session_stops_at_ten_times_its_limit`: a $0.50 limit and a model that diffs for ever.
  - At least 10 calls go out; the turn fails `spend_hard_stop` with the words; spend stays ≤ $5.
  - The refused call's reservation would have passed $5.
  - Every multiple below the stop is noticed, the first notice names the stop, and `turn.failed` carries the class.
- `tests_money::a_multiple_crossed_by_a_cut_calls_settle_is_noticed_then`: limit $0.0001, hard stop off, and the first
  call is cut. One `budget.reached` row at the cut's own settle.
- Three existing tests had limits so small that one call's reservation passes 10x. I set the multiple to 0 in them,
  since what each shows is the notice alone; their assertions are unchanged:
  - `tests_limits_notify::speech_past_a_notifying_limit_fits…` ($0.001)
  - theseusd's `headless::a_turn_past_a_limit_and_a_cap_that_notify_goes_on_and_exits_0` ($0.002)
- `tests_output`: the golden is rewritten (`THESEUS_GOLDEN=write`). The diff is two lines, the notice post and its
  narrative line, each gaining the hard-stop sentence.
- **Planted reverts:**
  - `self.hard_stop(..)?` removed from `plan`: `tests_overdraw::a_notifying_limit_stops_at_its_hard_multiple` and
    `tests_money::a_notifying_session_stops_at_ten_times_its_limit` failed. The two aws/hands "ten times" tests, which
    are unrelated, passed.
  - The after-call `spend_checked` removed: `a_multiple_crossed_by_a_cut_calls_settle_is_noticed_then` failed, and
    the other 8 passed.
  - Both files restored and `touch`ed, `git status` clean.
- FAST: the turn bench in each gate read plain 5, tool 9, batch 15 frames, all at budget.

---

## 4. The bench drivers set their own ceiling (theseus-l9on c) — abf0f38

**Places a bench starts a daemon:**
1. The Harbor adapter's two arms (`theseus_agent.Theseus`, `TheseusRouted`), through `theseus_bench.settings`.
2. bench/async's `TheseusAsync`, also through `theseus_bench.settings`.
3. bench/recall's scratch daemon (`drive.theseus_config`).

(bench/report and bench/harbor's other agents start no Theseus daemon.)

**What I changed.**
- `theseus_bench.settings` always sets `("kernel", "daily_spend_ceiling_usd")` to `day_ceiling(spend_limit)`.
  - That is `DAY_CEILING_PER_LIMIT` (2) × the trial's limit, or `THESEUS_BENCH_DAY_CEILING` when it names one
    (validated > 0).
  - It sits above the trial's limit, so exit 5 is still what ends a trial. It bounds what the limit doesn't count
    (the routed arm's Jev calls) and a daemon whose limit failed.
  - The trial limit (`THESEUS_BENCH_SPEND_LIMIT`, bench-parity's) is untouched.
  - The agents needed no code change, because `settings` reads the environment itself.
- Both bench profiles carry `daily_spend_ceiling_usd = 4.0  # set by the adapter`. The plain profile's header has one
  bullet for it.
- recall: `theseus_config(..., day_ceiling=None)` gives 2 × `--spend-limit`, or `--day-ceiling`.
- crates/theseus-core/AGENTS.md's day-ceiling line names the drivers.

**Design choice the owner should hear.** The ceiling is twice each daemon's own limit, not exactly the limit:
- At exactly the limit, the routed arm's Jev spend (counted by the ceiling, not by the session limit) would end
  trials as `daily_ceiling` instead of `spend_limit`.
- Each Harbor trial is its own daemon on a fresh store, so a "run-wide" ceiling can't be enforced per container. Twice
  the trial limit is the per-daemon bound.

If the owner wants the ceiling exactly at the run's budget, that is one constant (`DAY_CEILING_PER_LIMIT` in
theseus_bench.py and drive.py).

**Tests: each reads back the config its driver writes.**
- `test_bench.Profile.test_a_trial_sets_its_daemons_day_ceiling_explicitly` (stdlib): both profiles carry the key;
  limits 0.4/2/20 give 2× the limit, and an explicit value wins; `day_ceiling` reads the environment and refuses 0
  and -1.
- `test_bench.Routed.test_each_arm_writes_its_day_ceiling_into_the_config_it_uploads` (Harbor): `Theseus` and
  `TheseusRouted` `run()` up to the upload, with the uploaded TOML read back. Defaults, then
  `THESEUS_BENCH_SPEND_LIMIT=3` with `THESEUS_BENCH_DAY_CEILING=9`.
- `test_driver.TheseusAsyncConfig.test_the_config_it_uploads_carries_the_day_ceiling` (`ASYNC_HARBOR=1`): the same
  for the async arm.
- `test_drive.TheseusConfig…` (recall): 2× by default, an explicit value through `toml_dumps`/`tomllib`, and 0 raises.

**Suites I ran myself before the commit** (Harbor 0.23.0 in a Python 3.12 venv):
- bench/harbor: 148 OK with Harbor's python (none skipped); 148 OK with 41 skipped on stdlib 3.11.
- bench/async: 48 OK with 1 skipped (`ASYNC_HARBOR=1`); 48 OK with 12 skipped on stdlib.
- bench/report: 39 OK.
- bench/recall: 78 with **2 failures that are not mine**:
  - The failing tests are `TheseusDriver.test_the_smoke_runs_end_to_end…` and
    `test_mains_sizing_fails_the_mark…`.
  - They run only once `target/` binaries exist (earlier they skipped). Both fail on the daemon's measured overhead:
    14,015 tokens against the 13,640 the progression is planned at (cushion 50).
  - My diff touches no system prompt, tool or tools note. This drift belongs to recall-plan-fix
    (`generate.OVERHEAD_TOKENS`, last set at fdc2630).
- **Planted reverts:**
  - The ceiling line removed from `settings`: both Harbor tests and the async test failed.
  - recall's `t["kernel"]["daily_spend_ceiling_usd"] = ceiling` replaced with `pass`: `TheseusConfig` failed (plus the
    two overhead failures above).
  - Both files restored.

---

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran on each commit's tree (stages built by restoring each
step's files). The results were the same each time:
- keel, keel tests, fmt, shape, features, clippy, cockpit, test build and reader rule passed.
- The suite failed on the **33 known L1 tests** only: theseus-sandbox's contract tests, `spawn_100`, and theseusd's
  sandbox tests. These are root without a job cgroup (theseus-pv6i). Totals: 3,699/3,732, then 3,700/3,733, then
  3,703/3,736 twice.
- The phases after the suite, run by hand each time, passed: protocol types unchanged; turn bench (`--check --runs 5
  --burst 0`) plain 5/5, tool 9/9, batch 15/15, batch p50 about 2.1 s against about 2.39 s; `cargo deny --offline
  check` (advisories, bans, licenses, sources ok; `cargo deny fetch` succeeded).

**Under load** (four busy loops at nice 0, tests at nice 19): the 33 tests of this work (tests_money, tests_overdraw,
both tests_day_ceiling, cut_step, tests_limits_notify) passed 3 runs of 3.

**One failure outside the known list: `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`.**
- It failed in the first whole-tree gate and once more alone right after it. The assertion was "the live wrapper's job
  runs on: {dispatched 3, outcome_unknown 3, succeeded 11}", left 3, right 1.
- It then passed in all four per-commit gates and in three runs alone.
- It is a count of how processes were classed (wrappers), in code this work doesn't touch. I'm naming it as a finding
  rather than a flake; it did not reproduce afterwards.
- A base-tree comparison was cut short when the second build filled the disk; I freed it by deleting
  `target/debug/incremental`.

**Keel.** `python3 scripts/keel-guard.py` (with local `main` updated to origin/main, e57216c) reports `keel: ok … 0
findings` at every commit, including the bench toml spend key and the template key.

**Keel findings expected:** none.

---

## Live checks for the maintainer

Build first: `cargo build -p theseusd -p theseus`.

1. **Cut split on a stand-in.**
   - Run a scratch daemon with the bench profile, `[model] api_base` pointed at a stand-in that bills the first call
     with `cache_read_input_tokens: 20000, cache_creation_input_tokens: 3000` and never answers the second. My
     stand-in is a 60-line `http.server` script; it is not committed and I can add it if wanted.
   - Then `theseus --socket S ask "CUT diff these"`, and `theseus --socket S stop <session>` while the second call
     hangs.
   - `theseus --socket S --json ledger -k provider.cut` should show `split: "cache_read_write"`, `cache_read_input_tokens`
     around 20,000 or more, and `cost_usd` around $0.005, against about $0.07 for the old plain figure.
2. **Sticky ceiling.** Same daemon with `[kernel] daily_spend_ceiling_usd = 0.01`, then two `theseus ask`. Both exit
   1. The second says "stopped model calls at HH:MM … not made either". One `spend.ceiling` row.
3. **Hard stop.** `[kernel] spend_limit_usd = 0.05`, `spend_limit_mode = "notify"`, on a stand-in that always asks
   for a tool. The turn fails `spend_hard_stop` once spend + the next reservation would pass $0.50. With a Sonnet
   reservation of about $1.3, the very first call is refused at this limit, which itself shows the words. Use a
   profile with a small `max_output_tokens` to see several calls first.
4. **fix-git timeout repro, real model ($0.10 at most).** The Terminal-Bench fix-git task, the Theseus arm, Sonnet
   5.5, agent timeout 10.8 s.
   - `PYTHONPATH=$PWD/bench/harbor harbor run -d terminal-bench@2.0 -t fix-git -a theseus_agent:Theseus -m
     anthropic/claude-sonnet-5-5 --agent-timeout 10.8` (adjust to the adapter's usual flags).
   - Then read the trial's `provider.cut` row and the efficiency record's dollars.
   - Expected: the cut call's `split` is `cache_read_write`, its `cost_usd` is about a tenth of the old $0.016, and
     the trial's total is about $0.014 plus a few tenths of a cent.

## Left over, uncertain, and docs to change

- **The hard stop gates on the reservation.** Under notify a call's reservation (its 128,000-token output cap) can be
  large next to a small limit, so the stop can fall well before 10x spend: a $0.50 limit stopped at $3.95. This
  matches "money is a gate" and never passes 10x. If the owner prefers "stop once spend ≥ 10x", change one comparison
  in `overdraw::hard_stop`.
- `SpendMarks` is in memory. A crossing that settles while no daemon runs, or between a restart and the execution's
  first check, is noticed at the next multiple, as before. A task's carry into an idle parent is noticed at the
  parent's next loop.
- The cut split assumes the previous call's prompt is still in the provider's cache: same model, within the TTL. A
  refusal fallback between the two calls would make that wrong; it is rare, and the cap still holds.
- **Docs for the maintainer** (I did not touch the spec, status.md or design/):
  - Part III: one item for this step.
  - `docs/status.md`: the cut split, the sticky ceiling, `spend_limit_hard_multiple`, and the bench drivers' ceiling.
  - The spec's budgets section (§9/§3.x on limits, wherever budgets-notify is described): the hard multiple and the
    sticky day ceiling.
  - bench/README.md could name `THESEUS_BENCH_DAY_CEILING` and recall's `--day-ceiling`.
