# Cloud report: cancel-fast (theseus-dwoj)

Branch `cloud/20261006-cancel-fast`, cut from main at d279767f (store format 23, unchanged). Two sessions worked it:
the first wrote steps 1 and 2 (commits below) and stopped at a usage limit before its load check and its report;
this one (started 2026-10-07 01:17 UTC) reran every proof below on fresh builds, ran the remaining checks, and wrote
this report. No code changed in this session.

| commit | step |
|---|---|
| 223d0382 | 1: a cancel's stop waits on the processes' exits, not a backoff |
| 4e8a729e | 1 (fix): a tree stop signals new processes only at its looks, not at an exit's wake |
| ec89e31b | 2: a cancel of a running job is two frames, its own and its job's end |

All numbers: `theseus-sim bench lifecycle --phases cancel --runs 20`, debug builds frozen to /tmp, the same
`theseus-sim` (HEAD's, which prints each cancel's frames) for every build. "Quiet" is nothing else running; "loops,
nice 0" is four `while :; do :; done` loops beside the bench at the same priority; "loops, nice 19" is AGENTS.md's
recipe applied to the whole bench, so the daemon itself runs at nice 19 under four loops on four cores. Base is
d279767f.

## Step 1: the waits, event-driven (223d0382, 4e8a729e)

**Found.** As the brief said: the wrapper's `tree::stop` step 1 looked every 10 ms after its SIGTERM, and
`terminate_all` slept on `Stopping::poll`'s backoff (looks at about 0, 10, 30, 70, 120 ms), so a job that died at
once was seen at about 10 ms by the wrapper and its verdict read at about 30 ms by the daemon. Base p50 ~41.7 ms.

**Changed.**
- theseus-kernel tree.rs: step 1 keeps a pidfd per signalled process (checked against its start time, as every
  signal is) and sleeps on them (`wait_exit`, as step 3 does); a child no scan has seen yet is found at the next look,
  at most `LOOK` (10 ms) later.
- 4e8a729e: a scan woken by an exit only checks whether the tree is empty; a process found new is signalled only at a
  `LOOK`. Without this, a shell's SIGTERM trap had its cleanup's first child SIGTERMed the moment it forked:
  theseusd's `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker` caught it (its
  trap's file empty in 3 of 8 runs after 223d0382; 10 of 10 quiet and 5 of 5 under load after the fix).
- theseus-kernel job.rs: `Stopping` opens each asked wrapper's pidfd once its pid reads as the job's wrapper, and checks
  again after opening, so the descriptor never names a later process given that pid. `Stopping::exits` hands the owner
  those pidfds and the time to the next deadline (grace, answer, kill) when every unsettled job has one; otherwise
  `poll`'s step stays the wait. `Stopping` itself still never sleeps.
- theseus-core cancel.rs: the daemon's stop waits on those pidfds through `AsyncFd` (no worker held); `terminate`, the
  thread owner, polls them. The daemon's own stop uses the same path.

**Proved (this session).** A B B A, base vs 4e8a729e:

| run | build | p50 | p95 | min to max | frames/cancel | load (1 min) |
|---|---|---|---|---|---|---|
| quiet | base | 41.6 | 42.7 | 40.5 to 44.3 | 5 | 0.86 |
| quiet | step 1 | 10.9 | 11.4 | 9.7 to 12.7 | 5 | 0.83 |
| quiet | step 1 | 10.9 | 12.2 | 9.6 to 31.9 | 5 | 0.54 |
| quiet | base | 41.7 | 43.6 | 39.7 to 43.8 | 5 | 0.55 |
| loops, nice 19 | base | 414.8 | 580.5 | 283.8 to 648.3 | 5 to 11* | 5.29 |
| loops, nice 19 | step 1 | 350.4 | 549.1 | 263.4 to 576.5 | 5 to 9* | 5.71 |
| loops, nice 19 | step 1 | 354.5 | 524.8 | 239.5 to 525.5 | 5 to 6* | 5.35 |
| loops, nice 19 | base | 411.7 | 557.3 | 267.4 to 600.4 | 5 to 11* | 6.01 |

**Plant** (first session, quoted from 223d0382's message): both waits put back to the backoff, the row read 42.4 and
42.6 ms against 11.0 and 11.8. No unit test was added for step 1: the brief allowed one only if it proves by order,
and the gain is a wall-time one; the lifecycle row is its guard.

## Step 2: the frames, folded (ec89e31b)

**Found.** Counted from the WAL by the lifecycle bench's cancel phase (walcount's `Tail`), five synced frames per
cancel of one running job on base:

```
[execution, action, ledger:execution.cancelled]      the cancel (kernel)
[action, ledger:action.cancel]                        the acknowledgement (cancel_step)
[execution, action, ledger:action.cancel]             the verdict (cancel_step)
[ledger:action.cancel_verified]                       the verdict's fact (Ended::record)
[node]                                                the call's answer (answer_after_cancel)
```

`ExecutionCancelled` writes no row, so no frame of its own.

**Changed.** Now two:

