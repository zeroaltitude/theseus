# CLOUD_REPORT: tail2-chat

Branch `cloud/20261021-tail2-chat`, based on `origin/integ/threads` at `8b641b6b`. Store format unchanged (48). Cargo.lock
unchanged. No ceiling touched (`theseus-protocol/src/lib.rs` is 2,556 of 2,733; `theseus-discord/src/runtime.rs` is
unchanged at 3,499).

## Decisions and discrepancies the owner should hear first

- **The pin decision (made by the brief; the owner may override): no pin.** `theseus chat SESSION` keeps sending no `via`,
  and `may_route` is unchanged. A chat on a thread's own session is never re-routed (`board.of_session(&home)` returns
  `None` early); a named home session is the place's home, where routing is the feature. Pinning every named session
  would turn routing off for every resumed chat. Proved by `a_chat_on_a_threads_own_session_is_never_rerouted` (new).
- **The brief says unlink writes "one `thread.unlinked` row per end".** It does not today: an unlink writes one row for the
  pair and announces both ends (the existing test asserts 1). I kept "as today", and the new test asserts one row.
- **Commits.** Three, not five. 7kjb7, umb8a and fzt4v edit the same hunks of `chat/events.rs` and `chat/thread_ask.rs`
  and share the protocol type, so they are one commit naming all three ids. t8fow and ma3g8 are their own.
- **Not signed:** this VM has no key. Commits are unsigned.
- **Disk.** The VM's disk allowance ran out twice during a full-workspace debug build (the linker died with a bus error).
  I deleted `target/debug` and `target/debug/incremental` (rebuildable, inside the repo) and built with
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`. The gate below ran with those two variables set. Nothing outside the
  repository, /tmp and the scratchpad was deleted.

## theseus-7kjb7: `thread.placed` at placement

Found: `TurnSubmitResult.thread` only arrives when the placed turn ends, so the chat said nothing about where a message
went during a long turn.

Changed (commit `676eab4`):
- New `crates/theseus-protocol/src/thread_placed.rs` (`ThreadPlaced { session_id, placed, turn_id, tag }`), one `mod`,
  one `pub use`, one `THREAD_PLACED = "thread.placed"` in `lib.rs`; `Event::ThreadPlaced` in `events.rs` (its session,
  and its turn id in `turn_id()`); the type added to an existing line of `ts.rs`' list (the brief said an
  `include_str!`, but this branch's `ts.rs` lists types by name, and the `ts` test names the type it lacks);
  `cockpit/src/protocol.gen/` regenerated (`ThreadPlaced.ts`, one line in `index.ts`).
- `rpc/thread_flow.rs`: `route_to_thread` calls a new `tell_placed`, which builds an `EventSink` as `ask_thread` does and
  sends `Event::ThreadPlaced` to the submitting connection only, when the tag has a slug. Not stored; `thread.routed` at
  the turn's end stays the row.
- TUI: `notified_one` hands `ThreadPlaced` (for this chat) to `thread_placed`, which prints `→ thread <title>` and the
  `go there` line at once and records the turn as told; `turn_ended` prints them only for a turn not told, so a daemon
  without the notification still gets the line at the end. `placed_line` now takes `(&ThreadTag, &str)`.

**`Handoff.turn_id` is the placed turn's id.** `route_to_thread` mints it (`new_id("turn")`), stores it in the handoff, and
the turn adopts it: `turn.rs` takes its id from `turn_id_for(&sid)`, which reads the minted one
(`turn/inbound_step.rs`). The test checks the notification's `turn_id` equals the result's.

## theseus-umb8a: the placed turn and its tasks are the chat's

Changed (same commit): `Chat.placed` (one field, `thread_ask::Placed`: the last 8 placed sessions, and the told set).
`ours_in(session, turn)` is the chat's own session, or a session in `also` where an event that names its turn names one
in `mine`. `ToolProposed`, `ToolStarted`, `ToolEnded`, `TurnStarted`, `TurnFailed` and `TurnEnded` key on the turn id;
`ConfirmRequested` and `TaskChanged` carry no turn id and key on the session. A `TurnStarted` of a session in `also`
whose turn is not in `mine` is ignored. The result path adopts a placed turn it was not told of (`adopt_result`).

**What a second `work.watch` does on one connection: it replaces the first.** `Bus::watch_work` drops the connection's
earlier watcher (`w.retain(|x| x.conn != conn)`) and pushes the new one, whose scope is rebuilt from `roots`. So the chat
sends `work.watch` again with `roots = [its session] + also` (`Chat::watch_with`), interactive, limit 0; the snapshot
replaces the board's views (it prints no line for work already running). `watch_work(sid)` is unchanged for the two
existing callers.

## theseus-fzt4v: a held turn names no model

`turn_ended`: for `stop_reason == "held"` the line's second part is `held: no model ran` in place of the model's
name, the reason is not pushed, and `self.model` is not set from the result. Other turns' lines are unchanged (the
existing goldens and `tests.rs` expectations pass untouched).

## theseus-t8fow: unlink by the live slug

`unlink_ends` now takes the board; when neither record holds the other it looks for a link of `a` whose `live_of` is `b`,
or of `b` whose `live_of` is `a` (`merged_pair`), and unlinks that pair. Nothing found is today's no-op (a second unlink
writes no frame; asserted). `link_ends` unchanged.

## theseus-ma3g8: the select pick through the gateway

One new test in `tests_gateway/thread_ask.rs`; it needed no change to the code under test.

## Tests, and each planted revert

Each plant: break the code, run the named test, restore from a backup copy and `touch`; `git status` and a grep for the
planted text confirmed every file restored. The first is the key test.

| Test | Plant | Failure |
|---|---|---|
| `a_placed_turns_task_shows_in_the_chats_footer` (key) | `also` left out of the board's roots | `tests_thread_ask.rs:194` (the second `work.watch` has roots `[SID]`) |
| `the_placement_is_told_when_it_happens` | told-once check dropped | `tests_thread_ask.rs:218` (the line printed twice) |
| `another_turn_of_the_thread_is_not_the_chats` | `TurnStarted` keyed on `also` alone | `tests_thread_ask.rs:230` (a live line for the other turn) |
| `a_held_turn_names_no_model_in_the_chat` | `held` branch dropped | `tests.rs:188` (timed out waiting for `held: no model ran`) |
| `a_placed_message_tells_the_submitting_connection` | send dropped | `tests_thread_route.rs:1421` (no `thread.placed`) |
| `unlink_by_the_live_slug_after_a_merge` | old no-op | `tests_thread_link.rs:429` (`a` still linked) |
| `the_others_select_answers_like_a_press` | `chosen` returns the key | `tests_gateway/thread_ask.rs:173` (the ask was not answered with `t-two`) |

`a_chat_on_a_threads_own_session_is_never_rerouted` (new) proves existing behaviour, so it has no plant.
Note: the key test's plant fails on the watch's roots. The chat accepts any `work.changed` it is sent, so the daemon's
scope (the roots) is what brings the placed tasks; that is what the test pins.

## Running it

- `cargo nextest run --no-fail-fast -p theseus-protocol -p theseus-tui -p theseus-discord -p theseus-core` on the final
  code: **4,976 run, 4,976 passed, 26 skipped.**
- An earlier full run (before a clippy-driven refactor) had one failure,
  `theseus-core tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused` (agenda
  cap, `left: 1, right: 200`, under load). It is not on the flaky list and is in a file I did not touch; it passes alone
  and passed in the final full run. Named here, unexplained.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, after I split `route_to_thread` (it went to 102 lines
  once; `tell_placed` holds the send) and moved the TUI's thread events into `thread_event` and `submit_ended`.
- Keel guard: `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` printed nothing, exit 0.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run on the same content as the commits, before them):
door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule **passed**.
The suite phase **failed: 7,579 run, 7,545 passed, 34 failed**, all the known root-VM L1 cases
(theseus-pv6i): 33 in `theseus-sandbox::contract` and `theseusd::sandbox`, and `theseus-sandbox::bench spawn_100`. No
other test failed. I ran the phases after the suite by hand where they could run: the reader rule ran inside the gate
and passed; `protocol_types` holds (no diff in `cockpit/src/protocol.gen` once committed). Not run: the lifecycle, jobs and
turn benches (`THESEUS_GATE_NO_BENCH=1`, and the budgets are the owner's machine's) and the size step.

## Live check for the maintainer

On a scratch daemon with `[threads] enabled` and two threads, in the owner's own shell:
1. `theseus chat`, then send a message naming a thread (`#security-fix rotate the key`): the chat should print
   `→ thread security fix` and `go there: theseus chat <session>` **before** the turn ends, once, and a task that turn
   starts should show in the footer and in ctrl-t. Then `theseus chat <that session>`: no `→ thread` line and no
   re-routing.
