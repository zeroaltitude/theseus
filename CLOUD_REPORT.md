# CLOUD_REPORT: task-checkpoint-routing (theseus-ypnxt.9)

Branch `cloud/20261018-task-checkpoint-routing`, on `main` as cloned (`b95a2558`). Every commit's subject carries
`theseus-ypnxt.9`. Commits: `1bdec0ae` (A: the pin), `e2f00058` (B to E and G: checkpoints, the rule, the switch, the
cost), `6b421af4` (F: the words), `878894b5` (the text's cut made reachable and tested), `c83bd8df` (the slow-verdict test,
the cost note, the `AGENTS.md` lines), `80bdafdf` (one clippy `allow` the keel guard caught, removed); the report is the
last commit.

## What I found

- A task is one long turn: `turn.rs`'s `while run_model` routes once, at the first loop, and `route_to` refuses a
  second slot ("a turn routes once"). A task's brief is judged once (`brief_point`) and then its model never moves.
- A tool result's "failure" is `ResultStatus` `Error`, `Declined`, `Unknown` or `Cancelled`, written inline in
  `toolrun.rs` as the node's `is_error`. I made it `ResultStatus::is_error` (`node.rs:76`) and call it from both sites,
  so a checkpoint reads what the runner marks and invents no test of its own.
- The judge's verdict arrives on a oneshot at the moment its request answers, so "never waited on" is a `try_recv` at
  the next boundary. Its settled cost was only in the judge's day record (`judge.budget`), never on the task.
- The task's `Routed.from` already keeps a base through moves (`route_to` sets it once and clears it on a switch back),
  so the checkpoint's moves reuse `route_to` whole and the step-down comes out right with no stored field.
- The rig's brief for `routine_coding` picks `haikuhi`, so a Haiku base is the default test base.

## Steps

**A. Pinned (`1bdec0ae`).** Two tests on `tests_task_brief.rs`'s rig, run on `main` as it was (my work stashed, built,
run, committed, restored): a failing task on a cheap base ran every loop on Haiku and asked the pack once (the brief),
and a task started with `--profile opus` ran every loop on Opus and asked nothing. Converted in `e2f00058`: the first
(`a_failing_task_stays_on_its_base_through_every_loop`: `models == ["haiku"; 9]`, `connections == 1`) is now
`a_task_whose_loops_fail_moves_to_sonnet_at_a_later_loop` (`models[..3]` Haiku, Sonnet from a later loop and for the
rest, `route.decided` source `task checkpoint`, trigger `failing`, `cold_tokens` and `cache_usd` present, the session
`routed = {profile: sonnet, from: haikuhi}`), and the old assertion is kept as
`a_failing_task_with_checkpoints_off_stays_on_its_base` under `enabled = false`. The second is unchanged (its name and
assertions; `a_pinned_task_runs_every_loop_on_its_profile_and_asks_nothing`).

**B. The checkpoints** (`routing/checkpoint.rs`: `Tally`, `Trigger`, `Seen`; `turn/checkpoint_step.rs`). Asked at a
loop's end, applied at the next boundary that finds the verdict: failing (3 loops in a row with a failed result),
quiet (8 loops with no progress: a plan step done, the progress words changed, or a recovery after a failing run),
phase (the first open plan step moved) and report (the last open step done). Only a loop that touched a plan or task tool
reads the plan; a task with no plan has no phase or report trigger. At most one verdict per `min_loops_between` (4)
loops (a trigger inside the window waits for it, and the failing and quiet counts keep running), at most one in flight, and
a flight a window old is given up so its late answer is never applied after the next ask. Which tasks: the brief's mark
(`brief_step::marked`), i.e. `task.start` with no profile; a pinned task and every conversation pay one `match` per loop.

**C. The input.** `checkpoint::text`: the brief's first 600 characters, at most 12 plan steps with their states, the last
6 loops' `say` each with its first failed result's first line, and the trigger line (`failing: 3 failed loops in a row`,
`no progress for 8 loops`, `phase: plan → implement`, `before the report`); at most 1,500 tokens (estimated at 3
characters a token, which is conservative), cutting the oldest loops, then the plan's last steps, then the brief's tail,
never the trigger. It goes through `at_checkpoint` (`judge/inbound.rs`), route.v6's route-only path with `task: true` and
no node of its own; no pack's text and no prompt changed.

