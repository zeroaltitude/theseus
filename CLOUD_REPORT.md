# CLOUD_REPORT: money-soft (theseus-9e02)

Branch `cloud/20261011-money-soft`. Four work commits (below), then this report.

## 1. What I found

- The day ceiling lived in one counter (`theseus-kernel/src/day_ceiling.rs`) with one bar: the ceiling. I made the bar a
  function of the mode: the ceiling under `stop`, the runaway line under `notify`. The money row's sticky rule, its
  words and its tests then apply unchanged to whichever bar is in force.
- A warning needs a post, a row and a narrative line, none of which the kernel can write. The money row's
  `spend_checked` (one comparison before each call and after each settle) is the hook: `day_warned` rides there, so a
  warning adds no frame (its row and DM post go in the turn's next frame).
- The "push" in the brief has no global-event channel (`Event` is per session). The warning reaches the TUI/herdr/Discord as a
  session narrative line plus the DM post; health and the Money view read the counter directly.
- `README.md` and `docs/status.md` still say "at the limit, Theseus stops and asks you" (README lines 64 and 210). The
  CLOUD_TASK rules forbid me to edit them, so the proposed text is in section 6.

## 2. What changed (commits)

- `7338dfd7` bench: harbor profiles and `settings()` (so bench/async's arms too) and bench/recall's driver set
  `daily_spend_ceiling_mode = "stop"`; each config test asserts it. (Tightens; its own commit.)
- `8baa20ab` the mechanism, default still `stop` in this commit:
  - kernel: `DayCeiling` modes, runaway bar, warning levels (`take_warning`), per-execution 10-minute burn ring
    (`burn_record` at the settle, `burn_stop` at the plan, `KernelError::BurnRate`), `reset_budget` resumes a brake
    question (spend kept, window emptied).
  - core: config keys, `spend.day_warning` ledger kind and fact, `day_warned`, runaway words and class `daily_runaway`,
    `Called::Braked` and the brake's question words, the start's read of today's warning levels, health's `day_spend`,
    `budget.list`'s `mode/runaway_usd/step_usd/past`.
  - AWS: `[aws.accounts.<id>] monthly_warn_usd` (default half of `monthly_budget_usd`, else half the stack's limit),
    warned once a month from the six-hourly budget read (`aws/month_warn.rs`, row `aws.month_warning`, DM post).
  - CLI health first line (`render/day.rs`), `theseus budgets`, cockpit Money view (`lib/budgets.ts`, `Budgets.tsx`),
    protocol TS regenerated, template and both AGENTS.md ceiling paragraphs.
- `2aa76f6d` **the keel commit**: `daily_spend_ceiling_mode` defaults to `notify`. Carries the `Keel:` line with the
  owner's 09:25 quote. The tests of the stop mode (day ceiling, compaction, consolidation, Discord gateway) name `stop`.
- (4th) the kernel test pinning that an AWS reservation past the limit still refuses under the soft settings.

Store format: no bump. Nothing stored changed: the new row kinds are ledger rows, the new protocol fields are optional,
the question's `args.call.brake` is inside an existing JSON value, and the new META key (`aws.month_warned.<account>`) is
a new key.

## 3. Proof

- `cargo nextest run --workspace --no-fail-fast`, final tree: 3943 run, 3910 passed, 33 failed, 46 skipped. All 33 are the
  known L1 root failures (theseus-sandbox contract and `spawn_100`; theseusd `sandbox`). On the first run
  `theseus-kernel::tree a_wrapper_that_never_answers_is_killed_with_its_group_and_uncertain` also failed under load; alone it
  passes, with and without my changes.
- New tests: kernel `tests_runaway.rs` (6: notify makes the call; warning once per level, restart read-back, none under stop;
  runaway stop sticky until midnight, 0 never; burn at the line and not past the window; approved brake keeps spend and
  empties the window; AWS reservation still refuses), core `tests_day_notify.rs` (4: warn once with row, DM post, narrative,
  Money view and health; one warning per step; runaway words and class daily_runaway with its own post; brake pause, question
  words, resume), core `aws/hands/tests_part2.rs` (AWS group still asks and runaway mode still refuses under the soft
  settings), `render/day.rs` (the health first line), `budgets.rs` (the CLI line), cockpit (`budgets.test.ts`: 323 pass).
  The money row's stop-mode tests pass; only their config now says `stop` (the helper in `tests_day_ceiling.rs`, and one
  line each in compaction, consolidation and the Discord gateway test).
- Planted reverts (each restored and `touch`ed): warning never taken -> `the_warning_comes_at_the_ceiling_and_each_step_once`
  fails; the burn window never expires -> the burn test fails; runaway stop not sticky -> `the_runaway_line_stops_the_day...`
  fails; `bar()` ignoring notify -> 3 of 4 `tests_day_notify` fail; `overdraws_for` letting AWS reservations overdraw -> the
  new kernel AWS test fails (the core AWS tests alone did not catch it, which is why the kernel test exists).
- Python: bench/recall 87 OK; bench/harbor 184 OK and bench/async test_driver 26 OK, both also under a Harbor 0.23.0 venv with
  `ASYNC_HARBOR=1` (python3.12).
- fmt, shape and clippy `-D warnings` clean; `cargo deny check` ok; cockpit lint (warnings only, none mine), test and build ok.

## 4. Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in its **keel** phase on findings that are in the branch's
base, not mine (a clone has no signed acks: "scripts/keel-signers lists no key at the base"): `theseus-tui` assertion
count, two `too_many_arguments` allows, `long-files.txt` lines for `theseus-protocol/src/lib.rs` and `theseus/tests/golden.rs`.
I ran the later phases by hand instead (fmt, shape, features is built by the workspace build, clippy, cockpit, the suite, deny).
The brake and the runaway work needed no change to `long-files.txt`; `turn.rs` grew by about 16 lines under its 3525 ceiling.

**Keel findings expected** (mine):
- `crates/theseus-core/config/theseus.example.toml`: cap-raised, `daily_spend_ceiling_mode` (the code's default) -> `"notify"`.
  Why: the owner's 2026-10-11 09:25 instruction; in commit `2aa76f6d`, with the quote as the `Keel:` line. It is unsigned, so
  the owner has to ack it.
- The bench change tightens (`stop`); the AWS lines are not touched.

## 5. Live check (done here, and for you to repeat)

I did it on a scratch daemon (bench profile, `fake-model` stand-in, `daily_spend_ceiling_usd = 0.01`, `notify`):
- `daily_runaway_usd = 5.0`: `theseus ask hello` is made (`$0.0247`); `theseus health` line 1 is
  `⚠ model spend today $0.02, 2.5× the $0.01 daily ceiling: the work goes on, model calls stop at $5.00 (...); stop now with
  `theseus stop``; the ledger holds one `spend.day_warning` (`level 1`); `theseus budgets` says `day ceiling: PAST ... warns,
  calls go on`.
- Restart on the same state with `daily_runaway_usd = 0.02`: the next ask fails `daily_runaway` with "today's spend reached the
  $0.02 daily runaway line ..."; a second ask is refused with "the $0.02 daily runaway line stopped model calls at ... so this
  call was not made either"; health line 1 is the `⛔` stop; the ledger holds one `spend.ceiling` at `limit_usd 0.02`.
Repeat: `theseus-sim fake-model --addr 127.0.0.1:9449 --rules '[{"when":"","text":"ok"}]'`-style rules file, a copy of
`bench/theseus-bench.toml` with `api_base` and the keys above, `ANTHROPIC_API_KEY=x theseusd --config C --socket S --state-dir D`.
Not checked live: the owner's DM post (no Discord here; the row says `posted:false`), and the brake's card in Discord/web
(covered by `a_burning_session_is_paused_and_asks_to_resume`).

## 6. Left, uncertain, and docs to change

- **Docs for you to change:** `README.md` lines 64 and 210 and `docs/status.md` should say: "every model call reserves its worst
  case before it runs; the daily ceiling warns, and a runaway stops (the day at $2,000, a session at $200 in ten minutes); AWS
  spend still stops at its lines." Part III should record the keel flip and the brake. `docs/technical-overview.md` was not read.
- The warning is only taken at a turn's checks (before a call and after its settle), so spend by background work alone
  (judge, consolidation) shows in health and the Money view at once but posts its DM at the next turn. Say if you want it
  taken at `day_hold` too.
- The burn ring is per execution and counts settled provider calls only (not speech, not a task's carry); it is in memory, so a
  restart starts empty. A burn pause asks as a budget question: a decline leaves the session waiting; its next message asks again.
- Under `stop`, `daily_runaway_usd` is ignored (the ceiling is the bar).
- `monthly_warn_usd` uses AWS Budgets' actual spend, so it can lag by hours; there is no cockpit line for it (health's AWS line
  carries the budget as before). The AWS warning posts to the owner's DM kind `spend_ceiling`.

## 7. Every money rule after this change

| key | default | reaching it | sticky | scope |
|---|---|---|---|---|
| `[kernel] spend_limit_usd` + `spend_limit_mode` | $100, `notify` | notice at the limit and each multiple; calls go on (`ask`: waits for the owner) | no | session |
| `spend_limit_hard_multiple` | 10 | a call that would pass 10x the limit is refused, turn ends `spend_hard_stop` | no | session (notify only) |
| `[kernel] runaway_usd_per_10min` | $200 | the session's next call is not made; the owner is asked to resume (budget question; never expires); resume empties the window | no | session, per 10 minutes |
| `[profiles.*] max_loops` + `max_loops_mode` | per profile, `notify` | notice at the cap and each multiple, or the turn ends (`end`) | no | turn |
| `[kernel] daily_spend_ceiling_usd` + `daily_spend_ceiling_mode` | $200, `notify` | `notify`: a warning (DM, row, narrative, health line 1, Money view), nothing refused. `stop`: every call refused, class `daily_ceiling` | `stop`: yes, until midnight or a higher ceiling at the next start | day, all model calls |
| `daily_notice_step_usd` | $200 | another warning at each step past the ceiling, once a day each | n/a | day |
| `daily_runaway_usd` | $2,000 (0 never) | under `notify`: every call refused, class `daily_runaway` | yes, until midnight or a higher line at the next start | day, all model calls |
| place ceiling `spend_limit_usd`, MCP client limit | per place / client | the session asks (pinned) | no | session |
| background day caps (judge `shadow_limit_usd_per_day`, replay/audit/backfill limits, `[people] sweep_usd_per_day`) | as before | the work is skipped | no | day, that work |
| `[aws.accounts.<id>] monthly_warn_usd` | half of `monthly_budget_usd` | one Theseus warning a month (DM, row, narrative) | once a month | AWS month |
| `monthly_budget_usd` (AWS Budgets) | 50 | AWS's own alert and its month stop, unchanged | n/a | AWS month |
| `daily_budget_usd` | stack's | alerts only, unchanged | n/a | AWS day |
| `hourly_alert_usd` | $1 | one alert an hour, unchanged | no | AWS hour |
| `runaway_factor` | 10 | new AWS actions that reserve are refused until the hour or day turns, unchanged | until the hour/day | AWS hour/day |
| an AWS reservation past the session budget | n/a | `OverBudget`: the group is not run and the session asks, unchanged | no | session |
