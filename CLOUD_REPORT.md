# CLOUD_REPORT: bench-measure (theseus-53ujg.4)

Branch `cloud/20261019-bench-measure`, from `bbc65530` (store format unchanged: bench code only, no product code).

## Steps

### 1. The routed profile mirrors the live table (3de46889)
- Found: the routed profile named no effort for sonnet, opus and fable; no `opushi`; stale "route.v4" comments.
- Changed: `bench/theseus-bench-routed.toml` (sonnet, opus `medium`; fable `high`; new `[profiles.opushi]`; the comments fixed). `ROUTED_PROFILES` has `opushi`. `bench/README.md` says seven profiles. `bench_profile_routed.rs` asserts Medium, Medium, High and `opushi` High. The `resolved_profiles` golden was rewritten (`THESEUS_GOLDEN=write cargo test -p theseus-core tests_inherit`): only the sonnet, opus and fable effort lines moved, and the `opushi` block was added. `review` class and bounds were not spelled out in the file (no test needs them).
- Also: `test_bench.py`'s routed tests were already stale on main (`cfg["routing"]["mode"]`/`["modes"]` are now `rung`/`classes`; they failed before my change). Fixed them while updating them for the efforts and `opushi`.
- Proof: `cargo test -p theseus-core tests_inherit` 6 passed; `cargo test -p theseusd --test bench_profile_routed --test route_owner --test bench_routed_live --test bench_profile` 7 + 2 + 4 + 1 passed. Plant: fable's `effort = "high"` removed from the toml, so `bench_profile_routed` failed (`fable: left None, right Some(High)`) and `tests_inherit::the_owner_shaped_config_resolves_to_the_same_profiles` failed; restored, `touch`ed, `cmp` identical.

### 2. A plan can cap the effort (3de46889)
- Changed: `plan.py` (`EFFORTS`, `effort_bounds()`, `check` validates the key: a list of two known levels, low not above high; `commands` adds `--ak effort_bounds=low,high`), `theseus_bench.settings(effort_bounds=)` writes `("routing", "effort_bounds")`, `theseus_agent.Theseus.__init__(effort_bounds=)`, README line. `run.py` needed no change (it emits `plan.commands`' lines). No tier plan created.
- Proof: `test_plan.py::EffortBound` (3 tests: `a_plans_effort_bound_reaches_the_daemon_config`, a plan without one leaves the profile's, bad bounds refused). Plant: `settings` ignores the key, so the key test and the bad-bound test failed; restored.
- Not run here: the agent-level path through Harbor's `--ak` (Harbor is not installed; the agent tests skip). Maintainer: `python3 bench/harbor/run.py --plan <a plan with effort_bounds> --dry-run -- harbor run -a theseus_agent:TheseusRouted ...` should print `--ak effort_bounds=low,high` on each line.

### 3. The recall driver reads every history page (9591f92e)
- Found: `history --full` is the newest 200 nodes; the driver used only that page.
- Changed: `drive.history_pages(cli, session)` pages back from `nodes[0].position`, then each page's `older`, until none or no nodes, and returns one object with every node (oldest first, no `older`); the run writes one `raw/history-{i+1}.json` per session.
- Proof: `test_drive.py::HistoryPages::test_recall_delivery_reads_every_history_page` (fake CLI honouring `--before`, pages of 200, 200, 37 nodes, the marker only on the oldest page, counted as delivered), plus a one-short-page test. Plant: the paging loop disabled (`while False:`): both failed; restored.

### 4 and 5. The kill family measures Pi; the restarts find their binaries (440b4b49)
- Changed (`async_agents.py`, `driver.py`): `_after_run` records the killer's result on every exit, 0 included (Claude Code, Pi and OpenCode share it: the same pattern was in all three), and raises a non-zero exit that is not the kill's. `pi_stdin`'s run has `set -o pipefail`. The Pi kill finds it by `--comm pi --root` (`pi_arm` takes `lib` as `theseus_arm` does). `Arm` gained `locate` and `restart_at`: `fire_kill` resolves the CLI once with `bash -lc '<prefix> command -v <cli>'` and restarts with `PATH=<dir>:"$PATH" <absolute path>` for Pi and OpenCode (an empty answer keeps the bare name).
- Proof: `test_kill.py::Trigger::test_a_pi_kill_trial_records_its_kill` (a copy of `sleep` named `pi`, so its command line is just a `pi` path; the restart names the absolute path) and `Restarts::test_the_restarts_name_absolute_binaries`; `test_driver.py` checks `set -o pipefail;`. Plants: the old `--cmd '\bpi\b.*--mode rpc'` pattern failed the Pi kill test; `Arm.at` returning self (the bare name) failed both the kill test and the absolute-binaries test. Restored and `touch`ed.
- Not run: `test_a_clean_exit_still_records_the_kill` (exercises `_after_run`; skipped without Harbor). Maintainer: run `python3 -m pytest bench/async/test_kill.py -k clean_exit` under Harbor's python. I did not plant the pipefail line.

## Test counts (per directory: the three share test basenames, so a single pytest run collides)
- `bench/harbor`: 143 passed, 44 skipped. `bench/async`: 112 passed, 19 skipped. `bench/recall`: 90 passed, 3 failed:
  - `test_generate.py::Budget::test_the_prices_are_the_catalogs` (`0.1` vs `0.2` price): fails on the base too, before my changes.
  - `test_drive.py::TheseusDriver::test_a_run_plans_at_the_overhead_it_measures_and_records_it` and `::test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing` (end to end on a scratch daemon; compaction timing and "left running"): fail identically with the base's `drive.py` swapped back in, so they are not from this branch. They need the owner's attention.

## Keel guard
`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty output, exit 0.

## The gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: the door, keel (`0 findings`) and shape (`no file over 2500 lines but the 8 listed`) phases passed; `cargo fmt --check` passed. The test build then failed with `No space left on device`: this VM's disk allowance (about 40 GB) held the debug build (18 GB) and could not link the theseusd test binaries (two tries, after deleting test executables and incremental caches). So the suite and the phases after it (`machine_checks`) did not run. The touched Rust tests were run by themselves (above). No product code changed, so the maintainer's full gate is the check. I deleted build artefacts under `target/` only.

## What is left or uncertain
- Docs to change (not edited): `docs/design/` or the spec's bench item should say the routed profile now follows the owner's table (efforts, `opushi`) and that a plan may carry `effort_bounds`.
- Design choice for the owner: `_after_run` also applies to the Claude Code and OpenCode arms (same defect, same fix); a kill pending when the harness exits 0 costs up to a minute's wait and is recorded as "not measurable", as the non-zero path already did.
- The Pi kill by `--comm pi` also matches any process named `pi` that is a root of its own tree; the arm's container runs only Pi.
