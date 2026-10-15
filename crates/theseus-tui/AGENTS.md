# theseus-tui

`theseus-tui`, the terminal UI (design `stage2` §2.9; theseus-7yx, steps 10b to 10f): every session of a running
`theseusd` in one sidebar, with its task trees; the queue of what needs you, answered inline; and the session in
focus, with its history and its input line. A client like the CLI, over the same socket: it links theseus-protocol
and the CLI's library (`theseus_client`), never the core. An installed binary of its own (`tool` in its manifest),
which `theseus tui` execs.

Key modules: `run.rs` (the loop), `app.rs` (no I/O), `board.rs`, `ui.rs`, and `chat/` (the inline conversation). Read by: `theseus tui` and `theseus chat` (and bare `theseus` on a terminal), which exec it.

## What's here

- `src/main.rs`: the arguments (`--socket`, `--notify`), raw mode and the alternate screen, the signals that end
  the loop as a quit does (`ending_signals`: SIGTERM, SIGHUP, SIGINT and SIGQUIT), and the loop's start.
- The daemon's own work (theseus-jn17m): at each connect the TUI asks `work.watch` for `system` alone
  (`Purpose::System`), keeps the `Theseus` root's views by the position rule from `work.changed`, and the header says the
  root closed (`⚙ importing 41/120`, else `⚙ Theseus · 2 done, 1 failed`: `work_system::collapsed`). A system job's
  failure or long end (an Inform) is a line in the footer, never a ring.
- `src/term.rs`: the TUI's own terminal modes, focus events and bracketed paste: on when the loop starts, off on
  every way out (a quit, an error, a signal, and a panic's hook) (theseus-8hcg).
- `src/run.rs`: the loop. One connection, read in one `select!` with the terminal's events (read on a thread of
  their own) and one deadline; reconnecting with backoff; the frame clock.
- `src/app.rs`: the state and every change to it (keys, the daemon's messages, answers). It does no I/O: it returns
  the requests to send.
- `src/horizon.rs` (theseus-nuhsz): the Horizon section under the task tree (`ui::sidebar` gives it up to a third of the sidebar's
  height, none while nothing is ahead). The TUI sends `work.watch {limit: 0}` at each connect (and after `events.lost` names the
  `work` stream), keeps the future views by the position rule from `work.changed`, and draws `theseus_client::render::horizon`'s
  compact rows. `H` gives the section the keys (`Mode::Horizon`): `j`/`k` move, `z` snoozes a reminder (`wake.snooze`), `p` pauses
  or resumes a reminder or a routine, `r` runs a routine now, `x` cancels a reminder, a watch or a deferred task (a routine says
  to pause it or `theseus routine rm`), enter opens a routine's conversation, esc/`H`/tab leave; a key that does not fit the row
  says so and sends nothing. No timer of its own: the local midnight moves a reminder from This week to Today at the next draw.
  Tests: `src/tests_horizon.rs`.
- `src/new_task.rs` (theseus-7oq0w): `N` opens a one-line prompt for a brief; enter sends one `task.start` under the
  conversation the cursor is on (Home when it is on a task or on nothing), with no model turn, and esc starts nothing.
  `Mode::NewTask`, `App::brief`, `Purpose::NewTask`. Tests: `src/tests_new_task.rs`.
- `src/board.rs`: the board. Views applied by the position rule, titles, the questions, the rows and trees, and the
  queue.
- `src/work.rs` (theseus-k2vca): what `work.watch` says of each session (the position rule, the jobs under them) and
  the words of a sidebar row: `job 2:14 of usual 2:40 · said`, `model 0:31`, `asks: …`, `failed 15:13: why`, the plan's
  `3/5` first; the title is cut before the doing. `executions.watch` stays the board's feed (the questions, reminders
  and `confirm.resolved` ride it, not `work.watch`); this is the overlay that words the rows, and a daemon without
  `work.watch` leaves `live` false so rows say their labels as before. A row's words are used only when the work view's
  level equals the board's. A finished task's first report line shows under its row (`◆` until seen). The header
  counts `● needs you · ◐ working · ✗ failed · ✓ done`; the window title counts needs-you alone (`theseus ●1`; a
  failure is counted apart). Elapsed times repaint on the wall minute (one deadline in `App::deadline`), never each second.
- `src/ui.rs`: drawing into a ratatui buffer. Nothing here changes the app.
- `src/styled.rs` (theseus-rn7s): the pane's rows. A run of `Reply` lines is one reply, drawn whole with the CLI's
  markdown renderer (`theseus_client::render::md`) at the pane's width and turned into ratatui spans here; every
  other line wraps as before. A history reply is a header line (`[time] model:`, `Plain`) and its text as `Reply`
  lines. The reply streams in raw lines and is redrawn at each frame, so a half-typed `**` shows as typed until it
  closes. Test: `src/tests_markdown.rs`.
