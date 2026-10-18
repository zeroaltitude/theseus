# CLOUD_REPORT: acceptance-journey (theseus-ypnxt.8, theseus-h9tu1)

Branch `cloud/20261017-acceptance-journey`, cut from main `40aa83cd`. Three work commits, then this report:
`b82f023f` (the sim's `react`), `32ec0425` (the journey test), `8630ce2e` (the live script and its first report).

## What changed

**A. `theseus-sim discord react`** (`b82f023f`)
- `crates/theseus-sim/src/discord_cli.rs:69` `Cmd::React { fake, channel?, message, emoji, user }`, run at `:187`, beside `Press`.
- `crates/theseus-sim/src/fake_discord/reactions.rs:74` `control_react`, the body of `POST /_fake/react`; one route line at
  `crates/theseus-sim/src/fake_discord.rs:1909`. It calls the existing `FakeDiscord::react`; without `channel` the message is in
  the user's DM (the fake's DM channel is the user's id plus one).
- Test `reactions::tests::a_react_on_a_bot_message_reaches_a_binding_as_a_reaction_add` (in the same file).
- `crates/theseus-sim/AGENTS.md`: the `discord` line names `react`.

**B. The journey** (`32ec0425`, tweaked in `8630ce2e`): `crates/theseusd/tests/acceptance_journey.rs` (1,057 lines), plus a
paragraph in `crates/theseusd/AGENTS.md`. No helper added to `tests/common/` (only `Served` and `table` are used from it);
nothing in it is shared with another test yet.
- `rules` (`:50`): the scripted model. `Model` (`:84`): `theseus-sim fake-model` as a child, which dumps each request.
- `Rig` (`:225`): the daemon on the DM and a private `#lab`, the in-process fake Discord, the CLI, the gates, the readers.
  `Watcher` (`:172`): a bare `work.watch` client that stamps each `work.changed` against its `at_ms`.
- `Journey` (`:472`, `:483`, `:874`): one method for each step. `the_owner_starts_asks_answers_joins_pauses_ends_and_finds_two_tasks`
  (`:915`) runs them all; `a_change_reaches_a_watching_client_in_ten_ms_at_p95` (`:924`, ignored: real time) runs them again with
  the 10 ms bound.
- Seven ignored tests for the steps another row builds (list below).

**C. `scripts/acceptance/journey.sh`** and its first report `bench/results/acceptance-journey-2026-10-18.md` (`8630ce2e`).

## The journey, from one run (rig's clock, ms)

| step | at | what it measured |
|---|---:|---|
| 1 start | 1,132 | DM request starts the quay task; `theseus task start` starts the gulls task; each has a six-character short id |
| 2 appear | 1,174 | `theseus tasks` 18 ms; the quay post 17, card 0, board row 5; the gulls post 0, board row 0 |
| 3 ask | 11,051 | quay card 84 ms after the question was asked, gulls 0; board "Needs you"; **presence "needs you" 7,864** (the status line is paced to one write in 10 s, so this step waits for it) |
| 4 answer elsewhere | 11,250 | CLI answer: the Discord card shows "by the CLI" in 0 ms (23 ms in the exploration run), the board row leaves Needs you in 0; the Discord press on the gulls card: `theseus tasks` cleared in 45, the card shows "by discord:ana" in 9 |
| 5 join | 11,250 | typed in the post while the quay task's job holds at its gate; the join is in the task's history 40 ms after the job ends and in the model's next request 77 ms after |
| 6 pause and resume | 13,128 | the pause reaction: the work view reads `paused` in 25 ms; resume 28 |
| 7 end | 13,232 | both reports in the DM 97 ms after the last gate opens; the post ends `✅` with a different tag |
| 8 digest | 13,246 | `theseus digest` lists both |
| 9 find | 13,259 | `theseus tasks` with nothing open |

The journey takes about 13 s (unloaded); 8 of 8 consecutive runs passed (and 4 of 4 before the gate-file change).

**Interrupts per surface** (h9tu1 3). Discord: 2 question cards and 2 finish reports ring; no failure; the board, the forum card,
the `🧵` pointer and the `-# ↩ from the DM` line are created silent (asserted). The chat's own rings, counted apart and never
judged: 10 in this run (the two bind notices, the DM turn's tool line and reply, and each task's tool lines and replies in its
post). Closest two rings in one conversation: 1,979 ms (two reports in the DM). herdr `blocked` transitions: not run (below).

**Still fast** (h9tu1 6). A plain turn: 4 frames (the bench's `PLAIN_TURN_FRAMES` is 4; the brief says 5). A change to a watching
protocol client: p95 15–17 ms in the journey's own run (debug build, 4 cores, the daemon and the stand-ins busy), 8 ms in
the real-time test's run (44 changes). The 10 ms bound is the ignored real-time test's; the gate's run holds 100 ms.

## Tests and plants

Plants were run one at a time, the file restored and `touch`ed after each; `git status` clean after each.

| step | plant | the assertion that failed |
|---|---|---|
| A | `control_react` sends the reaction as the bot's own (`BOT_ID`) | `a_react_on_a_bot_message_…`: `left: "1553557000000000001", right: "7"` |
| 2, 3, 4 | `courier/v2.rs` `board_v2`: never edit an existing board | `no the board's Needs you within 4000 ms` (step 3) |
| 4 | `courier.rs` `settled`: the settle of an answered card is dropped | `no the card to show the CLI's answer within 1000 ms` |
| 5 | `[kernel] steer_queued = false` (the join waits to run as a turn after the task's own) | `no the join in the task's history within 2000 ms` |
| 6 | `task_react.rs` `hold`: a pause does nothing | `no the view to read paused within 1000 ms` |
| 7 | `courier.rs` `report_post`: no report post | `no both tasks finished within 25000 ms` |
| 7 | `forum.rs` `set_tag`: the done tag is never set | `no the post's final name and tag within 4000 ms` |
| 8 | `crates/theseus/src/digest.rs`: the digest prints nothing (the `theseus` binary rebuilt; `-p theseusd` alone does not rebuild it) | `step 8: the digest names Survey the north quay.` |

Not planted: step 1 (the short id is whatever the CLI prints) and step 9 (a negative: nothing open), and step 3's card buttons
(the labels assertion reads them, but I did not break the render).

**Under the load recipe** (the test at nice 19 beside four nice-0 loops; the daemon inherits the niceness): in four runs
every step kept its order and every failure was a time bound: the step 3 card past 2 s, the step 2 board row past 2 s, and
the change-latency p95 of 241 and 634 ms. Before the gate-file change, one loaded run lost the join to a fixed 2.5 s model
window; that race is gone (the window is a job at a gate the test opens). `THESEUS_JOURNEY_SLOWDOWN=8` also failed there, on the
p95 bound alone once the order held, so that bound is now the real-time test's.

## Budgets

The turn bench (`theseus-sim bench turn --check`, 5 runs): the plain turn within its frames; first request in tokens at the
p95: `dm` 9,411 of 15,000, `task` 6,585 of 8,000, `headless` 7,846 of 8,600, `cli` and `channel` ok. No model tool, no
always-loaded text, nothing on a plain turn was added: this row adds no product code.

## Overlaps

- `crates/theseus-sim/src/fake_discord.rs:1909`: one route line (`("POST", ["react"]) => …`) in the shared fake. The file was
  2,449 lines and is 2,450.
- No file of another row is edited. Names used that a landing lane renames: `theseus answer ID …` (not `confirm`); `task.list`
  is the only task read (no `task.get`); `execution.*` is not used.

## Findings (nothing fixed: each is outside this row)

1. **A CLI task started before the DM has a session is homed in a fresh "Reminders" Home, not the DM**, and gets no forum post
   and no board row (only its question reaches the DM). Started after the DM's conversation exists it is the DM's and gets both.
   The journey starts the CLI task after the quay task exists. `crates/theseus-discord/src/runtime/forum.rs` takes only tasks of
   an eligible place. No test: it is an ordering the brief's step 1 does not say.
2. **With `ping_window_secs = 10` a question can ring silent**: the DM's chat reply used the window first, so both question
   cards were created without a ring. The default (0, off) and the review's recipe are unaffected. The brief's "at most one
   ring per conversation per 10 s" is therefore recorded (closest: 1,979 ms, two reports) and not enforced by the product.
3. **A message typed into a task's post is steered in at the task's next loop**, not at once: with the task's job holding, the
   join is in its history when the job ends. The proof that it is not a queued turn is the report, which carries the join's
   answer (step 7). The brief's "the history shows it" is true from that moment.
4. **The status line goes from nothing to "🔴 N needs you"**: both tasks ask within a second, and the line is written at most
   once in 10 s, so a "⚙️ 1 running" moment is not catchable on a fast stand-in. Asserted: "needs you" within 15 s.
5. **`theseus tasks` with nothing open prints `no tasks`** and points at nothing. Step 9 as the brief words it is the ignored
   task-history test (`--ended`, and the footer's pointer).
6. **The board row can follow an edit by just over 2 s** (`BOARD_EVERY`); the journey allows 2.5 s for it, 2 s for everything else.
7. This VM's clone has `origin/main` at `9fa84735`, older than `40aa83cd`, so the gate's keel range counted the trim batch as
   this branch's work. I ran every gate with `THESEUS_KEEL_BASE=40aa83cd`. The disk (a fixed allowance) filled three times with
   debuginfo and incremental caches; the last gate ran with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` on a clean `target/`.

## Not run, and why

- **herdr** (a tab for each task, `blocked` while a task asks, marked done at the end): the scripted herdr is a model inside
  `crates/theseus/tests/herdr_up.rs`, not importable into theseusd's tests, and herdr is not installed here. The script prints
  whether it is installed and says its steps were not driven; the maintainer follows a scratch daemon in a herdr session
  (`theseus --socket <scratch sock> herdr up --follow --herdr-socket <its socket>`) while the script runs. h9tu1's checks 1, 2, 3
  and 6 on the herdr surface are therefore open.
- **The chat's `Conn::over` rig**: the TUI's, not theseusd's. The `/task new` step is an ignored test through `theseus chat --plain`.
- **The installed build**: `scripts/acceptance/journey.sh --profile release-thin` runs the install build's binaries (nextest builds the
  tests with that profile). Not run here (the owner's OK is needed, and a release build does not fit this VM's disk).

## Ignored steps, each with the row that removes the ignore

All in `crates/theseusd/tests/acceptance_journey.rs`; delete the `#[ignore = …]` line at the join of the named row.

| line | test | removed by | asserts |
|---|---|---|---|
| 923 | `a_change_reaches_a_watching_client_in_ten_ms_at_p95` | never (real time: the script runs it) | p95 ≤ 10 ms |
| 946 | `tasks_with_nothing_open_points_at_the_ended_tasks` | task-history (.1) | `theseus tasks` footer names `theseus tasks --ended`; `--ended` and `--ended gulls` list the task, `--ended quay` does not |
| 962 | `the_digest_names_each_task_with_its_report_line` | task-history (.1) | a digest line holds the title and the report line |
| 978 | `the_progress_line_is_on_the_board_row_and_in_tasks` | progress-words (.2) | `$0.03 of $25` on the board row and the `theseus tasks` row (the cost part of C1; no plan steps in this script, so no `step N of M`) |
| 996 | `a_paused_task_says_paused_first_and_the_cli_pauses_and_resumes_it` | progress-words (.2) and task-history (.1) | `theseus task pause|resume ID`; `paused · $0.03 of $25` on the board and the card |
| 1023 | `the_report_links_the_tasks_post` | discord-tasks (.5) | the report holds `<#post id>` |
| 1047 | `a_task_started_in_the_chat_has_a_short_id_and_a_title` | chat-start-pause (.4) | `/task new Count the terns` in `theseus chat --plain` prints the short id and the title, and the task exists |

Un-ignore note: `a_paused_task…` pauses a task that is asking; if pane-answers (.3) or progress-words changes where the progress
line sits, only the strings change. The pane-answers (.3) step (a herdr pane answering `owner_ask` by `2`) has no test here:
it is herdr's, and belongs in the script's herdr part.

## Proof nothing was lost

No product code changed. The sim gained a route and a CLI command; every existing sim and Discord test passes in the gate.
`python3 scripts/keel-guard.py --base 40aa83cd --head HEAD --quiet` prints nothing (exit 0). No dependency, no `Cargo.lock`
change, no store-format change, no spec/status/README/design edit.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=40aa83cd scripts/gate.sh`, last run on `8630ce2e` (clean build,
`CARGO_PROFILE_DEV_DEBUG=0`): door, keel, fmt, shape, features, clippy and cockpit pass; the suite ran 6,435 tests and 6,400
passed. Failing: the 33 known L1 tests (19 `theseus-sandbox::contract`, `theseus-sandbox::bench spawn_100`, 13 `theseusd::sandbox`:
the VM runs as root, theseus-pv6i), and two that pass alone on rerun, `theseus::herdr_up
a_finished_tab_closes_a_grace_after_it_is_seen_and_a_cancelled_one_at_once` and `theseus-core
tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future` (load; the second is a neighbour of an earlier
run's `the_agenda_expands_a_series_…`, which also passed alone). `acceptance_journey` passed in the suite. After the suite, by hand:
the generated TypeScript is unchanged (`git status` clean), the turn bench passes (above), and `cargo deny --offline check`
reports `advisories FAILED, bans ok, licenses ok, sources ok`: the advisory database fetched today holds an advisory against
a dependency of the tree as cloned (Cargo.lock is untouched by this branch).

## For the joiner

- Remove the ignores at each join as the table says. The journey's assertions of today's words that those rows change:
  `step 8` asserts the digest holds each title and `reported`; `step 9` asserts nothing open is shown; the pause step asserts the
  card holds `paused` (today `⏸ paused`). progress-words' new forum/board wording keeps `paused`, so they should survive.
- theseus-h9tu1: **update, do not close**: its six checks are met on Discord, the CLI and the protocol, and not yet on herdr
  (the `blocked` transition count, and the tab). theseus-eq7hz stays open. Suggested note: "the journey runs in the gate
  (`acceptance_journey.rs`); herdr's part is `scripts/acceptance/journey.sh`'s and not yet run".
- Docs that should change at the review: Part III's item for this row (the journey, its plants, the findings above); `docs/status.md`
  (the journey is in the gate); `docs/technical-overview.md` has no entry to change.
- Ask the owner: finding 2 (`ping_window_secs` can silence a question behind a chat reply) and finding 1 (a CLI task started
  before the DM's session exists has no forum post).
