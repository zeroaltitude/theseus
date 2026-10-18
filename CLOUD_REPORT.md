# CLOUD_REPORT: task-history, find any task later (theseus-ypnxt.1)

Branch `cloud/20261017-task-history`, from `main` at `40aa83cd`. Parts A, B, C and D are all done. Commits:

| Commit | What |
|---|---|
| `c9695c78` | A: `task.list` filters (protocol, core), the Discord board's and CLI's `TaskListParams` literals |
| `554073f4` | B + C: `theseus tasks --ended`/search, `task show`, `task pause`, `task resume`, `docs/cli.md`, the CLI's AGENTS.md |
| `eca43009` | D: the digest names each task; the window read; core/protocol AGENTS.md |
| `b6b6eec7` | format fix of one test the last edit left unformatted |
| `671b3fc7` | a live check inside the real-daemon digest test (task.list, `tasks --ended`, `task show` against a real daemon) |

**How the commits were made, plainly.** I wrote A to D together, then ran the gate once on the whole tree and split the
tree into commits by file (mixed files were cut at their part's seam). The gate result below is for the whole tree; the
intermediate commits were not each built and tested alone. B and C are one commit (`render/task_history.rs` holds both
parts). The maintainer merges the branch whole.

## What I found

- **Where a task's report lives.** The `TaskRecord`'s evidence entry `report:<session>` names the report's node (the
  task's last assistant message, written in the frame that ends the task: `task_graph::closed_by_report`). A task that
  failed or was stopped has no report node: its evidence `note`, or the execution's `ended_reason`, says why. The
  work board already read the evidence's note only (`push/work/views.rs::report_of`), which is empty for a task that
  reported well, so the board's `report.first_line` is the failure reason only; I did not touch it.
- **Why the digest lost its titles.** `fold_digest` folded any three reported tasks under one root into
  `📋 N tasks reported to <root>`, and the ledger fallback took the newest 2,000 rows of all its kinds together, so a
  task reported at the start of a long window was lost (the new test reproduces it).
- **`task.list` was already cheap enough.** The plain call reads every task record; the report is one point read per
  ended task, and only a filter asks for it (cost below).

## What I changed (file:line)

**A. `task.list` filters** (contract C2)
- `crates/theseus-protocol/src/tasks.rs:310` `TaskListParams` (moved from `lib.rs`; new optional `ended`, `since_ms`,
  `query`, `id`, `limit`; `narrows()` at `:346`), `:357` `TaskListResult` (new `details`, absent from the bytes when
  empty), `:372` `TaskDetail`. `lib.rs` is 2,706 lines (was 2,729; ceiling 2,733): it re-exports the three.
- `crates/theseus-core/src/rpc/task_history.rs` (new): `task_history` `:74` (the filters), `task_one` `:158` (`id`, through
  `task_graph::resolve`, with children), `haystack` `:138` (title, objective else brief, report line), `report_text`
  `:47` (the one reader of a report, used by the digest too), `first_line` `:35`. `methods.rs:757` calls it only when
  `narrows()`, so the plain call runs its old body.
- Generated TypeScript: `cockpit/src/protocol.gen/{TaskListParams,TaskListResult,TaskDetail,index}.ts`.
- **One deliberate difference from the brief:** the report line is **not** a field on `TaskInfo`; it rides
  `TaskListResult.details` (a list of `TaskDetail {task_id, report_line, report, paused}`). `TaskInfo` is built as a
  struct literal in eight places, six of them in other rows' files (Discord's render, the TUI), so a new field would have
  touched all of them. `report` (the whole text) and `paused` are filled only for an `id` read, which `task show` needs.
  If the owner wants it on `TaskInfo`, it is a mechanical move.

**B. `theseus tasks --ended [--since D] [QUERY]`**
- `crates/theseus/src/tasks_tree.rs:76` `history` (one `task.list`; `--limit` caps it, 50 by default), `:111` `ended_today`
  (the bare `tasks` that finds nothing open), args at `:30-65`; `main.rs:490` `subcommand_precedence_over_arg` so
  `tasks forecast` stays the subcommand beside the positional QUERY.