```
[execution, action, ledger:execution.cancelled]
[ledger:action.cancel, execution, action, ledger:action.cancel, ledger:action.cancel_verified, node]
```

- `ToolRuntime::terminate_all` split into `stop_backends` (stops hands, tasks, in-process calls as before) and
  `Stopped::write_verdicts`, which writes each job's acknowledgement, verdict and (when asked) its fact's row via
  `verdict_row`, as a hands group does, each job a nested transaction so one failure takes back only itself.
- The cancel (`cancel_execution_judged`, rpc/driver.rs) closes the session's terminals, then writes the jobs' last
  steps and the unanswered calls' answers (`answer_after_cancel_with`, toolrun/late.rs) in one `Kernel::frame`. That
  frame may be built twice (`external::under_hold`, an answer that is outside text), so cancels are counted for health
  and `theseus.cancel` only once it is written (`stopped`).
- **The acknowledgement** moved from before the ask to the verdict's frame, not into the cancel's frame. It says the
  stop was asked of the backend; written before the ask, a crash could make it untrue. What recovery does: it reads a
  job's cancel only as `a.cancel.is_some()` (the reconcile, the reaper's `wrapper_lost`, a late completion's
  resolution); nothing reads `acknowledged` except the AWS hands' Fargate verify, which is not this path. So a crash
  before the verdict now leaves `requested`, which recovery handles exactly as it handled `acknowledged`. Its row is
  kept, in the frame that says the stop ended.
- `Ended::record` announces a fact whose row already rode in its step's frame. A stop at a job's launch still records
  its facts through the turn. The hands' own paths (aws/hands/cancel.rs, group.rs) are unchanged.
- theseusd `tests/cancel_frames.rs` (new): `a_cancel_of_a_running_job_writes_two_frames` holds the two frames record
  by record on a real daemon and a real `sleep 60` job; `a_cancel_whose_answer_is_outside_text_is_two_frames_and_counted_once`
  does the same for a `gh` job (outside text: the frame built twice under the session's lock) and checks health counts
  one cancel. **This covers the daemon's cancel path (`execution.cancel` → `cancel_execution_judged`)**, not the core
  test rig's thread-job task path.
- The lifecycle bench's cancel phase prints the frames per cancel (min to max, and the largest set).
- Goldens: neither `core_output.txt` nor `kernel_frames.txt` moved; their tests
  pass in the gate. No record layout changed: no store format bump.

**Proved (this session).** A B B A, 4e8a729e vs ec89e31b:

| run | build | p50 | p95 | min to max | frames/cancel | load (1 min) |
|---|---|---|---|---|---|---|
| quiet | step 1 | 10.7 | 13.4 | 9.2 to 13.5 | 5 | 0.42 |
| quiet | step 2 | 8.2 | 10.5 | 7.1 to 11.7 | 2 | 0.35 |
| quiet | step 2 | 8.4 | 9.9 | 7.6 to 10.2 | 2 | 0.25 |
| quiet | step 1 | 10.8 | 12.7 | 10.1 to 13.9 | 5 | 0.16 |
| loops, nice 19 | step 1 | 351.7 | 521.0 | 236.0 to 532.5 | 5 to 6* | 5.85 |
| loops, nice 19 | step 2 | 320.8 | 472.5 | 203.4 to 576.4 | 2 to 5* | 5.57 |
| loops, nice 19 | step 2 | 345.5 | 469.5 | 214.4 to 512.4 | 2 to 8* | 5.49 |
| loops, nice 19 | step 1 | 345.4 | 546.3 | 224.0 to 572.7 | 5 to 11* | 5.37 |

And base vs both steps, four loops beside a bench at nice 0 (the owner's "beside busy loops" setting):

| build | p50 | p95 | min to max | frames/cancel | load |
|---|---|---|---|---|---|
| base | 40.9 | 49.2 | 20.6 to 50.1 | 5 | 4.25 |
| step 2 | 10.7 | 14.9 | 6.3 to 17.4 | 2 | 4.16 |
| step 2 | 9.0 | 14.7 | 6.1 to 15.9 | 2 | 4.11 |
| base | 44.3 | 49.6 | 21.1 to 50.3 | 5 | 4.07 |

\* Under nice 19 the frame count's maximum includes frames that are not the cancel's: a re-run printed the extra one
as `[ledger:memory.labeled, ledger:memory.gated, …]`, memory's labelling of the starting turn, landing late in the
cancel's window because the daemon is starved (its own starts took 1.0 to 1.4 s in that run). The minimum (5 on base,
2 on step 2) is the cancel's own. Under nice 19 the whole daemon is starved and the row (~350 ms, over the 250 ms
budget on every build, base included) measures scheduling, not the cancel; step 2 still shows ~10% lower p50/p95.
This VM's syncs cost microseconds, so step 2's three saved syncs show as ~2.5 ms here; on a disk at 9 to 30 ms a sync
they are worth 27 to 90 ms.

**Plants.**
- This session: the fact's row put back in a frame of its own (`write_verdicts(k, false)` at both calls in
  rpc/driver.rs): both cancel_frames tests fail, showing three frames
  (`[…, node]` then `[ledger:action.cancel_verified]`). File restored, touched, `git status` clean.
