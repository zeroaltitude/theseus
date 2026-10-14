# CLOUD_REPORT: chat (theseus-cyha), branch cloud/20261013-chat

## Step 1: bare `theseus` opens an inline conversation here

**Found.** Main's TUI is a board; `theseus` alone printed usage. Store format on this base is 33 (not 32); no store change here.
No daemon frame carries a running job's output, so a job's "output so far" cannot be shown (see Left).

**Changed** (0fc494d, plus the report commit):
- `crates/theseus-tui/src/chat/`: state.rs/events.rs (no-I/O state), keys.rs (the one key map), commands.rs (registry: /help, /quit; debug builds also /panic), dock.rs (`DockRows` provider list `ABOVE_INPUT`, status line as `Segment`s, tasks segment from TaskChanged), render_seam.rs (`reply_lines`, `tool_line`, marked "replaced at term-render's and tool-words' joins"), editor.rs (multi-line, history, paste chip), screen.rs (inline, append-only bytes, no ratatui Terminal), run.rs, guard.rs, env.rs, banner.rs.
- `theseus-tui` main.rs: only the `chat [SID]` mode switch and a `mod` line. CLI: `chat_cmd.rs`, `Cmd::Chat`, bare check; `run()` was moved under clippy's line limit by one helper (`exec_tui`).
- `scripts/f10/arm_theseus.py`: s1, s2, s6 (opens the chat for the job, returns to the board for S7), s8, helpers. `test_f10.py`: 20 pass unchanged.
- `scripts/chat-bench.py`: the FAST row.
- New dev-dependencies of theseus-tui: **vt100 0.15** (tests only; Cargo.lock gains vt100, vte and a few small crates) and libc (already in lock).

**Proof.**
- `cargo nextest run -p theseus-tui -p theseus`: 484 passed before the last additions; theseus-tui alone 132 passed. Chat tests: 55 over a scripted daemon + vt100 (banner, Q&A once, live line moves, idle draws nothing, <=31 frames/s, call row then line, question y/n, paste chip + one submit, ctrl+o, ctrl+j/backslash, history on disk, ctrl-c, / list, resize redraws only the dock, scroll, 80/120 goldens, reconnect, early message, tasks count); tests/chat_pty.rs (7, real binary on a pty: first screen, 60-line bracketed paste = one turn.submit, SIGTERM, SIGHUP, panic, ctrl-c twice, no daemon exit 3); crates/theseus/tests/chat.rs (5: bare on pty execs `theseus-tui chat`, pipe => usage exit 2, help unchanged).
- Planted reverts: (1) paste sent as keys: 4 tests failed (folded paste, pasted y/q, ctrl+o, pty paste). (2) dock redrawn on a timer: `an_idle_chat_draws_nothing_on_a_timer` and `the_live_line_moves...` failed. (3) no restore in the panic hook: first showed NO failure because `Drop` unwound and restored; fixed by making `Guard::drop` skip on panic (a release build aborts), then `a_panic_puts_the_terminal_back` failed as it should.
- Gate (NO_BENCH): keel 0 findings, fmt, shape, features, clippy, cockpit, test build, reader rule ok. Suite: first run hit disk-full (freed target/); second run failed only on: the known L1/sandbox set (theseus-sandbox 20, theseusd sandbox 12), `gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` (passed alone on rerun), and **`theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, which fails alone too (21 s); I changed nothing it uses (not in the known list; I did not check main's result)**. `cargo deny --offline check`: ok. Benches skipped.
- FAST (release-thin, 1,000-session synth-store, `python3 scripts/chat-bench.py --bin-dir target/release-thin`): prompt p50 2.6 / p90 3.2 ms; banner p90 37 ms; echo p50 0.6 / p99 6.7 ms (idle); RSS 5.3 MB. Echo while a turn runs can wait for the 33 ms frame.
- F10 (release-thin, `driver.py theseus --bin-dir target/release-thin --out /tmp/f10`): **19 of 42 at both sizes, beats 5 of 5** (main: 11). Gained: S1a S1b S1c S2b S2c S2d S6a S8b. Kept all 11 (S1d S3a S3b S3d S6c S6d S7c S8a S8c S10b S11a). B1: 3 ms. Still red that were asked: **S2a** (waits for term-render), **S6b** (no frame carries job output).
- Live captures (tmux, both sizes) are in the F10 run's screens (S1.open, S2.reply, S6.run2, S8.typed); excerpt 80x24 S8.typed shows `› [pasted 60 lines · 2.4 KB · ctrl+o shows it]what do these log lines...`.

**Live check for the maintainer:** `python3 scripts/f10/driver.py theseus --bin-dir <build> --out /tmp/f10` then `python3 scripts/f10/scorecard.py /tmp/f10/theseus-*`; `python3 scripts/chat-bench.py --bin-dir <build>`; by hand: `theseus` in a tmux pane on a scratch daemon, ask, paste 60 lines, `/quit`, `theseus chat <id>`.

**Left / uncertain.**
- S6b needs a scrubbed job-output push frame (or `work.peek`); the raw spool file is unscrubbed so the client does not read it. `LiveTool.last` is the slot.
- "mode or posture" shows `default`; "N tasks" counts task changes seen since start (cli-tasks should replace it with the board).
- A late result of a call the chat never saw proposed shows `● Run` without its argv.
- Messages typed on another surface into the watched session are not mirrored.
- Shift+Enter only on terminals known to report it (kitty, wezterm, ghostty); Ctrl+J, Alt+Enter, backslash everywhere.
- Reply wrap is one column short of the edge (a design choice; also what S2d reads).
- Docs to update: spec Part III item for theseus-cyha, status.md, AGENTS.md map line (root is already ~20.4 KB); store format unchanged.

**Keel findings expected:** none.
