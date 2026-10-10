# CLOUD_REPORT: bench-parity (theseus-w052, theseus-3lqk, theseus-8xp0, theseus-p3jl)

Branch `cloud/20261010-bench-parity`, based on `2fd1f65`. Bench only: nothing under `crates/` or `cockpit/` changed.
No benchmark ran and nothing spent.

## Commits

| # | Commit | What |
|---|---|---|
| 1 | `5d6a4f9` | w052 1: same packages |
| 2 | `a39667c` | list prices for Opus 5.5, Fable 5.1, Haiku 5.5 (the bench-dig finding) |
| 3 | `7155f59` | w052 2: trial limit (KEEL commit, alone) |
| 4 | `d8fa73a` | w052 3 and 4: quiet host, committed plan, `run.py` |
| 5 | `1bb4ce3` | w052 2/3/5 reports, 3lqk Pi: grader timeouts, over-$2 lines, overlap column, T4/T5, Pi slot |
| 6 | `e31bf52` | 8xp0: stop by what the run started |
| 7 | `32fe7dc` | p3jl: effort tests, build commit |

Commits 5 and 7 each hold several of the brief's items because hunks of `draft.py`, `efficiency.py` and `test_bench.py`
interleave (a hunk-level split was too costly); the commit bodies say what is in each.

## Step 1: the same packages (w052 1)
- **Found:** Harbor's `ClaudeCode.install` calls `ensure_system_dependencies(("curl","bash","nodejs","npm","procps"))` in `install`,
  which Harbor runs in the trial's setup, before the agent's timed phase. `BaseInstalledAgent` has the method, so the Theseus
  arm needs no list of its own.
- **Changed:** `theseus_agent.SYSTEM_PACKAGES`, called first in `Theseus.install` (`5d6a4f9`).
- **Proof:** `test_bench.Adapter.test_its_install_adds_claude_codes_packages_before_the_agents_clock`: reads Harbor's own source
  for the same tuple, runs the install on a scripted container (no `command -v` answers, apt-get found), asserts one
  `apt-get install` naming each package, as root, before the first upload, and that `run` installs nothing.
  **Plant:** deleting the call fails it (`0 != 1`, zero installs).
- **Live check:** below.

## Step 2: the trial limit (w052 2) and the over-$2 lines
- **Found / derivation** (comment in `theseus_bench.py`): the limit is `$2.00 + one reservation`, the reservation being
  128,000 output tokens at the output price plus 128,000 input tokens at the input price (the catalog's numbers), rounded up to
  $0.25: **$3.75 for Sonnet 5.5** (1.28 + 0.26 = 1.54, so 1.75). **Opus 5.5 comes to $5.25 (2.56 + 0.51 = 3.07 so 3.25), not the
  brief's "about 4.75"**: no single input allowance gives both 3.75 and 4.75 (Sonnet needs 110k to 235k input tokens of
  reserve, Opus at most 47k), so I kept the rule and the Sonnet number. The owner should say if Opus wants 4.75. The
  rule is `theseus_bench.spend_limit_usd(model)`; `THESEUS_BENCH_SPEND_LIMIT` still wins; the routed arm's 20.0 is unchanged.
- **Over $2:** the efficiency record has `over_budget` (the model's real spend past $2.00; written by `efficiency.write`,
  read by `load_trial`), and the draft's Analysis lists each such trial, a line each ("Trials past $2.00 of real spend").
- **Proof:** `test_the_plain_arms_limit_is_two_dollars_plus_one_maximum_reservation` (3.75, 5.25, each model's reserve fits
  at $1.99 spent, unknown model refused), the routed-arm limit test (now 3.75), `test_draft.Flagged.test_every_trial_past_two_dollars_gets_a_line`,
  `test_efficiency.Budget`. **Plant:** `spend_limit_usd` returning 2.0 fails two tests (`2.0 != 3.75`).
- **Keel findings expected:** `cap-raised`, `bench/harbor/theseus_bench.py` (`SPEND_LIMIT_USD` 2.0 -> 3.75, `ROUTED` unchanged), commit
  `7155f59`, made alone. The guard (run with `--base 2fd1f65`) reports 0 findings on this range, because the new value is a
  computed one; the owner should still know a spend cap went up. The README row for `THESEUS_BENCH_SPEND_LIMIT` is in that commit.
- **Not changed:** `bench/async/async_agents.py:95` still defaults `THESEUS_BENCH_SPEND_LIMIT` to `"2.0"` (the async bench,
  not R0). Left, to avoid a second cap change; it wants the same function if the async bench is to match.

