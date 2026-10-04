# Cloud report: a terminal toolset, a pty per session (theseus-n88g.4, B4)

Branch `cloud/20261004-terminal-tools`, cut from `main` at `d9b0931`. Started 03:01 UTC; this report written about 04:05 UTC.

## The step

### What I found

- `proc.run` is a job with a typed argv, no pty, and stdin from `/dev/null`, so nothing interactive can be driven.
- A toollet's `ToolCtx` doesn't know the call's session, and a terminal belongs to a session. I didn't change `ToolCtx`
  or the `Tool` trait. `toolrun`'s async path runs a `term.*` call through `Terms::run(tool, session, input, ctx)`
  instead of `Tool::run_async`. That is one `match` in `run_inproc`. The tools stay ordinary `Tool`s for planning,
  the catalog, and the gate.
- The gate already judges any plan that carries an `argv` the way it judges `proc.run`: the floor, the approve and
  allow lists, and the path arguments. So `term.open` plans its argv and cwd, and `term.send` plans its terminal's
  program's argv and cwd. Nothing in `policy.rs` changed.
- `ToolRuntime::brokered` would have named a broker grant for a terminal's program, which then never reaches it. A
  terminal's program now gets no grant, and `brokered` returns early for the `term` family. Sending a secret into a
  pty whose screen the model reads is a design question for the owner. I chose no.
- Sessions never end as such. Their execution does: complete, failed, cancelled, or out of budget. "The session's
  end" is that execution reaching a terminal state at the end of a turn (in practice a task that reported, or a
  failure). An ordinary conversation's terminals live until `term.close`, a cancel, a `/stop`, or the daemon's stop.
- `tests_output::the_cores_output_matches_its_golden` fails on this VM before my change: the VM runs in UTC, and the
  golden pins a `-` time-zone offset (`-#:#`). With `TZ=America/Los_Angeles` it passes. Not mine; see the gate.

### What I changed (commit `7d330f7`)

New:
- `crates/theseus-core/src/term/`:
  - `mod.rs`: `Terms`, the registry, at most 4 terminals a session. Opening, sending, closing one, closing a session's
    terminals, and closing all of them. `Core::close_terminals` closes all at the daemon's stop. Also the read's
    snapshot and its diff.
  - `pty.rs`: `posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r`, and `TIOCSWINSZ`. The child goes through
    `children::spawn(Kind::Owned)` with `setsid` and `TIOCSCTTY` and the operator's umask, with the job environment
    (`proc_env`, `THESEUS_SESSION`, `TERM=xterm`). A reader thread per terminal feeds the screen and writes the
    screen's answers (`6n`, `c`) back.
  - `vt.rs`: the VT model, plus `keys.rs` and `tools.rs` (the four tools).
- `fact/term.rs`: `TermOpened` and `TermClosed`, with ledger kinds `term.opened` and `term.closed` and their
  narrative lines.
