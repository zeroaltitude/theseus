# theseus (the CLI)

`theseus`: a thin client of `theseusd` for shells and pipelines, over the daemon's Unix socket or a spawned
`theseusd --stdio`. It links theseus-protocol and nothing else of Theseus (and sha2, for an episode's digests). Its library, `theseus_client`, is shared
with the terminal UI.

Key modules: `main.rs`, `cmd.rs`, `render.rs`, `client.rs`. Read by: (a binary).

## What's here

- `src/lib.rs`: the library, `theseus_client`: `client` (a connection; it prints nothing and knows no command line)
  and `render` (what a terminal shows of answers and events, as lines, each a text with a style tag).
- `src/main.rs`: the arguments, the connection, and a short `run` that matches the subcommand. A shared file: a lane
  changes it only at its join.
- `src/cmd.rs`: one function per subcommand, and `output()`. `ask --attach FILE` sends a text file's text and any
  other file's bytes, up to 32 MiB, for the daemon to keep and read (an image, a PDF; theseus-c9l6). `tui` connects nothing: it execs `theseus-tui`, found
  beside this binary or else on PATH, with `--socket` first and the arguments after it (step 10f).
- `src/work.rs` (theseus-hkx30): `theseus work`, the work board as a tree (`work.watch`), and `--watch`, a line for each
  `work.changed` view and each `work.notice`, reading again after `events.lost`. Minimal on purpose: `cli-tasks` builds
  the real tree on the same wire. Tests inside it. The `Theseus` root (the daemon's own jobs, theseus-jn17m) is a line of
  its own with its jobs under it: `◐ Importing Claude Code history · batch 41 of 120 · 3:12` (`work_system::line`), a
  failed job `✗`. `theseus status` (long form only) asks `work.watch` for `system` alone and puts `⚙ importing 41/120`
  after the time on its first line; `--short` and `--json` ask nothing more. `theseus index rebuild` is the owner's act
  (`OPERATORS`); the three importers number their batches (`import::Numbered`).
  Since theseus-5i8i it groups the board by thread (`board_lines`: a header per live thread, needs you first, then its open
  tasks, questions and wakes; then what is in no thread as the tree it was; `--flat`, or a board with no thread, prints
  today's tree), and `--watch` prints a line for each thread group that changes.
- `src/chat_cmd.rs` (theseus-cyha, theseus-6yka): bare `theseus` on a terminal (no argument; stdin and stdout both terminals) and `theseus chat [SESSION | -c | -r]` exec `theseus-tui chat [SESSION] [-c | -r]` as `tui` execs the board; `theseus -c` and `theseus -r` (the whole command line, on a terminal) are the same as `chat -c` and `chat -r`: continue this directory's last conversation, or pick one. A pipe, a script or any other argument keeps clap's usage text and exit 2. Tests: `tests/chat.rs` (a pseudo-terminal, a stand-in `theseus-tui`).
- `src/notify/` (theseus-f0bi9): how a ping reaches the owner, for every terminal client. `notices.rs` is the TUI's
  former notice queue (the policy's notices, a second of debounce, the burst fold); `mod.rs` has `Delivery`
  (`auto|toast|bell|osc9|osc777|off`), `Env` (WSL from `WSL_DISTRO_NAME` or `osrelease`, injectable) and `Notifier`;
  `toast.rs` runs `powershell.exe -NoProfile -NonInteractive -Command <fixed script> <title> <body> <sound>` on a
  worker thread (one in flight, one queued, 5 s kill; a missing binary falls back to the bell once and says so on
  stderr once). The three trailing arguments are `x` + hex words, never the text: `-Command` joins its arguments and
  parses them as code, so raw text would be code. Nothing is probed at start. `status_notify.rs` is
  `theseus status --watch --notify` (default `off`; a bell or OSC goes to /dev/tty, never stdout).
- `src/jobout.rs` (theseus-e1hu4): `theseus peek JOB` (one `work.peek`: the size and the last 20 lines, scrubbed) and
  `theseus attach JOB` (a peek, then `action.output` from its end, each chunk printed as it comes). A job is named
  whole (`act_…`) or by a unique end of at least four characters among the work board's jobs. Ctrl-C detaches
  (`action.unfollow`) and never stops the job; an `events.lost` naming the job's stream says so, peeks again and
  follows from there; a closed connection exits 1. `board` is the one arm of `run` for `work`, `peek` and `attach`
  (`run` sits at clippy's 100 lines). `theseus work` and `status` word a running job's time as
  `theseus_protocol::job_follow::job_words` does (`job 0:05 of usual 0:31`), and a view's `last_line` follows it
  (`›`); `work --watch` applies a view by (position, time), since a sample of a job's last line has its frame's
  position and a later time. `status`'s long form makes one more read, `work.watch`, for the jobs' times. Tests: inside
  `jobout.rs` (a scripted daemon over a duplex: order, the end, a detach that asks nothing that stops the job).
- `src/status.rs`: `theseus status` and `wait --any` (theseus-lweh). One read, `executions.watch {limit: 0}`, feeds
  `Board` (the executions that need you or work, by the position rule) and its forms: the long one (a header and a
  row each, a command under what asks), `--short` (`●1 ◐3 ✗1 ◆2`; empty when nothing works or waits), `--watch`
  (a line when a count or the first needs-you item changes, never on a timer, through a daemon's restarts), and
  `--watch --tab` (OSC 9;4 to `/dev/tty`, through tmux's passthrough inside tmux). `status` connects for itself,
  within 100 ms (`Conn::socket_within`), and `--short` exits 3 silently when it cannot. ◆ is `new_since_seen`, which
  returns 0 until the shared seen file joins (theseus-yus0). Tests: `status_tests.rs` (the pure parts and the watch
  on tokio's paused clock) and `tests/status.rs` (the binary against a scripted daemon). `theseus-sim bench status`
  times `status --short` against 1,000 sessions (p90 5 ms with `--check`).
  `wait --any` answers at once for a needs-you view that is there, but not for a failure that is already there (a
  failed execution stays on the board for good, so it would end every call); one that happens while waiting answers.
  `tests/status_edges.rs`: a daemon that connects and never answers (`--short` exit 1, silent, after 2 s), a dead
  daemon's socket file (exit 3), and a signal's clearing of the tab on a pseudo-terminal.
- `src/tasks_forecast.rs` (theseus-zz752): `theseus tasks forecast`, how many tasks of the last 7 and 30 days finished
  inside their forecast's p90, by class (`forecast.calibration`; the words are the protocol's). `theseus work` and the
  long `theseus status` show a task's spend against its median (`$0.42 of ~$0.60`); status asks `work.watch` once, only
  when a task row shows, and lets it go (`status::forecasts`).
- `src/agenda.rs` and `src/render/horizon.rs` (theseus-nuhsz): `theseus agenda [--watch]`, the future in one view: a header of
  counts, then Now, Today, This week, Later, Recurring · 30 days, Waiting on the world, Projects (a section only when it has rows),
  and a verbs line. It reads `work.watch {limit: 0}` (the snapshot holds the future views beside the tree) and `work.agenda` for
  the next 30 days, and draws with `render::horizon::build` (pure; the day is this machine's: `local_day`, `horizon_of`), which the
  TUI's Horizon section shares (`Row.compact` is the sidebar's words; `lines_from_views` builds the day sections from the views
  alone). `--watch` redraws on `work.changed`, after `events.lost`, and at the next local midnight (a client-side deadline; the
  daemon sends nothing for the day turning). A hidden `--now MS` is the tests' clock. `--json` is the two answers as they came.
  `theseus status` reads `status.short` too: `⏰ 16:52` (the next reminder) and `🔁✗1` (routines whose last run failed) join the
  short line, and the long one says `🔁✗ 1 routine failed`. Tests: `render/horizon_tests.rs` (the golden `render/horizon_golden.txt`,
  `THESEUS_GOLDEN=write`), `tests/agenda.rs` (a scripted daemon; `tests/golden/agenda.txt`).
- `src/answer.rs` (theseus-ds5os): `theseus answer ID OPTION [--note WORDS]`, one `work.answer` for every kind of question
  (a call, a spend limit, a layer-1 change, an extension's ack, a reminder). The id is sent as typed: the daemon resolves
  a whole id or a unique end of at least four characters, and an end naming two questions is refused naming both. The
  option is one of the question's own (`approve`, `approve_trust`, `decline`, `reset`, `accept`, `ack`, or a reminder's
  `done`, `snooze_10m`, `snooze_1h`, `tomorrow_9`); a wrong one is refused with the right ones. `work.answer` is in
  `client::OPERATORS`, so a job's shell is refused before anything is sent. `theseus confirm` stays for listing and for
  following the turn an answer resumes (`action.confirm`: the same handlers). Tests: `tests/answer.rs`.
- `src/task_start.rs` (theseus-7oq0w): `theseus task start BRIEF [--title T] [--in SESSION] [--budget USD] [--profile P]
  [--at WHEN | --in-time SPAN] [--quote-file FILE]… [--report-to home|board]`, one `task.start` call with no model turn; it prints
  the task's short id and where to follow it. The times go to the daemon as typed (`start_when`, read in the daemon's zone, as
  `remind`'s are). `--in` resolves a session like `watch` does. A quote file's text is outside text for the task (it holds
  the task's session). `task.start` is in `client::OPERATORS`, so a job's shell is refused before anything is sent. `task`
  (this verb) and `tasks` (the list) are separate subcommands. Tests: `tests/task_start.rs` (a scripted daemon).
- `src/open.rs` (theseus-x9swu): `theseus open turn [TURN] [--session S] [--print]`: `turn.inspect` for the link the daemon built from
  `[cockpit] public_url`, printed (stdout alone; `--json` the ids and the link) and opened with `$BROWSER`, `wslview`,
  `xdg-open` or `open` where one is found. The status line ends with ` · inspect <url>` when the result carries one. Tests:
  `tests/open.rs` (a scripted daemon), `render/later.rs`.
- `src/remind.rs` (theseus-8ux6x): `theseus remind WHEN NOTE` (`--every`, `--days`, `--until`, `--session`), `snooze`, `pause`, `resume` and
  `done`: the owner's verbs on wakes, one call each (`wake.create`, …), no model turn. With no `--session` a reminder lives in the owner's
  Home. A watching terminal says a fired reminder with its four answers as commands (`render/wakes.rs::noticed_lines`).
- `src/watch_cmd.rs` and `src/render/watches.rs` (theseus-f2r2b): `theseus watches` (list), `watches add TITLE --run 'CMD' | --url URL | --file PATH --until COND [--every 5m] [--for 3d] [--then notice|turn|task] [--field PATH] [--session S]`, `watches add TITLE --reply PLACE [--from WHO] [--words TEXT|/RE/] [--for 5d]` (a reply in a Discord place, no checks: theseus-rlnbf), `watches rm ID` (`wake.cancel`). `theseus watch SESSION` is the session follower and stays. `--run` is split into an argv as a shell splits words (no expansion, no operators). `watch.create` is in `client::OPERATORS`.
- `src/watch_cmd.rs` and `src/render/watches.rs` (theseus-f2r2b): `theseus watches` (list), `watches add TITLE --run 'CMD' | --url URL | --file PATH --until COND [--every 5m] [--for 3d] [--then notice|turn|task] [--field PATH] [--session S]`, `watches rm ID` (`wake.cancel`). `theseus watch SESSION` is the session follower and stays. `--run` is split into an argv as a shell splits words (no expansion, no operators). `watch.create` is in `client::OPERATORS`.
- `src/notices_cmd.rs` (theseus-slvl.9): `theseus notices rate up|down [ID] [--session S]`, the owner's verdict on a message
  Theseus sent first (`initiative.rate`; in `client::OPERATORS`), and `theseus persona`'s last lines, the week's promises kept and
  👍/👎 (`persona_cmd::week_lines`, over `initiative.week`; `--json` stays the persona list alone).
- `src/thread.rs` (theseus-nhew): `theseus thread` (list: slug, state, last activity, the session's context tokens and
  today's dollars, title), `thread new|show|edit|pause|resume|done|reopen`, and `ask --thread SLUG` (a turn in the
  thread's session). The writes are in `client::OPERATORS`, so a job's shell cannot make them.
- `src/routine.rs` (theseus-pvm5c): `theseus routine` (list: short id, series, title, next, per-run and month figures, the last run),
  `routine add SCHEDULE BRIEF [--title] [--budget USD] [--month USD] [--to dm|none|PLACE] [--catch-up one|none|auto] [--speak
  always|when-notable]`, `runs ID [-n N]`, `pause ID [--until WHEN]`, `resume`, `run` (once, the series untouched), `update`, `rm`. The
  writes are in `client::OPERATORS`, so a job's shell cannot make them. The lines are `list_lines` and `run_lines` (tested).
- `src/current.rs`, `render/thread.rs`, `interactive/thread.rs` (theseus-j0yh): a terminal's current thread. `theseus thread
  switch SLUG|N|PREFIX|home` prints `export THESEUS_THREAD=…` (or `unset`) and remembers the choice per tty in
  `$XDG_STATE_HOME/theseus/thread.json`; `ask` reads only the environment (a script without it asks home), and a reply made
  in a thread prints `→ title`. `watch --interactive` starts on the terminal's thread, takes `/t` (list), `/t N|NAME`
  (moves the watch to that thread's session), `/t home`, and prints a status line `[title · state · 41K ctx · warm]`
  (`render::thread::status_line`; warm from `ThreadRow.cache_ttl_secs` and `last_active_ms`). With no current thread it
  sends no `thread.*` request. Tests: `tests/thread_switch.rs`, `tests/thread_interactive.rs`. `theseus chat` (cli-tasks)
  had not joined: its joiner ports `/t` and the status line from `interactive/thread.rs`.
- `src/thread_import.rs`, `src/vestige.rs` (theseus-71ib): `theseus thread import --from openclaw` is the dry run (a numbered
  table, `render`'s words in `table_lines`, and a plan file, `thread-import-plan.txt` by default, of `[ ] N slug title` lines);
  `--apply 1,3,5` (numbers of a fresh table) or `--apply all-ticked --plan FILE` (the `[x]` lines, read by slug) makes those
  threads through `thread.import`; `--since-days` (30), `--min-cluster` (8). `--vestige PATH` (a dump file, or a directory's
  `vestige*.json`) turns each memory into an episode (`vestige::episode`, the id the digest of its text) and sends it with
  `import.episodes`; a note seen again is skipped. `thread.import` is in `client::OPERATORS`. Tests: `thread_import/tests.rs`,
  `vestige/tests.rs`, theseusd's `tests/thread_import.rs`.
  `thread seal SLUG [--reason] [--redirect] [--not-needed]` (the seal's report says what had crossed into other threads,
  "2 facts were already used in «pr round»", and offers `--redirect`), `thread unseal SLUG`, `thread new --sealed
  --split-from`; a sealed thread's name carries 🔒 in `thread list` and `thread show` (🔒? while a seal is proposed).
  `thread answer <ask> <1|2|new|home>` and `thread move <slug|home>` are the router's recovery (theseus-rha2): `thread/ask.rs`
  prints the question a `thread.asked` carries as `[1] [2] [n]ew [h]ome` and the line to answer it; both are in `OPERATORS`.
- `src/retry.rs` (theseus-7zgsm): `theseus dismiss ID` and `theseus retry ID [--note WORDS]`, one call each
  (`execution.dismiss`, `task.retry`); ID is a task's or its execution's id, or the last four characters of either
  (`resolve.rs`'s rule), and a name that is no failure, or a conversation to retry, is refused with words before
  anything is sent. Both are in `client::OPERATORS`: refused inside a job.
- `src/card_cmd.rs` (theseus-slvl.6): `theseus card` (what waits as questions, what was kept and declined, the spend), `card propose [--dry-run]`, `card keep|edit ID TEXT|decline ID`, `card done [--undo]`. The writes are in `client::OPERATORS`, so a job's shell refuses them; the list is a read. Its lines (`list_lines`, `propose_lines`, `answered_line`) are pure.
- `src/project.rs` (theseus-n5ui2): `theseus projects [--all]` (a bar of milestones done, `▰▰▰▱▱▱▱ 3/7`, the project, its next milestone and
  date, what is overdue, steps done, the weekly review's next run), `project show ID` (the tree with dates, overdue marked),
  `project new TITLE [--objective] [--accept]… [--milestone TITLE@WHEN]… [--review SCHEDULE|none] [--review-budget USD]`, `project attach SESSION
  ID` and `project detach SESSION`. `new` and `attach` are in `client::OPERATORS`, so a job's shell is refused. The review's run log is
  `theseus routine runs <review>`. The lines are `list_lines` and `show_lines` (tested).
- `src/standing_cmd.rs` (theseus-a7vi): `theseus standing` (the live records: id, kind, scope, since, source, text,
  and a last line "in every compile of their scope since they began: N compiles, M missing" from the manifests;
  `--all` adds the ended), `standing add [--kind order|preference|fact] [--scope everywhere|place:T|surface:NAME|session:ID | --surface
  terminal|discord|voice|cockpit|mcp] [--until AGE] [--replaces ID] TEXT`, and `standing revoke ID`. The writes are in `client::OPERATORS`, so a job's shell
  is refused. The rows' words are `list_lines` (goldens in its tests).
- `src/seen.rs` and `src/shown.rs` (theseus-yus0): the machine's seen file (`$XDG_STATE_HOME/theseus/seen.json`) and
  the one call, `shown::shown(conn, sessions)`, by which a command records what it displayed: after its output,
  at the position `session.wait` gives (one answered at once per session, 450 bytes; a snapshot is 70 KB and misses a session past its 200), never failing the command (one line on stderr). `history` (to the end of
  the session), `watch` (on Ctrl-C), and `confirm` call it; a watch the daemon closes records nothing, and a command inside a job (`THESEUS_SESSION`) records nothing: the agent's reading is not the operator's. Writes read the
  file again, merge by the greatest position under a lock file, and replace it atomically.
- `src/follow.rs`: `ask` under `--spawn` follows what its turn left for later (theseus-mqxk): main.rs spawns
  `theseusd --stdio --one-shot SECS` for an `ask`, each turn's result names its `later` (jobs running, a result
  queued, wakes), and while a turn can still come within `--follow-for` (`FOLLOW_FOR`, 30 minutes, the owner's call)
  the ask prints each one and stops when none is left, or says what still runs at the bound. `--json` stays one
  object (`combined`: the last turn's result, its spend summed, `asked` and `continuations`), and the exit code is
  the last turn's; a first signal stops the session's work (exit 9 when no turn ran), a second ends the run. A
  first signal during the ask's own turn means nothing is followed: a turn that ended done as its stop went out
  has what it left stopped, and exits 9 (`after`'s `signalled`). The turn and its follow share one signal
  listener (`follow::Signals`, made under `--spawn` only): one each lost a signal that came between them.
  `render/later.rs` is the status line's words for what a run left behind.
- `src/self_cmd.rs` (theseus-pw1q): `theseus self log [--since 7d] [--json]`, `self halt [why]`, `self resume`
  (sends the shell's `THESEUS_SESSION` as `from_job`, so the daemon refuses a job's resume and ledgers it, rather
  than the CLI refusing it unseen), `self digest`, and `self revert MERGE` (`self_cmd/revert.rs`, theseus-pw1q.10: the owner's
  veto of a self-join, sent with `from_job` as the resume is; the daemon runs main's gate on the revert before it answers). `self narrow [list|show ID|run [--profile owner|read] [--dry]|apply ID|decline ID|revert ID]`
  (`self_cmd/narrow.rs`, theseus-pw1q.15) is the narrower policies Theseus proposes for its hand roles: `list` and `show` read; `run`,
  `apply`, `decline` and `revert` are the owner's (`client::OPERATORS`; the daemon refuses a non-owner and a job's shell and ledgers it). The state's words are the library's `render/rsi.rs`
  (`self_state_line`), which health's `self` line uses too (`HealthResult.self_improve`).
- `src/keys.rs` (theseus-tnky): `watch --interactive`'s input. On a terminal it reads bytes (out of canonical mode and
  echo, signals on, restored on every way out, a panic's hook too) and keeps the line itself (text, backspace, Ctrl-U,
  Ctrl-W, Enter, Ctrl-D on an empty line), since a terminal's own line mode shows nothing until Enter, and the first
  key of a line is the `session.typing` notice's moment; a pipe is read by lines as before. `render/warm.rs` is
  health's `warm:` line.
- `render/defer.rs` (theseus-7m7w.2): `theseus tools`' families per place (over `tool.defer`: loaded or deferred, why, use,
  searches and misses; `-v` names each family's tools) and health's `tools:` line.
- `src/ledger_verify.rs` (theseus-bmkxk): `theseus ledger verify [--since POS] [--anchor SEQ:DIGEST]` (exit 0 whole, 1 at the
  first broken link, which it names) and `theseus ledger head`, over `ledger.verify`; `theseus ledger` with no subcommand is the
  tail as before. Its words are pure functions of the answer (`lines`, `head_lines`), tested in the file.
- `src/postmortem.rs` (theseus-m2tiu): `theseus postmortem [--since T] [--until T] [--session S | --task T] [--out FILE]`
  (`--json` the same facts as data; default window the last 12 hours; `T` is an age, a time of day, `yesterday 22:00`
  or an RFC 3339 time). It reads `ledger.window` a page at a time (read only: no frame, no lock) and hands the rows to
  the library's `render/postmortem_facts.rs` (`analyze`: a pure walk, the window's end the caller's) whose `Report`
  `render/postmortem.rs` words as markdown: Summary, Restarts, Timeline (quiet turns fold), Cost, Asked and answered,
  Open at the end. `--rows-out FILE` keeps the rows read as JSON Lines and `--from-rows FILE` reports from them with no
  daemon: how a real incident becomes a fixture (`tests/fixtures/postmortem/`, recorded by theseusd's
  `tests/postmortem_incident.rs`). Tests: `postmortem_tests.rs` (the times), `render/postmortem_facts_tests.rs` (each
  section on small row sets), `tests/postmortem.rs` (a scripted daemon, and the golden and the facts-in-order of the
  recorded incident; `THESEUS_GOLDEN=write` rewrites `incident.md` and `incident.json`).
- `src/harness/` (theseus-o73y): `theseus import claude-code|codex [DIR]`. `mod.rs` turns one session file into one
  episode of the import's format (its module comment says what a message is, what is left out and counted, what
  is rejected with its line number, and that a grown file has its new messages appended by the daemon, theseus-pjmj7); `claude_code.rs` and
  `codex.rs` read each source's lines; `run.rs` walks the directory (a subagent's file left out), sends the episodes
  to `import.episodes` a batch at a time, and prints each file's counts. The CLI scrubs nothing (it holds no vault
  value): the daemon scrubs every message of a harness source as it writes it (`import::write::scrubbed`). The
  default tag is the live tag already holding the source, else `<source>-<today>`, since an episode's hash covers its
  tag and a second run on another day would otherwise be refused. Tests: `harness/tests.rs`, and theseusd's
  `tests/import_harness.rs` (the real CLI against a real daemon).
- `src/import_sync.rs` (theseus-pjmj7): `theseus import sync [--from DIR] [--since T] [--agents a,b] [--tag T] [--dry-run]
  [--no-people]`. It asks `import.sync.plan`, runs the exporter (`scripts/openclaw-export.py`, built in with
  `include_str!`, `python3 -I -c`, its stdout a pipe: raw text is never in a file), sends the episodes a batch at a time,
  and ends with `import.sync.record` (the new cursor on a run that did not fail; a failed one records its reason, which
  is health's warning). Both methods are in `client::OPERATORS`. A systemd user timer runs it nightly
  (`scripts/user-service.sh sync-timer install`). Tests: `import_sync_tests.rs`, theseusd's `tests/import_sync.rs`.
- `src/own_bench.rs` and `src/render/own_bench.rs` (own-benchmark, theseus-pw1q.12): `theseus route report [--week]` (per
  (model, effort) cell: turns and spend, the replays one rung down and what `compare.v1` made of them, what you checked,
  the dollars movable at the stated confidence, and the replays' cost against `[route] replay_day_usd`) and `theseus
  turn replay SESSION TURN -P PROFILE [--effort E] [--no-compare] [--whole]`, a past turn's first request rebuilt byte for byte
  and sent to another profile (`--whole`: the whole turn, tools and all, on a scratch copy of its directory; the lines add the
  calls with what ran, what was stubbed and what was refused, the paths changed, the test runs, the caveats and the routing label;
  the report adds each cell's whole-turn columns and the machine labels fed). The replay is in `client::OPERATORS` (it spends money). The words are pure functions
  (`report_lines`, `replay_lines`, tested in the file).
- `src/print.rs`: the `Printer`, which writes the library's lines in one of four modes: `Text` (`ask`), `Quiet`
  (`ask --no-stream`), `Watch` (`watch`), and `Json`. On a terminal (`render::style::styled_stdout`: stdout a tty,
  `NO_COLOR` unset or empty, `TERM` not `dumb`, no `--plain`) `Text` and `Watch` draw the reply as markdown, by
  complete lines; a pipe, `NO_COLOR` and `--plain` print the text as it came, byte for byte (theseus-rn7s).
- `src/render/style.rs`, `md.rs` (+ `md/block.rs`, `inline.rs`, `table.rs`), `code.rs` (theseus-rn7s): the one renderer
  of a reply for a terminal. `style.rs` is the styled line (spans with bold, italic, dim, underline, strike, a
  16-colour foreground and an OSC 8 link target; `ansi()` and `plain()`), the terminal's width (`COLUMNS`, then the
  tty's size, else 80; prose wraps one column short of it, `prose_width`), and the two `Mode`s (`Ansi`: styles on spans;
  `Plain`: no style at all, a heading keeps its `#`, a quote its `>`, a link its address after its text). `md.rs` is
  `Markdown::new(width, mode)`, `push(&str) -> Vec<StyledLine>` (the lines that are final), `finish()` and
  `Markdown::render` (whole). **A streamed reply and the same reply whole render byte-identically**, whatever the
  cuts, because `block.rs`'s `Segmenter` decides where a block ends from the complete lines alone (a paragraph, a
  list, a quote and a table are drawn when the line that ends them arrives; a fence a line at a time) and `md.rs`
  draws a block as a pure function of its lines. A newline in a paragraph is a line break as typed, not CommonMark's
  space (the owner may revisit). Not read: indented code, setext headings, HTML, footnotes. `code.rs` is a lexer by
  hand for Rust, Python, JS/TS, shell, JSON, TOML, YAML, Go, C/C++ and diff (static tables; an unknown language
  prints plain); a code line wider than the block continues behind `↪`. The terminal UI draws the same lines, its
  `styled.rs` turning spans into ratatui's; this crate must not link ratatui. Tests: `tests/markdown.rs` (goldens in
  `tests/golden_md/*.md` at 80, 100 and 120 columns, plain and ANSI, `\e` for the escape; `THESEUS_GOLDEN=write`
  rewrites; the seeded random delta cuts; nothing wider than the width; the cost per 100 lines) and
  `tests/markdown_cli.rs` (the binary on a pty and on a pipe).
- `src/why.rs` (theseus-ighf): `theseus why [TURN] [--session S]`, what a turn's model was shown and why: it asks
  `context.explain` (a prefix or end of at least four characters names an older turn, resolved from its turn list),
  `policy.explain`, `node.list` (the turn's `tool_call` nodes, with the gate's own posture) and `ledger.tail` (its
  `route.decided` row), reads only, and prints the words of `theseus_protocol::why` (the renderer Discord's `/why`
  and the `/context` command to come call too; it writes plain lines, no colour). `--json` is the four answers as
  they came. Tests: `tests/why.rs` (a scripted daemon; goldens `why*.txt`; the requests are reads alone).
- `src/render/diffview.rs` (theseus-z6au): a diff and a result's first lines as `Line`s. A removed line is `Tag::Del`,
  an added one `Tag::Add`, and the changed words of a pair sit between `MARK_ON` and `MARK_OFF` in its text; `paint`
  turns them into SGR on a terminal and strips them everywhere else (`print::lines`, a pipe, `NO_COLOR`, `TERM=dumb`), and
  the TUI draws them with its own styles. `Flight::event` words a call from its `tool.proposed` (`summarize`), drops the
  start line of a call already worded, and under the end line shows the result behind a `⎿` gutter (the first 3 lines,
  a failed command's last 8, then `… all N lines`) or the edit's diff. A question's `preview` stands in for its input
  line (`answers`). Tests: `diffview.rs`'s and `print.rs`'s.
- `src/census.rs`, `src/render/census.rs` (theseus-7m7w.1): `theseus census [SESSION] [--place cli|dm|task|harness]
  [--exact] [--json]`, one read (`context.census`): the next request's system sections, every tool's description, schema and
  total tokens with its calls in the last 30 days and the last one's date, the messages, and the total against the window.
  The lines are `census_lines` (pure; tested in the file).
- `src/render/history.rs`: `theseus history`'s own lines: each node with its short id, which `theseus reach` takes
  (theseus-glyw), and `page_lines`, where a page ends and the command for the next one either way (theseus-xo0m).
  Its `node_lines` is the history's own; `render.rs`'s, which the terminal UI shares, stays apart.
- `src/fork.rs` and `src/render/fork.rs` (theseus-2qzx, R19): `theseus fork NODE [--title] [--profile] [--budget]
  [--session]` (`session.fork`: the node's own session is found by `node.reach` at generation 0, whose answer names the
  node whole, so the fork is a lookup), its two lines (`forked ses_…6664 at msg·a1b2c3 (position 166,201) → ses_…f00d
  "…" in 3 ms, 1.4 KB`, then `not restored: …`), the marks `theseus history` of a fork prints around the inherited
  prefix (a node whose session is not the history's own), and the forks `theseus reach` lists. Tests:
  `tests/golden_fork.rs`; theseusd's `tests/fork.rs` runs it on a real daemon.
- `src/pages.rs` (theseus-7bee): `theseus sessions` and `theseus executions` print the newest 50 (`session.list
  {n}`, `execution.list {n}`), a footer (`render/sessions.rs`'s `page_footer`) naming `--before CURSOR` and `--all`, which
  keeps the bare list. The CLI asks `session.list {n: 1}` where it wants the newest session (`cmd::NEWEST`, `watch
  --interactive`, `history` with no session) and `{ids}` where it wants one by id; it never asks for every session
  but under `--all` and `herdr sync`, which reconciles every pane. `history` reads the newest 200 nodes (`-n`).
  `resolve::executions` reads the executions a page at a time. `tests/pages.rs` records each request's params.
- `src/render/cite.rs`, `src/cite.rs` (theseus-ejdk): a reply's citations. `ask` shows each handle in the streamed reply
  as a numbered mark as it completes (`Marker`, which holds a handle split across pieces) and a footnote block under it
  (the source's header and `theseus history <handle>`); a handle that was no source of the turn is marked too, and its
  footnote says so. `--no-stream` and `history` mark only what has a source (`mark`, `footnotes`, `cited_node`; the
  builders are plain functions a renderer can call). `theseus history m:a1b2c3` shows the cited message whole
  (`node.reach` names the node, `session.history` reads its page). Tests: `tests/cite.rs`.
- `src/herdr.rs`, `src/herdr_slots.rs`, `src/interactive.rs` (theseus-l1l, theseus-qb1es): a `theseus watch` in a herdr pane
  (and `watch --interactive` anywhere) reports its session to the pane. `herdr_slots::slots` is the one function over a
  `WorkView` and a few facts (`Facts`: the model, the parent's name, the call running, the first line of a finished task's
  report) that words every slot: `agent` and `display_agent` the task's short name (at most 22 characters, cut at a word:
  herdr's own toast reads "<name> needs attention"), `title` the whole name, `state_labels` the state's label worded once
  (`needs you · fs.write log.md`, `budget · $10.02 of $10`, `failed · timeout (provider)`, `blocked · <reason>`, `turn 3 ·
  proc.run cargo nextest`, `job · 4 m`, `queued · wake`, `waiting on task a1b2c3`, `sleeping until 16:00`, `done · <first
  line>`), and ten tokens (`task kind session confirm cost parent since doing model report`, each at most 80 characters, at most
  16 allowed). `$kind` (`task` or `conversation`) is what marks a pane as Theseus's for `herdr sync`; `$session` finds its pane;
  `$confirm` is the open question's correlation id, for herdr's answer card. The elapsed time in `doing` and in a job's label
  moves on a timer (15 s while a call runs, 1 min for a job), the only one a quiet pane keeps. A lost daemon is not a way out:
  the watch (a socket one: `Conn::socket_path`) says `· reconnecting`, never releases the pane, retries from 100 ms doubling to
  2 s with nothing but a timer between tries, reads the session once (`read_session`) and says its labels and tokens again.
  A watch on a session that already failed prints why, then waits. When the work board's views drop in, only `from_execution`'s
  call changes. Tests: `tests/herdr.rs` (a fake herdr and a scripted daemon, `Restarting` for a restart),
  `tests/herdr_sync.rs`.
- `src/herdr_up.rs`, `src/herdr_up/state.rs`, `src/herdr_up/wire.rs` (theseus-fuw5d): `theseus herdr up [--follow] [--boot] [--pin S]
  [--finished-grace SECS] [--herdr-socket P]`. `up` lays out herdr's `theseus` workspace (one `layout.apply`, a tab per task whose
  pane's argv is `theseus watch <sid> --interactive`, nothing typed into a shell) and exits; run again it sends only reads.
  `--follow` (and `--boot`, which first closes the workspace's panes with no `kind` token) makes the `board` tab, whose pane runs
  `up --follow --here`, and exits; `--here` is the follower: one `work.watch` connection and one herdr `events.subscribe`
  (`pane.exited`, `tab.focused`, `pane.focused`), reconnecting to both, one `notification.show` when the daemon is lost and one
  when it is back, the Space row's counts (`workspace.report_metadata`) and `agent.view.set`. `state.rs` is pure (every method takes
  `now`): a task wants a tab while it needs you, works, waits or ended unseen (a conversation only while it needs you, a pinned
  session always); a cancelled task's closes at once, a finished one's a grace (600 s) after its tab or pane was focused; one
  deadline timer, none while nothing awaits. A task already finished at the first read gets a tab only if it ended within the
  grace. `wire.rs` builds each herdr request in one function: the shapes of `layout.apply`, `events.subscribe`,
  `workspace.report_metadata` and `agent.view.set` follow the design and were not checked against herdr's schema. `sync` is
  unchanged but for pins: `--pin` writes a `pinned` token on the pane, and a later sync keeps it. Tests: `tests/herdr_up.rs`
  (a scripted herdr and daemon), unit tests in `state.rs` and `wire.rs`.
- `src/herdr_answer.rs`, `src/herdr_keys.rs`, `src/herdr_setup.rs` and `contrib/herdr/theseus/` (theseus-pkaq): the herdr plugin and what
  its keys run. `theseus herdr answer [--pane P | --oldest | CID]` is a line-mode card for a popup: `pick` chooses the question (the
  one named, else the focused pane's `$confirm` token, else the oldest), `keys` takes the keys from the question's own `options`
  (`y`, `t`, `n`, `1`..`6`; words decline with a note), `card` words it, and an answer is one `work.answer` with author `herdr`;
  a `confirm.resolved` for the shown question says it was answered elsewhere. `herdr next` focuses the oldest question's pane
  (`agent.focus`), `herdr stop --pane P` asks y/N then runs `cancel` (a task) or `stop` (a conversation), `herdr popup NAME`
  opens one of the plugin's popup panes by `plugin.pane.open` (ids only in its environment), `herdr link URL` is the link
  handler (`parse_link` reads `theseus://answer|history|open/<id>` strictly; a click opens the card, never answers),
  `herdr plugin` prints the plugin's path and the one `herdr plugin link` command, and `herdr setup [--config FILE]` prints the
  config snippet and lists what a config lacks, read only. `links_under` prints the OSC 8 links under a question in
  `interactive.rs`, only when `HERDR_ENV=1`. The commands that need no daemon connection first (`answer`, `plugin`, `setup`,
  `popup`, `link`) run before `main`'s connect (`HerdrCmd::is_own`). The manifest, the snippet, the link pattern and the reporter's
  tokens are tied by one test (`herdr_setup_tests.rs`). The operator's guide is `docs/herdr.md`. Tests: the three `*_tests.rs`
  files and `tests/herdr_answer.rs` (a scripted daemon and a fake herdr).
- `src/names.rs`: what a session is called in every client (theseus-0n1v): a task's title (the store labels every
  task `task`), a conversation's label else its title, else its kind and the end of its id. The CLI, the terminal
  UI and herdr's reporter all call it; never read `label` before `title` at a site.
- `src/resolve.rs`: the one id resolver (theseus-0n1v) for `confirm`, `history`, `watch`, `stop`, `cancel`, `wait`
  and `explain`: a whole id, or a unique end of at least four characters; ambiguous, short and unknown names are
  refused, and `watch` of an unknown one exits 1. One read a resolution at most.
- `src/render/wakes.rs` (theseus-lherl): `theseus wakes`' line (a wake's title, a series' weekdays and end) and health's
  `wakes:` line.
- `src/render/time.rs`: every time in human output, on this machine's clock, its zone read at the first time printed
  (TZ, else `/etc/localtime` alone: jiff's `system()` lists the tz database); `--json` prints the daemon's numbers.

## Invariants

- **Built for shells**: the reply on stdout, diagnostics on stderr, `--json` for machines. Exit codes: 0 ok, 1 a
  server or provider error, 2 usage, 3 cannot connect; `theseus wait` exits 4 when it times out.
- **The protocol is the only way in.** The CLI computes nothing the daemon owns: what needs you comes from
  `attention()`, and a wait is the daemon's `session.wait`, never a poll.
- **Inside a job, it names the job's session.** `ask`, `sessions open`, and `watch --interactive`'s messages send
  the `THESEUS_SESSION` every job carries as `opened_from` (`client::job_session`), so a session that a holding
  session's job reaches holds its external text too (theseus-b5cl). And it refuses the operator's methods there
  (`client::refuse_in_a_job`: an answer, an undo, a trust, a publish, the AWS bootstrap, the ontology's writes),
  before sending anything (theseus-zmgb, theseus-8kk.1). Both are light guards: a job can strip the variable.
- **A move keeps the bytes.** A refactor here starts with goldens of today's output, and ends with every golden and
  every `--help` page byte-identical (Item 30; theseus-7yx's goldens before the library's move).

## Tests

- `tests/golden.rs` compares each scenario's output with its golden in `tests/golden/`. It runs the CLI at
  `TZ=<-07>7`, and the crate's own tests write times at the same fixed zone, so no test reads the machine's.
  `THESEUS_GOLDEN=write cargo nextest run --workspace -E 'package(theseus)'` rewrites them: only for an output change
  you mean, with the diff read.
- The follow's tests run the real binaries: theseusd's `tests/spawn_follow.rs`. A theseusd test runs the
  `theseus` beside its binary, which `cargo nextest run -p theseusd` does not rebuild: build `-p theseus` first.
- `tests/connect.rs` covers how the CLI reaches a daemon (`--spawn`, and exit 3). `main` reads exit 3 from the
  error's text, which the library writes, so these tests hold the two together.
- `tests/refusal.rs` covers a refused answer: `theseus confirm` prints the daemon's reason and exits 1.
- `tests/seen.rs` and `tests/seen_commands.rs` cover what `history`, `watch`, and `confirm` record and when: after the
  output (stdout already holds the page when the snapshot is asked), not for a page back or one with more to come, on
  Ctrl-C for `watch` (exit 0), nothing in a job, and not failing when the file can't be written.
- `tests/markdown_cli.rs` covers the markdown on a terminal: a pty draws the reply, and a pipe, `NO_COLOR`, `TERM=dumb`
  and `--plain` print no escape (theseus-rn7s).
- `tests/tui.rs` covers `theseus tui`: the exec (the stand-in runs in the CLI's own pid), the socket and the
  arguments passed through, and exit 2, saying where it looked, when `theseus-tui` is found nowhere. Each test links
  the CLI into a directory of its own, since the workspace builds the real `theseus-tui` beside `target/debug/theseus`.

## Traps

- A change to `client` or `render` changes the terminal UI too.
- `ask`'s summary line can start with an escape sequence, so a `grep '^…'` on its output never matches. Wait on the
  process's pid instead.