**D. The rule** (`routing::checkpoint::decide`, pure, reusing `routing::decide`'s walk and `STEP_DOWN`'s hold).
Only Jev elevates; the class's first usable profile under the cap; the base is the floor; two agreeing verdicts step down
to it (the first holds, the second switches; a verdict of another class or under its bar clears the hold); a cold switch up
waits for a second agreeing verdict at or above `cold_switch_tokens` (the warm prefix taken off as `decide_route` takes it);
`trivial` and `quick` move nothing and count toward a step down when their pick is no dearer than the base; the class's
confidence bar; the data floor through `route_profiles(sensitive)` and `data_floor`; `max_switches` moves up and a step
down past it (new `Reason::SwitchCap`, `switch_cap`).

**E. The switch.** `checkpoint_apply` (`turn.rs:1794`, beside `steer_in`): the same `route_to` as an inbound route, on a
slot of its own from `Slots` (8, `max_switches` is validated at load to at most 7), the provider resolved as the first
compile's switch resolves it, the effort rebuilt for the new profile (`route_effort`, now `pub(super)`), the trace root,
the reservation (each call reserves at its own model) and the session's `last_target` and `routed` follow; the next compile
is on the new model and strips thinking (a model change is a recompile). `route_to` and `route_effort` are widened to
`pub(super)`; nothing else of `route_step.rs` changed (`checkpoint: None` on the old row's construction).

**F. Config, records, words.**
`[routing.task_checkpoints]` (`config/routing_checkpoints.rs`, a new file, its field on `RoutingConfig`, the template
and the owner-shape fixture): `enabled = true`, `failing_streak = 3`, `quiet_loops = 8`, `min_loops_between = 4`,
`max_switches = 2`; `deny_unknown_fields`; a count of 0 is an error naming `enabled`. The `route.decided` row gains,
only on a checkpoint, `trigger`, `loop`, `cost_usd` and, on a switch, `cold_tokens` and `cache_usd` (priced at the target's
cache-write rate for its TTL); old rows read as before. The reply footer reads `routed: deep_coding (task checkpoint:
failing)` (Discord's `routed`, the cockpit's route line and its ledger line); `theseus task show` prints a `model` line
for the brief's pick and each move (`sonnet (from haikuhi, loop 7: failing)`, a step down says `step down`), from one more
`ledger.tail` that only `show` makes; the one progress line adds `on <profile>` last, only for a task off its base
(a `WorkView.moved_to`, optional with a default, fed by an optional `on` on a `loop.ended` row, so an unmoved task's
rows and line are byte-identical). The eight golden cases are untouched (`progress_line.json`, assertion `== 8`); the
moved cases are three in `progress_line_moved.json`, read by a Rust test and a cockpit test, rather than raising
the eight's count.

**G. Jev's cost.** `Kernel::charge_execution` (`theseus-kernel/src/spend.rs`): adds the settled cost to the task's
`spent` and, by the kernel's carry, to its parent's, with no row and no word to the day ceiling. It is called by the
judge when the checkpoint's call settles (`judge/inbound.rs`, `charge`), so a late or never-applied verdict still bills
its task. **The day's total counts it once**: `day_ceiling::read_today` reads the judge's point judgments from the
`judge.budget` record and a task's provider calls from `action.*` rows, "a task's cost carried to its parent's budget
writes no row of its own", so the checkpoint is in the day through the judge's record, and `charge_execution` neither
writes an `action.*` row nor tells the day ceiling (`book_spend` would, and would count it twice). The checkpoint still
reserves on the judge's day budget as every judgment does (`reserve_beside`). A task whose budget has less than
`RESERVE_MICROS` (30,000, $0.03) left skips the ask and records a row with reason `budget`; the breaker open, the judge
off or unreachable: no ask, no row, the task runs on.

## Tests and plants

All in `tests_task_checkpoint.rs` (integration, the rig of `tests_task_brief.rs` with a provider that takes 60 ms a loop
so a verdict lands at a boundary), `routing::checkpoint::tests`, `config::routing_checkpoints::tests`, the kernel's
`spend::tests`, the CLI's `task_history` tests, the protocol's `progress_line` tests and the cockpit's. A plant is made by
editing the named line, `touch`ing the file, running `tests_task_checkpoint` and `routing::checkpoint`, restoring with
`git checkout` and `touch`ing again; `git status` was clean after each.

| Plant | Fails (named test) | Other failures |
|---|---|---|
| never ask at a failing streak (`if false &&` on the failing arm of `Tally::observe`) | `a_task_whose_loops_fail_moves_to_sonnet_at_a_later_loop`, `a_failing_task_asks_once_per_window` | every test that needs a checkpoint to be asked |
| step down on the first verdict (`let held = true`) | `it_steps_back_to_the_base_after_two_agreeing_verdicts`, `a_task_steps_back_to_its_base_after_two_agreeing_verdicts`, `one_agreeing_verdict_then_a_disagreeing_one_leaves_it_up` | the cap and moves tests that steer by the second verdict |
| let the pick go under the base (`current == base` returns a switch to the cheaper pick) | `a_cheap_pick_never_goes_below_the_base`, `a_cheap_verdict_never_takes_a_task_below_its_base` | none |
| switch at once above `cold_switch_tokens` (`true \|\| agreed`) | `a_cold_switch_up_waits_for_a_second_verdict`, `a_cold_switch_waits_for_a_second_agreeing_verdict` | none |
| no cap on moves up (`if false` for `moves >= max_switches`) | `the_cap_stops_moves_up_and_not_the_way_down` (unit and integration) | none |
| ask every loop (`let open = true`) | `a_failing_task_asks_once_per_window`, `a_task_failing_for_twelve_loops_asks_three_times`, `a_phase_change_and_the_report_ask_and_wait_for_the_window` | the tests that count asks or Jev's calls |
| leave Jev's cost out (drop `charge(...)`) | `a_checkpoints_cost_lands_in_the_tasks_spent` | none |
| no bound on the text (`true \|\| ... <= max`) | `a_tight_bound_cuts_the_oldest_loops_then_the_plan_then_the_brief` | none (the caps hold the text under the bound by construction, so the cut is tested with a tight bound) |
| the turn waits for the verdict (a 700 ms sleep after the ask) | `a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes` | none |
| the task and pin guards removed | `a_pinned_task_runs_every_loop_on_its_profile_and_asks_nothing`, `a_conversation_turn_is_the_same_with_checkpoints_on` | none |

Also, with no plant (they hold a pure move or a reading): `a_task_whose_loops_fail_moves_to_sonnet_at_a_later_loop`
(the converted pin), `the_board_says_a_moved_task_is_on_its_new_model` (the work board's `moved_to` and the line, and the
`loop.ended` rows' `on`), the kernel's `a_charge_is_booked_to_the_execution_with_no_row`, the CLI's
`a_tasks_model_lines_are_its_brief_and_each_move`, the progress line's moved cases (Rust and cockpit), the footer's lines
(Discord and cockpit), `a_conversation_turn_is_the_same_with_checkpoints_on` (requests and the WAL's last position are
equal with `enabled` true and false), and the config's two tests. Not planted: `ResultStatus::is_error` (a pure move).

Runs under load (`nice -n 19` beside four busy loops): not run; the loop timing is on the provider's 60 ms, not a race
against a count, and every integration test passes in about 2 to 5 s. See "Left".

## Budgets

- `target/debug/theseus-sim bench turn --check` (the gate-built binary, this VM): all rows ok. `frames_plain` 4 at the p95
  (budget 4), `frames_tool` 8 (8), `frames_batch` 14 (14), `syncs_plain` 2 (2), `syncs_first_byte` 2 (2),
  `deltas_gathered` and `deltas_held` 0 missing; the first request at the p95: channel 7,391 tokens (budget 7,900), cli
  7,332 (8,000), dm 9,411 (15,000), headless 7,846 (8,600), task 6,585 (8,000). The plain turn's overhead is p50 66.8 ms,
  p95 84.9 ms (these timings are this VM's, not the owner's machine's, so I left the speed budgets alone).
- The owner-config fixture test (`config::tests_inherit::the_owner_shaped_config_resolves_to_the_same_profiles`, with the
  new `[routing.task_checkpoints]` block in `fixtures/owner_shape.toml`) passes in the gate's suite, as do the template's
  `example_template_uncommented_still_parses` and `the_templates_routing_section`.
- First request: no new tool, no always-loaded text (a checkpoint's text is a judge message, never a request to a model
  of the task's); a plain turn is the same 5 frames: `a_conversation_turn_is_the_same_with_checkpoints_on` holds the
  requests and the WAL position equal with the section on and off.

## Cost note (a stand-in, not a benchmark)

`a_cost_note_on_three_scripted_tasks`, Haiku base (`haikuhi`), the fake Jev answering `deep_coding` at the failing
trigger and `sophisticated` at the quiet one, run with `--no-capture`:

| Task | Model by loop | Checkpoints | Jev's cost | Task spent |
|---|---|---|---|---|
| fails three loops, then passes | haiku ×4, sonnet ×6 | failing | $0.000037 | $0.240330 |
| stalls for ten loops (no words) | haiku ×9, opus ×2 | quiet | $0.000030 | $0.173441 |
| clean | haiku ×9 | none | $0 | $0.017286 |

The fake provider's usage is scripted and its prices are the catalog's, so the dollars show the shape (a checkpoint's own
cost is three orders under a loop on a dearer model) and nothing of a real run. The benchmark rerun measures the rest.

## The one place to switch it off, and where it joins the turn

- Off: `[routing.task_checkpoints] enabled = false` (the only switch; `[routing] enabled = false` also turns it off, as
  route.v6 is then not asked).
- Joins `turn.rs` in four call lines and two declarations: the boundary `checkpoint_apply` (`turn.rs:1794`), the loop's end
  `checkpoint_ended` (`turn.rs:1992`), the failure flag in `run_tools` (`turn.rs:3082`), the slots beside the route's
  (`turn.rs:1545`, passed at `:1644`), and the `Turn` field (`turn.rs:421`, `:553`). `turn.rs` is at 3,384 of its 3,525
  ceiling (it was 3,359). `advance_step.rs` writes `on` on the loop's row.

## Overlaps

- **theseus-31pgz** (the morning lane): I touched `routing/brief.rs` not at all, `turn/brief_step.rs` in two small places
  (a new `marked()` reading the brief's mark, which `brief_point` now calls instead of its own `get_meta`), and
  `progress_line.rs` in one line (`on <profile>`, last, after the time part: keep both its forecast words and mine) plus
  its test and a second golden file. Resolve toward its brief picks and its progress-line words; mine is one more part.
- **The line that would make routine briefs Haiku-first again** once a failing Haiku task moves up: on `main` as cloned it
  already is (`config/routing.rs`, `fn routine_coding()`: `haikuhi`, `sonnet`). If 31pgz puts Sonnet for `routine_coding`
  briefs in `routing/brief.rs`'s `brief_base` (or in the class list), that single line is the one to revert. I changed
  neither.
- Shared files: `WorkView` gained `moved_to` (every struct literal of it in tests and renderers got `moved_to: None`:
  protocol, core, discord, tui, cli), `theseus-protocol/src/route.rs` (a doc line; `protocol.gen/TurnRoute.ts` and
  `WorkView.ts` regenerated), `fact/route.rs` (the `Checkpointed` block and one `narrate` arm), `fact/turn.rs` (`on` on
  `LoopEnded`), `push/work.rs` and `push/work/views.rs` (the board's `on`), `judge/inbound.rs` (`Point`, `at_checkpoint`,
  `charge`, `Answered.cost_micros`), `theseus-kernel/src/spend.rs`. Names a landing lane may rename: `Reason::SwitchCap`,
  `Answered.cost_micros`, `RouteDecided.checkpoint`.
- Store format: none changed, as ordered (a new META key and a ledger row's optional fields are not a format change; no
  new table, no new kind). The task said the store is at format 45; nothing here bumps it.

## Proof nothing was lost

- A conversation turn is byte-identical with the section on and off (`a_conversation_turn_is_the_same_with_checkpoints_on`:
  the six requests as JSON and the WAL's last position).
- A task started with a profile asks nothing and never moves (`a_pinned_task_runs_every_loop_on_its_profile_and_asks_nothing`,
  unchanged from the pin).
- The 500 tests of config, routing, `tests_route*`, `tests_task*`, `tests_registry`, `tests_output` (the output golden under
  `TZ=America/Phoenix`), `tests_layouts` and `tests_work*` pass; the CLI, Discord, TUI and protocol suites pass (1,759 tests;
  `theseus::markdown rendering_costs_little_per_hundred_lines` is a timing test that fails under load here and passes
  alone; one Discord thread test timed out once under load and passes alone).
- Old `route.decided` rows read as before (the new fields are written only on a checkpoint row), `loop.ended` carries `on`
  only for a moved task, and the eight golden progress cases are unchanged.
- `python3 scripts/keel-guard.py --base b95a2558 --head HEAD --quiet` prints nothing (exit 0). (`--base origin/main` as
  written would have judged a main that had moved on after a `git fetch`, and found 82 erosions that are other lanes'
  merges, not mine; I restored the local `origin/main` ref to `b95a2558` and passed the commit.) One `allow` I had added
  (`too_many_arguments`) was caught by it and removed in the last code commit.

## Live check for the maintainer

On a scratch daemon of the install build, with the real route pack:

```
theseus --socket /tmp/scratch.sock task start "Make the failing test in crates/foo pass; run it with cargo nextest" --budget 2
theseus --socket /tmp/scratch.sock task show ID          # shows `model  haikuhi (task brief)` and, after a checkpoint, `sonnet (from haikuhi, loop N: failing)`
theseus --socket /tmp/scratch.sock tasks --watch         # the line gains `· on sonnet` once it moves
theseus --socket /tmp/scratch.sock ledger tail --kind route.decided   # rows with source `task checkpoint`, trigger, cold_tokens, cache_usd, cost_usd
```

A task whose first loops fail should show a `failing` row at its third loop and a `sonnet` row a few loops later; a clean
task should show none. `theseus task start … --profile opus` must show none either way.

## Left, and what the owner should hear

- **Tasks made by a model** (`task.create`) have no brief mark, so they are not covered: only `task.start` tasks with no
  profile are. A pinned task and a model-made one are not told apart by a stored field (none may be added here).
- **A slow Jev**: a verdict that takes longer than `min_loops_between` loops is dropped for the next ask (the order says a late
  answer older than the next ask is dropped). With loops of seconds and a verdict of one second this does not bite; with
  fast loops and a slow Jev it can starve; the slow-verdict test widens the window to show a verdict landing.
- **The cap and the hold are per turn in memory** (`moves`, `hold`): a task that parks and resumes starts them again (its
  base and its moved profile persist, in `routed`). `max_switches` is therefore per turn, not per task across turns.
- The checkpoint's judgment reads the whole session's nodes on the blocking pool (as the brief's does), off the loop's path.
- A late checkpoint cost is charged when the call settles even if the task has ended (terminal executions take it as
  `book_spend` does).
- Docs for the maintainer to write (I did not touch the spec, `docs/status.md`, the README or `docs/design/`): a Part III
  item for this row (what it built, the proof, the divergences above: a flight a window old is dropped; the state and
  the cap are per turn; model-made tasks are out), `docs/status.md`'s roadmap row and its landed line, and `docs/design/`'s
  roadmap-v2 row for ypnxt.9.
- Not run here: the full-load recipe, a live check with real keys, the cockpit's rendering in a browser.
- A design choice to hear: step down is by class, so a `chat` verdict (an empty list, the base) counts toward it, which
  matches the conversation's rule; and `quiet` is the one trigger a stalled task on its base can raise, so Jev can move a
  stalled Haiku task up.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the head code commit (`80bdafdf`; the report was untracked
beside it): door, keel guard (0 findings; `0 findings acked`), keel tests, fmt, shape, features, clippy (workspace, all
targets, `-D warnings`), the clippy probe, the cockpit's lint, tests and build, the test build and the reader rule
(`tests_registry`, 15 of 15) all pass. The suite phase: 6,724 tests, 6,687 passed, 36 failed, 1 timed out:

- 33 are the known L1 failures (`theseus-sandbox::contract` ×22, `theseus-sandbox::bench spawn_100`, `theseusd::sandbox`
  ×13 of the list above, the root daemon's job has no cgroup, theseus-pv6i), none touched by this branch;
- `theseus::markdown rendering_costs_little_per_hundred_lines`, `theseus::herdr a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile`,
  `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs` (a timing cap, 2.0 s under load) and
  `theseus-discord tests_task_threads::a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`
  (a 120 s timeout): timing tests that fail under the gate's parallel load on 4 cores and **pass run alone** on the gate's
  own test binaries (I ran each once, in turn, after the gate: 0.03 s, 4.6 s, 0.6 s, 4.5 s). None is on a path this branch
  changes (the markdown renderer, herdr's pane, the judge's builders, Discord's task threads).

Not green by the letter: the gate stopped at "FAILED in suite", so its phases after the suite (the protocol types check,
the lifecycle and jobs benches, the turn and size steps) did not run in it. I ran the turn bench by hand (above); the
lifecycle, jobs and size benches I did not run (the speed budgets are the owner's machine's, as ordered). The type-
generation check ran in the suite (`the_cockpits_types_are_generated_from_the_rust_ones` passes, and
`cockpit/src/protocol.gen/` is committed regenerated).

A first gate attempt ran the VM out of disk in the test build (the gate's `RUSTFLAGS` rebuild every crate beside mine);
I deleted stale artifacts from `target/` and ran it again from the start. Nothing outside the repository was touched.
