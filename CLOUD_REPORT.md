# CLOUD_REPORT: grouped-call-order (theseus-0hufe)

Branch `cloud/20261013-grouped-call-order`, built on main at `9fa8473`. Store format untouched (32).

## Step 1: the dependency rule for programs (commit 01bb34b)

**Found.** `parallel::Touch` keyed a program only on its terminal, MCP server and the *file names* in its argv, so
`start-service web` and `query-service web` shared nothing and ran together. The third kill-family failure (a
`bisect` that named no failing commit) is very likely the same hole: its `make-worktree COMMIT` and
`test-commit COMMIT` share only a commit hash (no extension, no slash), and `test-commit` refuses with "no such
worktree" until `make-worktree` has finished (kill-tasks/kill-bisect's layer2.py, `_refuse(..., 8)`). The trial
transcripts are not in the repository, so I could not confirm it from the run; the rule below covers that pair.

**Changed.**
- `toolrun/operands.rs` (new): a call's operands. A non-flag word naming a thing, from `argv` and from `steps`; a
  script (`bash -c "a && b"`) is each simple command without its own name. Left out: the program, flags, bare
  numbers, `json`/`yaml`/`true`/… , redirects, `/dev/…`, and per pair of calls the leading subcommand words both begin
  with (at most two), so `npm test` twice or `kubectl rollout status` twice share nothing by themselves. A short
  flag's next word (`-C ../wt-a`) and `--flag=value` count as operands; `host:port` and `a:b` ports count as
  `port:N`. It keys on no tool's name.
- `toolrun/parallel.rs`: `Touch.ops`; `Open` keeps the open group's calls and the earlier groups' calls, and
  `admit(index, touch)` returns a `Verdict { joins, held }`. A call that shares a key or an operand with a call of
  the open group opens the next group (the later starts after the earlier has ended, as for files). `held` is the
  first earlier call of the response it shares something with, whether or not it had to wait.
- The held call's result ends with `[started after call 2, which names `web` too]` (`parallel::HELD`, a task-local
  like `BESIDE`; `beside_line()` returns both lines). No new system sentence; proc_run's description gained
  "a file or other thing (a service, a container)".
- `run_group` moved from toolrun.rs to `toolrun/calls.rs` (toolrun.rs 2,499 → 2,418 lines; no ceiling touched).
- parallel.rs's header documents the rule.

**Real-world cases** (operands.rs tests `what_depends_on_an_earlier_call_shares_an_operand` and
`independent_families_share_nothing`):
- stay in order: `systemctl --user start web` / `status web`; `docker run --name db img` / `docker exec db psql …`;
  `git worktree add ../wt-a` / `make -C ../wt-a`; `docker build -t app .` / `docker run app`;
  `docker run -p 8080:80 img` / `curl localhost:8080`.
- run together: sixteen `sleep 2`; `pytest tests/a.py` / `tests/b.py`; `curl https://a.example/x` / `https://b.example/y`;
  `make -C ../a` / `../b`; `git -C ../a status` / `../b status`; `cargo build -p core` / `-p cli`; two
  `kubectl get … -o json`; `npm test` twice.
- **`kubectl apply -f api.yaml` then `kubectl rollout status deploy/api` is NOT reached.** `api.yaml` and
  `deploy/api` share no whole word; splitting words at `/` and `.` would also join every `pytest tests/…` pair. A
  test pins it (`None`) and the header says so. An unchanged limit of the file-name rule too.
- Note: `systemctl start web` run twice with identical arguments is not held (an identical pair is one shape).

**Proved.**
- `cargo nextest run -p theseus-core -E 'test(/parallel/) | test(/toolrun/)'`: 148 tests (the three groups + config)
  pass; the paused-clock turns are in `tests_m3/parallel/order.rs`: `a_query_of_a_service_starts_after_its_start_has_ended`
  (the end of `start` precedes the begin of `status` in the call log, the result says `started after call 1 … `web``),
  `a_container_and_a_worktree_are_used_after_they_are_made`, `programs_that_share_nothing_run_together` (ten programs
  begin before any ends), plus unit tests in parallel.rs and operands.rs.
- **Planted revert**: `shared()` made to ignore operands → `a_query_of_a_service…`, `a_container_and_a_worktree…` and
  `a_program_sharing_an_operand_is_held…` fail; restored, touched, `git status` clean.
- Turn bench (debug build, this VM): `target/debug/theseus-sim bench turn --runs 10 --burst 0` → `turn_batch_p50:
  2132.6 ms at the p50, budget 2405.6 ms: ok`; frames_batch 14 (budget 14), frames_plain 4, frames_tool 8, all ok.
  Four independent `sleep 2` still run together, so the batch kind is where it was (2,435 ms on the owner's machine
  at the join; I did not rebuild main for a before on this VM).

## Step 2: the cap by class (same commit)

