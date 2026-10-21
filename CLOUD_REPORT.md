# CLOUD_REPORT: rough-edges (theseus-ztkhg, theseus-hzdlf, theseus-ikw4p, theseus-dowi0)

Branch `cloud/20261021-rough-edges`. The clone's base is `88ddae5` (CLOUD_TASK.md names `1d092ded`; the keel guard below was run
against `origin/main`). Four commits, one per piece, each pushed:

| piece | commit | subject |
|---|---|---|
| A | `dab5854` | progress: spend and time past their usual say so; a tool names its time; old words carry their age (theseus-ztkhg) |
| B | `6814472` | discord: a task that ended before any sync saw it running gets its one post (theseus-hzdlf) |
| D | `9d2e906` | herdr: one question, one block in a pane; the guide's keys and log command are true (theseus-dowi0) |
| C | `b7dbab7` | tui: N says where the task goes; outside text draws cell for cell; one-turn conversations fold (theseus-ikw4p) |

The rollback of each piece is the revert of its commit (wording and drawing fixes, no switch, no store format change, no new
dependency: Cargo.lock and the package-lock files are untouched).

## 1. Files left over their line ceiling

None. `scripts/shape.sh`: `ok (no file over 2500 lines but the 8 listed)`. `runtime.rs` was not touched; `crates/theseus-protocol/src/lib.rs`
was not touched (no new wire type); `forum.rs` grew by 3 lines (1,458 -> 1,461) and `threads.rs` by about 30 (new code in
`runtime/threads/tests_owed.rs`).

## 2. What changed, per piece, with the words before and after

### A. Progress words (theseus-ztkhg)

1. **Spend past its usual.** `WorkForecast::spent_of` (used by `cost_part`, `forecast_cost`, `status.rs`, the TUI row and the cockpit):
   - at or under the median, unchanged: `$0.40 of ~$0.60 (limit $2)`;
   - past it: `$0.04 of ~$0.02 (limit $1)` -> **`$0.04, past its usual ~$0.02 (limit $1)`**. An `of ~` figure is never below the spend.
2. **Time past the median.** The countdown runs to the median only (`time_left` now also takes the spend):
   - a 6-minute task with a 4-minute median read `~<1 min left`; it now reads **`past its usual time`**;
   - the old golden case "past the median, time left from the p90" (15 min of a 10/20 forecast) read `~5 min left`; it now reads
     `past its usual time` and is renamed "past the median time, no countdown to the p90";
   - once the spend is past its median the time part says `past its usual time` too, whatever the clock.
   - Golden `progress_line.json`: 10 -> 12 cases (new: "spend past its median before the time is" = `$0.04, past its usual ~$0.02 (limit $1) · past
     its usual time`; "spend under its median" = `$0.01 of ~$0.02 (limit $1) · ~6 min left`). `cockpit/src/lib/progressLine.ts` and its test follow it.
     `progress_line_moved.json` is unchanged and still holds.
