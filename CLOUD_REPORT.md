# CLOUD_REPORT: cli-tasks round 2 (theseus-mnd9g)

Branch `cloud/20261013-cli-tasks-r2`, built on main as cloned (`baaa598f`), round 1 (`b6cfc4b0`) merged in with a plain
merge. Commits:

- `c6a956e0` the merge and the re-integration (one commit: a merge commit, so the conflict resolution is its diff).
- `cd691854` a live-check finding: a task's question was told twice in the transcript.
- this report's commit, last.

No store change (no `MANIFEST_FORMAT` bump), no protocol type, no config key, no new dependency (`Cargo.lock` unchanged).

## What the review asked, point by point

### 1. The chat's questions, twice: one model now

Chat-ask's model stays and is extended; round 1's second model is deleted (`Pending`, `footer::question_rows`, `Board::{focus,
writing, note}`, its own Tab/Esc/`y`/`n`/`t` handling).

- `chat/ask.rs` holds one question model. `Asked::{Own(ConfirmRequest), Task(TaskQ)}`: the conversation's own question, and the
  questions this conversation's tasks ask (`tasks::TaskQ`, listed from the board's views). `Chat::asked()` lists them (own
  first, then the tasks', oldest asked first), `Chat::current()` is the one that has the keys.
- One dock: `ask::rows` draws either with the same rows (a task's title says who asks: `? Paint log asks: …`, then its expiry,
  then the keys). `dock.rs` has `ask::rows` once; the footer of working tasks stays its own provider above it.
- One key table. `keys::ASK_KEYS` gains six rows, `'1'..'6' => Ask::Choice(0..5)`; the existing test that holds the table to
  unique keys covers them. `y n v t` mean the same on either kind; `a`/`e` (grant) stay the conversation's own. On an `ask`
  question (choices) `y`/`n` are text, as round 1 had it; `1`..`6` on any other question is text (a message that starts with a
  number is a message).
- Answers: the conversation's own is `action.confirm` (chat-ask's, unchanged); a task's is `work.answer`, author `theseus chat`,
  refused inside a job (`client::refuse_in_a_job`). `n` takes the optional note on either (chat-ask's flow: the line is the
  note, Enter sends, Esc goes back; round 1's "Esc declines without a note" is gone: Enter on an empty note declines
  without one).