- `crates/theseus/src/render/task_history.rs` (new): `lines` `:73` (mark, short id, title, how it ended and when, spend,
  the report line cut to the width; columns aligned), `since_ms` `:124` (an age `30m 12h 7d 2w`, or a calendar day at this
  machine's midnight; the CLI had two private age parsers and no date one, so this one is new, built on `local_day_at`),
  `nothing_open` `:115`.

**C. `task show|pause|resume`**
- `crates/theseus/src/task_start.rs` `TaskCmd::{Show, Pause, Resume}`, `show` `:90`, `hold` `:106`, `hold_line` `:125`;
  `render/task_history.rs::show` `:174`; `client.rs` `OPERATORS` (+`task.pause`, +`task.resume`; the table is 84 now).
  `task.pause`/`task.resume` carry no words of their own (they return `{task_id, paused, changed}`; "Pausing …" is the
  act's internal label), so the CLI words the result; an ended task's refusal is printed as the daemon says it.
- `main.rs`: the hidden `Task` command's help names the three verbs (`help all` lists it; the everyday `help` is
  unchanged, and the `tasks` help text is unchanged so the help goldens did not move).
- `docs/cli.md`, added (the `tasks`, `task` and `digest` entries):
  > To find a task later: `theseus tasks --ended [--since 7d|2026-10-15] [WORDS]` lists the ended ones, newest first, one
  > line each with how it ended, when, what it spent and its report's first line; `theseus tasks WORDS` searches every
  > task (title, brief and report line, all the words, any case). With nothing open, `theseus tasks` says how many ended today.
  >
  > `theseus task show ID` prints one task whole (its brief, plan, children, spend, times, its whole report, and the
  > commands that act on it now); `theseus task pause ID` holds a running task at its next loop and `theseus task resume ID`
  > lets it go on (yours alone, refused inside a job).
  >
  > (digest) A task that reported is named with its report's first line (up to five under one conversation, then the rest
  > by title); a window too long to read whole says where it stopped.

**D. The digest**
- `crates/theseus-protocol/src/work_digest.rs`: `DigestLine.report`, `digest_line_with` `:175`, `NAMED_MAX` (5) `:194`,
  `rest_line` `:199`, `fold_digest` (reported bursts are never folded to a count: the first five are told, the rest are
  `📋 and N more: A, B` with at most twelve titles then `, …`), `DigestMore` `:118` + `digest_more` `:126`;
  `digest_text` appends the note. `WorkDigestResult.more` is optional.
- `crates/theseus-core/src/rpc/digest.rs`: `HARD_ROWS` (20,000) replaces `LEDGER_ROWS`; `digest_window` `:340` pages each
  kind from the window's start (`Page.after`, 500 a page, one lane a kind in turn); at the hard stop it drops the rows
  after the slowest lane's position and sets `more` (`since_ms` = that row's time, `command` =
  `theseus tasks --ended --since 14h`); `with_reports` `:314` fills `Raw.report` for both the ring and the ledger path from
  `Core::report_text` (scrubbed like the title in a guarded scope).
- `crates/theseus/src/digest.rs` prints `digest_more`. Discord's post and `/digest` use `digest_text`; the cockpit's
  plate and the chat's welcome line were not changed (see "For the joiner").

## Tests and plants

