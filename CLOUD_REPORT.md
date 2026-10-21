# CLOUD_REPORT: tail-hold (branch `cloud/20261021-tail-hold`)

Store format: unchanged (48). No new dependency. No ceiling raised: `theseus/src/render.rs` is 3,122 / 3,128 and
`theseus-discord/src/render.rs` 3,001 / 3,009 (rustfmt wrapped my footer test and one `matches!`).

## Per Beads id

### theseus-vymxh: no argv file after a failed spawn (commits 6d99d3a, format fix after it)
- Found: `spawn_detached` writes `<spool>/argv/<id>` before it spawns; only `remove_pid` removed it.
- Changed: `Spool::remove_argv` (best effort); `remove_pid` calls it; `spawn_detached` calls it with `inspect_err` when
  `children::spawn` fails, keeping the same error and context.
- Proof: `job::tests::a_failed_spawn_leaves_no_argv_file` (a missing wrapper binary: `Err`, no file; then `sh`
  standing in for a running wrapper: the file is there while it runs). Planted revert (the `inspect_err` line
  deleted): FAIL `assertion failed: !spool.argv_path("act_nospawn").exists()`; restored and touched.
  `cargo nextest run -p theseus-kernel`: 250 passed, 3 skipped.
- Note: the success case uses `sh -c 'sleep 5'` as the stand-in wrapper and kills it by pid; it is not a real
  wrapper (the existing `tests/tree.rs` and theseusd job tests cover real ones).

### theseus-2rvwv: a held turn says so (commit 8769f4f)
- Changed: `personal_step::STOP_REASON = "held"`; `hold_reply` sets `t.stop_reason` and the node's `stop_reason`
  (`model` and `provider` stay). Discord footer: `held: no model ran` in place of the model, and no
  `stopped: held` bit. CLI status line: `[profile → held, no model ran · …]`. No `turn.rs` change.
- Tests: `tests_personal_hold::a_held_reply_names_no_model`, `render::tests::a_held_turns_footer_names_no_model`
  (Discord; the `end_turn`-class result's footer is checked equal to today's start), `render::held_tests::a_held_turns_line_says_no_model_ran`
  (CLI; new file `crates/theseus/src/render/held_tests.rs`, one `mod` line, since render.rs had no room).
- Planted reverts, each FAIL: `t.stop_reason` unset (core test: `stop_reason: "end_turn"` in the result);
  Discord `usize::from(true)`; CLI `if false`.
- Readers of a turn's stop reason, checked (`git grep stop_reason`): `narrative::end_phrase` printed
  `the stop reason is held` for it: FIXED with an arm and a test. `theseus/src/outcome.rs` (exit code: unknown reason
  is `Done`), `voice.rs`, `mcp.rs` (match only `awaiting_confirm`/`budget`/`stopped`), `judge/mod.rs` and the nudge and
  end-note steps (act only on `no_tool_calls`; a held turn was `end_turn` before and is skipped as before),
  `noop_step` (its list is `nothing_new`/`awaiting_confirm`), the cockpit (prints the string; the OK pill is `end_turn`
  only: a held node shows a neutral pill, fine) all treat it as a finished turn. One reader needed a change, so the
  limit of three was not reached.
- Not changed, for the owner to hear: the TUI chat's turn line (`theseus-tui/src/chat/events.rs`) still prints the
  model's name beside a `held` word; it was not in the row's files.

### theseus-ztkhg.1: ages on three surfaces (commit 916a9c2)
- TUI Job row (`Form::Job` gains `said_at_ms`, `doing_ms`); `theseus status` (`of_views(views, now_ms)`, composes
  `step_words` and `aged_said`; `lines()` passes `herdr::now_ms()`; `Progress::line()` untouched); Discord task view
  (`detail` and `post_card` take `now_ms`).