## Step 3: quiet graders (w052 3)
- **Found:** Harbor records a verifier's timeout as `exception_info.exception_type == "VerifierTimeoutError"`; the draft
  counted it as "error (exit or harness)", an agent failure.
- **Changed (`d8fa73a`, `1bb4ce3`):** `bench/harbor/quiet.py` and `run.py`. A run starts through `run.py`, which refuses (exit 3,
  every reason) when the gate lock (`$THESEUS_GATE_LOCK_FILE` or `~/.cache/theseus-gate.lock`) is held (a non-blocking `flock`),
  when `/proc/pressure/cpu` `some avg10` is above **5.0** (`MAX_CPU_PRESSURE`; an idle host reads under 1 and 4 trials make about
  that much themselves), or when `-n` is above 4; a line with no `-n` gets `-n 4`. A host with no PSI file is not refused
  and the record says the reading was unavailable. `load_trial` sets `grader_timeout` and no agent `error`; the draft's ending is
  "grader timeout" and those trials are listed apart. The matrix cell is "-" with the tip "grader timeout".
- **Overlap column** (bench-dig finding): `draft.py` computes each trial's `overlap` (other trials of the report on the host between its agent's
  start and its verifier's end), writes it to the CSV and a per-arm summary in the JSON, and the Threats to validity section
  names it (no such column existed in the repo before; I defined it this way).
- **Proof:** `test_quiet` (8 tests: fake pressure file at 5.01/5.0/0.4, missing file, a held `flock` and its release, `-n` forms,
  all reasons at once), `test_draft.Flagged`, `test_report.Graders`. **Plants:** lock check disabled fails 2; threshold made
  unreachable fails 2; empty `GRADER_TIMEOUT_ERRORS` fails 2 (`test_draft`, `test_report`).

## Step 4: the attempt plan first (w052 4)
- **Changed (`d8fa73a`):** `bench/harbor/plan.py`, used by `run.py`. The plan is read only from `bench/plans/` (`{"name","k"}` or
  `{"name","attempts":{task:n}}`); the run refuses unless `git ls-files --error-unmatch` finds it and `git diff HEAD` (staged or not) is empty;
  the record `<jobs>/<job>.run.json` names the plan, its sha256, the commit that last touched it (`commit`) and HEAD. Per-task plans become one
  `harbor run` per distinct attempt count (`-k`, `-i` per task, job name `-k<n>`); a command line naming `-k` or `-i` is refused.
  `bench/plans/r0-sonnet-5-5.json` (k 3) is added as R0's plan (the owner may replace it).
- **Proof:** `test_plan` (13 tests on a scratch git repo): uncommitted, staged-only, edited-after-commit, staged edit, outside `bench/plans/`, malformed,
  the commit in the record, groups, the launcher refusing with exit 3 and starting nothing. **Plants:** the `git diff` check removed fails 3;
  the `ls-files` check removed fails 2.

## Step 5: T4 and T5 (w052 5)
- **Changed (`1bb4ce3`):** `bench/report/friction.py` + `test_friction.py`; `draft.py` prints T4 and T5 in every harbor report (and `t4`, `t5` in the data file).
  T4 per arm: trials with a tool-level error (flagged result, not starting `[exit code` or `Exit code`), malformed inputs by tool, invented names, counts and shares
  over trials with a trajectory. T5 over pairs every arm solved: mean and median dollars, mean model calls, mean agent seconds, output tokens per call (pooled).
- **Judgment calls:** malformed and invented are told apart by each harness's refusal wording (data tables `MALFORMED`, `INVENTED` in `friction.py`:
  Theseus `INVALID_JSON` / `Invalid input:` / `Unknown tool` / `unknown field`; Claude Code `InputValidationError` / `unexpected parameter` / `No such tool available`; Pi
  `Validation failed for tool` / `Tool x not found` / `must NOT have additional properties`). I wrote the Claude Code and Pi wording from Harbor's and my memory of their
  messages, not from a real trajectory: **the maintainer should check the T4 table against one real trajectory per arm** (a refusal worded otherwise is counted a tool-level error only).
  "Calls" in T5 is model calls. **Defect fixed on the way:** the brief's flags omit Pi's own `extra.isError` (`pi_atif.py` keeps `toolName`, `isError`); it is read too.
