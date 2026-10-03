# theseus-tui

`theseus-tui`, the terminal UI (design `stage2` §2.9; theseus-7yx, steps 10b to 10f): every session of a running
`theseusd` in one sidebar, with its task trees; the queue of what needs you, answered inline; and the session in
focus, with its history and its input line. A client like the CLI, over the same socket: it links theseus-protocol
and the CLI's library (`theseus_client`), never the core. An installed binary of its own (`tool` in its manifest),
which `theseus tui` execs.

Key modules: `run.rs` (the loop), `app.rs` (no I/O), `board.rs`, `ui.rs`. Read by: `theseus tui`, which execs it.

## What's here

- `src/main.rs`: the arguments (`--socket`, `--notify`), the terminal's modes, and the loop's start.
- `src/run.rs`: the loop. One connection, read in one `select!` with the terminal's events (read on a thread of
  their own) and one deadline; reconnecting with backoff; the frame clock.
- `src/app.rs`: the state and every change to it (keys, the daemon's messages, answers). It does no I/O: it returns
  the requests to send.
- `src/board.rs`: the board. Views applied by the position rule, titles, the questions, the rows and trees, and the
  queue.
- `src/ui.rs`: drawing into a ratatui buffer. Nothing here changes the app.
- `src/detail.rs` (the focused session's history and events, as the CLI's `render` lines), `src/card.rs` (a
  question's card), `src/notice.rs` (the bell, OSC 9, OSC 777), and `src/seen.rs` (done until seen).

## Invariants

- **The protocol is the only way in.** What needs you is each view's `attention`, which the daemon computes; an
  answer is `action.confirm`, a stop `execution.stop`.
- **Quiet by construction.** No timer runs but the deadlines the app asks for. It redraws on events, at most 30
  frames a second.
- **The position rule.** A view applies only if its position is greater than the last one applied for its session.
  The first snapshot after a connect is the truth for every session no event has updated on the new connection: a
  daemon restarted on another store may have lower positions.
- **What you have seen is the client's**, on this machine, never the server's:
  `$XDG_STATE_HOME/theseus/tui-seen.json`.
- **`theseus tui` passes its `--socket` first**, and the last `--socket` wins, so one typed after `tui` is kept.

## Tests

- `src/tests.rs`: the TUI over a scripted daemon (a JSON-RPC stream on a duplex, through `Conn::over`), drawn on
  ratatui's `TestBackend`, whose buffer each test reads as text. Run them as
  `cargo nextest run --workspace -E 'package(theseus-tui)'`.
- The CLI's `tests/tui.rs` holds `theseus tui`: the exec, the socket and the arguments passed through, and exit 2
  when `theseus-tui` is found nowhere.

## Traps

- A change to `theseus_client`'s `client` or `render` changes the TUI too.
- `theseus-tui` has no `--spawn`: a TUI over a spawned `--stdio` daemon would show only that daemon's sessions.
- A live check runs it in tmux, at 80×24 and 160×48, over a scratch daemon's socket. Never point it at the
  operator's socket from a test.