3. **`model 1:48` while a tool runs.** See section 4. `ExecIn.last_settled_ms` (in memory, no stored field) is the end of the execution's newest
   settled call; `Thinking`'s `since_ms` is `max(turn start, last_settled_ms)`: **`model 1:48` (the whole turn's time) -> `model` with the time since the last result came back**. `doing_of` is still O(the task's open actions); the settle adds one `HashMap` lookup per settled action.
4. **Words older than what the task does.** One helper beside `progress_line`: `progress_line::aged_said(said, at_ms, doing_since_ms, now_ms)`.
   `Round 1 done. Continuing.` -> **`Round 1 done. Continuing. (2 min ago)`** when the words are older than the doing's `since_ms` and over 60 s old
   (`(N h ago)` past an hour). Called by `theseus tasks` (`render/task_tree.rs`) and the board's line (`work.rs::view_line`).
   `Progress.at_ms` is the time the board first saw the words (`progress::carried`), so the age is true.
5. **The rollup.** `(1 need you, 1 working, 0 done, 0 failed)` -> **`(1 need you, 1 working)`**; `(1 working, 0 done, 0 failed)` -> **`(1 working)`**;
   all zero shows nothing (`work.rs::rollup`, for `view_line` and `system_line`).

### B. A forum post for every task (theseus-hzdlf)

`TaskThreads::owed(view)`: a task born (`started_at_ms`) at or after the binding's start (`since()`, read at the first `load`/`next`) and over is owed
its post. `next()`'s `(None, true)` returns `Next::Create` for it (only for a place that makes threads, a task with a session, no failure
backoff, and none made this run: `Inner.born`, kept after a thread's place goes, so a retired thread never makes a second). `make_task_thread` already
names the thread with the task's current mark (`task_name(Mark::of(state))`) and re-syncs afterwards, so the next sync renames and retires it as for any
task. In `forum.rs`, `one()`'s `if ended || homed { return None }` is now `if homed || (ended && !owed) { return None }`: an ended, not-homed task owed a post gets its
index card made as a running one is, and the worker's existing last-state edit sets the final card and tag. A homed task's post is made on request by
`make_task_thread` (`Home::Post`), so the same `Create` covers it. A task born before the binding started gets none ("an upgrade back-fills nothing").

### C. The TUI board (theseus-ikw4p)

1. **Where `N` puts a task**: `new task: the brief (enter starts it)> ` -> **`new task under Home: the brief (enter starts it)> `** (no row, or the cursor on a task)
   or **`new task under DM: …`** (the conversation's title). A `/` filter moves the cursor to its first matching row on each keystroke and paste
   (`App::cursor_to_first_match`). The default limit is unchanged (see section 7).
2. **Clean drawing**: `crates/theseus-tui/src/glyph.rs`, `glyph::tidy`, drops U+FE0E, U+FE0F and U+200D. Called by `Board::name` (every sidebar, notice and prompt name),
   `board::one_line` and `work::one_line` (labels, said, report), and `ui::put` (every pane line).
3. **One-turn conversations fold** (`one_turn.rs`): idle (ready or idle level), `turns <= 1`, no tasks under it, no pending question, not done-until-seen, last active
   over an hour ago (`SessionInfo.last_active_ms`, never when it is 0) -> one row at the foot, `▸ 4 one-turn conversations`. **The "existing fold key" does not exist**:
   `board.rs` folds idle children into a ` +N` suffix with no key. I made enter on the fold row open it (`▾`) and again close it (`App::one_turn_open`); only the whole
   tree folds (a filter or `b/w/r/d` shows what matches). A conversation that needs the owner (a question, `NeedsYou`) never folds.
4. **A dismissed failure**: `Row.dismissed`; the row reads `✗ dismissed` / `✗ … · dismissed` (the mark dimmed), and `App::is_done` is false for it.
5. **The digest line**: `chat/welcome.rs`'s one line, `◆ {line}` -> `{line}` (the summary already begins with `◆`).

### D. The herdr pane (theseus-dowi0, items 1, 3, 4, 5)

1. **One question, one block**: `Watch.shown` holds the ids of the questions whose card is on screen. The daemon tells a question twice (the `session.wait` read and
   the `confirm.requested` event carry it); the second told prints no card, and the line that re-says the prompt below an event now prints the prompt alone (it also printed the
   links line again for a plain approval). Before: card, `[answer] [history] [open]`, `approve? [y/N/t/note]` x2. After: once each.
2. `docs/herdr.md`: `herdr plugin log` (no id; on 0.9.1 an id is `unknown option: theseus`; it shows each action's command, exit code, stdout and stderr); `prefix+n` starts
   the task under Home (the DM) and passes no `--in`; `prefix+shift+a` goes to the longest-waiting question, often a self-improvement one.

## 3. Each test, its plant, the goldens

Plant = break the code, show the named test fail, restore and `touch`, `git status` clean (a script, `/tmp/plants/plant.py`, restores in a `finally`).

| test | plant | what failed |
|---|---|---|
| **`progress_line::tests::a_spend_past_its_usual_never_reads_as_under_it`** (key) | `spent_of` restored to `"{} of ~{}"` for every spend | it, `the_golden_cases_hold` and `forecast::tests::the_forecast_reads_as_the_example_does` (3 failed) |
| cockpit `progressLine.test.ts` (reads the same golden) | `costPart` back to `of ~` for every spend | `the Rust golden file, case by case`: expected `$0.04, past its usual ~$0.02 (limit $1) · past its usual time`, got `$0.04 of ~$0.02 (limit $1) · past its usual time` |
| `core tests_work_doing::a_running_tool_is_named_with_its_own_time` | (a) Thinking back to the turn's start; (b) `doing_of` skips the open actions | both fail (a: thinking counts from the turn's start, not the command's end; b: `Thinking` while the command runs) |
| `progress_line::tests::old_words_carry_their_age` | `aged_said` always bare | fails |
| `work::tests::a_rollup_shows_its_non_zero_parts` (CLI) | the old `0 done, 0 failed` format (filter removed) | fails |
| `threads::tests_owed::a_task_that_ended_before_its_first_sync_gets_one_post` | `(None, true)` back to `Nothing` / drop the `born` guard | fails (both) |
| `threads::tests_owed::an_old_ended_task_gets_no_post_at_start` | drop the `started_at_ms >= since()` test | fails |
| `tests_task_owed::a_fast_task_has_exactly_one_post_in_its_final_state` (end to end) | `(None, true)` back to `Nothing` | **passes**: on this VM the sync always saw the fast task running (see section 4); it holds that a fast task ends with exactly one post, named `✅`, tagged done |
| `tui tests_board_edges::n_says_where_the_task_goes` | the old prompt | fails (with `a_filter_puts_the_cursor_on_its_first_row`, which reads `N` after the filter) |
| `tests_board_edges::a_filter_puts_the_cursor_on_its_first_row` | no `cursor_to_first_match` after a typed character | fails |
| `tests_board_edges::outside_text_draws_cell_for_cell` | `glyph::tidy` returns its input | fails: `a variation selector reached the screen` (row: ` ● confirm proc.run  ⚙\u{fe0f}  a1b2c3 · scan the quay`, the extra blank is ratatui's second cell for the two-cell width). The vt100 read-back agrees with both versions (vt100 0.15 is not tmux), so the test's guard is the selector assertion plus the read-back; see section 5 |
| `tests_board_edges::one_turn_conversations_fold_and_a_question_never_does` | the fold count forced to 0 | fails |
| `tests_board_edges::a_dismissed_failure_never_reads_done` | `Row.dismissed` false | fails |
| `tests/herdr.rs::one_question_one_prompt_in_a_pane` | second print restored (`again` branch off) | fails: the card, links and prompt print twice |

Goldens and tests that moved with the words, in the commit that moved them: `progress_line.json` (10 -> 12 cases; one renamed, one new word); `progress_line.rs`'s case count; the TS test's
`~5 min left` line and case count; `crates/theseus/src/work.rs`'s `(1 need you, 1 working, 0 done, 0 failed)` line; `crates/theseus/src/board.rs`'s
`(1 working, 0 done, 0 failed)` line; `tests_new_task.rs`'s two `new task: the brief` checks (now `new task under DM: the brief`). No file golden under `tests/golden*` moved.

## 4. What the tests showed

- **hzdlf: which path loses the post.** By reading, both early returns lose it for a task a sync first sees over: `threads.rs::next`'s `(None, true) => Next::Nothing` (a channel's
  thread, and a homed task's post, which `make_task_thread` makes on request) and `forum.rs::one`'s `if ended || homed` (an index card of a shared forum). I could **not** make
  the post disappear end to end on this VM: with a task that answers at once, and with `Places`-free models, the work engine's `Tasks` message reached the place while the task was
  still running every time, and holding REST (`set_delay_ms`) does not hold the sync, which decides before any REST. The unit tests drive `next()` and `owed()` directly with a
  view seen over for the first time, and they fail on the plants. I did not run it under load.
- **`model 1:48`.** Not reproduced as a missed tool: in a task whose step is a foreground `proc_run sleep 3`, the base already returned `DoingKind::Job` (or `Tool`) with the call's
  own label (`proc_run`'s `is_job`: dispatched `proc.run`) while it ran: the plant that skips the open actions fails the test, the base passes that half. What the base got wrong is the
  Thinking clock: `since_ms` was always the turn's start, so after any call (or a long turn) the row read `model` with the whole turn's time. That is fixed (`last_settled_ms`). I did not
  find a state in which a running `proc.run` is invisible to `doing_of` (a `Planned`-but-dispatched action is the suspect the brief named; the kernel sets `Dispatched` and
  `dispatched_at_ms` together, and `is_job` reads `dispatched_at_ms`, so it holds). If the dry run's row really named `model` during the 25 s, the likely cause is a row built from a
  view older than the dispatch (the TUI/CLI form maps `Job` and `Tool` correctly); I could not check it live.
- **dowi0: the second print.** Two causes, both in `interactive.rs`: the read printed the card (`confirm_lines`) and the `confirm.requested` event printed it again; and `say_prompt(false)`
  (the re-say below an event) printed the links line again for any non-`owner_ask` question. Fixed both; the re-say of the prompt line alone stays by design (the prompt is the line the terminal ends on).

## 5. What could not run on the VM, and the commands

- **The full gate.** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran phases door, keel, keel tests, fmt, shape, features, clippy (with one real finding: `normal_key` was 104 lines; fixed by
  extracting `enter_row`), clippy-probe, cockpit, then **failed in "test build" with `No space left on device`**: this VM's writable allowance is about 40 GB and the workspace's debug test binaries are about 22 GB beside it.
  After freeing space I ran what the rest of the gate runs for the crates this row touches (below). Not run: the suites of the other crates (theseusd result below), the speed benches (not asked), `cargo deny`
  (run by hand after `cargo deny fetch`: `bans ok, licenses ok, sources ok, advisories FAILED` on RUSTSEC-2026-0382 (rustls-webpki, fix >=0.103.16) and RUSTSEC-2026-0381 (webpki-roots, fix >=1.0.10) in the lock file this branch did not touch; the advisory database is today's).
- Suites run, `TZ=America/Phoenix cargo nextest run -p theseus-core -p theseus-discord -p theseus-tui -p theseus -p theseus-protocol --no-fail-fast`: **5,404 tests run: 5,404 passed, 25 skipped**.
  Also `cargo clippy --workspace --all-targets -q -- -D warnings` (clean), `cargo fmt --all -- --check` (clean), `cd cockpit && npm test` (**481 pass**).
- `cargo nextest run -p theseusd --no-fail-fast`: **321 run, 308 passed, 13 failed, 4 skipped**; the 13 are all `theseusd::sandbox` (the L1 tests the brief names: this VM runs as root).
- **tmux**: `tmux 3.4` is installed. I fed the bytes crossterm writes for the 180x50 board (the test's frame, with the `⚙️` title) to a tmux pane (`tmux -L x new-session -d -x 180 -y 50 'cat frame.bin; sleep 4'`, `capture-pane -p`): **tmux 3.4 counts `⚙️` two cells, as ratatui does, so the overdraw did not reproduce here** with or without `glyph::tidy` (without it the row reads `⚙️ a1b2c3 · scan the quay  $1.20 │`, with it `⚙ a1b2c3 · scan the quay f $1.20 │`, both aligned). The dry run's terminal counted one; `tidy` makes the TUI independent of which a terminal does. I ran no live TUI against a scratch daemon. The maintainer's check, on a scratch daemon with a session titled `⚙️ a1b2c3 · scan the quay`:
  `tmux -L scratch new-session -d -x 180 -y 50 'theseus --socket <scratch socket> tui'` then `tmux -L scratch capture-pane -p`: the sidebar row must read `⚙ a1b2c3 · scan the quay`, the right pane
  `no session open: enter opens the one under the cursor`, with no stray letters; `N` must say `new task under Home: …`.
- **Discord live**: on a scratch daemon bound to a test guild, `theseus task start "say ok" --budget 0.05` from the CLI with the stand-in model answering at once: the task has exactly one
  post (forum) or thread, named `✅ …`, tagged `done`; restart the daemon and start nothing: no post is made for any older task.
- **herdr live**: in a herdr pane, `theseus watch SESSION --interactive` with a question open: one card, one `[answer] [history] [open]` line, one `approve? [y/N/t/note]`.
- The cockpit's npm tests ran here (`cd cockpit && npm ci && npm test`).

## 6. Keel guard

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing, exit 0. (The gate's own keel phase, against `1d092ded`: `keel: ok … 37 files changed, 0 findings acked`.)

## 7. Follow-ups and design notes

- **The age words (A4) reach `theseus tasks` and the board's line only.** Not done: the TUI board row's `Job` form (its `said` is the job's), `status_progress.rs`, `Progress::line` (it has no clock; a
  `line_at` would), Discord's `board_v2`/`summary`. A person still sees stale words there. Same helper, one call each.
- **Fold key.** There was no existing fold key; enter on the row is mine. If the owner wants a letter, it is one match arm in `normal_key`.
- **The fold's row** shows the folded conversations' summed spend at its right edge and a `·` mark.
- **hzdlf, the forum index path** has no end-to-end test of its own (a decision test of `owed()` covers it); it is the same predicate. A test that holds the place's sync (a `PlaceMsg` gate) belongs
  with the test-infrastructure row's `gate()`.
- **hzdlf, a task that was running at a restart** and had no thread gets one only if it is still running at the first sync (the old rule); if it ends before that sync it is "born before the
  binding", so it is not owed one. Accepted: the rule says an upgrade back-fills nothing.
- **The default task limit** (a quarter of the parent's): not changed. With the new words a task's `limit $1` reads as a limit, not as a cap blown through, and the start's flash names it. No change recommended.
- **dowi0 item 2** (Tab in the answer card) and herdr's keys: untouched, as the brief says.
- The tui `AGENTS.md`, the CLI's `AGENTS.md` (herdr pane) and the Discord `AGENTS.md` have a sentence each for what changed. The spec's Part III item and `docs/status.md` are the maintainer's.
