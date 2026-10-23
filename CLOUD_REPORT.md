# CLOUD_REPORT: tail7-recall (theseus-5ni7j, theseus-b80si)

Branch `cloud/20261021-tail7-recall`, based on `ae420a49` (the task commit is `ae211aa9`). Store format: 49, untouched (nothing here is stored). No new dependency. No line ceiling raised; no file grew past one.

Commits (signed; the VM had a key):
- `e1151227` README numbers (5ni7j 1)
- `834847c1` drive test (5ni7j 2)
- `f8e6ce7c` read-only scratch test (5ni7j 3)
- `b3e1b1a9` keep-warm telemetry on the deferred path (b80si 4)
- `d3c3c41d` data-gate word from the turn's decision (b80si 2)

## 1. The README (5ni7j 1)
Commands, run from the repository root:
`python3 bench/recall/generate.py --seed 7 --size smoke --overhead 13943 --out <dir>` and the same with `--size full`.

Output, the lines the README quotes:
- smoke: `scratch context window 56000, a request budget of 37,904`; two logs of 8,894 and 8,885 tokens; the second read at the mark (turn 10), crossing at turn 11; `$0.57` an arm.
- full: `scratch context window 101000, a request budget of 80,904`; marks 120, 320, 520; `$11.98` an arm.

README lines changed (only these):
- "about $0.54 an arm for the smoke and about $12.02 for the full" is now "$0.57 ... $11.98".
- "46000 for the smoke (a budget of 30,404; two logs of about 4,650 tokens, the second at turn 11) and 94000 for the full (73,904)" is now "56000 ... (a budget of 37,904; two logs of about 8,890 tokens, the second at turn 11) and 101000 for the full (80,904)".

**Uncertain, for the owner:** `bench/recall/test_generate.py::Budget::test_the_prices_are_the_catalogs` fails on this base (generator's cache-read price 0.2 against the catalog's 0.1 for `claude-sonnet-5-5`). I did not touch the generator or the catalog (out of scope). The dollars the generator prints, and so the README's, use the generator's price. The cost change is the owner's to hear from the maintainer. That failure is the only red in `python3 -m unittest discover -s bench/recall -p "test_*.py"` (100 tests).

## 2. The drive test (5ni7j 2)
`test_the_smoke_compacts_once_however_early_the_index_comes_up` now waits (polling for `<run>/daemon/**/index/sock`, 30 s cap, the assertion names the cause) instead of `time.sleep(8)`.
- Wall time, this VM: before 11.6 s, after 5.8 s (`python3 -m unittest test_drive.TheseusDriver.<test>`).
- Plant: `generate.py` put back as `c666c38d^1` has it (the plan before tail6-tests). The test fails: `[11, 23] not found in ([10], [11])`, the second compaction at turn 23. `generate.py` restored and touched; `git status` clean for it.

## 3. The read-only test as root (5ni7j 3)
`remove` is now `remove_with(path, remove_dir)` (one parameter, a closure `|p| std::fs::remove_dir_all(p)`). New test `a_read_only_tree_is_thawed_before_its_second_removal` fails the first try with `PermissionDenied`, as a non-root runner's would, and in the second try asserts every directory has `0o700` bits. The old test stays.
- Plant: comment out `thaw(path)` in `remove_with`, running as root (`id -u` = 0). The new test fails: `".../store/sealed" is 40555 at the second try`. The old `a_read_only_tree_is_removed` still passes under the plant (that was the finding).

## 4. The real route (b80si 2)
The decision is made again, in `decide_floored` (`turn/route_step.rs`), as for a cleared ask, only when the turn was not admitted and made a decision (`RouteState.assume_cleared` makes `floor()` read admitted for that one call; the result is `RouteState.cleared_pick`). `data_gate_why` uses `cleared_pick` when set. It falls back to the old list walk (`cn_would_win`) only for a turn with no decision from `decide_floored`: an open task's brief, a turn with no verdict or steering, and a decision made at a checkpoint (`checkpoint_step`). The floor's behaviour is unchanged; `data_floor` is untouched. This covers detours, switches, holds, steered picks, unsure verdicts and the step-down.

Tests (`tests_data_footer.rs`):
- `a_detour_to_a_restricted_profile_kept_off_shows_the_data_gate`: a trivial (detour) turn, the cleared decision names `dsflash`; the row says `detour`, `data_gate: verdict_private`, the footer shows it, counted once.
- `an_unsure_verdict_that_stays_on_its_base_shows_no_data_gate` (the mirror): the mode lists the restricted profile first, but the verdict is below the mode's bar; no word, no counter.
- The existing `a_private_turn_a_us_model_wins_anyway_shows_no_data_gate` and the other data tests stay green.
- Plant: `data_gate_why` reads `None` for `cleared_pick` (the list walk only). The mirror test fails at `tests_data_footer.rs:147` (a gate word on a turn that stayed on its base). The detour test passes under the plant: the list walk was already right for it, so the key regression test is the mirror one. I found no rig where the old walk under-showed.

## 5. Keep-warm telemetry (b80si 4)
One line in `build_telemetry` (`rpc/mod.rs`): `let _ = self.runner.keep_warm.telemetry.set(t.clone());`. Test `telemetry::tests_daemon_path::keep_warm_exports_on_the_daemons_path`: a core built without a pipeline has an empty cell; after `install_telemetry` it is set. Plant: drop the line; the test fails at `assert!(core.runner.keep_warm.telemetry.get().is_some())`.

## Runs
- `cargo nextest run -p theseus-core data scratch telemetry`: 136 passed. The five new tests pass together.
- `cargo nextest run -p theseus-core` whole: one failure in the first run, `tests_lsp_edits::the_block_adds_no_frame`; it passes alone and passed in the gate's full suite (load).
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0).

## The gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` because the disk filled once at full debug info; the first run died on "No space left", the second ran to the suite). It passed door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule. The suite failed only on the known cases: theseus-sandbox's contract tests and `spawn_100`, and theseusd's `sandbox` tests (root VM, theseus-pv6i), 69 lines in the log, all of them those; and `theseus-discord tests_forum_archive::a_clear_before_the_end_still_counts_as_read` at 101 s under load, which passes alone in 2.5 s. I then ran the phases after the suite by hand: protocol types (no diff in `cockpit/src/protocol.gen`), `cargo deny --offline check` (ok), the turn bench `--check --runs 5 --burst 0` (ok), size step (skipped: no release build). Lifecycle and jobs benches are skipped by `THESEUS_GATE_NO_BENCH`. Counted green.

## Live check for the maintainer
On a scratch daemon with a private session and a restricted-origin profile in the mode's list: send a message Jev judges private (`data=private`) to a `trivial` turn and check the footer says `data gate: verdict_private`; send a message the mode judges at low confidence (below the mode's switch bar) and check there is no gate word and `theseus.route.data_gate` does not move. With `[telemetry]` set and a headers secret, check `theseus.keep_warm.reads` is exported after the pipeline builds after serving.

## For the integrator
`docs/status.md`: "The footer's data-gate word and its counter now read the turn's own routing decision (detour, switch, hold, steer, unsure) made again as for a cleared ask, falling back to the mode's list only for a turn with no decision; keep-warm's telemetry is wired on the deferred path; the recall README quotes the 56000/101000 plan."

## Follow-ups
- The generator's cache-read price differs from the catalog's (above): the README's dollars follow the generator.
- 5ni7j 5 (the one leftover tmp entry): I did not see its cause.
- The cost of the second decision per unadmitted turn is one pure `routing::decide`; the turn bench passes (plain p95 32.1 ms overhead, within its check).