- **Deviation to see:** the brief says the Discord runtime is out of scope but also to thread `now_ms` through the call
  site. The two callers are in `runtime/live_board.rs` and `runtime/forum.rs`; each gained one argument,
  `self.shared.core.kernel.now_ms()`. Nothing else in `runtime*` changed.
- Tests (2 min old words show `(2 min ago)`, 30 s old do not): `work::tests::a_job_rows_old_words_carry_their_age`
  (TUI), `status_progress::tests::old_words_carry_their_age`, `render::task_view::tests::old_last_words_carry_their_age`.
  Planted plain words on each surface: all three FAIL.
- Out of this row (as briefed): the issue's third note (`model 1:48` while a `proc.run` ran, not reproduced, likely a
  stale view) is the maintainer's live walkthrough.

### theseus-c93va: scorer fallback and resume's one read (commit 9ae36a6)
- `abstained_as` falls back to the asked subject's own words when none is distinctive; docstring updated. New test
  `test_a_subject_of_shared_words_alone_is_named_by_them`. Plant (fallback dropped): FAIL
  `'neighbours' != 'misleading'`. `python3 -m unittest test_score` (in `bench/recall`; pytest is not installed here): 20 OK.
- `resume.rs`: the pid is read once and used for `alive` and `meta.pid`. No behaviour change beyond the race; no
  plant. Covered by theseus-core's suite (3,798 passed).

### theseus-jjnud (commit 9aacc61)
- `interactive.rs`: kept the branch and made it say a re-told question prints nothing. Pure comment; no plant. The
  theseus crate's suite passes (1,661 tests across theseus, theseus-tui, theseus-discord).

## Runs
- `cargo nextest run -p theseus-core`: 3,798 passed, 25 skipped. `-p theseus-discord -p theseus -p theseus-tui`:
  1,661 passed. `cargo clippy --workspace --all-targets -- -D warnings`: clean. Cockpit protocol types: no change.
- `cargo deny check`: advisories, bans, licenses, sources ok.
- Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0. Note
  this clone's `origin/main` is `1d092de`, not the brief's base `6918e3c`.

## The gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`)
Door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule pass. The suite phase
failed on the known L1 tests only (this VM runs as root: theseus-pv6i): 12 `theseus-sandbox::contract` tests,
`theseus-sandbox::bench spawn_100`, and 13 `theseusd::sandbox` tests (names in the log), plus one load flake:
`theseus-core rpc::tests_ordered::a_turns_root_carries_its_wait_in_the_lane`, which is not on the flaky list and
passes alone (0.4 s) once the load was gone. I ran the phases after the suite by hand: protocol types (clean), the
supply-chain check (ok). The lifecycle, jobs, turn and size benches are the owner's machine's and were skipped by
`THESEUS_GATE_NO_BENCH`.
- Process note: my first gate run stopped in fmt (a test I had committed unformatted; fixed in the next commit) and the
  second ran out of disk in the test build; after clearing `target/debug/incremental` and stale test binaries the
  third reached the suite. I committed steps 2 to 5 after their crate tests and clippy and the whole gate ran after,
  not before each commit.

## Live check for the maintainer
- `cargo nextest run -p theseus-kernel -E 'test(a_failed_spawn_leaves_no_argv_file)'`: one pass.
- On a scratch daemon in a shared Discord place, send a message that reaches for the owner's personal details: the reply
  footer reads `… · held: no model ran · 1 loop · …`, and `theseus watch`'s line reads `[… → held, no model ran · …`.
- `theseus tasks`/`status` with a task whose last words are over a minute older than its current work: `status` shows
  `step N of M · words (2 min ago)`; the TUI Job row and the Discord Details view do the same.

## Left, or worth hearing
- The TUI chat's turn line still names a model for a held turn (see above).
- Docs to change at review: `crates/theseus-core/AGENTS.md`'s privacy-warning entry says the stop reason stays
  `end_turn`; it is now `held`. The spec's Part III item for theseus-i1z13 likewise.