- Esc keeps chat-ask's meaning (back to typing; the question is one compact row; `tab: answer it`). Tab: with the line empty, after
  Esc it gives the question its keys back (chat-ask's); otherwise, with more than one question waiting, it moves the keys to the
  next (`AskState::focus`; the dock says `tab next (N more)`).
- `AskState::seen` resets the answering state (`sent`, `note`, `typing`) exactly when the question that has the keys changes
  (`ask_sync`, called from `ask()`, the resolve, and every board change), so a second key while an answer is in flight is no
  second answer, for either kind.
- `/stop` is one command: Esc's stop (`stop_turn`, `keep_jobs: true`); `/stop --tasks` (alias `all`) also sends `tasks: true`,
  as `theseus stop --tasks` does, refused inside a job (`refuse_stop_all_in_a_job`). The Esc stop itself is untouched.
- The status line's `◐ N tasks` is the board's count once the board is live; the older per-task-event count stays the
  fallback for a daemon without `work.watch` (`Chat::task_changed`, in `tasks.rs`).

### 2. The merge on main

13 files of conflicts, all resolved: the chat (`mod.rs`, `state.rs`, `events.rs`, `dock.rs`, `keys.rs`, `commands.rs`), the TUI's `app.rs`
(main already opens its `work.watch` with `interactive: true`, so round 1's `Purpose::Screen` request is dropped), the CLI's `main.rs`,
`chat_cmd.rs`, `interactive.rs`, `work.rs`, and the two `AGENTS.md`.

**A deviation the owner should hear about.** Round 1 made `theseus work` an alias of `theseus tasks --watch`, replacing the board's
reader. On main, `work.rs` has since grown: threads (`board_lines`, `--flat`), the job-follow last line and the job ids `peek` and
`attach` take (`theseus work` is where a person reads them), tests in `theseusd/tests/work.rs` and `thread_board.rs` that run the real
`theseus work`. So **`work.rs` is main's file again, untouched but for `notice_line` made `pub(crate)`**, and round 1's tree and
watch are a new file, `tasks_tree.rs`, behind `theseus tasks` (`#[command(flatten)] args: TasksArgs` beside the `forecast`
subcommand, whose dispatcher now hands the plain case to it). `theseus work` is therefore **not** an alias of `tasks --watch`;
if the owner wants the alias, it is one line in `run`, at the cost of the thread view and the job ids. Say which.

Also from main: `Cmd::Chat` has `-c`/`-r` and now `--plain`; `run()` in `main.rs` is at clippy's 100 lines, so line mode is
reached from `run_or_own`, not from `run` (`chat_cmd::line_mode(cli)` connects for itself with `crate::connect`).

### 3. The tree uses main's words

`render/task_tree.rs`: the activity is the job's own last line (`job 2:14 › leg 5: 4.5 fathoms`, from the job view's `doing.last_line`),
and the plan in the one wording every surface shares, `Progress::line` (`· 4/9 · sounding the channel`; round 1 read `current`,
which nothing else shows). A failed task, not dismissed, names `theseus retry ID · theseus dismiss ID` beneath its row, in the
watch's event lines and in line mode (`task_tree::failed_commands`, `event_commands`); the verbs exist on main (retry-dismiss).
An activity is held to 60 columns in the layout so a long plan line does not squeeze the titles (`ACTIVITY_MAX`).

## What was proved

Environment notes first: this VM's `origin/main` ref in the clone was older than the clone's base, which made the keel guard judge main's
newer commits (whose signatures it cannot verify here) as mine; I ran `git fetch origin main` (read only) and the guard then judged
`baaa598f..tree`: `keel: ok … 0 findings`. The disk filled twice (a debug build with symbols is 28 GB here), so the final gate ran with
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` after a `rm -rf target`; nothing else differs.

### Tests

- `cargo nextest run -p theseus-tui -p theseus` (before the last commit): **849 passed**; the row's own tests (`chat::`, `task_tree`,
  `task_lines`, `tasks_cli`, `chat_plain`, `golden`): **210 passed, run 5 times in a row, no flake**.
- New or changed tests of this round: `tests_tasks.rs` (round 1's 15, moved to the merged keys, + `the_conversations_question_and_a_tasks_share_one_dock`,
  `a_digit_is_text_on_a_question_without_choices`, `tab_goes_round_the_questions`, `a_long_question_is_told_once`; `/stop --tasks`
  asserted with `keep_jobs`), `task_tree_tests.rs` (+ `a_failed_task_names_retry_and_dismiss`; the three goldens in
  `tests/golden_tree/` regenerated: job last line, plan wording, the retry line), `tasks_cli.rs` (`watch_says_each_task_event_and_is_a_screen`
  also asserts the failure's commands).
- **Under load** (`nice -n 19` beside four busy loops at nice 0, 3 runs of the row's tests): 2 runs clean, 1 run had one failure,
  `theseus-tui chat::picker::tests::a_thousand_sessions_filter_well_inside_a_frame` (7.4 s; chat-sessions' timing test, on main, not
  this row's; it passes 5 of 5 unloaded). Not on the flaky list; **it should be**, or its bound relaxed by its owner (a finding, not mine).

### Planted reverts (each: plant, `nextest -p theseus-tui -E 'test(chat::)'`, restore, `touch`, `git status`)

| Plant | Caught by |
|---|---|
| `y` meets a typed message (the empty-line check in `key()` off) | `tests_tasks::a_message_that_starts_with_y_never_meets_a_one_key_answer`, `tests_ask::a_character_that_is_no_answer_starts_the_message_and_tab_gives_the_focus_back`, `tests::y_is_text_when_the_line_is_not_empty` |
| the footer repaints each second (`repaint_at` always the next second) | `tests_tasks::the_footer_repaints_on_the_minute_and_on_no_timer`, `nothing_waits_on_the_clock_when_no_task_works` |
| a task's answer goes through `action.confirm` instead of `work.answer` | 5: `a_task_asks_by_name_and_y_answers_it_as_theseus_chat`, `a_digit_answers_an_ask_question_with_its_choice`, `tab_moves_to_the_next_question_and_v_shows_the_call`, `n_takes_a_note_and_esc_goes_back_to_the_question`, `the_conversations_question_and_a_tasks_share_one_dock` |
| a second key while an answer is in flight (the `sent` guard off) | `tests_ask::y_confirms_with_the_author_theseus_chat_and_the_dock_paints_in_one_frame`, `the_conversations_question_and_a_tasks_share_one_dock` |
| a digit is a choice on any question | `tests_tasks::a_digit_is_text_on_a_question_without_choices` |
| a task's question told again after its event (the live-check bug) | `tests_tasks::a_long_question_is_told_once` |

### The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (the last run, after the last code change): door, keel (0 findings), keel tests, fmt,
shape, features, clippy (`-D warnings`), cockpit (lint, test, build), test build, reader rule: **ok**. The suite: **5,975 run, 5,942 passed,
33 failed, 47 skipped; the 33 are exactly the known L1 failures** (`theseus-sandbox::contract` 19, `theseus-sandbox::bench spawn_100`,
`theseusd::sandbox` 13: the root VM, theseus-pv6i); no other test failed. The phases after it, by hand, as `machine_checks` has them:
protocol types (`cockpit/src/protocol.gen` unchanged: ok), the turn bench `--check` (`frames_plain 4/4`, `frames_tool 8/8`,
`first_request_tokens_cli 7519/8000`, `dm 14933/15000`, `task 6775/8000`, `channel 7217/7900`, `headless 8036/8600`: ok, no token added to any
always-loaded set), `cargo deny --offline check` (ok). Not run: the lifecycle and jobs benches (`THESEUS_GATE_NO_BENCH`), and the size check (no
release build).

`scripts/long-files.txt`: **I deleted the `crates/theseus/tests/golden.rs` entry.** Round 1 had grown that file three lines past its
ceiling by rewriting one golden call; I folded the two task goldens' shared step into a closure (2509 -> 2498 lines, under 2,500), and the
gate then demands the entry go (a shrink; a loosening was not needed).

## The live check (done here, on a scratch daemon with the stand-in model)

`theseus-sim fake-model --rules` scripts five tasks; a scratch `theseusd` (own config, socket and state dir), `theseus chat` in tmux at 100x30.
Captured (trimmed to the dock):

```
 ◐ tasks · 3 needs you · 1 done                         <- after `y` on the first question: "Check the rigging.: approved (approve)"
 ● 4c4bac  Count the buoys.        2m  asks: proc.run run `sleep 170` in /tmp/…
 ● 7067fe  Check the tide tables.  2m  asks: proc.run run `sleep 170` in /tmp/…
 ● c633a1  Paint the log.          2m  asks: fs.write create /tmp/…
 ? Count the buoys. asks: proc.run sleep 170?
   expires in 12:29
   [y] approve · [n] decline · [v] view · tab next (2 more) · esc type
```

`Tab` moved the keys to "Check the tide tables."; `n`, a note and Enter sent the decline with the note (`  Check the tide tables.: declined
(decline)`, the task ended); `y` on the buoys approved a job, and the footer read `◐ 4c4bac  Count the buoys.  2m  job <1m`. `theseus chat -c`
told each waiting question once from the snapshot. `theseus tasks` (COLUMNS=120):

```
○ df319b  Please start five tasks. First, paint t…
  ● c633a1  Paint the log.                            2:53  asks: fs.write create /tmp/claude-0/-home-user-theseus/ae87…
            theseus answer 0a9877 approve · theseus answer 0a9877 decline --note …
  ◐ 4c4bac  Count the buoys.                          2:53  job 0:05
1 needs you · 1 working
```

The live check found one bug the scripted tests did not (a question with a long prompt was told twice: `cd691854`, with its test and plant).
`theseus tasks --json` and `--graph` print the task list as before; `theseus tasks forecast` and `theseus work` work as on main.
FAST: `theseus tasks` is one `work.watch` read; the whole process (debug build, tiny store) took p50 13.5 ms, p90 17.3 ms, the same as
`theseus health` (p90 17.6 ms), so the tree adds no measurable work to one request. Not measured on the bench's store.

### What the maintainer should run (the owner's keys are not needed)

```
cargo build -p theseus -p theseus-tui -p theseusd -p theseus-sim
python3 mkconfig.py …   # or any scratch config with [model]/[providers] api_base at the stand-in; [policy.tools] fs.write = "approve"
target/debug/theseus-sim fake-model --addr 127.0.0.1:9448 --rules rules.json &   # five task_create calls, task rules first
target/debug/theseusd --config c.toml --state-dir s --socket sock &
tmux new -d -s live -x 100 -y 30 'THESEUS_SOCKET=sock target/debug/theseus chat'
```

Then type "Please start five tasks. …" (quotes of 20+ characters in the rules), and expect: a footer `◐ tasks · N needs you`, one row a
task, one question in the dock with `tab next (N more)`; `y`, `n` + note + Enter, Tab, `v`, `Esc` (one compact row, `tab: answer it`);
`/tasks`, Ctrl-T, `/cancel ABCD`, `/stop --tasks`; `theseus chat --plain`, and a pipe, print task event lines instead.

## Left open, and docs to change

- Not done: the tree on the TUI's sidebar (tui-rows is main's and untouched); a one-key answer for a question that asks for a free-text
  choice (`choice:other`) stays `theseus answer ID …`.
- The `/` command list shows six rows, so `/tasks`, `/cancel` and `/stop` are after `/quit` (main's test needs `/quit` among the six);
  they are in `/help` and complete as you type. Raising the list's rows is chat-ask's decision.
- Docs: the spec's Part III item for this row and its version line; `docs/status.md` (the roadmap row; `theseus tasks` as the tree;
  `work` is not the alias); the two `AGENTS.md` files are updated in this branch (`crates/theseus/AGENTS.md`, `crates/theseus-tui/AGENTS.md`).
- Keel: the merge deletes nothing from a test or a budget; the only ceiling edit is the shrink above.
