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
  the real tree on the same wire. Tests inside it.
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
- `src/answer.rs` (theseus-ds5os): `theseus answer ID OPTION [--note WORDS]`, one `work.answer` for every kind of question
  (a call, a spend limit, a layer-1 change, an extension's ack, a reminder). The id is sent as typed: the daemon resolves
  a whole id or a unique end of at least four characters, and an end naming two questions is refused naming both. The
  option is one of the question's own (`approve`, `approve_trust`, `decline`, `reset`, `accept`, `ack`, or a reminder's
  `done`, `snooze_10m`, `snooze_1h`, `tomorrow_9`); a wrong one is refused with the right ones. `work.answer` is in
  `client::OPERATORS`, so a job's shell is refused before anything is sent. `theseus confirm` stays for listing and for
  following the turn an answer resumes (`action.confirm`: the same handlers). Tests: `tests/answer.rs`.
- `src/remind.rs` (theseus-8ux6x): `theseus remind WHEN NOTE` (`--every`, `--days`, `--until`, `--session`), `snooze`, `pause`, `resume` and
  `done`: the owner's verbs on wakes, one call each (`wake.create`, …), no model turn. With no `--session` a reminder lives in the owner's
  Home. A watching terminal says a fired reminder with its four answers as commands (`render/wakes.rs::noticed_lines`).
- `src/thread.rs` (theseus-nhew): `theseus thread` (list: slug, state, last activity, the session's context tokens and
  today's dollars, title), `thread new|show|edit|pause|resume|done|reopen`, and `ask --thread SLUG` (a turn in the
  thread's session). The writes are in `client::OPERATORS`, so a job's shell cannot make them.
- `src/standing_cmd.rs` (theseus-a7vi): `theseus standing` (the live records: id, kind, scope, since, source, text,
  and a last line "in every compile of their scope since they began: N compiles, M missing" from the manifests;
  `--all` adds the ended), `standing add [--kind order|preference|fact] [--scope everywhere|place:T|session:ID]
  [--until AGE] [--replaces ID] TEXT`, and `standing revoke ID`. The writes are in `client::OPERATORS`, so a job's shell
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
  than the CLI refusing it unseen), and `self digest`. The state's words are the library's `render/rsi.rs`
  (`self_state_line`), which health's `self` line uses too (`HealthResult.self_improve`).
- `src/keys.rs` (theseus-tnky): `watch --interactive`'s input. On a terminal it reads bytes (out of canonical mode and
  echo, signals on, restored on every way out, a panic's hook too) and keeps the line itself (text, backspace, Ctrl-U,
  Ctrl-W, Enter, Ctrl-D on an empty line), since a terminal's own line mode shows nothing until Enter, and the first
  key of a line is the `session.typing` notice's moment; a pipe is read by lines as before. `render/warm.rs` is
  health's `warm:` line.
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
- `src/print.rs`: the `Printer`, which writes the library's lines in one of four modes: `Text` (`ask`), `Quiet`
  (`ask --no-stream`), `Watch` (`watch`), and `Json`.
- `src/why.rs` (theseus-ighf): `theseus why [TURN] [--session S]`, what a turn's model was shown and why: it asks
  `context.explain` (a prefix or end of at least four characters names an older turn, resolved from its turn list),
  `policy.explain`, `node.list` (the turn's `tool_call` nodes, with the gate's own posture) and `ledger.tail` (its
  `route.decided` row), reads only, and prints the words of `theseus_protocol::why` (the renderer Discord's `/why`
  and the `/context` command to come call too; it writes plain lines, no colour). `--json` is the four answers as
  they came. Tests: `tests/why.rs` (a scripted daemon; goldens `why*.txt`; the requests are reads alone).
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
- `tests/tui.rs` covers `theseus tui`: the exec (the stand-in runs in the CLI's own pid), the socket and the
  arguments passed through, and exit 2, saying where it looked, when `theseus-tui` is found nowhere. Each test links
  the CLI into a directory of its own, since the workspace builds the real `theseus-tui` beside `target/debug/theseus`.

## Traps

- A change to `client` or `render` changes the terminal UI too.
- `ask`'s summary line can start with an escape sequence, so a `grep '^…'` on its output never matches. Wait on the
  process's pid instead.