- **Proof:** `test_friction` (10 tests, invented trajectories for the three arms) and `test_draft.Flagged.test_every_report_prints_t4_and_t5...`. **Plant:** counting a command's exit code as an error fails 5.

## Step 6: Pi's slot (3lqk)
- **Found:** all eight validated colours were in use, and the method says a ninth hue is never generated. Pi takes **slot 5, shared with `bm25`**: the harness family (slots 1 to 5) and
  the memory family never meet in one figure. `charts.HARNESS_ARMS` and `MEMORY_ARMS` name the families; the old "each arm its own slot" test now holds per family and "eight slots, none skipped". The palette figure
  wraps at seven rows a column (13 rows no longer fit two columns of six). `draft.arm_key` maps `pi`, `pi_agent`, `pi-*`.
- **Validator:** the dataviz skill's `validate_palette.js` on the five harness slots, adjacent pairs: light PASS (CVD 9.1, normal 19.6; contrast WARN for aqua, yellow, magenta as for the existing set), dark PASS (CVD 8.4, normal 19.3). Not a repo test (the
  validator is a skill file, and a conditional skip would trip the keel). **docs/benchmarks/README.md** (not mine to edit) still says Pi has no slot and lists slot 5 as `bm25` only: it needs the Pi row and the shared-slot sentence.