- `crates/theseus-protocol/src/term.rs`:
  - `TerminalInfo` (health's `terminals`).
  - `summary()`: the tool line, shared by the CLI and Discord.
  - `health_line()`.
- `tests_term.rs`.

Small edits in shared or long files:
- `toolrun.rs`: the field, registration, the async dispatch, recording the open and close facts, and the early
  return in `brokered`.
- `turn.rs`: 8 lines, the close at the execution's end.
- `rpc/driver.rs`: the close at a cancel and at a `/stop`.
- `outbox.rs`: `finish_stop` closes all terminals first.
- `rpc/methods.rs`: health's line.
- `config.rs`:
  - `crate::term::NAMES` in the `[policy.tools]` check.
  - The template tests' counts.
- `external.rs`: `Listed::in_text`.
- `protocol/lib.rs`: `pub mod term`, health's `terminals` field (skipped when empty), and `ledger.rs`'s 2 kinds.
- CLI `render.rs`: a `⌨ term.send t1 "print(1)⏎" Ctrl-C` line on `tool.proposed`, and a health line per terminal.
- Discord `render.rs`: `summarize` names the terminal and keys.
- The template: `[policy.tools]` lines for the four tools. `term.read` and `term.close` are open, like the other
  reads. `term.open` and `term.send` are commented, so they inherit `enforcement`.
- The regenerated TypeScript.
- `theseus-core/AGENTS.md`'s Terminals bullet.

No new dependency. Cargo.lock and the package-lock files are unchanged. No stored record changed, so
`MANIFEST_FORMAT` was not bumped: the two new ledger kinds are strings in the existing ledger record, which an older
build reads as unknown kinds.

The tools in detail:
- `term.open {argv, cwd?, rows?, cols?, quiet_ms?}`:
  - Class Run, async.
  - Returns the id (`t1`, `t2`, … counted across the daemon) and the first screen, after up to `quiet_ms` (default
    300) of quiet, at most 3 s in all.
- `term.send {terminal, text?, keys?}`:
  - Class Run.
  - Sends the text first (each `\n` is Enter, CR), then each named key: Enter, Tab, Escape, Backspace, Delete,
    Insert, Space, the arrows, Home, End, PageUp, PageDown, F1–F12, and `Ctrl-x`, `C-x`, or `^x`.
  - An unknown key is invalid input.
  - Answers once the keys are sent. It returns no screen.
- `term.read {terminal, quiet_ms?, until?, timeout_ms?}`:
  - Class Read.
  - Gives the numbered rows (blank trailing rows elided), the cursor, whether it is full-screen, and the program's
    state (running, exited N, signal N).
  - Also gives the lines that scrolled off the top since the last read (a scrollback of 2,000 lines), and which rows
    changed since the last read.
  - `until` is plain text that may span rows; its trailing blanks are cut, since rows' are. It takes precedence over
    `quiet_ms`.
  - The wait is bounded by `timeout_ms` (default 10 s, at most 60 s), and it ends early when the program ends.
- `term.close {terminal}`: class Read. It sends SIGHUP and SIGTERM to the process group, to every descendant, and to
  every process holding the pty's slave open (found in `/proc/*/fd`), waits 500 ms, then sends SIGKILL to what is
  left and reaps the program.

### How I proved it

Tests, all in theseus-core (21 new, plus 1 in theseus-protocol):

- `term::tests` (13):
  - Golden screens from recorded escape sequences:
    - a shell with a backspace;
    - tabs, the deferred wrap, and scrolling into the scrollback;
    - cursor moves and every erase;
    - insert and delete of characters and lines;
    - a scroll region with reverse index;
    - the alternate screen entered and restored (a recorded `vim` start and quit, and a recorded `less` session);
    - escapes dropped whole (OSC, charsets, SGR, cursor shape, DCS) and UTF-8 split across feeds;
    - the answers to `6n`, `c`, `>c`, and `5n`.
  - The named keys.
  - Real programs on ptys:
    - `sh`: a prompt, `echo $((6*7))` landing on its row, a read's "Changed since the last read: rows …", and a
      quiet read's "Nothing changed". Ctrl-C ends `sleep 4242; echo after` (the sleep goes, `after` never prints, and
      the shell takes the next command). Another session can't reach the terminal. Its health line. The close leaves
      the shell dead.
    - `python3 -q`: `>>>` appears, `sum(range(101)) * 3` gives `15150` on screen, Ctrl-C prints
      `KeyboardInterrupt`, and Ctrl-D ends it, which the read reports.
    - `cat`, listed in `external_programs`: it echoes, every screen is marked outside text, and Ctrl-C ends it. A
      shell becomes marked once it is sent `gh issue view 1`.
    - The 4-terminal limit, with another session unaffected.
    - A close leaves no child: a background `sleep` that ignores HUP and TERM, and a `setsid sleep` that left the
      tree, are both gone after the close.
- `tests_term` (7), through the whole core:
  - The gate's decisions: `term.open python3` waits on the approve list, `term.open cat` runs on the allow list,
    `term.open op …` is the floor, and `term.open sh` is at `notify`. `term.send` to a python3 terminal waits and
    `term.send Ctrl-C` to a cat terminal runs, each by its program. An unknown terminal is invalid input. The classes.
  - A listed program's first screen holds its session (`via: program`): the next `term.send` waits, and `term.read`
    doesn't. Also the `term.opened` row and health's line.
  - An unlisted program holds nothing.
  - A terminal closed, with its `term.closed` row (`by`) and its program dead:
    - by a cancel;
    - by a `/stop`;
    - by the daemon's stop (two sessions);
    - by a task's session end: the task opens `sleep 4747` and reports, and the terminal closes with no
      `term.close`.