`[tools] parallel_reads` (32) and `parallel_writes` (16) beside `parallel_runs`; `run_group` takes the cap from the
group's class; 0 reads as 1 (`ToolRuntime` field `.max(1)`). Template lines added, with the operand sentence on
`parallel_runs`. Admission and sandbox limits are unchanged (a slot only decides how many *start* at once).
Tests: `sixteen_programs_run_in_one_wave_and_a_seventeenth_waits` (most at once 16, the 17th begins after the first
end, `tool_use_id`s come back p00…p16 in order) and `the_cap_of_programs_is_the_config` (cap 3 holds 3). The reads
and writes caps have no test of their own beyond the existing groups (they use the same semaphore code).

## Step 3: parallel_runs 8 → 16 (commit c816b01, a commit of its own)

`config/limits.rs` and the template. Two tests that set `parallel_runs = 16` explicitly in the first commit now use
the default.

Layer 1 against the stand-in (`bench/async/layer1.py run --harness theseus --bin target/debug --sim
target/debug/theseus-sim --runs 3`, debug build; wall around the CLI, median of 3):

| N | cap 8 (before) | cap 16 (after) |
|---|---|---|
| 1 | 2.130 s | 2.137 s |
| 2 | 2.122 s | 2.122 s |
| 4 | 2.139 s | 2.137 s |
| 8 | 2.173 s | 2.166 s |
| 16 | **4.212 s** | **2.202 s** |

Raw: /tmp/l1/before-cap8.json, /tmp/l1/after.json (not committed).

## Keel findings expected

- rule: a cap raised; path: `crates/theseus-core/src/config/limits.rs` (`default_parallel_runs` 8 → 16) and
  `crates/theseus-core/config/theseus.example.toml`; why: the task's own change (sixteen `sleep 2` in one wave).
  commit c816b01. `python3 scripts/keel-guard.py` printed `keel: ok … 0 findings acked` before and after it (the guard
  may not count this one as a cap; listed in case it does at the merge).
- Two new caps (`parallel_reads`, `parallel_writes`) are additions, not raises.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the first commit's tree: door, keel, keel tests,
fmt, shape, features, clippy (`-D warnings`, clean), cockpit, test build and reader rule passed. The suite failed on:
- the known L1 set (theseus-sandbox contract/`spawn_100`, theseusd sandbox tests; root VM, theseus-pv6i);
- `theseus-kernel::tree a_wrapper_that_never_answers_is_killed_with_its_group_and_uncertain`: passed alone (3.2 s);
  it ran under the suite's load;
- `theseus-sim fake_model::tests_paced::a_paced_answer_keeps_its_first_byte_and_spacing_on_a_kept_connection`: passed
  alone;
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: **fails on main
  too** (3 of 3 runs with my changes stashed, 21 s timeout "the thinking's create, held, and the reply's post"); it
  passed once on my tree and failed four times; no tool call is in it, so it is not from this row. Not in the
  known-flaky list: the maintainer should look at it.
The phases after the suite: protocol types (no TypeScript changed: `cockpit/src/protocol.gen` is clean); deny **did
not run** (`cargo deny --offline` cannot find all crates, the fetch in setup covered the advisory db only); lifecycle,
jobs and turn benches skipped by `THESEUS_GATE_NO_BENCH=1` (the turn bench was run by hand above). The second commit
(default 16) changes two constants and a template line: config, template and parallel tests re-run green
(135 passed), `cargo fmt` clean, keel guard ok; I did not rerun the whole gate for it.

Disk note for the next session on this VM: the 40 GB allowance filled twice from `target/debug/incremental` and
duplicate test executables; build with `CARGO_INCREMENTAL=0`.

## For the maintainer

- Layer 2, the four controls and the kill family's `service` task, Theseus arm, 3 trials each
  (`bench/async/README.md`, "Run it"):
  ```
  export THESEUS_BENCH_BIN_DIR=$PWD/bench/bin PYTHONPATH=$PWD/bench/harbor:$PWD/bench/async HARBOR_TELEMETRY=0
  .venv/bin/harbor run -p bench/async/tasks -i service -a async_agents:TheseusAsync -m anthropic/claude-sonnet-5-5 -o jobs --job-name async-theseus-controls -k 3
  .venv/bin/harbor run -p bench/async/kill-tasks -i kill-service -i kill-bisect -a async_agents:TheseusAsync -m anthropic/claude-sonnet-5-5 -o jobs --job-name kill-theseus -k 3
  ```
  (the control task names are in `bench/async/families.py`; pass each with `-i`). Expect 0 order violations and
  reward 1 on the service controls; the results of the held calls should end `[started after call N, which names …]`.
- Layer 1: `python3 bench/async/layer1.py run --harness theseus --bin target/release --sim target/release/theseus-sim --runs 5 --out …`;
  n=16 should be about the n=1 wall plus a few tens of ms.
- Docs to change (not touched here): the spec's Part III item for theseus-0hufe; `docs/status.md`; the async bench
  report (the cap-8 two-wave row is now one wave).
- Design choices to hear: the "shape" exemption (two leading positional words in common are not shared things) is a
  heuristic; it makes identical pairs run together and errs toward waiting for everything else. A held call's note
  also appears on a call that merely sits in a later group than a call it shares with (it did start after it).
