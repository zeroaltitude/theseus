# Cloud report: asyncbench (theseus-2wxa)

Branch `cloud/20261010-asyncbench`, from main at 2fd1f65 (store format 26, untouched: this task is Python only).
Started 20:16 UTC, 2026-10-10. Every commit is Python, Markdown or task files under `bench/async` and `bench/report`.
No Rust, no new dependency, no import beyond the standard library (and Harbor where the adapter already imports it).
`async_agents.py` is untouched: OpenCode needs no injection arm (step 4).

## Steps

### 1. Layer 2: twenty chores with independent slow sub-steps (e6813e2, a fairness fix in 6334ed5)

**Found.** bench/async had six families, one ledger library (`tools/asyncbench.py`), and a `check` hard-wired to
its own families. `test_tasks.Trial` copied a task's `environment/app/` as flat files only, and failed on a
directory (fixed here: `copytree`, as the image's `COPY app/ /app/`).

**Changed.**
- `tools/layer2.py` (new, copied beside `asyncbench.py` into each Layer 2 task's image and verifier): 29 tools, each
  slow step a drawn duration (10 to 45 s, sleep-based) on the existing hash-chained ledger; facts drawn on first use
  and kept in the tools' state (which test fails, which host logged the error, which commit is first bad, ...); the
  ideal wall per family (the critical path of the drawn durations, each step's first run, refusals aside); the
  overlap measure; the order rules (`DEPS`) and `order_violations`; and the twenty checks. A dependent step behaves as
  the real one would (the app run before its build runs yesterday's binary; a query of a service still starting, a
  migration out of order, a link before its libraries, a test in a worktree not yet made are refused fast). Any
  dependent start before its prerequisite's end is an order violation, reward 0 whatever the answer.
- `families.py` (new, not in any image): each task's instruction, oracle, files and shape. 8 programs-then-combine,
  4 reads/fetches, 2 writes, 4 controls, 2 mixed, exactly the brief's table. Counts are the chores' own (3 to 8).
- `sync.py` writes every Layer 2 task whole (task.toml with `metadata.async.layer = 2` and `shape`, instruction,
  Dockerfile, app files, oracle, verifier, both library copies, wrappers).
- `asyncbench.check(family, checks=None)`: Layer 2 passes its own table, so the ledger's checks (chain, durations,
  time scale) are shared.

**Proved.**
- `python3 -m unittest discover -s bench/async`: 92 tests, OK (15 skipped: the Harbor and end-to-end ones).
- Each oracle earns reward 1, no order violation, an ideal > 0, and leaves its TMPDIR empty (all 20; also 5 manual
  rounds of all 20 oracles, 0 failures, after fixing one bug the rounds found: api-summary's draw ran out of names
  for 36 functions from 26 words, 3 of 5 rounds).
- A planted wrong effect per family earns 0 with the problem named (16 plants: every non-control family; the
  controls' wrong effect is the order one below).
- Each control and mixed family with a dependent step started before its prerequisite earns 0 while every other
  problem is empty, i.e. the answer is right and only the order cost it (6 tests).
- The oracles' overlap: host-logs 2.0 to 6.05, doc-questions 3.0 to 8.05, migrations and pipeline at most 1.0.
- Ideal and overlap on hand-worked ledgers.
- `bisect` fairness fix: the check demanded the commit before the first bad one be tested good even when that is the
  oldest, which the instruction says is good, so a search that trusts it scored 0. Now `i > 1`. Test
  `Fair.test_a_search_that_trusts_the_oldest_commit_earns_reward_1` pins the draw (first bad = the second commit);
  planted revert (`i > 0`): 3 of 3 runs fail; fixed: 3 of 3 pass.
- **Under load** (four `while`-busy loops at nice 0, the tests at nice 19; a script file, `python3 busy.py`, since a
  `sh -c` loop was refused by this environment): the first two loaded runs of `test_layer2 test_score_layer2
  test_layer1` failed the same 8 tests both times: the six order plants and the controls' overlap lower bound
  (0.5), and bisect's plant hit `Trial.run`'s 60 s timeout. Cause: the plants started a dependent step while its
  1 to 3 s prerequisite ran, and a starved `python3` start took longer than that, so the step started after the end.
  A timing plant, not a scorer fault. Rewritten (6334ed5): each plant starts the dependent step before its
  prerequisite runs at all (the rule is judged by ledger order, so this is the same violation, with no timing), the
  controls' overlap keeps its upper bound (≤ 1.0) and drops the lower, and `Trial.run` takes a timeout. Third loaded
  run: 45 tests, OK, in 718 s.

### 2. The scorer's new columns (e5b141b, Theseus task sessions in 6334ed5)

**Changed** (`score.py`; existing columns unchanged, the new ones after them):
- **overlap**: the ledger's slow-step time over the slow phase's wall (first slow start to last slow end, monotonic);
  slow tools are Layer 1's one slow job, or every Layer 2 tool.
- **calls per response and multi-call share**: from the ATIF trajectory's agent steps, over responses that call a
  tool (a final answer counts in neither), pooled over an arm's trials; else OpenCode's own stream. Theseus's task
  sessions' answers (`theseus-history-<task>.json`) are added after the conversation's (6334ed5), as Claude Code's
  ATIF holds its subagents' sidechains.
- **order violations**, in families with rules.
- Layer 2's ideal and wait-tax window; `score.py LABEL=DIR` names a job's arm (the three Theseus arms are all
  `theseus-async` to Harbor).

**Proved.** `test_score_layer2.py`: fixture trials for four arms, one trajectory per harness format: Theseus's and
Pi's through their own converters (`theseus_atif.py`, `pi_atif.py`), Claude Code's in Harbor's shape, OpenCode's
from its stream. Under `ASYNC_HARBOR=1` in Harbor 0.23.0's venv (Python 3.12), Harbor's own Claude Code converter on
a fixture session whose responses are split into per-block entries (as Claude Code writes them) gives one step per
response, `[2, 0, 3, 1]`, and Harbor's OpenCode converter likewise. Planted reverts:
- overlap's phase taken as the whole ledger: caught by `test_layer2.Overlap` (0.08 for 1.0);
- violations not counted: caught by the six order tests (reward 1 for 0) and two column tests (0 for 1); after the
  order tests' rewrite, again all six.

### 3. Layer 1's drivers (bebe029)

`bench/async/layer1.py`: the rules for `theseus-sim fake-model --rules` (N calls of `sleep 2`, N in 1, 2, 4, 8, 16,
per harness in its own tool names: Claude Code `Bash {"command", "description"}`, Pi `bash {"command"}`, OpenCode
`bash {"command", "description"}`, Theseus `proc_run {"argv"}`), each behind a mark no other mark holds; each
harness's command pointed at the stand-in by its own setting; the wall around the CLI; a count is "not measurable,
why" when a run fails or ends sooner than one call's 2 s. `--sim` starts the stand-in.

**Proved here** on Theseus (main's debug build, the only harness installed): n=1 2.21 s, n=2 4.28 s, n=4 8.34 s:
main runs one response's calls in turn. `ASYNC_E2E_BIN=target/debug python3 -m unittest test_layer1.EndToEnd`
passes. Found on the way: at `spend_limit_usd = 1.0` every Theseus run stopped with "the session reached its spend
limit" before any call ran (the money gate reserves a call's worst case first); at the bench's $2.0 it runs. Worth
knowing for anyone who writes a stand-in profile with a small cap.

**Not run here**: Claude Code, Pi, OpenCode (not installed, and the brief says to write the commands).

### 4. The OpenCode arm

No Layer 2 family sends a second message, so no `OpenCodeAsync`: the arm is bench/harbor's
`opencode_agent:MeasuredOpenCode` as it is, named in the README with its command. On `interrupt` and `cancel`
it reads "not measurable" (score.py already does that for an arm with no driver report).

### 5. The report's generator (bb9a56c) and the README (9e2555a)

`bench/report/parallel.py`: `--arm KEY=JOBS` (theseus-before, theseus-d1hi, theseus-after, claude-code, pi,
opencode), `--layer1 KEY=FILE`, `--date`. Writes `docs/benchmarks/<date>-asyncbench-parallel-calls.{md,json,csv}`
and `img/<report>/*.svg` in light and dark through draft.py's writer (an existing .md kept unless `--force`). The
answer first, computed from the numbers; Layer 1's wall against N per harness (small multiples, the in-turn line and
the 2 s floor); wall over ideal by shape and arm (figure) and by family (table); calls per response and multi-call
share by arm; round trips and dollars per solved task; the controls' violations; threats; the projection to
Terminal-Bench (the design's 0.2%); and in the .json, the omnibus's rows (its "Every report" line, the index row,
a data row), printed too. `test_parallel.py` drafts a six-arm fixture plus four Layer 1 files, one not measurable.
Rendered with headless Chromium and checked by eye in both modes.

## The live check for the maintainer

1. Harbor reads the tasks and the oracle earns 1 on all twenty (Docker):
   `.venv/bin/harbor run -p bench/async/tasks -x parallel -x wait-tax -x interrupt -x fanout -x cancel -x contention -a oracle -o jobs --job-name pc-oracle`
   then `python3 bench/async/score.py jobs/pc-oracle --out /tmp/pc-oracle`: 20 rows, success 1/1 each, overlap
   well above 1 on the 16 non-controls, 0 order violations.
2. Layer 1, each harness first (whether it can be driven so is the first thing to prove):
   ```bash
   python3 bench/async/layer1.py rules --out /tmp/l1/rules.json
   target/release/theseus-sim fake-model --rules /tmp/l1/rules.json --addr 127.0.0.1:9448 &
   python3 bench/async/layer1.py run --harness claude-code --runs 1 --counts 1,4 --out /tmp/l1/cc-try.json --keep /tmp/l1/keep
   python3 bench/async/layer1.py run --harness pi          --runs 1 --counts 1,4 --out /tmp/l1/pi-try.json --keep /tmp/l1/keep
   python3 bench/async/layer1.py run --harness opencode    --runs 1 --counts 1,4 --out /tmp/l1/oc-try.json --keep /tmp/l1/keep
   ```
   Each should show n=1 about 2 s plus the harness's start, and n=4 either about the same (together) or about 8 s
   (in turn). A run under 2 s means its calls never ran (`ok: false`). What may need a change, in order of risk:
   Claude Code's side requests (title, quota) if `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` leaves any that the
   stand-in's SSE answer upsets, or a non-streaming request; its root refusal of `--dangerously-skip-permissions`
   (the driver sets `IS_SANDBOX=1`); Pi 1.0.4's `models.json` fields (written in the shape Harbor's Pi writes) and
   whether `--no-session` exists in that version; OpenCode's `permission.bash` key and whether its bash tool insists
   on a `description`. Then the full runs with `--runs 5` for every arm (README, "Layer 1").
3. The run itself: README "The parallel-calls run", then `bench/report/parallel.py` as written there.

## Left, uncertain, and for the owner

- **Docs to write at review**: the report under docs/benchmarks/ (the generator drafts it), its row in
  docs/benchmarks/README.md and omnibus.md/.json (the generator prints them), and bench/README.md's file table could
  name `bench/report/parallel.py` and `bench/async/layer1.py` (I left bench/README.md alone: siblings edit bench/).
- **Palette**: the registry has no slot for Pi, OpenCode or a third Theseus arm. The generator maps Theseus before to
  Theseus's blue, the arm with the sentence to the batching paragraph's slot, d1hi to the gray, Pi and OpenCode to
  `other`; every mark is labelled, but d1hi's gray and other's gray are one colour in the legends.
- **Calls per response's denominator** is responses that call a tool; a final answer counts in neither. The owner
  may prefer all responses.
- **Ideal walls** assume every step is needed: an arm that stops early (host-logs, once the culprit is found) is
  measured against the steps it ran.
- **Layer 2 counts** (3 to 8 a family) were chosen as the chores' own; none is 16, so d1hi's cap of 8 is reached
  only by doc-questions and scaffold.

## Keel findings expected

None. `python3 scripts/keel-guard.py --base 2fd1f65` before each commit: `keel: ok`, 0 findings. (The six order
tests were renamed and rewritten in 6334ed5: keel matched them and found nothing, `keel: ok`.)

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f65 scripts/gate.sh`. `THESEUS_KEEL_BASE` because
this clone's `main` and `origin/main` refs are at a9ad950, behind the branch's base 2fd1f65: without it, keel judged
main's own history since a9ad950 (5 findings in files this branch never touched).

- Before the first commit: keel ok, shape ok, fmt, clippy, cockpit, test build ok; suite 3682 tests, 3649 passed,
  33 failed: exactly the known L1 set (theseus-sandbox's 20 contract tests and `spawn_100`, theseusd's 12 sandbox
  tests: root with no job cgroup, theseus-pv6i). The phases after it, run by hand: protocol types ok, nothing
  compiled under the lock, turn bench ok (plain 5 frames, tool 9), `cargo deny --offline check` ok.
- Before the last commit: the same: keel ok (247 files, 0 findings), shape ok, the compile phases ok; suite 3682 tests, 3649 passed, 33 failed, the same known L1 set and nothing else; by hand after it: protocol types ok, nothing compiled under the lock, turn bench ok (plain 5 frames, tool 9), deny ok. No timing-list test failed in either gate.
- bench/'s suites: `python3 -m unittest discover -s bench/async` 92 tests, OK (15 skipped: the Harbor and end-to-end ones); under Harbor's venv with
  `ASYNC_HARBOR=1`: 92 tests, OK (2 skipped: the end-to-end ones); `bench/report`: 42 tests, OK; `bench/harbor`: 145 tests, 1 failure,
  `test_sampler.InANamespace.test_the_samplers_share_of_a_core_is_bounded_at_a_set_count` (a CPU-share bound against
  a measured floor: 0.00548 against 0.00525). Not this branch's: run alone, it failed 4 of 6 here and 4 of 6 on a
  clean worktree of the base 2fd1f65.
