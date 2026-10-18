# CLOUD_REPORT: task-model-by-brief (theseus-ypnxt.7)

Branch `cloud/20261017-task-model-by-brief`, on `40aa83cd`. Commits: `1f941a05` (pins), `39ff44d8` (the rule),
`a31e24d4` (clippy line limit).

## What I found (the brief's premise was off)

The brief says inbound already judges a task's brief with `task: true`. It does not, for the first turn. A task
opened by `task.start` runs its first turn from the driver (`continue_execution`, `input: None`), and `inbound_point`
runs only for a person's message node. `task: true` is for a message sent to a running task. So a task's first turn asked
route.v6 nothing, and `route_base` returned at once for a session with no `routed`. The task ran on the parent's
`last_target` for its whole first turn, whatever the brief said. Claim "no new model call" therefore cannot hold
literally: the brief's judgment is a new route.v6 request. I kept it as small as possible: route.v6 alone (no
classify.v3, no role.v1), one request, and only for an unpinned owner-started task's first turn.

## What changed

- `crates/theseus-core/src/turn/brief_step.rs` (new): `mark`, `brief_point` (task's first turn, marked, route live:
  `JudgeService::at_brief` for the brief node), `decide_brief`, `place_cap` (moved out of `decide_route`).
- `crates/theseus-core/src/routing/brief.rs` (new): `brief_base`, the pure rule. **The one place to revert the
  rule:** `turn/brief_step.rs:38` (`brief_point`'s early return) turns the whole thing off; `routing/brief.rs`
  holds the choice.
- `rpc/task_start.rs`: the open's frame also writes META `task.route.<session>` when no `profile` was given
  (`Draft.by_brief`). `target_of` unchanged: the parent's stays the fallback.
- `judge/inbound.rs`: `at_brief` (`inbound_early_of(msg, route_only)`), and the brief is judged by its words,
  not the preamble (`brief_of`).
- `turn/route_step.rs`: `RouteState.brief`; `decide_route` hands a brief to `decide_brief`; `route_to` clears `routed`
  for a brief (the class's profile is the base, not a move); a late brief verdict is not kept for a later message;
  `record_route` says `source = "task brief"`.
- `fact/route.rs`: `follows` only when present; narration `(task brief)`.
- Words: `crates/theseus/src/task_start.rs` help `(routed from the brief by default)`; Discord footer and
  cockpit `routefooter.ts`: `routed: routine_coding (task brief)`; `TurnRoute.source` doc; generated
  `cockpit/src/protocol.gen/TurnRoute.ts`; `theseus-core/AGENTS.md` (routing section).
- The model's `task_create` is not changed (it has no profile except for checks, and its refusal text still says
  true things), so its tokens are unchanged. Its tasks still inherit the parent's model; not in this row.
- No config key: nothing needed a switch. No store format change (META key). No new dependency.

## Tests and plants

`tests_task_brief.rs` (7), `routing::brief::tests` (4). Pins first (`1f941a05`): parent's model by default, profile
pins. The first pin was converted: old `assert_eq!(ran_on(&r), "claude-fable-5-1")`, no `route.decided` row,
`jev.connections() == 0`; new: `claude-haiku-5-5`, `last_target.profile == "haikuhi"`, `routed` none, one Jev
request, row `source: "task brief"`. The pinned-profile test is unchanged.

| Behaviour | Test | Plant | Failure |
|---|---|---|---|
| brief picks class profile | `a_task_with_no_profile_runs_on_the_profile_its_briefs_class_picks` | `brief_point` disabled (parent's always) | 5 of 7 fail, incl. this one |
| cap caps | `a_capped_places_task_is_never_routed_above_the_cap` | `place_cap(..).and(None)` | `left: claude-opus-5-5 right: claude-sonnet-5-5` |
| late verdict sets no base | `a_late_verdict_sets_no_base` | `keep = true` | fails |
| Jev off / under bar / profile pin / design up | the other four | not planted (they hold today's path) | - |

The step-down rule: a brief leaves `routed` empty, so `from` is empty and `step_down` cannot go below the brief's
base; covered by `routed.is_none()` and `last_target` in the first test (no separate test of a later step-down).

## Budgets

`theseus-sim bench turn --check` (debug, this VM): plain turn 4 frames; `first_request_tokens_cli` 7332 (8000),
`_task` 6585 (8000), `_dm` 9411 (15000); `tests_first_request` and the owner-config tests pass. Lifecycle bench
(debug, 10 runs): cold start p50 31.7 ms (50), shutdown p50 12.5 (100), SIGKILL restart 31.3 (150), swap 39.5 (200): ok. Two rows
MISSED on this VM, neither on my path: "push's seed: first executions.watch" p50 1.1 ms (budget 2, p95 2.4) and
"health after the seed" p50 5.9 ms (budget 5); the bench reads timing on a shared VM and nothing there reads a
task. Cloud gates skip benches; I ran these two by hand.

## Cost note (stand-in, scripted verdicts, not a benchmark)

Three briefs from a Fable conversation. Before: all three ran on `fable`. After: typo fix (routine_coding) ->
`haikuhi` (Haiku 5.5, the config's first routine_coding profile); a design question (sophisticated) -> `opus`;
a refactor (deep_coding) -> `opus` by the default class table. Tested: typo and design. The owner's classes
decide the rest; the defaults send routine coding to Haiku, not Sonnet.

## Live check for the maintainer

On a scratch daemon with real Jev: start a conversation on `fable`, then
`theseus task start "Fix the typo in docs/x.md"` and `theseus why <task session>` (should say `task class
routine_coding, placed by task brief, moved from fable to <profile>`); `theseus task start --profile fable "..."`
should show no `route.decided` row; with `[judge] enabled = false` the task stays on fable.

## Overlaps

Touched other rows' files, small: `judge/inbound.rs` (discord-tasks owns `take_offer`: I added `at_brief`/`inbound_early_of`,
and `brief_of` use in `input()`); `fact/route.rs`; `routing.rs` (3 `pub(super)` test helpers, one `mod` line, a doc
bullet); `decide_route`'s cap expression is now `Self::place_cap` (review-fixes edits this route step: keep my
`if t.route.brief` first lines and `place_cap`). Names used that a landing lane renames: none (`task.get` not used).

## Proof nothing was lost

Plain conversation turns never reach `brief_point` (`tc.task.is_none()` first); `theseus-core` route/task/output
tests (252) pass, including `tests_output`'s golden and the frame budget. `follows` stays in correction rows.
protocol, discord and CLI crates: 1342 passed. Cockpit `npm test`: 454 passed.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, clippy, cockpit,
reader rule: ok. Suite failed on: the ~66 L1 sandbox tests (root VM, theseus-pv6i, known);
`watch::tests_watch::a_file_watch_is_met_within_one_interval_of_the_change` (timing; passed alone);
`theseus-discord tests_task_threads::a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it`
timed out at 120 s in the suite and once alone, then passed alone three times (3.4 s), also with my change
disabled; it uses a model's `task_create`, which never carries the mark, so I think it is a VM timing flake, but I
could not prove it on a clean main. Later phases (`machine_checks`) not run by hand.
`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty. (The gate's own keel phase
reads the local `main` ref; I fast-forwarded it to `origin/main` first, else it reports unrelated findings.)

## For the joiner

- Disk: this VM's allowance fills (a test build is ~8 GB); `CARGO_INCREMENTAL=0` helped.
- Doc updates for you: spec Part III item and `docs/status.md` (a task's model is routed from its brief);
  `docs/design` roadmap row.
- Design choices to hear: a brief judged `trivial`/`quick` makes the cheap profile the task's base (no detour for a
  task); `chat`/`other` keep the parent's; the pin/mark is a META key, so a task opened by `task.create` (the model),
  retries, routines and checks are not routed by brief.
