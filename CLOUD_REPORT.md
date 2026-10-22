# CLOUD_REPORT: tail3-tests

Branch `cloud/20261021-tail3-tests`, base `29f5dcad`. Store format unchanged (48). `Cargo.lock`: `git diff --stat Cargo.lock` is empty. No new dependency. No line ceiling touched. Commits are unsigned (no key on the VM).

## Per Beads id

### theseus-0wu8t (eec1cdbd): waits name what they wait for
- `test_support::until_event(what, f)`: prints `waiting for {what} ({n} s)` with `eprintln!` every 30 s of wall time; no deadline (doc keeps the reason and says the line names a hang in nextest's captured output). Callers: `tests_task_checkpoint.rs` (3), `until_not_deciding` (now built on `until_event`), `tests_task_brief::run` (waits on `until_event("the task's first turn")`, no 20 s clock).
- `crates/theseus/tests/herdr.rs`: the `turn.submit` hold is a local `recv_timeout(30 s)` loop that prints the same line; a disconnected channel ends it as `recv()` did.
- Captured line from a deliberately hung local run (a temporary test, removed): `waiting for a lost event (30 s)`.

### theseus-2ix1k (75120208): the effort lapse on a virtual clock
- Seam: `Core::build_with_clock(parts, clock)` (`#[cfg(test)]`) stores the clock in a thread-local that `Core::build` takes once (`clock_for_build`), else `RealClock`. `Parts` and `Core::build`'s signature are unchanged.
- Why a thread-local, not `build` calling `build_on`: moving `build`'s two `#[expect(clippy::…)]` attributes onto a new function reads as `allow-added` to the keel guard (first gate run failed there), which a cloud row cannot ack.
- `tests_route.rs`: `rig_on_clock` (rig with the kernel on a given clock) and `try_turn_attached` (the turn's failure returned; `turn_attached` unwraps it). `tests_turn_effort.rs`: `effort` and `says` are `pub(crate)`.
- New `tests_effort_lapse.rs`, two tests: `a_refusal_mark_holds_an_hour_by_the_kernels_clock` (400 naming `output_config` → one `provider.effort_refused` row, retried once without the message; at mark+1 h−1 ms no `output_config` message; at mark+1 h it carries one again) and `a_400_that_does_not_name_output_config_marks_nothing` (finding 2: no retry, no row, next request still carries it).

### theseus-40cuo (60d9bb9f): the link with no web UI
- `CockpitConfig::turn_url` removed (its only other caller was `rpc/inspect.rs`; `turn.rs` already used `turn_link`). `TurnInspectResult.url` = `turn_link(..).unwrap_or_default()` (empty = no web UI, doc only in `theseus-protocol/src/inspect.rs`; `cockpit/src/protocol.gen/TurnInspectResult.ts` regenerated, doc comment only).
- `theseus open turn` with an empty url prints one line (set `[web] enabled = true`, or `[cockpit] public_url`) and opens nothing; `--json` prints `"url": ""`.
- `config/cockpit.rs` tests moved from `turn_url` to `turn_link`, plus `[web] enabled = false, port = 17433` → `None`. New `rpc::tests_inspect::no_web_ui_means_no_link`.

### theseus-ta1be (e756257b)
1. `fact::route::tests::a_checkpoints_move_names_what_moves`: turn checkpoint says "the conversation moves", task checkpoint "the task moves".
2. `psl = "2"` in root `[workspace.dependencies]`; core says `psl.workspace = true`.
3. `watch/mail_auth.rs` module doc: the list is the one compiled into the `psl` release `Cargo.lock` names; a later suffix needs a crate bump.
4. **No new test: `tests_wait_beat::a_held_job_posts_nothing` already is this test** (job past its file's head, `meta` has no `beat`, `statuses(r)` empty). Verified by plant (below).

### theseus-y4ajh item 1 (ab55e122)
`scripts/AGENTS.md`, under the gate's commit recipe: `Keel:` lines go in the last paragraph beside `Co-Authored-By`. Items 2 and 3 not touched.

## Planted reverts (each restored from a backup copy, then `touch`ed; `git status` checked)
| Plant | Test that failed | Failing line |
|---|---|---|
| `all` ignores the hour (`now_ms < at + HOLD` → `true`) | `tests_effort_lapse::a_refusal_mark_holds_an_hour_by_the_kernels_clock` | `tests_effort_lapse.rs:78: an hour on, the messages are tried again` |
| `refused_by` drops `error.contains("output_config")` | `a_400_that_does_not_name_output_config_marks_nothing` | `tests_effort_lapse.rs:96: the 400 ends the turn` |
| `inspect.rs` falls back to `http://localhost:7433` | `rpc::tests_inspect::no_web_ui_means_no_link` | left: `"http://localhost:7433/session/ses_…/turn/turn_inspected"`, right `""` |
| narration says "turn" | `fact::route::tests::a_checkpoints_move_names_what_moves` | left `…the turn moves from sonnet to fable.` vs right `…the conversation moves…` |
| beat posts before `clock.beat` returns (post on `None` too) | `tests_wait_beat::a_held_job_posts_nothing` | `tests_wait_beat.rs:323`, left `["Job j1 (true) is still running: 10 min 1 s so far, no output since it started."]` |

## Proof
- `cargo nextest run -p theseus-core tests_task_brief tests_task_checkpoint tests_turn_effort tests_effort_lapse tests_wait_beat config::cockpit fact::route judge::people rpc::tests_inspect`: 62 passed. `cargo nextest run -p theseus herdr`: 76 passed.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean.
- Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0). (`origin/main` was stale in the clone until fetched; the gate's keel phase then passed.)
- The full `-p theseus-core` / `-p theseus` suites ran inside the gate's suite phase (below), not separately.

## Gate
`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build, reader rule passed. Suite phase: 7,636 tests, 74 failures, all explained:
- Known L1/root: `theseus-sandbox::contract` (all clauses), `theseus-sandbox::bench spawn_100`, `theseusd::sandbox` (all).
- Timing under the build's load, each **passes when rerun alone**: `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`, `theseus::markdown rendering_costs_little_per_hundred_lines`, `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`.
- Not run as gate phases: protocol types (checked by hand: `cockpit/src/protocol.gen` is committed, no diff), benches (`THESEUS_GATE_NO_BENCH`), and `deny`: `cargo deny --offline … check bans licenses sources` failed here because the advisory/index data is not fetched on this VM (`attempting to make an HTTP request, but --offline was specified`). No dependency was added and `Cargo.lock` is unchanged, so nothing for it to find, but the maintainer should run `scripts/gate.sh`'s deny phase.
- The VM's disk allowance ran out three times during the gate; I deleted `target/debug/incremental` and large test executables (inside the repo only) and set `CARGO_INCREMENTAL=0`.

## For the maintainer
- Live check: none needed for this row (test helpers, a link fallback, a manifest line, docs). To see the link change: start a scratch daemon with `[web] enabled = false`, `port = 17433`, run a turn, then `theseus open turn --print` should print the "no web UI runs" line, not a 7433 URL.
- Deny: `cargo deny check` (with its fetch) before the join.

## Not done / choices for the owner
- AGENTS.md sentences for tail3-google's docs (not edited by me): `crates/theseus-core/AGENTS.md` could say that `Core::build_with_clock` (test-only) gives a test's kernel a virtual clock, and that `test_support::until_event(what, f)` prints a `waiting for …` line every 30 s. The `turn_url` mention in the `turn.inspect` paragraph ("left out, `http://localhost:<[web] port>`") should add "and no link at all when the web UI is off" (`TurnInspectResult.url` is empty).
- `theseus-y4ajh` items 2-3 (keel guard files) and `theseus-k7lzu`, `theseus-94782` are left, as the brief says.