- First session (from ec89e31b's message): the acknowledgement back before the ask (three frames, fails); the count
  inside the frame's closure (health counts 2 on the `gh` job, fails).

**Load check of the frame test** (the one the first session lost): `cancel_frames` and `job_approval` together at nice
19 beside four busy loops, 6 runs: 6 of 6 passed (6 tests each, 7.4 to 8.4 s). Quiet: 15 of 15 for cancel_frames,
job_approval and versions.

## The other checks the brief names

- theseus-kernel, job/tree/cancel/stop tests (`-E 'test(/job|tree|cancel|stop/)'`): 43 passed.
- theseusd `tests/job_approval.rs`, `tests/versions.rs`, `tests/cancel_frames.rs`: 15 passed; `tests/stops.rs`: 1 passed.
- theseus-core cancel/hands/stop tests: 110 passed; `aws::hands`, `tests_cancel` and telemetry tests: 106 passed
  (tests_part2's `action.cancel_verified` counts and `theseus.cancel` counts included).
- `theseus-sim bench lifecycle --runs 10 --check`, every phase, on HEAD's debug build: LIFECYCLE OK. Cancel p50 8.1,
  p95 9.2 ms, 2 frames a cancel; cold start p95 20.1, shutdown 8.3, kill/restart 17.3, swap 20.9 ms.
- `theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames, tool turn 9, both at budget.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on ec89e31b: fmt, shape, features,
clippy, cockpit, test build, reader rule passed; suite 2991 run, 2958 passed, **33 failed: exactly the known L1 set**
(theseus-sandbox's 19 contract tests and `bench spawn_100`, theseusd's 13 `sandbox::` tests; a root daemon's job has
no job cgroup, theseus-pv6i). No other test failed, no flake retried. The phases after the suite, run by hand:
protocol types (protocol.gen unchanged) ok; lifecycle and jobs benches skipped by NO_BENCH (lifecycle run separately,
above, OK); turn bench ok; `cargo deny --offline check`: advisories, bans, licences, sources ok. `CARGO_INCREMENTAL=0`
was set only because the VM's disk filled with three builds' incremental caches; it changes no result.

## The live check (the maintainer's, on the owner's machine, settled)

1. A B B A, each with PSI beside it (`cat /proc/pressure/io /proc/pressure/cpu` before and after):
   ```
   cp target/debug/theseusd /tmp/theseusd-main      # built on main
   cp target/debug/theseusd /tmp/theseusd-dwoj      # built on this branch
   for d in main dwoj dwoj main; do
     target/debug/theseus-sim bench lifecycle --phases cancel --runs 20 --theseusd /tmp/theseusd-$d
   done
   ```
   Expect: p50 from 74 to 77 ms (main) down to roughly 20 to 30 ms quiet (two syncs at ~9 ms plus ~5 ms of work);
   the `cancel: N to N frames` line reads 5 on main and 2 on the branch.
2. Plants:
   - Step 1: in theseus-core cancel.rs, make `terminate_all` ignore `Stopping::exits` (sleep `poll`'s step), and in
     theseus-kernel tree.rs step 1 sleep `LOOK` instead of `wait_exit`; touch both, rebuild: the row's p50 returns to
     about main's.
   - Step 2: `stopped.write_verdicts(k, true)` → `false` at both calls in theseus-core rpc/driver.rs; touch, then
     `cargo nextest run -p theseusd --test cancel_frames`: both fail with three frames. The setsid cancel test
     (`job_approval::a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too`) should pass with or without the
     plants (it guards the kill, not the speed).

## Left, uncertain, and for the owner

- **The budget.** Proposal: the cancel row's budget from 250 ms to **100 ms** (owner's machine: expected p50 20 to
  30 ms quiet, and two syncs at 20 to 30 ms each beside builds' IO puts p95 around 60 to 80 ms). The maintainer
  measures and sets it.
- **A frame check for the bench.** Not added (the bench only prints). Proposal: `--check` fails when the *minimum*
  frames per cancel exceeds 2, or count only frames carrying a cancel's rows: under starvation, memory's labelling
  frames land in the window, so a maximum would flake.
- **The acknowledgement's meaning changed**: `acknowledged` is now written with the verdict, in the same frame, so on
  the daemon's cancel path no store ever holds an `acknowledged` action with its job still running. The AWS hands
  keep the old order. If anything is meant to read "asked but not yet answered" from the store, it now sees
  `requested`. Worth a line in the spec's record of the step.
- **Docs to change** (not edited here): Part III's item for theseus-dwoj (the five frames and the two, the
  acknowledgement's move, the A B B A tables); docs/status.md's lifecycle row for cancel; theseus-kernel's AGENTS.md
  already describes `Stopping::exits` (223d0382), and theseus-core's AGENTS.md has the new frame rule (ec89e31b).
- Step 1's new child-found-late bound is one `LOOK` (10 ms): a job that forks after the SIGTERM is signalled at most
  10 ms later, as before.