Runs:
- `cargo nextest run --workspace -E 'package(theseus-core) and (test(term::) or test(tests_term))'`: 21 of 21.
- The whole theseus-core package: 602 tests run, 601 passed, and 1 failed, the time-zone golden (pre-existing, above).
- Under load (AGENTS.md's recipe: four `while :; do :; done` loops at nice 0, the tests at `nice -n 19`): 5 runs of
  the 21, all 5 green. The loops were killed by their pids.

Planted reverts:
1. A terminal left open at its session's end (`if session_ended && false` in `turn.rs`):
   `tests_term::a_tasks_terminal_closes_at_its_sessions_end` failed ("no the task's terminal closed in 20 s"). The
   other 6 passed. The file was restored, touched, and the tests pass again; `git status` was checked.
2. A listed program's screen not marked external (`let external = None` in `Terms::open`): two tests failed.
   - `term::tests::cat_echoes_and_a_listed_programs_screen_is_outside_text` failed with "its open's screen is
     outside text".
   - `tests_term::a_listed_programs_screen_holds_its_session` failed with "the send waits after the screen".

   The file was restored and touched, and `git status` was checked.

My own live check, on a scratch daemon of this build: a fresh state dir, the template with the model's `api_base` at
a scripted stand-in Messages API on 127.0.0.1 (a short Python server), a fake `op`, and Discord and the web UI off.
One `theseus ask` drove `term.open python3 -q`, `term.send "print(6 * 7 * 1000 + 1)\n"`, `term.read until "42001"`
(it appeared), and `term.close`. It then drove `term.open vim -u NONE -N <work>/note.txt`, `term.read` quiet,
`term.send "ihello from vim"` then Escape, `term.send ":wq\n"`, and `term.read` ("its program exited (0)"); the file
held `hello from vim`. Finally it ran `term.open sleep 4949`.
- The CLI showed `⌨ term.send t1 "print(6 * 7 * 1000 + 1)⏎"` lines, and `theseus health` showed a `terminal t3
  (sleep) · ses_… · pid … · 24x80 · running · …` line.
- `theseus stop <session>` closed both terminals (`by: its conversation was stopped`), and `sleep 4949` was gone.
- A second ask opened three more, `theseus shutdown` ran, and nothing was left. After a restart, the ledger showed
  three `term.closed` rows `by: the daemon stopped`.
- Every process I started was stopped by its pid.

### The live check for the maintainer

With a real model, on a scratch daemon of the step's build (its own `--config`, `--socket`, and `--state-dir`), with
`python3` and `vim` on PATH, and `[policy] enforcement = "notify"`:

```bash
D=$(mktemp -d)                      # the scratch state dir; write $D/config.toml from your scratch template
theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state &
theseus --socket $D/sock ask "Use the terminal tools (term_open, term_send, term_read): start python3 on a \
terminal, compute 2**100 + 7 there, and tell me the number. Then open vim on $D/note.txt, write the line \
'hello from a terminal', save and quit, and close every terminal you opened."
cat $D/note.txt                     # hello from a terminal
theseus --socket $D/sock ledger -n 300 | grep -E 'term\.(opened|closed)'
theseus --socket $D/sock health | grep '^terminal'   # none left, if the model closed them
```

What it should show:
- `⌨ term.open python3`, `⌨ term.send t1 "2**100 + 7⏎"`, and `⌨ term.read t1 until …` lines.
- The answer `1267650600228229401496703205383`.
- `term.opened` and `term.closed` rows (`by: term.close`).
- On Discord, the tool lines name the terminal and keys.

Then the closes:

```bash
theseus --socket $D/sock ask "Open bash on a terminal and run 'sleep 600' in it, and leave it running."
theseus --socket $D/sock health | grep '^terminal'   # terminal tN (bash) … running
theseus --socket $D/sock stop <session>              # its term.closed row says: its conversation was stopped
pgrep -af 'sleep 600' || echo gone
theseus --socket $D/sock shutdown                    # with one open instead: by "the daemon stopped"
```

And the hold: set `[policy] external_programs = ["gh", "cat"]`, then ask for `term.open cat`, type into it, and read.
The second `term.send` should wait for approval. Its reason names the external text (`term.open cat`), and
`theseus health` lists the session under external text.

### What is left, or uncertain, and what the owner should hear

- **Grants**: no broker grant reaches a terminal's program (`brokered` returns early). A program that needs its
  token (`gh`) runs through `proc.run`. Should a terminal ever get one, the screen would need the job wrapper's
  `redact` too.
- **In memory**: terminals don't survive a restart or an exec. Their ptys die with the daemon, and the kernel hangs
  up each program's session. The rows are the record. A restart onto a changed note (the exec) goes through
  `finish_stop`, so it closes them first and records it.
- **An ended program keeps its slot** until `term.close` or its session's end, so the model can still read its last
  screen. The zombie is reaped at the next read, at health, or at the close.
- **Ids are daemon-wide counters** (`t1`, `t2`, …). Every run checks the session. A `term.send` plan by id can name
  another session's terminal's argv in its own gate record, and the call is then refused at run. Ids random per
  session would close that small leak.
- **`term.close` is class Read** so it never waits on the external-text hold: it only stops what the session
  started. Say if it should be Run.
- **The VT model leaves out**: colors and attributes, wide characters (a CJK character or an emoji takes one cell),
  combining marks, insert mode (`4h`), origin mode, program-set tab stops, character sets (line drawing shows as
  ASCII letters), and the mouse. `until` is plain text, not a pattern (no regex crate in core).
  - Were a crate allowed, I'd pick `vt100`, which models the screen whole, wide characters and attributes included.
    It would bring `vte`, `unicode-width`, and `itoa`.
- **The cockpit** (left alone) should show, per session:
  - each open terminal from health's `terminals`: id, program, size, running or ended, its bytes, and an
    outside-text badge with the listed program;
  - its last screen as a monospace block, read from the latest `term.read` or `term.open` result node of that
    terminal;
  - the `term.opened` and `term.closed` rows in the session's timeline, with `by`;
  - a close button that is the operator's `/stop`. A dedicated `term.close` from the cockpit would need a protocol
    method, which this step doesn't add.
- **Docs for the maintainer to write**:
  - `docs/status.md`: the step, the four tools, and the template lines.
  - The spec's Part III item: what it built, its proofs, the divergences above (the session's end as its execution's
    end, no grants, close as a read), and the open items.
  - Possibly §3.23 or §3.24's list of toollets, to add the terminal family as a native exception to "no stdin".
  - The README is unchanged.

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. These phases passed: fmt, shape, clippy, bench
build, test build, and the reader rule (9 of 9). The suite ran 1,773 tests: 1,741 passed, 32 failed, and 10 were
skipped. All 32 failures are the sandbox's root-only refusals (theseus-pv6i: "the daemon runs as root, and Linux
exempts root from RLIMIT_NPROC"):
- `theseus-sandbox::bench spawn_100`;
- 19 `theseus-sandbox::contract` clauses;
- 12 `theseusd::sandbox` tests.

The suite's other test outside my change: without `TZ` set,
`tests_output::the_cores_output_matches_its_golden` also fails here, because the VM is in UTC. It fails the same way
on clean `main` and passes with a `-07:00` zone.

So I ran the phases after the suite myself:
- protocol types: clean once staged.
- `theseus-sim bench turn --check --runs 5 --burst 0`: 5 frames at the p95, budget 5, ok.
- `cargo deny --offline check`: advisories, bans, licenses, and sources ok.
- `web/` lint and build, and the cockpit's lint, test, and build: ok.
- `crates/theseusd/web/dist`: unchanged.

The lifecycle and jobs benches were skipped (`THESEUS_GATE_NO_BENCH`).
