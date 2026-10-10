# The head-to-head speed bench

Theseus against Claude Code on one stand-in model (theseus-7gir.13): how long each harness takes to start, to send a
typed prompt, to show the first words of a reply, to turn a tool's result into the next request, and to resume a long
session; and what CPU and memory each spends doing it. The stand-in answers every request the same way for both, after
a fixed first-byte delay and in fixed chunks, so the model's time is equal and what differs is the harness.

## What you need

- A release build: `scripts/build.sh --profile release-thin`, which leaves `theseusd`, `theseus`, `theseus-tui` and
  `theseus-sim` in `target/release-thin/`. The stand-in is `theseus-sim fake-model` (with `--log`, `--ttfb-ms`,
  `--chunks` and `--chunk-ms`).
- Claude Code at the pinned version, `CLAUDE_CODE_VERSION` in `claude_code.py` (2.1.296), on `PATH` or given with
  `--claude`. Another version is refused unless `--any-claude-version`. Without Claude Code (or with
  `--theseus-only`), the Theseus arm runs alone and `run.json` says why.
- Python 3.11, the standard library only. Linux: the bench reads `/proc` and drives pseudo-terminals.

Nothing reaches a real API. Claude Code runs with `ANTHROPIC_BASE_URL` at the stand-in on 127.0.0.1 and a dummy key,
in a scratch HOME and CLAUDE_CONFIG_DIR per run; each Theseus run is a scratch daemon with its own config, state dir
and socket. The operator's daemon and its socket are never touched.

## Running

```bash
scripts/build.sh --profile release-thin

# Every row, both arms, 10 runs each, A/B interleaved.
python3 bench/h2h/run.py --bin-dir target/release-thin --runs 10 --out /tmp/h2h

# Some rows; one arm.
python3 bench/h2h/run.py --bin-dir target/release-thin --rows T1,T2,T3 --runs 20 --out /tmp/h2h-t123
python3 bench/h2h/run.py --bin-dir target/release-thin --theseus-only --runs 10 --out /tmp/h2h-theseus

# The report: docs/benchmarks/<the run's date>-head-to-head-speed.md, its .json, and its figures.
python3 bench/h2h/report.py /tmp/h2h/run.json
python3 bench/h2h/report.py /tmp/h2h/run.json --out-dir /tmp/h2h-report --force
```

The stand-in's timing: `--ttfb-ms 300 --chunks 8 --chunk-ms 25` (the defaults). T5's session length:
`--long-turns 50`. T7's span: `--idle-secs 10`. The run's directory keeps every raw sample (`run.json`), each arm's
stand-in log (`standin-<arm>.jsonl`), the rules (`rules-<arm>.json`), and every terminal recording (`pty/*.rec`, one
line per read: the monotonic ns and the bytes in hex; `pty.replay` reads one back). None of it goes in the repository:
the report and its data file do.

## The rows

Every time is CLOCK_MONOTONIC, the bench's `time.monotonic_ns()` and the stand-in's log alike. Screen stamps come from
`pty.Transcript`: the output as text (escapes dropped, `ESC[nC` as n spaces), each read stamped as it arrives, and a
marker stamped by the read that completed it. Each reply is `<word>-<marker> … end-<marker>`, the marker a fresh word
the prompt carries (`h2h-hello marker=M1A2B3C`), so a stamp names its turn.

