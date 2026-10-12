# CLOUD_REPORT: kill-family (theseus-7ajc)

Branch `cloud/20261011-kill-family`. Commits: `f2b8d806` (the family), `b2fb4809` (find takes the library path; real-daemon test).

## Found
- `fire` mixed trigger waiting and delivery; the trigger wait is now `_await_trigger`, shared (trigger dicts unchanged, old tests pass).
- No plain family writes effect lines except health-restart (`restart`), fanout, cancel, contention. Kill variants need effects, so the tools call `asyncbench.effect()`, which writes only where `/var/lib/async/kill-family` exists (the kill image's marker). Plain ledgers and rewards are unchanged.
- A step that was running when the harness died has no `end`; `verify_steps` tolerates that, so the chain and the plain checks work unchanged with `kill`/`restart` lines.

## Changed
- `tools/asyncbench.py`: `kill-record`, `restart-record`, `procs` (container-side process finder), `effect`, `kill_columns`, `unfinished_alive`; `check` writes `verifier/kill.json`.
- `tools/layer2.py`: effects on migrate-db, convert-export, make-worktree, build, run-app, start-service; `KILL_BASES`, `base()`, `KILL_REPEATS`.
- `driver.py`: `KillInjection`, `kill_of`, `kill_time` (seeded), `fire_kill`, `wait_restarted`, `Arm` plus `theseus_arm`, `claude_arm`, `pi_arm`, `opencode_arm`. `injection_of` returns None for kind=kill.
- `families.py`/`sync.py`: six `kill-<family>` tasks in `bench/async/kill-tasks/` (apart from `tasks/` so existing run commands never pick them up): health-restart, migrations, csv-merge, bisect, pipeline, service; three mid-step, three seeded-random. Instruction text identical to plain (tested). Each oracle keeps a journal, is killed once (SIGKILL to its process group, kill recorded first), restarts and finishes.
- `async_agents.py`: kill wiring in all four arms; new `OpenCodeAsync`. **Not run: Harbor is not installed here** (only py_compile). It is the first thing the live check exercises.
- `score.py`: per-trial kill columns, `wilson`, `kill_rows`, "The kill family" table with 95% Wilson intervals. README documents it all.

## Proof
- `python3 -m unittest` in bench/async: 127 ran, OK (17 skipped: Harbor/E2E-gated). `test_kill.py`: 33 tests (+1 gated E2E).
- Oracles: all six kill families at scale 0.05: reward 1, 0 duplicates, 0 orphans, 0 lost work, resumed, TMPDIR left empty, chain verifies with kill line.
- Naive stand-in (plain oracle rerun after the kill, via `fire_kill`): health-restart gives duplicated_effects>=1, orphans>=1, lost_work>=1; csv-merge duplicates>=1.
- Planted reverts (column forced to zero, then restored, `sync.py` rerun, files touched):
  - duplicates: 5 failures (Columns x2, NaiveStandIn x2, Scoring)
  - orphans: 4 (Columns x2, NaiveStandIn, Scoring)
  - time to resume: 9 (Columns x2, all 6 oracles, Scoring)
  - lost work: 4 (Columns x2, NaiveStandIn, Scoring)
  - completed: 1 (Scoring)
- Real daemon on the stand-in model (`ASYNC_E2E_BIN=target/debug python3 -m unittest test_kill.TheseusEndToEnd` from bench/async): `theseusd` and its CLI SIGKILLed 1 s into migration 001, restarted, trial settled, reward 1; columns: duplicated 0, lost 0, orphans 0, resumed true, time to resume 2.37 s. The job outlived the kill (end after the restart, adopted).
- Keel: `python3 scripts/keel-guard.py` ok, 0 findings. Keel findings expected: none.

## Live check for the maintainer
```
export THESEUS_BENCH_BIN_DIR=$PWD/bench/bin-after PYTHONPATH=$PWD/bench/harbor:$PWD/bench/async HARBOR_TELEMETRY=0
.venv/bin/harbor run -p bench/async/kill-tasks -a oracle -o jobs --job-name kill-oracle
.venv/bin/harbor run -p bench/async/kill-tasks -i kill-health-restart -a async_agents:ClaudeCodeAsync -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 -o jobs --job-name kill-smoke -k 1
python3 bench/async/score.py jobs/kill-smoke --out /tmp/async-kill-smoke
```
Expect oracle reward 1 on all six; in the smoke trial `agent/async-driver.json` has `kill` (pids, `restart_exit` 0), the ledger a `kill` then `restart` line then a start by a new process, and `verifier/kill.json` the columns. Repeat the one-trial smoke per arm (Theseus, Pi `PiAsync`, `OpenCodeAsync`). The full run (6 arms x 6 families x `-k 2` = 72 trials) is in bench/async/README.md "The kill run": expected about $25-45, at most $144 at the $2 cap.

## Uncertain
- The continue flags (`claude --continue --print`, `pi --continue`, `opencode run --continue`) come from the CLIs' docs; only Claude's `-c` was checked against the installed `claude --help`. Whether each keeps its model/effort/permission flags across continue is for the live check. The shared resume prompt is "Continue."
- Theseus restart relies on the daemon's own resume; socket file left stale after SIGKILL worked on the stand-in.
- Orphan definition (README): never-ended and alive at the check, or writing after the restart line while its step was started again. A job the harness adopts is not an orphan; a literal "any pre-kill process writing after restart" would count Theseus's surviving jobs. The owner may prefer that.
- The oracle's kill can duplicate only if it lands in the ms between a tool's effect line and the journal mark; not seen.
- Main table's "orphans" column for kill trials counts killed steps; read the kill table.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule pass; suite 3932 run, 3899 passed, 33 failed, all in theseusd::sandbox / theseus-sandbox L1 tests (the known root-VM failures, theseus-pv6i); no other failure. Nothing under crates/ changed, so later phases (protocol types, benches skipped by NO_BENCH) were not rerun. `cargo deny fetch` ran in setup.
