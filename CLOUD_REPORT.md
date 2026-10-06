# Cloud report: flake-causes (theseus-d006, 0u6g, 1g8j, r4hn, rnl3)

Branch `cloud/20261006-flake-causes`, from main at d279767f. Started 19:44 UTC, report written 21:35 UTC.

How every number below was taken: each test's binary built once, then one test with `--exact --test-threads 1` in a
loop. "Under load" is the AGENTS.md recipe: the test at `nice -n 19` beside four `sh -c 'while :; do :; done'` loops
at nice 0, on this 4-core VM. The loops were killed by their own pids. Main's binaries were copied aside before any edit,
so "main" means d279767f's build of the same test.

## 1. theseus-d006: `term::tests::a_close_leaves_no_child_behind`

**What I found.**
- On main the leak did not reproduce:
  - 0 failures in 30 runs under load;
  - 0 in 100 beside 8 busy loops;
  - 0 in 60 beside main's whole theseus-core test binary running in a loop (`--test-threads 8`) plus 2 busy loops;
  - 0 in 15 + 15 with two copies of main's test running at once, one under load.
- The cross-talk reading reproduces by its mechanism. With one stray `sleep 4343` alive on the machine, main's test
  fails 3 runs of 3 ("a child outlived its terminal"). The new test passes 3 of 3 beside the same sleep.
- On the owner's machine, gates from several worktrees run at once. Another tree's run of this test, starved between
  starting its sleeps and closing them, matches `marked("4343")` for as long as it lives. So does a sleep an earlier
  run leaked, which lives 72 minutes.
- The candidate leak in the brief is real in principle: `setsid` forking between the close's last scan and its
  SIGKILL. I wrote a rescan in `Pty::close` (after the kill, look again with `descendants` and `holders` until a look
  finds nothing alive, bounded at 2 s). I could not build a test that fails without it, so I left it out
  (AGENTS.md: a fix starts from a failing test). If the new diagnostics ever show a survivor in a session of its own
  whose parent is 1, that rescan is the fix.

**What changed.** 43603a76 (term/tests.rs only):
- The sleeps' seconds carry the run's pid, seven digits wide (`sleep 4343.0012345`), so no run's marker is a prefix of
  another's.
- A failure prints each survivor's pid, ppid, session, pgrp, state, start, cgroup and cmdline (`described`).

**Proof.**
- After the change (`Pty::close` as on main): 0 failures in 30 runs under load, 1.22 to 1.54 s each.
- Two runs at once from two loops, one under load: 0 of 30 and 0 of 30, 0.83 to 1.71 s each.
- Plant: `holders` taken out of both scans in `Pty::close`. The test fails, and names the survivor as
  `pid 23831 ppid 1 session 23831 pgrp 23831 state S start 428419 cgroup 9:name=systemd:/`, the
  `sleep 4344.0023805` that left by `setsid`. File restored, touched, `git status` clean. The planted survivor
  ignores SIGTERM (the trap is inherited), so I killed it with SIGKILL by its pid.

## 2. theseus-0u6g: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up`

**What I found.**
- The time is the work itself, not a wait. Main's test takes 6.87 to 7.05 s alone. Under load one run passed 400 s and
  was killed by my timeout.
- The new test prints its phases:
  - idle, 400 opens take 0.38 to 0.46 s, the board is caught up 5 ms later, and the drain 7 ms after that;
  - under load, 400 opens take 7.5 to 37 s (about 20 to 90 ms per open), and the board and the drain add under 1 s.
- So the 5,000 opens alone come to minutes at nice 19. No change to a wait would help.

**What changed.** d83019a6:
- `Push::backlog_cap` (an `AtomicUsize`, `BACKLOG_CAP` by default) is the cap a new connection takes;
  `rpc/server.rs` passes it to `outbound::channel_capped`.
- `Shared.capped: bool` became `cap: usize`. A raw channel's cap is `usize::MAX`, which never drops.
- The test sets the cap to 256 and opens 400 sessions. It first asserts that a connection takes `BACKLOG_CAP` unless
  a test sets it.
