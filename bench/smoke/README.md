# The release smoke

A CI-level check of a release candidate (theseus-yf6pj): about an hour, a few dollars, the owner's WSL host, five
parts, each PASS or FAIL against the floor the last release wrote. It gives confidence in a release; it is not a
measurement, so nothing here is published, and nothing is written under `docs/benchmarks/`.

```bash
scripts/release-smoke.sh                      # all five parts; the verdict is report.md's first heading
scripts/release-smoke.sh --parts "route resume" # a subset (the $0 ones are recall and resume)
scripts/release-smoke.sh --write-floor        # at a tag: the floor is each passing part's reading
```

| Part | What runs | PASS needs | Cost |
|---|---|---|---|
| `tb` | Terminal-Bench 2.0's slice (`tb-slice.toml`, plan `bench/plans/smoke-tb10.json`): 8 stable tasks and 2 controls, k=1 at HIGH, 4 at once, through `bench/harbor/run.py` on the candidate's static binaries | 10 trials ran; every stable task passed; both controls failed; dollars per solved task and the part's wall within +30% of the floor | about $1 |
| `route` | `theseus judge replay --cases` over route.v10's 58 cases, three runs, on the live daemon | 3 runs answered; each 54 of 60 on the model (the two pin tests, W39 and W40, counted as met, as the design's 60 does) and every must; the slowest run within +30% of the floor | about $0.02 |
| `recall` | `scripts/recall-tests.sh`, as the gate runs it, with the candidate's binaries for the driver tests | it ran tests (a "skipped" exit 0 is a FAIL) and they passed | $0 |
| `resume` | `resume.py`: a scratch daemon on the stand-in model runs a six-step tool task and is SIGKILLed mid-tool at steps 2, 4 and 6; each job finishes while the daemon is down | 3 kills, 3 resumes, 3 `execution.interrupted` rows, each step's effect on disk exactly once, every job's own output seen by the model after the restart, 6 jobs succeeded and no "outcome unknown", the turn ended | $0 |
| `dm` | `dm.py`: five turns through the live daemon's CLI (a greeting, a quick fact, a tool call, a task start, the task's answer); the tool turns read a file with the file tools (`SMOKE_DM_FILE`, else this checkout's `Cargo.toml`), never a shell, since a command can park the turn on the owner's approval; any approval asked is declined and fails the part | 5 replies, each within 60 s, with its footer and the reply it should have; a task really started and ended; the session's `turn.ended` rows read, and no alarm rows in its or the task's ledger | cents |

Every PASS needs a positive count first (10 trials, 3 runs, tests found, 3 kills, 5 replies): an empty set meets
every "no failures" bar, so it never counts as one.

**The slice** was picked once, on 2026-10-30, from the 10-26 HIGH run (89 tasks x 3) and the 10-27 MAX run (30 hard
tasks x 3) of 1.0.3 as shipped, and is not re-picked per release: `tb-slice.toml` says why each task is in it, and
`test_smoke.py` holds it against those CSVs. A control that passes means the grader or the slice is broken, or
Theseus learned the task; either way the slice is read again before the release is called.

**The floor** (`floor.json`) is written only by `--write-floor`, and only from parts that passed: a part that failed
keeps the previous floor's entry. A passing part's reading becomes the floor the next release is held to, so the
+30% slack can compound across releases: each tag's commit of `floor.json` shows every move, and a re-base is a hand
edit. Dollars are compared only on one price table (`bench/harbor/price_basis.py`); a floor on another is said, not
compared.

**The host**: each trial is a Docker compose project with its own network, and leftover networks from earlier runs
exhaust Docker's address pools, so every trial fails before its agent starts. The tb part probes a network first,
and names any trial that errored before its agent ran as the host's failure, not the candidate's.

**The candidate** is named by its commit: `SMOKE_SHA`, else the install directory's name
(`/usr/local/lib/theseus/<sha>`). The tb part fails when the static build it uploads (`bench/bin/build-commit`) has
other Rust sources than the candidate's.

**Route on a live pack.** `judge replay --cases` refuses a pack the build dispatches ("… is wired"), so once a
release makes route.v10 live, the route part fails with that refusal until the replay can take a live pack.

**The DM part drives the CLI, not Discord**: the test channel needs the owner's own hand. The task it starts posts
as any task does; the locked DM task flow is observed, never changed. On the live daemon each run leaves two
sessions (the conversation and its task's), the task's report wherever a CLI task reports, and three `judge.replay`
rows from the route part.

**Resume covers one path**: each job finishes while the daemon is down, so the restart reads its settled result. A
job still running at the restart is adopted as background and its result comes back later as a message of its own,
which the stand-in model (matching a turn's last text) cannot follow; that path is the daemon's tests' alone.

The keys the tb part needs (`ANTHROPIC_API_KEY`, `TYPESAFE_API_KEY`) come from the environment, reach only Harbor's
process, and are never written. Each run writes `runs/<date>-<sha8>/`, of which only `report.md` is committed: the
rest (the Harbor jobs, the scratch daemons, the live daemon's replies) stays on the host.

| File | What it is |
|---|---|
| `smoke.py` | Each part's judgement, the verdict, the report and the floor; the script's helper. |
| `resume.py`, `dm.py` | The drivers of parts 4 and 5. |
| `tb-slice.toml` | The ten tasks and why. |
| `test_smoke.py` | Their tests: `python3 -m unittest discover -s bench/smoke -p 'test_*.py'` (Python >= 3.11). |
