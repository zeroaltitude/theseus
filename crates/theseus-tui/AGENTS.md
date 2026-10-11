# theseus-tui

`theseus-tui`, the terminal UI (design `stage2` §2.9; theseus-7yx, steps 10b to 10f): every session of a running
`theseusd` in one sidebar, with its task trees; the queue of what needs you, answered inline; and the session in
focus, with its history and its input line. A client like the CLI, over the same socket: it links theseus-protocol
and the CLI's library (`theseus_client`), never the core. An installed binary of its own (`tool` in its manifest),
which `theseus tui` execs.

Key modules: `run.rs` (the loop), `app.rs` (no I/O), `board.rs`, `ui.rs`. Read by: `theseus tui`, which execs it.

## What's here

- `src/main.rs`: the arguments (`--socket`, `--notify`), raw mode and the alternate screen, the signals that end
  the loop as a quit does (`ending_signals`: SIGTERM, SIGHUP, SIGINT and SIGQUIT), and the loop's start.
- `src/term.rs`: the TUI's own terminal modes, focus events and bracketed paste: on when the loop starts, off on
  every way out (a quit, an error, a signal, and a panic's hook) (theseus-8hcg).
- `src/run.rs`: the loop. One connection, read in one `select!` with the terminal's events (read on a thread of
  their own) and one deadline; reconnecting with backoff; the frame clock.
- `src/app.rs`: the state and every change to it (keys, the daemon's messages, answers). It does no I/O: it returns
  the requests to send.
- `src/board.rs`: the board. Views applied by the position rule, titles, the questions, the rows and trees, and the
  queue.
- `src/ui.rs`: drawing into a ratatui buffer. Nothing here changes the app.
- `src/detail.rs` (the focused session's history and events, as the CLI's `render` lines), `src/card.rs` (a
  question's card), `src/notice.rs` (notices by the protocol's one policy, `theseus_protocol::notices`, delivered by
  the bell, OSC 9, OSC 777). Done until seen is `theseus_client::seen`, shared with the CLI.

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
  `node.written` marked, above a reply that streamed while it was read (theseus-v6yc).
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
