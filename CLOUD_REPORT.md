# CLOUD_REPORT: thread-record (T1), theseus-nhew

Branch `cloud/20261013-thread-record`. Code commit `4e9c2d0`; this report is the last commit.

## What I found
- On this clone main's store format is **32**, not 27 as the brief says; I bumped to **33**.
- A place's outbox target is `discord:<place>`, so binding `thread:<slug>` through `bind_place_to` would have given a thread's session a Discord target and sent its notices to a courier that does not exist. I made a virtual place's session **target-less** (`outbox::virtual_place`), like a CLI session's. It is then private by the existing "no place" rule, and `places.rs` also classes `thread:*` / `discord:thread:*` private explicitly.
- The index tender (`theseus-index`) still reads `discord.session.thread:<slug>` as place `discord:thread:<slug>`. T3 (recall) must decide how recall treats that place; core reads it as private through `try_target` = none.

## What I changed (4e9c2d0)
- `theseus_protocol::threads` (new): `ThreadInfo`, `WorkingSet`, `Seal`, `ThreadState`, `ThreadRow`, the eight methods' params, `slug_ok`/`place_of`/`slug_of_place`. `lib.rs`: +2 lines (`pub mod threads;`, one method line). `turn.submit` gains `thread` (27 call sites got `thread: None`).
- `theseus_core::threads` (new): `Board` (immutable snapshot: `get`, `all`, `of_session`, `live(PlaceClass)`, `open`), `Threads` holder (`all`, `write` = one frame, `bind`). `Core::threads()` gives the Board; `Core::warm_threads()` after serving (theseusd). Records are META `thread:<slug>`; the session is the place's own record (`discord.session.thread:<slug>`), `last_active_ms` is the session's, so a turn writes nothing for its thread.
  - Seam deviation: `of_session`/`live` return references into an `Arc<Board>` snapshot, so `Core::threads()` returns `Arc<Board>`, not a bare `Board`.
- `rpc/threads.rs`: `thread.new|list|show|edit|pause|resume|done|reopen`, all `judge_act(Act::Thread)` (owner in a private place). `fact/thread.rs`: six facts, six `LedgerKind`s. `thread.done`: thread frame first, then the card as a harness note in the session (a frame of its own) and `retire_session`. `turn_submit`: `thread_session` opens/binds on first use and after a retire (an owner `/new` = retire; `bind_place_to` supersedes).
- CLI `crates/theseus/src/thread.rs` (`thread list|new|show|edit|pause|resume|done|reopen`, `ask --thread`); six writes in `client::OPERATORS`. Discord `runtime/thread.rs` (`/thread new|list`). `[threads] enabled, max_live` in `config/threads.rs` and the template. AGENTS.md of core/protocol/theseus/discord updated.
- Store format 33 (`store.rs`); `THREAD_AT_33` literal and a format-32-store test in `tests_layouts.rs`. Updated "this build's format" assertions 32→33 in `tests_layouts.rs`, `store.rs`, `theseusd/tests/versions.rs` (33→34 for the "newer store" case); `voice.rs` command count 15→16.
- No golden changes were needed (`core_output.txt`, `kernel_frames.txt` pass unchanged).

## How I proved it
- New tests (all pass): `rpc::tests_threads` (10: each edit one frame + row; no-op edit writes nothing; reuse of the session; paused/done refused; restart reload; shared place cannot address/list/show/edit/pause/bind and the class is private; `/new` supersedes and the thread follows; done retires + leaves card; `max_live`/enabled/slug/topic limits; board read in memory; two threads overlap on the paused clock (3 s model, both within 5.5 s); daily ceiling stops another thread's call), `threads::tests`, `config::threads::tests`, layouts (2), CLI (3), Discord (2), `a_jobs_process_cannot_write_threads`.
- **Planted reverts**: thread classed shared (`places.rs` branch off) → `a_shared_place_can_neither_address_list_nor_bind_a_thread` failed at the class assertion. Board re-read from the store (`Threads::held` returns None) → `a_turn_in_a_thread_reads_the_board_in_memory` failed. Both restored and touched.
- Gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: keel, fmt, shape, features, clippy, cockpit, test build, reader rule all pass. Suite failed only on: the known L1 sandbox/contract tests (root VM), and `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, which **fails 5 of 6 runs on the unmodified base too** (checked with `git stash`) so it is a pre-existing race on this VM, not this change. Also seen once under load, passing alone: `theseusd versions a_stdio_daemons_stop_waits_for_a_task_that_holds_the_core`. I ran the later phases myself: `cargo nextest -p theseus-protocol` (87 pass, TS regenerated and committed), `cargo deny check` ok, `python3 scripts/keel-guard.py` ok (0 findings).
- Environment note: this VM's disk allowance filled several times mid-gate (jobs then fail the 1 GB disk floor); I cleaned workspace artifacts and reran.
- FAST: `theseus-sim bench turn`: plain turns 4 frames, 2 syncs before first byte, all rows ok. `bench lifecycle --runs 10 --check` (debug build, 4-core VM): first run all ok; second run missed two p95 rows (cold start p95 89.9 ms, health p95 8.0 ms), single outliers; budgets are for the owner's machine.

## Live check (done here, on a scratch daemon with `fake-model`, 2.5 s per answer)
Three threads made, then `theseus ask --thread tides|moons|winds` sent at once: all three answered in ~2.54 s each (concurrent), in three distinct sessions; `theseus thread list` showed three rows with context 16.0k and $0.03 today. A paused thread refused with "`theseus thread resume` lets it take messages". The owner's machine should repeat with `theseus-sim fake-model --rules <rules with hold_ms>`, a config with `[model] api_base`, and `theseus thread list`.

## Left / uncertain
- Keel findings expected: none.
- Docs for the maintainer: spec Part III item, `docs/status.md`, version line; store format 33's line is in `store.rs`; renumber if another row bumps first.
- `thread.done`'s note + retire are separate frames after the thread's; a crash between leaves a done thread whose session is not yet retired (benign: the next message cannot go there; only `session.retire` is needed).
- Discord `/thread` is untested against the real gateway (unit tests only). `turn.submit {thread}` from Discord is refused (no `DiscordOrigin` on that param); T4 adds addressing.
- Last activity in the list is max(record, session); `thread.edit` does not bump it.