- The board's catch-up is awaited on `Push::feed()` (`wait_for(p >= last)`), not the 10 s `until` poll.
- What the test still proves: a connection that stops reading fills to the cap, then what passes is dropped and
  counted; once it reads again it drains and hears one `events.lost` naming the stream (changed + dropped = events);
  its re-snapshot equals a fresh client's; and health's count equals what was dropped. What it no longer proves is the
  rule at 4,096 over a real connection. That is held by `outbound::tests::past_the_cap_notifications_drop_until_the_queue_drains`
  at `BACKLOG_CAP` itself, and by `tests_push_once.rs`, which fills `BACKLOG_CAP`. Only that test ever sets the cap.

**Proof.**
- 0 failures in 30 runs under load, 8.2 to 41.5 s each. Idle: 0.51 to 0.57 s.
- Plant: a dropped notification not counted (`lost.0 += 0` in `Outbound::notify`). The test fails at
  `dropped > 0 && changed >= CAP - 16`. Restored and touched.

## 3. theseus-1g8j: `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`

**What I found.**
- Main fails 5 of 30 under load, each time `left: 0, right: 5` in the first half.
- The brief's reading checks out against tokio 1.53.1's `blocking/pool.rs`. `spawn_task` pushes to the back of the
  queue, and spawns a thread from the caller when no pool thread is idle. Pool threads `pop_front`.
