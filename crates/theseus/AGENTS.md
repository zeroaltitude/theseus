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
- `src/print.rs`: the `Printer`, which writes the library's lines in one of four modes: `Text` (`ask`), `Quiet`
  (`ask --no-stream`), `Watch` (`watch`), and `Json`.

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

- `tests/golden.rs` compares each scenario's output with its golden in `tests/golden/`.
  `THESEUS_GOLDEN=write cargo nextest run --workspace -E 'package(theseus)'` rewrites them: only for an output change
  you mean, with the diff read.
- `tests/connect.rs` covers how the CLI reaches a daemon (`--spawn`, and exit 3). `main` reads exit 3 from the
  error's text, which the library writes, so these tests hold the two together.
- `tests/refusal.rs` covers a refused answer: `theseus confirm` prints the daemon's reason and exits 1.
- `tests/tui.rs` covers `theseus tui`: the exec (the stand-in runs in the CLI's own pid), the socket and the
  arguments passed through, and exit 2, saying where it looked, when `theseus-tui` is found nowhere. Each test links
  the CLI into a directory of its own, since the workspace builds the real `theseus-tui` beside `target/debug/theseus`.

## Traps

- A change to `client` or `render` changes the terminal UI too.
- `ask`'s summary line can start with an escape sequence, so a `grep '^…'` on its output never matches. Wait on the
  process's pid instead.
