# CLOUD REPORT: daemon-flakes (theseus-jtrc, theseus-y0lm)

Branch `cloud/20261006-daemon-flakes`, from main at 57f265f2 (store format 23, unchanged: no stored record changes).
Commits on top of the task commit:

- `1857d1e2` turn: a stopping daemon begins no continuation and sends no model call (theseus-jtrc)
- `859b09d5` tests: a stubborn job's SIGTERM trap takes its time with no fork, whole before it is visible (theseus-y0lm)

On this VM (4 cores, root), neither red reproduced under the brief's load recipe. Each cause below was found and then
proved directly: a probe of a real `--stdio` daemon for jtrc, and a planted trap child for y0lm. Each fix has a test
that fails without it, shown by a planted revert.

## 1. theseus-jtrc: the first-byte test's second request

### What I found

The second request is the **session driver's own retry** (reading (a)). It is not a resend by the HTTP client, and
the CLI does not submit twice.

- With `transient = 0`, the turn fails with `timeout` (FirstByte), a class that passes with time. `Failing::after`
  says backoff. The turn settled its failed call, so turn.rs wakes the execution at once (`woke_on_fault`), and the
  driver (harness.rs `drive`) starts the continuation as soon as the turn ends. The driver's backoff map is written
  only when a continuation *errors*, so the first retry never waits (theseusd's `tests/failures.rs` pins "the
  driver's first retry fails at once").
- A probe (a scratch test, not committed) drove `theseusd --stdio` by hand with the same config and stand-in:
  - With the client idle 3 s after the failed answer: the second request came 24 ms after the answer, and the log
    showed `continuation turn … stop_reason=no_tool_calls`.
  - With `shutdown` sent right after the answer, as `ask`'s `close` does: in **every** run (3 of 3) the continuation
    began before the stop and reached its model call *during* the stop. Only the runtime's end kept the request
    off the wire. Logging reqwest's error (a probe-only patch, reverted) gave `ConnectError("tcp connect error", …,
    "A Tokio 1.x context was found, but it is being shutdown.")`. The call failed as `network` about 30 ms after
    `stop phase="began"`, while `runtime dropped` came at 44 ms. Under load the connect beats the runtime's end and
    the stand-in sees request 2. That fits "about 1 run in 3 at nice 19".
- (b) ruled out: the HTTP client's first request timed out before its first byte and was dropped; the second
  request is the continuation's, by the daemon's log line. (c) ruled out: `ask` sends one `turn.submit`
  (`submit_stoppable`), and the daemon's log shows one input turn and one continuation.
- No turn start or call reads the stop: `stop_has_begun` is read only by the adjacency warm build. Also,
  `shutdown.notify_waiters()` reaches only a driver parked in its `select!` at that moment; a busy driver missed it.

### What I changed (`1857d1e2`)

The stop's per-core mark, `Outbox::stopping()`, is set by the stop's first step (`stop_record` → `stop_sending`)
for `shutdown`, SIGINT, SIGTERM and a restart alike. These now read it:

- `Core::continue_execution` (rpc/driver.rs) begins nothing once the stop has begun: `Ok(None)`. The execution
  stays queued, and the next start's driver takes it.
- The driver's loop is `while !core.outbox.stopping()` (harness.rs), so a stop's wake missed while busy still ends
  it. A net +1 line, to stay under clippy's 100-line limit on `drive`.
- A call about to be sent (`call_model`, before the stream is first polled; turn.rs gets one 3-line call) is
  settled failed **unsent**: `ProviderError::Network { "not sent: the daemon's stop began before the call went
  out" }`, through the existing `settle_failed`. That is exactly how the passing runs already settled it (a refused
  connection, `network`, transient). So the run says backoff, the execution is woken, and the next start retries
  it, with no request. The logic is in the new `turn/stopping_step.rs`.
- No in-turn retry while stopping (`turn/retry_step.rs`): with the bench profile's `transient = 4`, a stopping
  daemon would otherwise wait out four backoffs and settle four unsent calls.
- `tests_stopping.rs` (new, theseus-core), both proved by order (the stop begins, then the driver or turn asks):
  - `the_driver_begins_no_retry_once_the_stop_has_begun`: a first-byte timeout, `transient = 0`; the execution is
    queued; `stopping_on`; `continue_execution` is `None`, the provider saw 1 request, and the execution stays
    queued.
  - `a_call_asked_after_the_stop_began_is_not_sent`: `stopping_on`, then a turn with `transient = 4`; it fails
    `network`/transient with "not sent", 0 requests, one `provider.error` row (no in-turn retry), `turn.next`
    backoff, and the execution queued and resume-pending.