| Part | Test | Plant | Failure |
|---|---|---|---|
| A | `rpc::task_history::tests::ended_keeps_the_ended_and_the_open_apart` | drop the `ended` check | `assert!(open.tasks.is_empty() && open.details.is_empty())` (`tests_task_history.rs:137`) |
| A | `…::query_finds_a_word_of_the_title_the_brief_or_the_report` | search the title only | `assert_eq!(q("ADRIFT"), ["Chart the buoys"])` (`:170`) |
| B | `render::task_history::tests::the_list_is_a_line_a_task_with_its_end_its_money_and_its_report`, `…the_report_gives_way_to_the_width`; `tasks_cli::ended_lists_the_ended_tasks_with_their_report_lines` | print without the report line | the lines end at the money, not at the report |
| B | `tasks_cli::since_is_sent_as_a_time` | ignore `--since` | `since_ms` absent in the request (`tasks_cli.rs:460`) |
| C | `render::task_history::tests::show_of_an_ended_task_has_the_whole_report_and_no_stop_command`; `task_history::show_reads_the_task_by_id_and_prints_it_whole` | `show` without the report | `Report\n  Painted it.` missing |
| C | `task_history::pause_and_resume_send_their_own_methods`, `a_job_cannot_pause_or_resume` | `pause` sends `task.resume` | `req["method"]` was `task.resume` (`task_history.rs:176`); the job refusal named the wrong verb (`:213`) |
| D | `work_digest::tests::three_reported_tasks_are_three_titled_lines_with_their_reports`, `…past_five_the_rest_are_named_together`; core `rpc::task_history::tests::three_reported_tasks_are_three_titled_lines`, `…seven_reported_tasks_are_five_lines_and_the_rest_named` | fold back to the count (`&& false` on the new branch) | the words are `📋 3 reported to …`, not three titled lines |
| D | `rpc::task_history::tests::a_window_of_2500_rows_still_finds_the_task_reported_at_its_start` | read 2,000 rows (`HARD_ROWS = 2_000`) | `assert!(d.more.is_none(), …)` (`:409`): the read stopped short |

Each plant was restored, `touch`ed and `git status` was clean after it. The plants ran in three builds (core pair; CLI
four; digest pair).

Other tests added: `the_default_call_answers_what_it_always_did` (the plain answer has exactly `records` and `tasks`, and a
task has exactly the 18 keys it had before; `details` absent), `since_cuts_by_the_time_a_task_ended`, `id_names_one_task…`
(by the end of its id, whole report, not-found code), `limit_takes_the_newest`, `nothing_open_says_what_ended_today`
(both the old words from an older daemon and the new ones), `forecast_is_still_a_subcommand`, `since_reads_an_age_or_a_day`,
`the_ledger_note_says_where_the_read_stopped`, `past_the_hard_stop_the_digest_says_where_it_stopped`, and the real-daemon
check in `theseusd/tests/digest.rs`.

**The default call's JSON is pinned by key set, not by a copy taken before the change.** `TaskInfo` and `TaskRecord` are
untouched, so the 18 task keys are today's; I read them from the unchanged code (the first run printed them) rather than
from a stash of the old build, which the disk could not hold. `details` has `skip_serializing_if = "Vec::is_empty"`.

## Before and after

Bare `theseus tasks`, nothing open, three tasks ended today (`tasks_cli::nothing_open_says_what_ended_today`):
```
before:  no tasks
after:   no open tasks · 3 ended today: theseus tasks --ended
```
(A daemon that cannot answer the second read, or has none to count, still prints `no tasks`.)

Three reported tasks under one root (`work_digest::tests`, `rpc::task_history::tests::three_reported_tasks_are_three_titled_lines`):
```
before:  📋 3 tasks reported to harbour survey
after:   📋 Chart the buoys reported: Charted 41 buoys; two were adrift.
         📋 Paint the quay reported: Painted the quay railings green.
         📋 Sound the channel reported: Soundings done.
```
Seven: five of those lines, then `📋 and 2 more: Task 6, Task 7`. The summary still counts every task (`◆ 7 reported`), so
the chat's welcome line (`digest_summary`) is unchanged. A window of 2,500 ledger rows after a reported task: before, the
task was dropped (newest 2,000 across kinds); after, it is found, and `more` is `None`.

`theseus tasks --ended` (aligned; from the unit test): `✓ a1b2c3  Paint the quay     done 30m ago        $0.40  Painted the quay railings green.`

## Read cost, on a store of 5,000 tasks

`rpc::task_history::tests::the_report_line_costs_this_much_on_5000_tasks` (ignored; `--run-ignored only --no-capture`; a debug build):
`5000 tasks: records read (the plain call's cost) 62.8 ms; their report lines 168.9 ms (33.8 µs each)`. So a filtered read
adds about 0.17 s per 5,000 ended tasks, one node read each; the plain call adds nothing. It measures the task records and
the report reads, not the execution scan `tasks()` already does for the plain call.

