# CLOUD_REPORT: cli-tasks (theseus-mnd9g)

Branch `cloud/20261013-cli-tasks`. Commits: 6289128a (tree), 7c986ad3 (chat), 08e5c1b7 (clippy), 314b443e (fixes after the wider suite).
Note: 6289128a alone does not build (main.rs names `chat_cmd::line_mode`, added in 7c986ad3); they land together.

## What I found
- `theseus chat` already existed on main as `theseus-tui chat` (theseus-cyha), so the footer and one-key answers live there (the owner's decision 1), not in a new CLI `chat.rs`/`footer.rs`: they are `crates/theseus-tui/src/chat/tasks.rs` and `footer.rs`. The chat had y/n/t for its own question only and watched no work board.
- `work.watch` with `roots: []` sees nothing but counts as an interactive watcher; the TUI board uses it as its "screen" call.
- `origin/main` in this clone was 156 commits behind the branch base, so the keel guard failed on a foreign unsigned ack until `git fetch origin main` (no tree change).

## What I changed
1. `theseus tasks` (`render/task_tree.rs`, `work.rs`, `main.rs`): one `work.watch {limit 20}` read, a tree: conversation, one line per task (mark, short id, title, elapsed, activity: `asks: …`, `job 2:14 · leg …`, `model 0:31`, report first line), the exact answer command beneath a task that asks, footer of counts. Title column gives way first (floor 12), then activity. `--graph` and `--json` are the old `task.list` output (bytes unchanged); `--session` roots the watch; `--watch` sets `interactive: true` and prints task events; `theseus work` = `tasks --watch`. No `retry`/`dismiss` verbs exist on main, so none is named.
2. Chat (`theseus-tui/src/chat/`): `work.watch {roots:[sid], interactive:true}` at open and reconnect; a titled line when a task starts/asks/reports/fails (`task_tree::events`); footer (summary + up to 5 working/asking rows + `+N more · ctrl-t`) as a dock provider with minute clocks; question keys `y`, `n` (then optional note; esc = no note), `t`, `1`..`6` for `ask`, `v`, `tab`, `esc` = writing mode (a message starting with "y" is text); `work.answer` with author `theseus chat`, refused in a job (`refuse_in_a_job`); `/tasks`, ctrl-t, `/cancel ID`, `/stop [all]`; bell only for an Interrupt via `notices::deliver`. TUI board also opens an interactive `work.watch` with no roots.
3. Line mode: `theseus chat --plain`, a pipe or `TERM=dumb` runs `watch --interactive`, which now watches the tree and prints the same task event lines (`task_lines.rs`). One-key answers are screen-only; line mode answers with `theseus answer`.
No store change, no config key, no protocol type change.

## Proof
- `cargo nextest run -p theseus -p theseus-tui`: 615 passed (after fixes). New: `render/task_tree_tests.rs` (goldens `tests/golden_tree/tree_{80,120,200}.txt`, one line per task, command beneath, ask choices, title-before-activity, events), `tests/tasks_cli.rs` (binary: one read, `--json`/`--graph` unchanged, `--session`, `--watch`/`work` interactive + event lines), `tests/chat_plain.rs` (five `started` lines, question + commands, report, failure; opens a conversation), `task_lines.rs` tests, `chat/tests_tasks.rs` (15: five started lines + footer, +3 more, `y` sends `work.answer` author `theseus chat`, `2` -> `choice:1`, note/esc, y-as-text, refused inside a job, tab/v, /tasks, /cancel, /stop all, bell only on Interrupt, repaint on the minute not on a timer, no deadline with nothing running).
- Existing tests rewritten (not deleted): `work.rs` tree test and change-line test (new words, same assertion counts); `golden.rs` `tasks_none` runs `tasks --graph` (same golden bytes); `herdr.rs` request list gains `work.watch`; `chat.rs` sets TERM=xterm.
- Planted reverts: footer on a 1 s timer -> `the_footer_repaints_on_the_minute_and_on_no_timer` and `nothing_waits_on_the_clock_when_no_task_works` failed; one-key answers with a non-empty line -> `a_message_that_starts_with_y_never_meets_a_one_key_answer` and `chat::tests::y_is_text_when_the_line_is_not_empty` failed. Both restored and touched.
- clippy `--workspace --all-targets -D warnings`: clean at 08e5c1b7. `cargo fmt` clean. Keel guard: 0 findings after fetching origin/main.

## Not done / uncertain
- **The full gate did not finish**: one run failed in clippy (fixed), the next hit "No space left on device" in the test build (I deleted `target/debug/incremental`). The whole workspace suite, cockpit, deny and TS-generation phases were not run on the final tree. Run `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`.
- **Live check on tmux not done** (out of time). To run: scratch daemon with `theseus-sim fake-model --rules` as `api_base`, `theseus chat` in tmux 100x30, five `theseus task start` briefs under its session (one asking for `fs.write`, one failing), expect the footer rows, a titled `started` line each, `y` sending `work.answer`, and `theseus tasks` at `COLUMNS=120`.
- `theseus tasks` 6 ms p90 bench and chat startup time not measured; startup adds one `work.watch` request after `session.open`.
- Elapsed in the footer is minute-granular (`<1m`, `5m`); the tree shows `m:ss` at print time.
- Failed tasks stay in the footer's counts but not its rows (no dismiss verb to clear them).
- An `events.lost` in line mode does not re-read the task tree.
- Docs to update at review: crates/theseus and theseus-tui AGENTS.md (done in-branch); spec item and docs/status.md are the maintainer's.

Keel findings expected: none.