- The first-byte test's count assertion now prints, when wrong, each request the stand-in saw: its arrival after
  the first, its unix wall time (to set beside the daemon's log), the client's port, its body, and `run.stderr`
  (the daemon's log). `FakeModel` gains `peers()`, the client's port per request. That is a field and an accessor,
  nothing restructured.
- crates/theseus-core/AGENTS.md: one sentence under "The turn".

### How I proved it

- Before, under load (nice 19, four nice-0 busy loops, the test's binary built once, `--exact`): **30 runs, 0
  failures**, 10.7–13.2 s each. It would not fail here, so the probe above is the reproduction.
- After, under load: **30 runs, 0 failures**, 11.0–13.6 s each.
- The probe again, on the fix: the continuation's call fails with `network error: not sent: the daemon's stop began
  before the call went out`, and the stand-in saw 1 request.
- Planted reverts (each restored, `touch`ed, `git status` clean of it):
  - A, `continue_execution`'s check removed: `the_driver_begins_no_retry…` fails (the continuation ran: `unwrap`
    on its error, tests_stopping.rs:135).
  - B, the call step's check removed: `a_call_asked_after_the_stop_began…` fails (the turn was answered: `expect_err`
    at :157).
  - C, the in-turn retry's check removed: the same test fails with `a retry inside the turn`, left 5, right 1.
  - The first-byte test under load with the fix reverted: not shown failing, since it never failed here even
    before. The core tests carry the order proof.
- bench_profile.rs whole, 5 times under load: 6/6 each run, 11.8–13.5 s.

### Answers the brief asks for

- **Would daemon-stops' `shutdown` fix alone close it?** No. The continuation begins at the failed turn's end,
  before the `shutdown` arrives. A `--stdio` daemon that stops at once on `shutdown` only shortens the window
  between the stop and the runtime's end, and a call that connects inside it still goes out. Only a check at the
  call closes it.
- **What a real model call during a stop costs a bench trial:** a whole request with the trial's context, billed
  for its input (and any output streamed before the runtime's end cuts it), whose answer nobody reads. Nothing in
  the stdio store accounts for it. With in-turn retries on, the stop could also have sent more of them. It did not
  delay the stop: the runtime's drop cancels the future.

### Left, and choices for the owner

- **A residual window, before any stop.** If the client is slower to send `shutdown` than the daemon is to reach
  the continuation's call (about 20 ms of daemon CPU after the answer, unloaded), the retry is sent before the stop
  begins. That is a legitimate driver retry, and the test would see 2. That ordering can't be ruled out without
  changing theseus-ljr. Two options to choose from:
  - (1) The driver's first retry of a transient failure waits the backoff too (today it is immediate, and
    `tests/failures.rs` pins that).
  - (2) A `--stdio` daemon leaves a failed client turn for its client.
  turn/retry_step.rs's doc ("a headless run … gets no later retry") holds by that margin today.
- **The class is `network`,** with a message that says "not sent". A class of its own (`stopping`) needs a
  `ProviderError` variant, which is serialized. I kept to the existing class, which is what these calls recorded
  before. Such a turn still counts as a failed turn (`count_failed_turn`) during the stop.

## 2. theseus-y0lm: the stop test's empty `job-<i>.term`

### What I found

A third cause, neither (i) nor (ii): **the wrapper's own stop SIGTERMs the trap's `date`.**

- `tree::stop` (theseus-kernel tree.rs, phase 1) scans the tree every 10 ms through the grace and SIGTERMs every
  process it has not met yet, including ones born after the first signal.
- The trap `date +%s%N >> job-<i>.term` forks a child. Dash opens the `>>` in that child, then execs `date`. A
  `date` that is alive at one of those scans dies of SIGTERM, which leaves the file empty (or missing). That
  happens 0 to 2 s into the grace, long before any SIGKILL, so the failing run lasts as long as a pass. That is
  what both reds showed. Under load, the fork, exec and run span a scan more often.
- Direct proof: a scratch trap `/bin/sleep 0.5; echo $? > job-<i>.slept; date … >> job-<i>.term` gave `.slept` =
  **143** (SIGTERM) in all three jobs, with the stop's `took` 2.015 s.
- (i) and (ii) were not seen. Every verdict was the wrapper's own (`termination_verified`), and `took` was about 2 s.
  No kernel change is needed or proposed.

### What I changed (`859b09d5`)

- The job is `bash -c` now. Its trap takes the time in the shell with no fork (`t=$EPOCHREALTIME`, written in µs),
  ignores SIGTERM from then on (`trap "" TERM`, so anything it forks inherits the ignored signal), and writes
  `job-<i>.term.tmp` with builtins, then `mv`s it to `job-<i>.term`, so the file is whole before it is visible. The
  job is just as stubborn: only SIGKILL ends it.
- On a missing or empty file the test panics with `stop_seen`: each job's files, every row of the stop's
  `action.list` (`cancel`, the verdict), the stop's answer and `took`, and theseusd.log's lines for the stop.
- **What it proves now:** the same order (theseus-3dsz). Every job's SIGTERM, timed as its trap began and with no
  fork, came within one grace of the others. A trap slow to *write* still proves the order. A job with no time at
  all fails, with what tells (i), (ii) and this cause apart.
- crates/theseusd/AGENTS.md: a trap entry (a test trap must not fork what it records).

### How I proved it

- Before, under load (the binary with a `|| echo $? >> job-<i>.date` witness): **30 runs, 0 failures**, 11.0–15.4 s.
  Then at 12 busy loops (3 per core, past the recipe), 26 runs: **0** on the SIGTERM file, **17** on the health
  bound ("health waited on the stop", stops.rs's `slowest < 1500 ms`, single answers of 1.5–2.9 s), and 1 that saw
  no three jobs within 40 s. Runs 2–5 of those overlapped a full disk and a compile here. I stopped that run at 26
  (the daemon binary was rebuilt under it).
- With the new trap and the same planted `/bin/sleep 0.5` before its `mv`: `.slept` = **0** in all three jobs, and
  `.term` was whole.
- After, under load: **30 runs, 0 failures**, 10.4–11.8 s.
- Plant: `terminate_all` (core cancel.rs, cancel-fast's file, restored and `touch`ed) stops each job only after the
  last one's verdict. The test fails on its spread (stops.rs:295, "one grace, not three").
- stops.rs whole, 5 times under load: 1/1 each run, 10.8–11.9 s.

### Left

- **The health bound is a finding.** At 3 busy loops per core (the daemon at nice 19), health's answer during the
  stop passed 1.5 s in 17 of 26 runs. I did not show whether health waited on the stop or on the scheduler. At the
  recipe's one loop per core it never failed (60 runs, before and after). On a 16-core machine the owner's load
  25–33 is about 2 per core.

## The live check (the maintainer's, on the 16-core machine)

Build each tree once (`cargo build -p theseusd --tests --bins && cargo build -p theseus --bins`). Then, for each of
main and this branch, with `B` the newest `target/debug/deps/bench_profile-*` and `T` the newest
`target/debug/deps/stops-*` (not `.d`):

```bash
pids=(); for i in $(seq "$(nproc)"); do sh -c 'while :; do :; done' & pids+=($!); done
f=0; for i in $(seq 100); do nice -n 19 "$B" --exact a_first_byte_timeout_is_retried_inside_the_headless_turn >/tmp/jtrc-$i.log 2>&1 || { f=$((f+1)); echo "jtrc run $i failed"; }; done; echo "jtrc failed $f of 100"
f=0; for i in $(seq 100); do nice -n 19 "$T" --exact a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker >/tmp/y0lm-$i.log 2>&1 || { f=$((f+1)); echo "y0lm run $i failed"; }; done; echo "y0lm failed $f of 100"
kill "${pids[@]}"
```

- **Main** should fail as the issues say: jtrc with `2` requests, and y0lm with `job <i> had no SIGTERM`.
- **This branch** should never fail.
  - A jtrc failure prints each request (`request 1: +… ms (unix …), port …`). Set its unix time beside the daemon's
    `stop phase="began"` (run with `THESEUS_LOG=info,theseus_core::startup=debug`). A request *before* the stop
    began is the residual window above. One *after* it is a bug in this fix.
  - A y0lm failure prints `stop_seen`: `took` near 5 s with an uncertain verdict is (ii), and a whole `.tmp` with no
    `.term` is a starved `mv`.
- Then `scripts/gate.sh --retries 0`, if the gate takes it (else `cargo nextest run --workspace --retries 0`).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the committed tree (both commits; the gate ran once,
on the tree that became both):

- fmt, shape, features, clippy, cockpit and test build pass. The suite ran 3,028 tests: 2,994 passed, 34 failed, 24
  skipped.
  - 33 are the known L1 failures (theseus-sandbox's contract tests and `spawn_100`, theseusd's sandbox tests;
    theseus-pv6i, a root daemon).
  - 1 is `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy` (theseus-1g8j‡,
    known). It passed rerun alone.
- After the suite, run by hand: protocol types ok (no protocol change); no crate compiled under the lock; `theseus-sim
  bench turn --check --runs 5 --burst 0` ok (5 and 9 frames); `cargo deny --offline check` ok (advisories, bans,
  licenses, sources). The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.
- Two earlier gate runs failed, and both are explained:
  - clippy's `too_many_lines` on `drive` (fixed before the commit);
  - a suite whose 11 job tests met the disk floor ("1,009 MB free, below the floor of 1,024 MB"): this VM's disk
    allowance, filled by build artifacts, which I deleted.
- Sub-step greenness: the two commits touch disjoint files. The gate ran on both together, not on the first alone.

## Docs the maintainer may want to change

- Part III and docs/status.md: the stop's mark now gates continuations and model calls (theseus-jtrc), and the stop
  test's trap (theseus-y0lm).
- The spec's text on theseus-ljr's driver retry: say that the first retry is immediate and that a stopping daemon
  begins none.
