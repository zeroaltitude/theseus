# CLOUD REPORT: thread-route (theseus-rha2, row T4)

Branch `cloud/20261013-thread-route`. Work commits (all unsigned, trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5)`):

| Commit | Step |
|---|---|
| `2e08676` | judge: `classify.v2` with the `thread` question, `Source::Threads`, `Action::ThreadRoute`, the builder, goldens |
| `90c8479` | protocol, config and core: pre-router, addressing, decision table, ask, wait, misroute guard, recovery, facts |
| `6998762` | Discord ask card and `/move`; CLI `[1] [2] [n]ew [h]ome`, `thread answer`, `thread move` |
| `c10d5c2` | `scripts/thread-route-check.py` (live check) |

The gate ran once over the whole tree (not once per commit): the pieces depend on each other (the core's tests name `classify.v2`, the Discord
test counts commands), so the split is by path, not by independently green step. Say so if you want the history squashed.

## What was found
- Threads (T1) give a board, a virtual place per thread and a session per thread, so routing means choosing a session before `turn.submit` runs.
- The inbound point (`JudgeService::at_inbound`) runs inside the turn, after admission. To add no call, `inbound_early` begins it from
  `route_to_thread` (before the turn), keyed by session with a pre-minted turn id; the turn takes that `Early` instead of asking again.
- A thread session has no outbox target, so its replies need an overlay (`Outbox::show_at`) to reach the Discord place the message came from.

## What was changed
1. **Addressing** (certain, before any judge): `#slug`, `/t slug`, "in the <title> thread:", and a reply to a routed turn (`in_reply_to`).
2. **Pre-router** (`thread_route.rs`, pure, in memory): entity hits against each thread's working set, stickiness (`[threads] sticky_half_life_secs` = 600), reply chain; fused as noisy-or.
3. **`classify.v2`**: v1's questions word for word plus `thread` (`options_from threads`, at most 50 + `new_thread` + `home`), in the same inbound request.
4. **Decision table** (`thread_route::decide`): >= 0.90 quiet tag; 0.60 to 0.90 visible tag with a move handle; below that one ask line; new-thread offer on one tap; a one-off runs at home; only two or more live threads wait. Discord buttons in `runtime/thread_ask.rs`; CLI `thread/ask.rs`.
5. **`[threads] route_wait_ms` = 400**: a timeout keeps the pre-router's pick as a `guess`; `thread.routed` counts it (`timed_out`).
6. **Misroute guard** (`toolrun/misroute.rs`): the first write or `proc.run` outside the thread's roots is held through Approve, only when the placement was below the act bar.
7. **Recovery** (`rpc/thread_move.rs`): "that was for ...", `/move <slug>` (Discord), `theseus thread move`: one redirect note in the wrong thread, one re-submit, one correction (a `wrong thread` label on the `thread` question and `Steer::Thread` in the correction layer, so the next close message follows).
- New keys (all with defaults and in the template): `[threads] sticky_half_life_secs`, `route_wait_ms`, `ask_wait_secs`. No store change.