| Row | What | From | To |
|---|---|---|---|
| T1 | Start to a ready prompt | the surface's spawn | Claude Code: its prompt's footer, "bypass permissions on". Theseus: the TUI's board with the run's session on it and "socket ok" (its daemon already serving, as Theseus runs; the daemon's own start is `daemon_start_ms` beside it) |
| T2 | Typed prompt to the request leaving | interactive: the Enter after the prompt's echo; one-shot: the spawn | the arrival of the turn's first request at the stand-in |
| T3 | Model's first byte to text on screen | the stand-in's first byte of the reply's response | the reply's first marker on screen (one-shot: on stdout) |
| T4 | Per tool call (read, shell; and task3's edit) | the last byte of the response asking for the tool | the arrival of the next request, which carries its result |
| T5 | Resume of a long session | the surface's spawn on the session (`claude --resume`; the TUI, Enter opening it) / the one-shot's spawn (`ask -s`, `-p --resume`) | its last reply's marker on screen and the prompt ready / the request's arrival |
| T6 | CPU per turn | `/proc` utime + stime of the surface's tree (and Theseus's daemon) before Enter | the same after the reply's end; one-shot: `wait4`'s rusage |
| T7 | Idle CPU and memory | `/proc` samples every 0.5 s for `--idle-secs` after the turns | CPU as a share of one core; RSS's mean |
| T8 | Under neighbour IO | T1 to T4 again, beside a loop of `dd … bs=1M count=64 conv=fsync` | |
| task3 | A three-tool task end to end | Enter (one-shot: spawn) | the reply's end marker (one-shot: the exit); read `notes.txt`, edit a line, run `wc -l`, answer |
| first_request_bytes | The first request's size | | the stand-in's `req_bytes` for the turn's first request |

The work is the same for both arms, each in its own tools (`standin.py`): Claude Code's Read, Edit and Bash, and
Theseus's fs_read, fs_edit and proc_run, on the absolute path of `notes.txt` in the run's work directory. Every tool is
open on both: Theseus's policy `open`, Claude Code with `--dangerously-skip-permissions`.

## The files

| File | What it is |
|---|---|
| `run.py` | The runner: the rows, A/B interleaved (ABBA by pairs), and `run.json`. |
| `report.py` | The report: the answer first, the table, the figures (bench/report/charts.py), the method, the threats. |
| `standin.py` | The stand-in's rules for each arm, its process, and its log's reader. |
| `pty.py` | The terminal driver, the transcript and its stamps, and the `/proc` sampler. |
| `oneshot.py` | One-shot runs: stamps, `wait4`'s CPU and peak RSS, the join with the log by time, the A/B order. |
| `joins.py` | The pure joins: T2, T3, T4, T6 and T7 from stamps and the log. |
| `claude_code.py`, `theseus.py` | The arms: Claude Code's scratch home and command lines; Theseus's scratch daemon and surfaces. |
| `fixtures/` | A recorded stand-in log, a recorded TUI transcript, and a run.json, for the tests. |
| `test_*.py` | The tests: `python3 -m unittest discover -s bench/h2h -p 'test_*.py'`. |

## Traps

- Claude Code refuses `--dangerously-skip-permissions` to root outside a sandbox; on a root box the arm sets
  `IS_SANDBOX=1`. Its scratch settings turn auto mode off, or its first-run offer covers the prompt and T1 never
  comes.
- The TUI's `i` opens the input line only once the session is on screen; the bench presses it until the line's `>`
  shows. Typing before then would send the prompt's letters as the board's keys (`q` quits).
- A full-screen surface draws a word in pieces, by cursor moves between redraws, so a marker can be split in the
  transcript. `pty.Screen` keeps the 120x40 grid as well, and a marker the bench watches is stamped by the read after
  which the grid first holds it (the earlier of the two stamps wins).
- Claude Code (2.1.296) merges messages of one role: a new prompt lands in the user message that holds the turns
  before it (earlier prompts and results), a turn's own results can sit before its prompt, and every request carries
  `<system-reminder>` blocks. The stand-in takes a turn's opening text from the last plain text block, and its step
  from the requests it has answered for the run's marker, so both harnesses get the same steps. A turn whose reply
  never ends stops its session (`TurnStuck`), since a prompt typed into a busy surface is queued.
- A Theseus scratch daemon's socket and state dir live in a short directory under `/tmp` (`h2h-*`, removed at exit):
  a Unix socket's path must fit in 108 bytes, and a spool socket under a long state dir does not bind, so the daemon
  falls back to its 1 s heartbeat for a job's completion (it says so only in its log), which reads as a 1 s tool call.
- `pty.py` is not the standard library's `pty`: the runner and the tests load it by path, as `h2h_pty`.
