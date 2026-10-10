# theseus (the CLI)

`theseus`: a thin client of `theseusd` for shells and pipelines, over the daemon's Unix socket or a spawned
`theseusd --stdio`. It links theseus-protocol and nothing else of Theseus. Its library, `theseus_client`, is shared
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
- `src/print.rs`: the `Printer`, which writes the library's lines in one of four modes: `Text` (`ask`), `Quiet`
  (`ask --no-stream`), `Watch` (`watch`), and `Json`.
- `src/render/history.rs`: `theseus history`'s own lines: each node with its short id, which `theseus reach` takes
  (theseus-glyw), and `page_lines`, where a page ends and the command for the next one either way (theseus-xo0m).
  Its `node_lines` is the history's own; `render.rs`'s, which the terminal UI shares, stays apart.
- `src/pages.rs` (theseus-7bee): `theseus sessions` and `theseus executions` print the newest 50 (`session.list
  {n}`, `execution.list {n}`), a footer (`render/sessions.rs`'s `page_footer`) naming `--before CURSOR` and `--all`, which
  keeps the bare list. The CLI asks `session.list {n: 1}` where it wants the newest session (`cmd::NEWEST`, `watch
  --interactive`, `history` with no session) and `{ids}` where it wants one by id; it never asks for every session
  but under `--all` and `herdr sync`, which reconciles every pane. `history` reads the newest 200 nodes (`-n`).
  `resolve::executions` reads the executions a page at a time. `tests/pages.rs` records each request's params.
- `src/names.rs`: what a session is called in every client (theseus-0n1v): a task's title (the store labels every
  task `task`), a conversation's label else its title, else its kind and the end of its id. The CLI, the terminal
  UI and herdr's reporter all call it; never read `label` before `title` at a site.
- `src/resolve.rs`: the one id resolver (theseus-0n1v) for `confirm`, `history`, `watch`, `stop`, `cancel`, `wait`
  and `explain`: a whole id, or a unique end of at least four characters; ambiguous, short and unknown names are
  refused, and `watch` of an unknown one exits 1. One read a resolution at most.
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
- `tests/tui.rs` covers `theseus tui`: the exec (the stand-in runs in the CLI's own pid), the socket and the
  arguments passed through, and exit 2, saying where it looked, when `theseus-tui` is found nowhere. Each test links
  the CLI into a directory of its own, since the workspace builds the real `theseus-tui` beside `target/debug/theseus`.

## Traps

- A change to `client` or `render` changes the terminal UI too.
- `ask`'s summary line can start with an escape sequence, so a `grep '^…'` on its output never matches. Wait on the
  process's pid instead.