- **Proof:** `test_draft.test_a_pi_job_drafts_and_is_charted_in_its_own_colour` (drafts a Pi job, both modes' SVGs carry slot 5 and the name), `test_charts.test_pi_has_a_slot...`. `test_report`'s "arm outside the palette" test used `pi` as its example; it uses `aider` now.
  **Plant:** removing the `pi` key fails 3.

## Step 7: the stop (8xp0)
- **Changed (`e31bf52`):** `measure.snapshot_script` (the arm runs it after the sampler's start and before Harbor's run, in each of Claude Code's, Pi's and `measured.py`'s arms, into `<state>/pids.before` as `pid:starttime`) and a new
  `stop_agent_script(names, grace, baseline)`: SIGSTOP every process not in the list except its own shell, ancestors and children; rescan to a fixpoint; SIGTERM + SIGCONT; wait the grace; SIGSTOP again (the CONT let them run) and rescan; SIGKILL all.
  Falls back to the name-based stop when the list is absent or empty. Start time in the list guards against a recycled pid.
- **A bug I hit and fixed:** my first version did not re-stop the forker after the SIGCONT, so the final rescan never converged (the test hung 90 s); the planted form is in the commit.
- **Proof:** `test_measure.Namespace` (8 tests) runs the stop for real inside `unshare -rpf --mount-proc` (the sandbox cannot reach this host's processes), with stand-ins under `stopme-*` names: orphan through `( cmd & )`, a SIGTERM-ignoring forker at 0.2 s and 0.05 s, a recycled pid, no list, empty list, the snapshot, and that each arm calls the snapshot and passes the list. The existing stop tests and the arm tests were
  updated to the new call (same assertion counts). **Plant (main's script behind the baseline):** 4 fail (orphan: `{'2': ..., '9': stopme-child 301}` left; forker: dozens of `stopme-child 305` left). These tests **need `unshare` with user namespaces** and fail (not skip) without it; the maintainer's WSL2 has it.
- The brief's "1 of 1 and 21 to 23 left" were not re-measured; the plant shows the same shape.

## Step 8: the record says what ran (p3jl)
- (a) `test_claude_code_agent.Arm.test_the_record_names_the_effort_the_trial_asked_for_not_the_default` and `test_driver.Agents.test_the_claude_arm_records_the_effort...` (`ASYNC_HARBOR=1`). **Plant:** stamping `measure.EFFORT` fails the first (`'medium' != 'high'`); stamping a constant `"medium"` fails the second.
- (b) Neither `bench/build.sh` nor `theseus --version` wrote a commit (the brief said build.sh does; it did not, and health is not reachable from a bench record). `build.sh` now writes `build-commit` (HEAD, `-dirty` for tracked changes) beside the binaries; the install
  copies it to `agent/build-commit.txt`; every record has `build_commit` (null off the Theseus arm). `test_bench.Adapter.test_two_builds_tell_apart_in_the_record_by_their_commit` installs two builds and reads two commits beside the same `theseus 0.0.1`. **Plant:** removing the `record_build` call fails it.

## Findings from the bench dig
- Routed list prices: done (`a39667c`), test `Routed.test_every_model_the_routed_profile_names_has_a_list_price`; plant (opus row deleted) fails. Haiku 5.5's long-prompt tier is not modelled (noted in the code).
- Overlap column: done and named in threats (step 3).

## Live check for the maintainer
```bash
export PYTHONPATH=$PWD/bench/harbor:$PWD/bench/report HARBOR_TELEMETRY=0 THESEUS_BENCH_BIN_DIR=$PWD/bench/bin
bench/build.sh && cat bench/bin/build-commit                      # a commit hash (+ -dirty if the tree is)
# an uncommitted plan is refused (exit 3, "is not committed"):
echo '{"name":"x","k":1}' > bench/plans/scratch.json
python3 bench/harbor/run.py --plan bench/plans/scratch.json -- harbor run -d terminal-bench@2.0 -i fix-git -a theseus_agent:Theseus -m anthropic/claude-sonnet-5-5 -o jobs --job-name x; echo $?   # 3
rm bench/plans/scratch.json
# the committed plan, one smoke task per arm:
for a in theseus_agent:Theseus claude_code_agent:MeasuredClaudeCode; do
  python3 bench/harbor/run.py --plan bench/plans/r0-sonnet-5-5.json -- harbor run -d terminal-bench@2.0 -i fix-git -a $a -m anthropic/claude-sonnet-5-5 -o jobs --job-name fair-${a%%_*}
done      # -k 3 from the plan, -n 4 added; jobs/<job>.run.json names the plan's commit
# packages in the Theseus container (the install log, or in a run): node, npm, ps, curl exist
docker exec <theseus trial container> sh -c 'command -v node npm ps curl bash'
jq '{effort,version,build_commit,over_budget}' jobs/fair-theseus/*/agent/efficiency.json   # medium, theseus 0.0.1, the commit, false
jq .effort jobs/fair-claude/*/agent/efficiency.json                                         # medium
python3 bench/report/draft.py harbor --suite terminal-bench@2.0 --date $(date +%F) --slug live --arm theseus=jobs/fair-theseus --arm claude-code=jobs/fair-claude --out /tmp/live
grep -n "T4, tool friction\|T5, cost" /tmp/live/*.md                                          # both tables, with a real trajectory each
# a timeout (a task with a short agent timeout): no process of the run survives into the verifier, in either arm
```
Also check the T4 table against one real trajectory per arm (step 5), and the stop on a real timeout (`docker top <container>` after the cancel).

## Gate and suites
- Gate (run twice, the same 33 failures both times; `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f65 scripts/gate.sh`): fmt, shape, features, clippy, cockpit, test build, reader rule, keel pass. The
  suite phase fails **only on the known L1 tests** (20 in `theseus-sandbox` incl. `bench spawn_100`, 13 in `theseusd::sandbox`: the root-daemon cgroup refusal, theseus-pv6i); I ran the later phases by hand: `protocol types` (clean), `cargo deny --offline check` (ok).
  The lifecycle and jobs benches are skipped (NO_BENCH). No other failures, no timing test failed.
- **Without `THESEUS_KEEL_BASE`** the gate's keel phase fails on 5 findings from main's own history (`crates/theseus-tui/src/tests.rs` assert-removed, two `allow-added`, `scripts/long-files.txt`): this checkout's local `main` is at `a9ad950`, behind the branch's base `2fd1f65`. None is mine; with `--base 2fd1f65` the guard reports 0.
  Keel findings expected from this branch: the `cap-raised` above only.
- Python (run before each commit; final state): `bench/harbor` 179 tests (Harbor 0.23 in a 3.12 venv: all pass; stdlib python3.11: 43 skip for Harbor, as before),
  `bench/report` 56, `bench/async` 48 (`ASYNC_HARBOR=1` under Harbor: pass; stdlib: pass with skips). `bench/recall` (not mine) has 2 failures
  (`test_drive`: the daemon's prompt is 13,958 tokens, past the 13,640 the plan expects): recall-plan-fix's.
- I did not re-run the suites commit by commit: they ran on the working tree before staging and on the final commit; the staged subsets were assembled by hunk.

## Left or uncertain
- `docs/benchmarks/README.md`: Pi's slot (above). `docs/`/spec: the README text for bench is in `bench/README.md` (a "A fair run" section).
- The Opus limit (5.25 vs about 4.75), the async arm's default limit, the CPU-pressure threshold (5.0), and `bench/plans/r0-sonnet-5-5.json` are choices for the owner.
- T4's refusal wording for Claude Code and Pi is unverified against real trajectories.
- `run.py` was tested with a fake runner; no real `harbor run` went through it.
