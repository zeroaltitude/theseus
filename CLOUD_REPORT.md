# Cloud report: thread-cli-tui, T5 (theseus-j0yh)

Branch `cloud/20261013-thread-cli-tui`. Code commit: f9a3630 (the report is the last commit on top).

## What I found
- T1 (thread-record) had joined; T2, T3, T4 had not, and neither had `theseus chat` (cli-tasks). So the reply tag is the
  turn's own thread, with no routed tags (`↪ move`, "guess") to render yet, and the `/t` / status line live in
  `watch --interactive` only. **Joiners:** port `interactive/thread.rs` (`/t`, status line, tag) to `theseus chat`, and render T4's routed tags beside `reply_tag`.
- `thread.list` carried no warm/cold: one field added, `ThreadRow.cache_ttl_secs` (the profile's cache TTL for the session's last
  turn; not stored, so no format bump). Warm = now - `last_active_ms` < TTL.

## What I changed
1. Protocol/core: `ThreadRow.cache_ttl_secs`, `CacheTtl::secs()`, filled in `thread_rows`; TS regenerated; core test asserts it.
2. Library `theseus_client`: `current.rs` (THESEUS_THREAD, per-tty file `$XDG_STATE_HOME/theseus/thread.json`, `pick`: home / number / slug /
   unique 4+ char start of slug or title), `render/thread.rs` (status line at any width, warmth, reply tag, picker row).
3. CLI: `theseus thread switch X|N|home` (prints `export THESEUS_THREAD=…`/`unset`), `ask` uses the thread only from the env
   (a named `-s` session wins), reply tag `→ title` on stderr; `watch --interactive` starts on the terminal's thread, `/t`, `/t N|NAME`,
   `/t home`, status line, tag; a thread with no session opens one with its first message and the watch moves to it.
   With no current thread no `thread.*` request is sent.
4. TUI: Ctrl-T picker (filter as you type; Enter switch, `p` pause, `r` resume, `d` done with a one-key confirm (`d` again), `a` all, Esc),
   `/t` in the input line, header status line (only once a thread is known, so a user with no threads sees the old header), pane header tag.
   Keys taken: Ctrl-T and `/t` (no letter key). Inside the picker `p r d a` are commands while the filter is empty; `/` makes them type.
5. F10: `scripts/f10/threads_check.py` + stand-in rules (`steps.py` T.* prompts), python tests in `test_f10.py`.

## How I proved it
- `cargo nextest run -p theseus -p theseus-tui`: 436 passed. New: tests in `current.rs`, `render/thread.rs`, `interactive/thread.rs`, `tests/thread_switch.rs`
  (3), `tests/thread_interactive.rs` (2: switch, list, move, tag, home, first-message-opens; and no thread.* request without a thread),
  `theseus-tui/src/tests_threads.rs` (7: header at two widths, picker snapshot/filter/no request on keys, `a` toggle, Enter switch,
  p/r/d and two-step d, `/t`, sessionless first message). Core/protocol/discord: 2478 of 2480 pass (see below).
- Planted reverts, each caught: `ask` reading `of_terminal` instead of the env -> `thread_switch::ask_uses_the_thread_only_when_the_shell_exported_it` fails;
  `d` acting on the first press -> `tests_threads::p_r_and_d_act_and_d_asks_twice` fails; warmth always warm -> `render::thread::tests::warm_inside_the_ttl_cold_after_and_silent_without_one` fails. Files restored and touched; `git status` clean.
  (The "current thread read from the store" revert was not planted: nothing reads the store for it; the picker test asserts one `thread.list` for any number of keys.)
- F10: `python3 scripts/f10/threads_check.py --bin-dir target/debug --out /tmp/f10-threads` -> `threads: PASS` (two threads on the stand-in, switched by `/t`,
  replies in order, status line `[Tide log · active · 16K ctx · warm]` / `[Moon table …]`, each session holds only its own words). `python3 -m unittest discover -s scripts/f10`: 24 OK.
- FAST: `theseus-sim bench turn --check`: 4 frames a plain turn. `bench lifecycle --runs 10 --check` (debug, this VM): every start/stop/swap/restart budget ok;
  one miss, "health, after the seed" p50 5.2 ms vs 5 ms, a path I did not touch (rerun was also over; slow 4-core VM, debug build). No timer added.
- fmt and clippy `-D warnings` clean; `cargo deny --offline check` ok; keel guard: 0 findings.

## Live check for the maintainer
`python3 scripts/f10/threads_check.py --bin-dir target/release-thin --out /tmp/f10-threads` shows `threads: PASS`. For the TUI by hand (I did NOT run a pty-driven TUI here;
the scripted-daemon tests cover it): on a scratch daemon with three threads (`theseus thread new a|b|c`, one `ask --thread` each), `theseus tui`, Ctrl-T, type to filter,
Enter, then `i`, send; the header shows the thread's status line and the pane header `→ title`.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: keel, fmt, shape, features, clippy, cockpit, test build, reader rule all pass; the suite ran 4492, 4457 passed, 35 failed:
33 are the known L1/root failures (theseus-sandbox contract and bench, theseusd sandbox); `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
fails identically on the base without my changes (verified by stash); `theseusd stop_checkpoint::a_clean_stop_with_every_service_on_leaves_nothing_to_replay` failed under a full disk
and passes alone. I ran deny (ok) myself; the later phases after the suite (generated TypeScript is committed) were not rerun end to end because the VM's disk allowance
(about 40 GB) was exhausted by debug builds twice.

## Keel findings expected
None.

## Left / uncertain
- `theseus chat`, T4's routed tags, and the TUI pty live check (above).
- Interactive loop: the herdr reporter keeps its first session's title after a `/t` move.
- The current-thread file is last-writer-wins across terminals (a convenience, not a record).
- The header shows `home` only once a thread is known (a user with none sees no change); the owner may prefer it always.
