# CLOUD_REPORT: progress-words (theseus-ypnxt.2)

Branch `cloud/20261017-progress-words`. Commits: A 785164fa, B cfaf4199, C c80d1b8c, D 92d2f22c, E 02ba87b5.

## What changed
- **A** `theseus-protocol/src/work/progress_line.rs` (new): `progress_line(view, now_ms)`, plus `progress_rest` (the line without `paused`), `step_words`, `cost_words`, `time_left`. Golden `work/golden/progress_line.json` (8 cases, exactly the task's). `coarse`'s words matched the golden for cases 4-6 unchanged (`~6 min left`, `~5 min left`, `past its usual time`); minutes under an hour, `<1 min`, hours as `~2 h left`. `Progress::line()` now gives `step N of M · said` (was `3/5 · said`, N = done+1); callers' tests, `tests/golden_tree/*` follow. `tests_work::view` is `pub(crate)`.
- **B** `theseus/src/render/task_tree.rs`: a task's activity = `paused`, doing, then `progress_rest`, then the model's words. The footer-only cost column is removed (the line holds the cost once, in tree and footer). `events()` gained Step, PastUsual (once), Paused, Resumed; `event_line` words them. The chat prints the same events (`tui chat/tasks.rs` match arm).
- **C** TUI sidebar rows (`theseus-tui/src/work.rs`: `Facts.paused/timing`, `doing()`), chat task-view header (`chat/task_view.rs`: paused, then step/$/time left). The footer is B's shared renderer.
- **D** `theseus/src/work.rs::view_line` prints `[progress line]` then the model's words (reaches the herdr board tab, the follower and `--watch`); pane label starts `paused · `; Space row `paused` token (`⏸N`, empty when none; `herdr_up/state.rs`, `wire.rs`).
- **E** `discord/render/board_v2.rs::working_line(v, now_ms)`; `runtime/forum.rs`: `Tag::{Paused,Stopped}`, `Tag::of(&WorkView)`; the existing `read_tags` adds missing tags to an older forum (once: a forum that has them is not patched); the default forum is made with six. Golden: one case added ("planned and paused"), the existing cases are byte-identical.

## Tests and plants
| Test | Plant | Failure |
|---|---|---|
| `progress_line::the_golden_cases_hold` | swap paused and step | `left: step 3 of 5 · paused · …` |
| `task_tree::a_step_change_prints_one_line…` | print on every frame; drop the Step push | assertion at the step/no-change lines |
| `tui tests_work::a_row_says_paused_and_time_left` | drop `paused` in doing() | fails |
| `tui chat::tests_task_view::the_header_says_paused…` | drop progress_rest in header | fails |
| `work::a_task_line_has_the_step…` and `herdr_slots::a_paused_task_says_paused_in_its_label` | view_line without the line; paused label off | 3 failures |
| `forum::a_paused_post_is_tagged_paused…` | paused→running | fails |
| same + `a_state_has_one_tag…` | cancelled→failed | both fail |
New tests also: `a_row_ends_with_its_progress_line…`, `a_paused_task_says_so_first_in_its_row`, `past_its_usual_time_is_told_once`, `paused_then_resumed_print_two_lines`, `paused_tasks_are_counted`, `an_old_forum_gains_the_two_new_tags_once`, the Space-row token test. Converted assertions (old → new): `3/5 …`→`step 4 of 5 …`, `[1/3]`→`[step 2 of 3]`, tags `four`→`six`, chat `$0.31` once→`$0.31 of $5` twice (footer and tree), `failed` for cancelled→`stopped`.

## Before / after
- CLI/chat footer (golden): before `◐ 5a877c Buoy survey 3m $1.20 job 2m › leg 5: … · step 5 of 9 · sounding the ch…`; after `◐ 5a877c Buoy survey 3m job 2m › leg 5: … · step 5 of 9 · $1.20 of $5 · s…`.
- Discord golden added: `⏸️ **Task number 2** … · paused · fs.edit · step 3 of 5 · $0.42 of $2 · ~6 min left · since <t:…:R>`.
- herdr/`--watch` line: `◐ Sound the channel  turn 2  [step 3 of 5 · $0.40 of $2 · ~6 min left]  Reading depths`.
- Live-board edit count in a scripted 10-minute run: **not measured** (no sim run done). By construction the count cannot grow: the board is rebuilt only on a task change as before, no timer was added, the lane's 2 s pace is untouched, and the time-left text changes at most once a minute.

## Budgets
Not run (no model/token-path change; the turn bench and owner-config fixture were not run here).

## Overlaps
- `theseus-sim/src/fake_discord/threads.rs`: added `seed_forum_tags` (test helper).
- `theseus-tui/src/chat/tasks.rs`: 4 match arms for the new events. `chat/tests_tasks.rs`, `chat/tests_task_view.rs` (chat-start-pause's area; tests only).
- No renamed names used; no `task.get`.

## Proof nothing was lost
Unchanged Discord golden cases; old forum tags kept; the forecast bracket `[$x of ~$p50]` in `view_line` is **dropped** (the line's time left / past replaces it): a design choice for the owner. `Event` is exhaustive, so any other consumer fails to compile. Line mode (`task_lines.rs`) does not print the new events.

## For the joiner
- Regenerate nothing (no protocol types changed). Docs: spec Part III item for ypnxt.2; AGENTS.md of cli/tui/discord already updated.
- cockpit-tasks: point the TS test at `work/golden/progress_line.json`.
- `~/.cargo/config.toml` on this VM had `incremental=false` added (disk).

## Gate
`THESEUS_KEEL_BASE=40aa83cd TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit, test build pass; the suite fails only on the known L1 sandbox tests (theseusd::sandbox, theseus-sandbox contract tests, spawn_100). origin/main in this clone is older than the base, hence THESEUS_KEEL_BASE. The phases after the suite were not run by the gate; `protocol types` was run by hand (passes). Benches skipped (NO_BENCH). Earlier runs showed the discord `tests_task_threads`/`tests_war_room` shared-place timeouts, which also time out on the unchanged base (checked `a_shared_place_refuses…`); the last gate run had none. `python3 scripts/keel-guard.py --base 40aa83cd --head HEAD --quiet`: empty output.