## What was proven
- `cargo nextest run --workspace -E 'test(thread_ask) | test(tests_thread_route)'`: 15 of 15. Judge crate: 143 tests; core router unit tests (`thread_route::tests`): 14.
- Every band, addressing order, pre-router cases, ask card on Discord's fake (`tests_gateway/thread_ask.rs`) and in the CLI, timeout fallback counted, guard only below act, `/move` writes one note and re-submits once and the next close message follows, a stranger's answer does not count.
- **Scripted 40-message check** (`forty_unaddressed_messages_find_their_thread_and_ask_rarely`, fixture `crates/theseus-core/fixtures/thread_route_40.json`): top-1 >= 90% and asks <= 10% of the clear messages, asserted; `guesses == 0`.
- **Planted reverts**, each restored, `touch`ed and checked with `git status`:
  - guard skipped (`misroute_hold` call in `toolrun.rs:1173` replaced by `d`): `a_write_outside_the_threads_roots_is_held_only_below_the_act_bar` FAILED, 11 others passed.
  - a second inbound call (hand-off's `early` forced to `None` so the turn asks again): `jevs_thread_answer_rides_the_one_inbound_request_and_the_bands_route` FAILED (the `jev.seen().len() == 1` assertion, after a 20 s wait).
- Frame budget: `tests_m3::a_plain_turn_stays_within_its_frame_budget` passed in the suite (a plain turn is still 5 frames incl. admission as before); routing starts no timer (the wait is a `tokio::time::timeout` on the existing inbound oneshot, only with two or more live threads).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `scripts/shape.sh`: ok. `python3 scripts/keel-guard.py`: 0 findings (I first had 3 `allow`/`expect` additions and removed them by splitting functions). Keel findings expected: none.

## Gate result
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: **FAILED in suite**, 4426 passed, 34 failed, all known or proven pre-existing:
- 32 L1/sandbox tests (`theseus-sandbox::contract` and `theseusd::sandbox`, root VM: "L1 is unavailable: the daemon runs as root"), plus `theseus-sandbox::bench spawn_100` (same cause).
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: fails identically on the branch's base commit built in a clean worktree (checked), a timing test on this VM.
- Phases before the suite all passed (door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule). The phases after the suite (generated TS check, `cargo deny`) did not run because the gate stops at the suite: the TS test (`the_cockpits_types_are_generated_from_the_rust_ones`) passed inside the suite and `cockpit/src/protocol.gen/` is committed; `cargo deny --offline` cannot run here (no advisory database offline). A fault found on the way and fixed: `join_names_a_voice_channel_and_leave_nothing` counted the commands (`/move` makes 17); updated.
- The disk allowance forced me to delete build artifacts twice; `CARGO_INCREMENTAL=0` after that.

## Live check
- Stand-in (run here): a scratch daemon of this build (`theseusd --config cfg.toml --socket /tmp/scr/sock --state-dir /tmp/scr/state`) with `theseus-sim fake-model` and `theseus-sim fake-jev`, judge on, Discord/web/index off; then `python3 scripts/thread-route-check.py --theseus target/debug/theseus --socket /tmp/scr/sock`. The path works end to end (threads made, 40 messages sent with `theseus ask --json --session`, two asks answered with `thread answer`, the script reports). Result **top-1 26/40 = 65%, asks 2 of 38 clear**: expected, because the fake Jev has no way to script a *choice* answer (`--say` scripts Noul answers only), so everything not decided by entities relies on stickiness. It is not a measure of the real router; the scripted core test (which scripts Jev's choice) is.
- For the maintainer, on a scratch daemon with the real key and a real model, `[discord]` off: `scripts/thread-route-check.py --socket <scratch sock>` (exit 0 when top-1 >= 90% and asks <= 10% of the clear messages; costs about forty short turns). In #theseus-test: make two threads with different refs (`/thread new`), send an unaddressed message that names one's PR, one that is ambiguous (press a button on the card), then `/move thread:<slug>`.

## Left open (limits)
- Discord: a thread session's *live* stream events (thinking, tool lines) do not reach the place actor, only the durable post does; the place actor serialises turns per place.
- Pending asks, router memory (stickiness, routed turns) and thread corrections are in memory and are lost at a restart (no store change, by the brief).
- The early judgment's rows carry the home session and the pre-minted turn id.
- The reaction/button pick (a ↪ on a reply) is not built; `/move` and "that was for ..." are. The TUI does not show asks.
- Under the private-class gate, a non-owner in a private guild channel could be routed.
- The scripted-Jev accuracy has not been measured against a real Jev.

## Docs that should change (I touched none of these)
- Spec: a Part III item for T4 (what is above), the version line in `docs/spec/01-front.md`, §3 (threads) for the router and the new keys, P0 (rule 3) for `thread.asked`, `thread.answer`, `thread.move` and the `thread.routed` / `thread.asked` / `thread.answered` / `thread.moved` rows.
- `docs/status.md`: Updated line, recently landed, the roadmap's T4 row. `docs/design/roadmap-v2.md`, `README.md` of design if it lists the pack versions (`classify.v2` supersedes v1 in WIRED).
- Store format unchanged; no `tests_layouts` change.