- `src/follow.rs` (theseus-e1hu4): the follow pane. `l` on the session under the cursor finds its running job
  (`action.list {execution_id, unsettled}`), peeks it (`work.peek`) and follows from the peek's end (`action.output`);
  chunks and the end arrive as notifications and `esc` sends `action.unfollow`. `Follow` is the pane's pure state
  (a bounded tail of lines, an open line, the next byte expected, a gap said as `-- N bytes skipped --`); `events.lost`
  naming the job's stream, and a reconnect, peek and follow again. Its header says `job 0:05 of usual 0:31`. The
  sidebar's own row words are `tui-rows`'. Tests: `src/tests_follow.rs`.
- `src/board_threads.rs` (theseus-5i8i): the threads on the board (`App.board_threads`; `src/threads.rs` is thread-cli-tui's
  picker). Fed by tui-rows' one `work.watch` (a connection holds one), views and `ThreadGroup`s applied by the position rule, and the sidebar's Threads block over
  `theseus_protocol::group_threads`: a fold-able group per live thread (`▾`/`▸`, the one header wording the CLI shares), its
  open work under it, a pending seal as a needs-you line. `T` gives the block the keys (up/down, enter folds a thread or
  opens an item's session, esc leaves). A daemon that refuses `work.watch` costs no word and no block. Tests:
  `src/tests_board_threads.rs`.
- `src/detail.rs` (the focused session's history and events, as the CLI's `render` lines), `src/card.rs` (a
  question's card), `src/notice.rs` (a re-export: the notices, their delivery and the toast are
  `theseus_client::notify`, shared with `theseus status --watch`; the TUI keeps the viewer in `app.rs`). `--notify`
  is `auto` (default: a Windows toast under WSL, OSC 9 where known, else the bell), `toast`, `bell`, `osc9`, `osc777`
  or `off`; without the flag the daemon's `[tui] notify` (read from `health`) says (theseus-f0bi9). Done until seen is `theseus_client::seen`, shared with the CLI.
- `src/selfview.rs` (theseus-pw1q.19): the self view, `L` (`l` is the follow pane's): a pane over the whole body with the switch's state in the CLI's
  words (`render::self_state_line`), "Self-improvement is off. Nothing has run." while `[self] mode` is off, the day's
  and the newest branch's spend (`SelfLogResult.budget`, wide screens only), and the last 20 `self.log` rows, newest
  first, one line each (time, kind, what, cost, undo); enter shows the picked row whole, esc goes back, then closes. A
  screen under 100 columns folds to the state and the newest five. It reads `self.log` when it opens and again on an
  event it already hears (an execution that changed state, a task, a question), one read at a time and a second wanted
  meanwhile made when the first lands: no timer. The switch's move from any surface is `self.changed`; a row with no execution
  beside it (a night's exam pass) shows at the next such event or when the pane is opened again. Tests: `src/tests_self.rs`.
- A reply's citations (theseus-ejdk): `Detail::history` draws a page's cited replies with numbered marks and the
  footnote block under them (`session.history`'s `cites`, the CLI's `render::cite`), and `Detail::cited` does the same
  for a turn this TUI sent, from its `turn.submit` answer's `cites`; a reply another surface's turn streams in stays
  raw until the history is read again. Tests: `src/tests_cite.rs`.
- The owner's standing records (theseus-slvl.4; `src/standing.rs`): a `📌 N` pill in the header, the live records that hold
  in the focused session on this surface (`standing.list {session_id, surface: "terminal"}`: asked when a session is
  focused, after a turn of this TUI's, at a reconnect and when `p` opens the list), and `p`, a box over the body with a
  line a record (id, kind, scope, text) that any key closes. Nothing polls; an older daemon's refusal says nothing.
  Tests: `src/tests_standing.rs`.

## The chat (theseus-cyha)

`theseus-tui chat [SESSION]` is a second mode of this binary: an inline conversation in the current directory, which
bare `theseus` on a terminal and `theseus chat [SID]` exec (the CLI's `chat_cmd.rs`; a pipe or any argument keeps the
usage text and exit 2). It is **inline and append-only**: a finished line is written once into the terminal's own
scrollback (so search, copy and scroll work as always, in tmux too), and only the dock at the bottom is ever redrawn.
It uses no ratatui `Terminal`: `chat/screen.rs` writes the bytes (cursor up to the dock's first row, erase down, the
new lines, the dock again, in synchronized-output mode 2026).

- `chat/state.rs`, `events.rs`: the state and every change to it, no I/O (as `app.rs`). A key, a paste, a daemon message
  or the clock goes in; requests come out as `Effect`s and finished lines in `Chat::out`. A turn's events (the session's
  alone, whoever sent the turn) become dock rows while they run and one scrollback line when they end; a turn is ended
  once however it is told (`turn.ended` or `turn.submit`'s answer).
- `chat/keys.rs`: the one key map (`Action`). `chat/commands.rs`: the one command registry (`COMMANDS`; `/help`,
  `/quit`; a debug build also has `/panic`, the test that proves the panic hook). A later row adds an entry and its
  handler in a module of its own. `/` lists them, filtered as you type.
- A turn's end prints `  inspect <url>` under its line when the result carries a link (theseus-x9swu), and `/inspect` says the
  last one again (`Chat::last_inspect`).
- `chat/dock.rs`: the dock's rows, from provider functions (`ABOVE_INPUT`: the live line, calls in flight, a question,
  a notice) a later row adds to (cli-tasks puts its task rows there), the input, the command list, and the status line
  as a list of `Segment`s (mode · directory · model · cost; the lowest `keep` goes first when it is too wide).
- `chat/rewind.rs` (theseus-23rk): Esc Esc on an empty line while no turn runs (`Chat::rewind_escape`; a single Esc during a
  turn is chat-ask's stop and is not bound) asks `files.changes` and opens the picker in the dock (`rewind::rows`, a
  provider in `ABOVE_INPUT`): each of your messages with its time and `tides.py +1 −1` or `no file changes`; Enter, then
  `f` (`files.restore`, then `session.fork` at `fork_at`), `c` (fork only) or `o` (restore only), Esc never mind. After
  a fork the chat goes on in it: the screen is cleared (`screen::CLEAR`, a finished line written raw), the fork's
  history is read and shown, and the message is back in the input. The first message has no node before it, so `f` and
  `c` refuse it. Tests: `chat/tests_rewind.rs`.
- `chat/render_seam.rs`: the one place a reply and a tool call are worded: `reply_lines` is term-render's markdown
  renderer, `tool_line` tool-words' `summarize`; nothing else renders text.
- `chat/editor.rs`: the multi-line input (Ctrl-J, Alt/Shift-Enter, `\` then Enter), this directory's history
  (a file of its own under `$XDG_STATE_HOME/theseus/history/`, named for the directory's last component and an FNV-1a
  hash of its path, `chat/env.rs`; a directory with no file yet reads the one file of before, `chat-history.json`, and
  never changes it), and the folded paste: more than 5 lines is one chip
  (a private-use character in the buffer, written back whole by `take`), so the whole message is one `turn.submit`.
- `chat/run.rs`: the loop, as `run.rs`: quiet by construction (a deadline only while something runs: the dock's clock
  at 10 Hz, the banner's 250 ms wait, a notice), at most 30 frames a second. `chat/guard.rs`: raw mode and bracketed
  paste on, put back on every way out; on a panic the hook alone does it (`Drop` skips it, as a release build's abort
  would).
- It does one `session.open` ({dir}) and one `session.watch` at start (and `session.list {n: 100}` for the banner's last
  conversation here); `theseus chat SID` reads `session.list` to find the session, then `session.history` once.
  A message typed before the session opens is held and sent once it does. `session.typing` goes once per idle spell.
- chat-ask (theseus-wy6v.1): `chat/ask.rs` shows a question in the input's place (the call in words, the hunk through
  `chat/diff.rs`, who asks, the expiry, the keys of `keys::ASK_KEYS`, one table: y n v t a e) and answers with
  `action.confirm`, author `theseus chat`; the keys work only with the line empty and the focus on the question (Esc
  goes back to typing, Tab back to it). `chat/stop.rs`: Esc during a turn sends `execution.stop` with `keep_jobs`
  (jobs that went to the background run on) and the transcript says what was verified. `chat/queue.rs`: a message
  typed while a turn runs waits in the dock, ↑ pulls it back, Ctrl-Enter stops and sends; it goes when the turn ends
  (steer-queued will deliver it into the turn: only `release_queue` changes). Tests: `chat/tests_ask.rs`.
- **Coming back** (theseus-6yka). `theseus -c` (`theseus-tui chat -c`) asks `session.list {dir, n: 50}` and continues
  the directory's most recently active conversation (`chat/reopen.rs`: `last_here`: a conversation, with a turn, not
  retired, `started_on` not `discord`, so never a task's, a DM's or a shared place's); with none it says so and opens
  a new one. `theseus -r` (`chat -r`) asks `session.list {dir, n: 200, with_text}` once and shows the picker
  (`chat/picker.rs`, in the dock in place of the input): typing filters the list in memory (every word of the query in
  the title, labels, directory, or the words of the first and last message and of the newest prompts; no request, no
  model), up/down choose, Enter continues it, Tab reads and shows every directory's (`session.list {n: 1000,
  with_text}`, once), Esc clears the query and then opens a new conversation, Ctrl-D leaves. The chosen conversation's
  `session.history {n: 500}` is replayed through the transcript's own lines (`events.rs::history`): the last 30 turns
  (`reopen::window`; a message over 6 lines is shown folded, its last lines kept), older turns one line, `N earlier
  turns · /history shows them`, which `/history` answers a page back (`before` the first replayed node). Leaving
  prints `theseus -c` and `theseus chat END`. Tests: `chat/tests_reopen.rs`, `picker.rs`'s own.

- **Modes** (theseus-lyxy): Shift+Tab asks the daemon for the conversation's next mode (`session.mode`, ask → edits →
  plan → auto); in the chat it is the key map's `CycleMode`, in the board it works at the input line only (`i`), since in
  the list Shift+Tab is still the walk back through the queue. The mode and the plan are read once when a conversation is
  opened or focused (`session.mode` without a mode) and kept by the `session.mode` notification; the status line says
  `edits (shift+tab)` and `plan 2/5` (the board's pane header too), the checklist goes into the transcript each time it
  moves (a notification whose plan changed, never one that only repeats it), and a plan's question shows its numbered steps and takes `e` for the edits-mode yes: chat-ask's one key table (`keys::ASK_KEYS`) has `e` as `Ask::Grant`, and the plan's grant offer is the question's own. The plan is the daemon's plan items (`session.mode`'s `plan`), the same the work board counts.
  Tests: `src/tests_mode.rs`, `chat/tests_mode.rs`.

Tests: `chat/tests.rs` (a scripted daemon through `Conn::over`, its bytes read back as a screen by `vt100`, a dev-dependency
only; the paused clock and the chat's wall clock move together; goldens `tests/golden/chat_80.txt`, `chat_120.txt`,
rewritten by `THESEUS_GOLDEN=write`), and `tests/chat_pty.rs` (the real binary on a pseudo-terminal against a fake daemon
on a Unix socket: the first screen, a bracketed paste of 60 lines as one message, and the terminal put back after SIGTERM,
SIGHUP, a panic and Ctrl-C twice). vt100 can read back only one screen of scrollback: keep a test's transcript within it.

- A question's card shows the call in words and the daemon's `preview` (`card::preview`, at most 12 rows of
  `diffview::preview_lines`); the pane shows tool lines in words, a failed command's last lines and a finished edit's
  diff (`render::Flight`). The changed words of a diff line are `MARK_ON`/`MARK_OFF` in its text: `ui::put` draws them
  on a background of their own and `ui::wrap` carries a mark over a cut. Tests: `src/tests_diff.rs`.

- `src/threads.rs` (theseus-j0yh): the current thread, the header's status line (only once a thread is known), Ctrl-T's
  picker (type to filter; `p` `r` `d` (twice) `a` act while the filter is empty, `/` types them; Enter switches), `/t` in the
  input line, a thread's session tagged `→ title` in the pane header. Requests: `thread.list` on opening the picker, at a
  connect with a thread current, and after a reply in a thread; keys in the picker send none but the acts. Tests:
  `src/tests_threads.rs`.

## Invariants

- **The protocol is the only way in.** What needs you is each view's `attention`, which the daemon computes; an
  answer is `action.confirm`, a stop `execution.stop`.
- **Quiet by construction.** No timer runs but the deadlines the app asks for. It redraws on events, at most 30
  frames a second.
- **The position rule.** A view applies only if its position is greater than the last one applied for its session.
  The first snapshot after a connect is the truth for every session no event has updated on the new connection: a
  daemon restarted on another store may have lower positions.
- **What you have seen is the client's**, on this machine, never the server's:
  `$XDG_STATE_HOME/theseus/seen.json`, one file per machine shared with the CLI, whose `history`, `watch`, and
  `confirm` record what they showed (theseus-yus0). A save merges by the greatest position and replaces the file
  atomically; the TUI takes up what the CLI recorded at each save. `tui-seen.json` is read when the new one is absent.
- **`theseus tui` passes its `--socket` first**, and the last `--socket` wins, so one typed after `tui` is kept.

## Tests

- `src/tests.rs`: the TUI over a scripted daemon (a JSON-RPC stream on a duplex, through `Conn::over`), drawn on
  ratatui's `TestBackend`, whose buffer each test reads as text. Run them as
  `cargo nextest run --workspace -E 'package(theseus-tui)'`.
- `src/tests_failure.rs` (theseus-7zgsm): a failed task's card (`[r] retry  [d] dismiss  [o] open`), `r` and `d` on the failed
  row under the cursor sending `task.retry` and `execution.dismiss` (anywhere else they still filter ready and done), a
  refusal said on the card, and the row leaving needs-you when the push says dismissed.
- `src/tests_answer.rs` (theseus-ds5os): the answer keys over `work.answer` (a budget question's `y` sends `reset`; a
  question that says `may_answer: false` sends nothing and says where to answer; a `confirm.resolved` from another
  surface clears the card).
- `src/tests_answer.rs` also holds a question with options (`owner_ask`, theseus-j7cje): its card lists the options (`card.rs`), the keys 1 to 6 send `choice:<i>` through `work.answer`, and `y` and `n` send nothing.
- `src/tests_notice.rs`: the policy's notices on the harbour rig (a question rings once with its words, spend rings
  nothing, a wake on a root rings once, a burst rings once). Push a view, `settle`, then move `NOW`: a view the loop
  takes after the clock moved is due a second later than the test expects.
- `src/tests_names.rs` (theseus-0n1v): a task named by its title in the tree, its notice and the arm prompt; a
  session whose first answer lacked its name asked again as its turns grow; the input line's `profile` and
  `carried`; Enter after a filter. The rig pins the CLI's time zone (`render::time::pin_for_tests`).
- `src/tests_paste.rs`: a paste is one event, never keys: text in the input line, sent on enter; elsewhere it opens
  the input line and answers, quits, stops, and arms nothing; with no session open it is dropped, never sent to a
  later one. Enter at start opens the first row. Bracketed paste is off on every way out, each of the four signals
  sent to the test's own process among them, read from the bytes the loop wrote (theseus-8hcg).
- `src/tests_order.rs`: the app alone, its order forced by hand: another surface's message lands in the place its
  `node.written` marked, above a reply that streamed while it was read (theseus-v6yc); and a fired reminder
  (`wake.noticed`, theseus-8ux6x) rings once with the policy's words (`notices::reminder_notice`), a check-in rings
  nothing, and `--notify off` rings nothing.
- `src/tests_stop_all.rs` (theseus-cdcwr): `S` asks once more (`S` or `y` confirms, any other key keeps the conversation) and sends
  `execution.stop` with `tasks`; `s` is unchanged.
- `src/tests_typing.rs` (theseus-tnky): the input line's first key sends one `session.typing` per idle spell (not
  one a key, none from the filter's keys), and an older daemon's refusal says nothing.
- `src/tests_work.rs` (theseus-k2vca): the regatta (a job, a model, a tool, a question, a failure) at 80 and 120
  columns, the minute tick and the countdown on the paused clock, header and title counts, a report under its row, the
  old daemon's fallback, and `events.lost` on the work stream.
- `src/tests_standing.rs` (theseus-slvl.4): the pill counts the records the daemon says hold in the open session (asked
  for its session and the `terminal` surface), none shows no pill, an older daemon's refusal says nothing, `p` lists them
  by kind and scope, and with no session open it says so.
- `src/tests_sync.rs` (theseus-pjmj7): `health` is read at each connect (`Purpose::Health`) and the OpenClaw sync's `warn`, a
  failed or a missed night, shows in the header; an older daemon's answer or refusal says nothing.
- The CLI's `tests/tui.rs` holds `theseus tui`: the exec, the socket and the arguments passed through, and exit 2
  when `theseus-tui` is found nowhere.

## Traps

- The `start_paused` tests need tokio's `test-util`, which the manifest's dev-dependencies name, so
  `cargo test -p theseus-tui` builds alone too (it once built only through the workspace's feature unification).
- The input line's `turn.submit` carries the session's last profile with `carried: true`, as `watch --interactive`
  does (theseus-0n1v): without it the daemon's live profile takes the turn, another model without a word.
- **A paste is never keys.** `TermEvent::Paste` goes to `App::paste`, never through `key`: a pasted `q` would quit
  and a pasted `y` would answer the card. A new terminal mode goes in `term.rs`'s `enter` and `leave`, both.
- A change to `theseus_client`'s `client` or `render` changes the TUI too.
- `theseus-tui` has no `--spawn`: a TUI over a spawned `--stdio` daemon would show only that daemon's sessions.
- A live check runs it in tmux, at 80×24 and 160×48, over a scratch daemon's socket. Never point it at the
  operator's socket from a test.