2. `theseus thread link a b`, `theseus thread merge b c`, `theseus thread unlink a c`, then `theseus thread show a`
   (no links) and `thread show c` (no `a`).
3. In a shared place, a reach for the owner's personal details is held: a TUI chat on that session prints
   `held: no model ran`. (The held reply itself is on the tail lane, not this base.)

## Left, and uncertain

- After a reconnect the chat re-watches only its own session (`opened` uses `watch_work(sid)`), so a placed session's
  tasks drop off the footer until the next placement. The bound is 8 sessions in memory; none is persisted.
- `ConfirmRequested` and `TaskChanged` carry no turn id, so any question or task change of a placed session's later turns
  (from Discord, say) reaches the chat while the session is in `also`. The brief asked for session keying there.
- Only the submitting connection hears `thread.placed`; a second client watching the chat's session does not.
- `ts.rs` change: the type was added to the existing line, not an `include_str!` (see above).

## Docs a sibling should write

- core `AGENTS.md`, the thread router paragraph: "A message the router places in a thread is told to the submitting
  connection when it is placed (`thread.placed`: the chat's session, the thread's, the turn's id, the tag; no row, no
  frame; `rpc/thread_flow.rs::tell_placed`). `thread unlink a c` after c absorbed b unlinks the pair the link sits on
  (`thread_link.rs::merged_pair`)."
- `theseus-tui` `AGENTS.md`, the chat-thread bullet: "`thread.placed` prints `→ thread <title>` at placement; the placed
  turn and its session are the chat's (`Chat.placed`: the last 8 sessions; a second `work.watch` replaces the first's
  roots, so the chat sends it again with them); a `held` turn's line says `held: no model ran`."
- `theseus-protocol` `AGENTS.md`: `thread_placed.rs` (theseus-7kjb7), the `thread.placed` notification.