## Budgets

- Owner-config fixture (`tests_first_request::every_kind_of_place_is_within_its_budget`, `--no-capture`): cli 7,353 of 8,000;
  task 6,606 of 8,000; dm 9,432 of 15,000; channel 7,391 of 7,900; headless 7,867 of 8,600. The owner's shape: cli 7,426;
  dm 9,710; task 6,679; channel 7,597; headless 7,940.
- Turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`): frames_plain 4 (budget 4), frames_tool 8 (8), frames_batch
  14 (14), syncs_plain 2 (2), syncs_first_byte 2 (2), all `ok`; first-request tokens cli 7,332, task 6,585, dm 9,411.
- No model tool, no always-loaded text and nothing on a plain turn's path was added: every new read is a command, a key or a
  page. A plain turn stays 4 frames (the bench).

## Overlaps

Lines I touched in other rows' files, each small:
- `crates/theseus-discord/src/runtime/board.rs:122` `..Default::default()` on the `TaskListParams` literal (discord-tasks' area).
- `crates/theseus-discord/src/runtime/tests_digest.rs:30` `more: None` in the `WorkDigestResult` literal.
- `crates/theseus/src/main.rs:490` one attribute on `Tasks`; the hidden `Task` command's doc; `crates/theseus/src/client.rs` two `OPERATORS` rows and the length 84; `crates/theseus/src/cmd.rs:1459` the literal's `..Default::default()`.
- `crates/theseus-core/src/tests_task_graph.rs`: the scripted model's report is now a `Mutex<String>` with `Model::report_with` (a test helper used by my tests).
- `crates/theseus-protocol/src/ts.rs`: `tasks::TaskDetail` and `DigestMore` added to the existing type-list lines (no new line).
- `crates/theseusd/tests/digest.rs`: the two assertions on the reported words (old `…Count to three. reported`, new
  `…Count to three. reported: Nothing to do.`) and the live check.
- Names a landing lane renames: none used. I read tasks through `task.list` with `id`, never `task.get` (`task_get`/`TASK_GET` untouched); I print `theseus answer ID …`, never `confirm`; wire names are as on `main`.
- Generated: `cockpit/src/protocol.gen/` (committed with the types). The maintainer regenerates it at the merge.

## Proof nothing was lost

- The plain `task.list`, `work.digest` for kinds other than reported, `tasks`, `tasks --json`, `tasks --graph`, `tasks --watch`, the help
  screens (the everyday and `help all` goldens pass unchanged) and `digest_summary` are as they were: their tests all pass.
- `DigestLine.report`, `WorkDigestResult.more`, `TaskListParams`' filters and `TaskListResult.details` are optional with defaults and absent from the bytes when unset.
- Old assertions converted to the wording this task orders (shown): `protocol/tests_work_digest.rs` `a_burst_of_three_folds_into_one_line_under_its_root`
  (`📋 3 tasks reported to harbour survey`, count 3) became `a_burst_of_three_failures_folds_into_one_line_under_its_root` (`✗ 3 tasks failed under harbour survey`, count 3: the fold itself is unchanged for the other kinds) beside the new reported tests; `a_burst_keeps_the_order_of_the_other_lines` uses a cancelled burst; `theseusd/tests/digest.rs` as above. No test was deleted, ignored or weakened, no budget or ceiling raised.
- Keel guard against the base I was given (`python3 scripts/keel-guard.py --base 40aa83cd --head HEAD --quiet`): **empty**. Against the current `origin/main` (`c579c49b`, which has moved since) it lists eight erosions that are `main`'s newer tests missing from this older branch, not changes of mine.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the whole tree: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe,
cockpit, test build, reader rule **passed**. The suite: 6,468 run, 6,434 passed, **34 failed**: 33 are the known L1 failures
(theseus-sandbox `contract` ×19 and bench `spawn_100`; theseusd `sandbox` ×12 and its `the_jobs_bench_l1_row`; the VM runs as root,
theseus-pv6i), and **1 was mine**: `theseusd::digest::what_happened_while_a_client_was_away_is_told_once_from_where_it_left`
asserted the old `… reported` words; I converted it (above) and it passes, with the new live check, on the final tree. By hand after the suite:
protocol types clean (`git diff cockpit/src/protocol.gen` empty), turn bench ok (above), `cargo deny check bans licenses sources` ok
(advisories not checked: no database fetched here), clippy `-D warnings` on the final tree clean, `cargo fmt --check` clean.
Not run: the lifecycle and jobs benches (`THESEUS_GATE_NO_BENCH`), the size step (no release build), a second full suite after the last
two commits (they change one test file and its formatting; I ran that test). No flaky test needed a retry; the Discord test
`tests_task_threads::react::in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing` timed out once under a parallel `-p` run and passed in the gate.

## Live check for the maintainer

On a scratch daemon with some ended tasks (a fresh state dir, `--socket`):
1. `theseus tasks` with nothing open and a task ended today: `no open tasks · N ended today: theseus tasks --ended`.
2. `theseus tasks --ended --since 7d`: one line a task, newest first, a report line at the end; `theseus tasks --ended --json` is the `task.list` answer with `details`.
3. `theseus tasks <a word from a report>`: only the tasks holding every word; `theseus tasks forecast` still prints the calibration.
4. `theseus task show <short id>`: state, `reports`, brief, plan, spend, started/ended, the whole report, then the commands; on a running task `theseus task pause <id>` then `show` says `paused` and offers `resume`; `theseus task resume <id>`; on an ended one `pause` prints the daemon's refusal (`Task … has ended (complete).`) and exits 1.
5. `theseus digest --since 24h` after three tasks reported: three titled lines with their report lines. In Discord, the 08:00/18:00 digest post and `/digest` show the same lines.
(The real-daemon test `theseusd::digest` already runs 2-4 and 5 against a real daemon and the real CLI.)

## What is left, uncertain, or the owner's to hear

- **`--since` is also a filter on open tasks** (`updated_at_ms`), as the brief says; `theseus tasks --since 1d` with no `--ended` lists open and ended tasks updated since then.
- **`theseus tasks WORDS` searches open tasks too**, not only ended ones: "find any task later".
- **The digest's `more` note is told by the CLI and `digest_text` (Discord's post and `/digest`)**, not by the cockpit's plate (`sincedigest.ts` draws `words` lines only) or the chat's welcome line (counts only). That is one TypeScript line each if wanted; I stayed out of `cockpit/src/` (cockpit-tasks owns it).
- **A hard stop reads from the start.** The lines are whole up to the time the read reached; `more` says when and gives `theseus tasks --ended --since Nh` (days from two days up). While the ledger index's shape is still being built after a start, the fallback scans the last 20,000 rows and sets no `more` (the old behaviour, a bigger window).
- **The generic "N reported to ROOT" count wording is now reached only by `digest_words` called directly**; `fold_digest` no longer produces it. Left in place (a count word for the kind) rather than deleted without a reader.
- `task show` of a plan item (no session) says it has no run to show; `task.list id` still returns the record.
- Disk: this VM's allowance (about 40 GB) was spent by debug builds; I removed stale duplicate build artifacts under `target/` and ran with `CARGO_INCREMENTAL=0`. Nothing under `/root/.rustup` or `/root/.cargo` was touched.

## For the joiner

- Store format unchanged (no stored record changed): `TaskRecord`, `TaskInfo` and the ledger kinds are as they were; `HARD_ROWS` and the filters are read-side.
- After merging with progress-words, nothing in `render/task_tree.rs` was touched. My list uses `usd`, `coarse`, `short_id` from it.
- When discord-tasks joins, `/tasks show:done` can switch to `task.list {ended: true, since_ms, limit}`; chat-start-pause's picker can pass `query`.
- Regenerate `cockpit/src/protocol.gen/` at the merge (the protocol's test does it); `TaskListParams`, `TaskListResult`, `TaskDetail`, `DigestLine`, `DigestMore`, `WorkDigestResult` change.
- `ts.rs`'s type list got two names on existing lines; `lib.rs` has 27 lines of room more than before.