- A temporary variant of the test (main's runtime, with no start) printed which thread ran the probe and, afterwards,
  the worker thread's own policy. It failed 10 of 40 under load, and all 10 read the same way:
  `probe ran on <tid> policy 0; worker thread <other tid> policy 5`. The SCHED_IDLE thread started from the idle
  thread took the worker task, and the worker's SCHED_OTHER thread ran the probe.

**What changed.** 4b5fabc0 (test module only):
- `started()` builds each runtime, then spawns a task on it and awaits it, so the worker holds its pool thread before
  anything else is queued.
- `probe()` returns the thread id with the policy, and each assertion names the probe's thread, the worker's and the
  idle thread's.
- Both halves keep their meaning.

**Proof.**
- 0 failures in 30 runs under load, 0.25 to 0.64 s each.
- Plant: the second half in the fault's shape (`h.enter()` plus `h.block_on` of the request itself). It reads
  SCHED_IDLE, `left: 5, right: 0`. Restored and touched.

## 4. theseus-r4hn: theseus-kernel `tests/children.rs`, `a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child`

**What I found.**
- It did not fail on main here: 0 of 30 under load, 0 of 100 beside 8 loops, 0 of 60 beside the core suite.
- The cause is shown directly. A Python probe spawned `sh` with argv[0] `/x/theseus-index` and read
  `/proc/<pid>/cmdline` at once after the spawn returned. It read empty 333 times in 2,000 idle, and 1,991 in 2,000
  under load.
- The test is shielded by its own order: it spawns flock and sleep and waits for flock's child before `relearn`.

**Decision: fix `relearn`, the product path.**
- After an exec restart, a tender read mid-exec stays an orphan for the image's whole life. The supervisor then starts
  a second tender, which exits 3 on the first's lock.
- `relearn` now classes each child with `learn(read, wait)`. An empty cmdline of a live child is read again every
  1 ms, up to `EXEC_WAIT` = 500 ms for the whole relearn.
- Why 500 ms: job.rs measured the window at up to 84 ms for a starved child.
- Only an empty cmdline waits, so a start with no child mid-exec waits for nothing (FAST).
- A cmdline still empty at the bound is an orphan as before, or a zombie if the child became one.
- `relearn` runs in `adopt_children` before the runtime exists, so the 1 ms sleep holds no worker. It does hold the
  registry lock, but nothing else runs yet.

**What changed.** fb4045a2:
- children.rs: `learn`, `in_exec`, `EXEC_WAIT`, and a by-order unit test with a cmdline reader the test controls
  (empty, then the tender's), which also covers a wrapper, the bound, a full line with no wait, and a gone process.
- tests/children.rs spawns the stand-in tender just before `relearn`.
- The kernel's AGENTS.md has one line on it. theseusd main.rs is untouched.

**Proof.**
- 0 failures in 30 runs under load, 0.48 to 0.57 s each.
- Plant: the re-read removed (`return None` on the first empty read). `children::tests::a_child_in_its_exec_is_read_again`
  fails. Restored and touched.
- Uncertain: the integration test with the plant still passed 10 idle, 10 under load and 30 beside 8 loops. Even
  spawned last, the tender has finished its exec by the time `relearn` reads it. Only the unit test guards the
  re-read here.

## 5. theseus-rnl3: theseus-aws-catalog `one_service_decodes_in_under_5_ms`

**What I found.** Main fails 2 of 30 under load: `ec2 took 680.7ms, over 50ms` and `559.3ms`.

**What changed.** c16cdbd0:
- The decode is timed by `CLOCK_THREAD_CPUTIME_ID` in both builds, with the same bounds: 50 ms in debug, 5 ms in
  release, the real check.
- `libc = "0.2"` is a dev-dependency. Cargo.lock gains only the `libc` line in the crate's dependency list, and no
  package.
- The test prints both clocks.
- What CPU time proves: the decode, pure computation over the embedded blob, costs under the bound. A decode that
  waited (on a lock or a read) would pass; the catalog has neither.
- I chose CPU time in release too, not wall time there. On a loaded machine a release wall-clock bound flakes the same
  way.

**Proof.**
- 0 failures in 30 runs under load, 6.4 to 7.6 s per run. ec2 measured 9.5 to 11.7 ms on the CPU and 557 to 820 ms on
  the wall.
- Plant: each timed decode done 9 times over. `ec2 took 99.39ms, over 50ms`. Restored and touched.
- Not run here: `cargo test --release -p theseus-aws-catalog`.

## Suites

- theseus-core's `term::`, `tests_push::` and `learning::` tests (34), five passes under load with `--retries 0`: 34 of
  34 each time (nextest's own times 34 to 45 s).
- theseus-kernel's and theseus-aws-catalog's suites: 191 run, 191 passed, 1 skipped.
- `cargo fmt --check` and `cargo clippy -p theseus-core -p theseus-kernel -p theseus-aws-catalog --all-targets -D warnings`
  are clean.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the final tree before it was
split into the commits above (the same content).

- The first run died in clippy: the session's disk was full (target/debug/incremental had reached 17 GB). I deleted
  `target/debug/incremental` and ran again with incremental builds off.
- Every phase before the suite passed.
- Suite: 2,990 run, 2,957 passed, **33 failed**, 24 skipped. All 33 are the known L1 failures on this root VM
  (theseus-pv6i): theseus-sandbox's contract tests and its bench's `spawn_100`, and theseusd's `sandbox` tests. No other
  test failed.
- Phases after the suite, run by hand:
  - protocol types: `cockpit/src/protocol.gen` unchanged;
  - turn: `theseus-sim bench turn --check --runs 5 --burst 0` gave frames_plain 5 of 5 and frames_tool 9 of 9, ok;
  - lifecycle and jobs: skipped by `THESEUS_GATE_NO_BENCH`.
- The commits were not gated one by one: one gate covered their union.

## Live checks for the maintainer (the owner's 16-core machine)

Build each test binary on main and on this branch (`cargo nextest run -p <crate> --no-run`, then
`ls -t target/debug/deps/<bin>-*`). Then, for each test, loop it 200 times at `nice -n 19` beside one nice-0 busy loop
per core (`for i in $(seq $(nproc)); do sh -c 'while :; do :; done' & done`):

```
TZ=America/Phoenix nice -n 19 target/debug/deps/theseus_core-<hash> --exact <test> --test-threads 1
```

Tests: `term::tests::a_close_leaves_no_child_behind`,
`tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up`,
`learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`, children's
`a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child`, and catalog's `one_service_decodes_in_under_5_ms`.

What each should show:
- Main fails as the issues say; this branch never does.
- tests_push prints `lag prove: … opened in …, applied at …, drained at …`.
- The catalog test prints both clocks.
- d006 from two trees at once: never "a child outlived its terminal". If it does, the message now names the survivor:
  a `sleep 4344.<pid>` with ppid 1 in its own session points at the close's fork window (§1).
- The gate with `--retries 0`.

## Left and uncertain

- d006: cross-talk is shown only by its mechanism (a stray sleep). Two copies of main's test at once did not collide
  here. The rescan for `setsid`'s fork window was written but not shipped, because no test fails without it (§1).
- r4hn: only the unit test guards the re-read (§4). `EXEC_WAIT`'s 500 ms is a judgement: the start path pays it only
  while a child is mid-exec, against a second tender for the image's life.
- 0u6g: `Push::backlog_cap` is a public field only tests write. Its reader is `serve_connection`, so it fits the reader
  rule, but the owner may prefer a `#[cfg(test)]` setter.
- No doc outside AGENTS.md was edited. For docs/status.md: the five tests are fixed at their causes.
