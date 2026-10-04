# Cloud report: five gate flakes (theseus-81kk, -lgtj, -qh0u, -1m3s, -dsmp)

Branch `cloud/20261004-gate-flakes`, cut from `d9b0931`. Started 2026-10-04 03:00 UTC. One commit per flake, then
this report (drop it at the merge).

The VM: 4 cores, 15 GB, root, UTC. "Load" below is the preamble's recipe unless it says otherwise: the test at
`nice -n 19`, four `sh -c 'while :; do :; done'` loops at nice 0, killed by their pids. I ran the test binaries
directly (`<binary> --exact <test>`), not `cargo nextest`: at nice 19 beside the loops, cargo's own start took about
two minutes a run. The scripts: a loop that runs one test N times and keeps each failure's output.

| flake | commit | reproduced on base | after the fix, under load | planted revert |
|---|---|---|---|---|
| theseus-lgtj | `b4800a5` | not at load (260 runs); yes by a planted slow append | 50/50 | caught (2) |
| theseus-qh0u | `70e101d` | yes, 4 of 6 runs at 32 loops | 20/20 at 4 loops; 10/10 at 32 loops (load 33) | caught |
| theseus-dsmp | `cc3d2ef` | n/a (a planted revert is the case) | 20/20 each of the two property tests | caught in 0.15 to 0.41 s, ~130 MB |
| theseus-81kk | `54c5d6a` | not from outside (40 runs, half with a slowed `op`); yes in a unit test | unit 30/30; both versions tests 20/20 | caught |
| theseus-1m3s | `7093b64` | no (20 runs at the recipe; see its section) | diagnostics only | n/a |

## theseus-lgtj: the flush test on the paused clock (`b4800a5`)

**Found.** `webui::tests::a_flush_writes_every_held_row_in_one_frame` holds six refusals in a 300 ms span on the
wall clock. When the six appends take longer than the span, a later refusal finds its kind's span over and is
written at once (`Refusals::note`'s `_ => Some(r.take())`), not held. The timer task cannot run first: the test
does not await between the refusals. So the failure is "2 rows and 4 frames, not 1 and 2".

**Changed.** `#[tokio::test(start_paused = true)]`, as its two siblings (theseus-56r7). `Refusals` already uses
`tokio::time::Instant`, so the clock stands still while the rows are written, and the closing 600 ms sleep advances it.

**Proved.**
- On base it did not fail here: 200 runs under load, and 60 more beside an fsync storm (`dd ... conv=fsync` in a
  loop), all passed. The appends on this VM's disk are fast.
- So I planted the slow disk: `std::thread::sleep(160 ms)` after each pair of refusals in the test. On the wall clock
  (base) it fails as the gate did: `assertion left == right failed: the first of each at once; left: (2, 4); right:
  (1, 2)`. On the paused clock (the fix) the same plant passes. (70 ms a refusal stays under the span and passes
  both ways.)
- A product revert: `flush` writing each held row in its own frame fails the fixed test: `the held rows are one
  frame; left: 4; right: 3`.
- After the fix, 50 of 50 under load.

## theseus-qh0u: the reads after a barrier meet at a rendezvous (`70e101d`)

**Found.** `tests_m3::parallel::a_write_and_a_program_are_barriers` asserted that the two reads after the program
(150 ms each) overlapped in wall-clock time. On a loaded machine one ran before the other started.

**Changed.** `Timing::rendezvous_after(skip, n)`: the next `skip` runs go on alone and the `n` after them wait for
each other once started, with theseus-i1i4's 4 s give-up. `rendezvous(n)` is `rendezvous_after(0, n)`, so the
seven-calls test is unchanged. The barrier test sets `rendezvous_after(2, 2)` before its second turn: the read of x
and the program run alone, and y and z cannot end before both have started. The overlap assertion stays, and now
holds by construction for a parallel dispatch. A serial dispatch never brings the second read to the rendezvous, so
the first gives up its wait and ends before the second starts, and the assertion fails. +20 lines in tests_m3.rs
(7,695 against its 8,050 ceiling).

**Proved.**
- Base: 30 runs at the recipe passed. At 32 busy loops, 4 of the first 6 runs failed: `the reads after it ran
  together` (tests_m3.rs:6366). I stopped it there. That run had an fsync loop beside it too; I had left the earlier
  `dd` loop running by mistake.
- Fixed: 20/20 at the recipe, and 10/10 at 32 loops (load 33).
- Planted revert: every call dispatched alone (`toolrun.rs:758`, the `ToolClass::Read` grouping made false) fails
  the fixed test: `the reads after it ran together`, in 5.3 s.

## theseus-dsmp: a split that stops moving panics at once under test (`cc3d2ef`)

**Found.** `within` abandons the split's thread on timeout, and the thread goes on pushing empty parts. Proptest
then runs and shrinks more cases, each leaving another such thread.

**Changed.** Under `#[cfg(test)]` only, `split_text` counts the turns of its loop and asserts it has made no more
than the text's length in bytes. Each turn moves the text on by at least a byte or ends the split, so a working
split never reaches the cap. The panic drops `within`'s sender, so `recv_timeout` returns at once and the property
fails ("split_text did not return"), as it did on a hang, only fast. A release build has no counter. I chose this
over a child process with an address-space limit: no new process plumbing, and it catches any loop that stops
moving, whether or not it allocates.

**Proved.**
- Planted revert of the progress guarantee (`if cut == 0 {` made false): all three split tests fail in 0.13 to
  0.21 s under nextest (`split_text stopped moving through its text`). Run alone, the two property tests took 0.41 s
  and 0.15 s, at a peak RSS of about 134 MB and 132 MB (rusage of the test process), against the >1.9 GB the issue saw.
- Fixed: 20/20 under load for each property test (`any_text_splits_and_keeps_its_earlier_cuts`,
  `a_long_fence_with_no_newline_still_splits_and_keeps_every_character`).

## theseus-81kk: no secrets row after the stop's last checkpoint (`54c5d6a`)

**Found.** The issue's reading is right: `Core::watch_secrets` appended `secrets.resolved` or `secrets.failed`
whenever the secrets settled, and never asked whether the daemon was stopping. If an `op` answers after
`Core::finish_stop`'s `checkpoint_for_close` and before the runtime drops the watcher, its row lands after the last
checkpoint and the next start replays it. The window is narrow: from that checkpoint to the runtime's end
(telemetry's flush, at most 1 s, when telemetry is on).

**Changed (product).**
- `Core` has a `closed: RwLock<bool>`. `finish_stop` calls `close_late_rows()` just before its last checkpoint,
  which takes it to write (through `theseus_store::blocking`, since it waits for an append in progress) and sets it.
- The watcher's two appends go through `Core::ledger_unless_closed`, which holds the lock to read across the append
  and, once closed, drops the row with a debug line. So the checkpoint and a late row cannot interleave: the row is
  either before the checkpoint or not written. A secrets outcome is not lost: the next start resolves and ledgers
  its own.
- The smaller `watch_secrets` no longer needs its `#[expect(clippy::cognitive_complexity)]`. Clippy failed the first
  gate on the unfulfilled expectation, so I removed it.
- I gated only the watcher. Other rows written on their own time after serving can race the stop in the same way:
  the driver's `driver.started`, and the index tender's rows. I did not see those race here. They could use
  `ledger_unless_closed` too, but `harness.rs` and the tender are other changes' areas. I did not make the store
  refuse every append after its close checkpoint: that would also drop kernel records such as a job's completion.
  For the owner: is "drop late rows" the right rule for every such writer?

**Changed (test).** The versions tests wait, before they stop, for everything a start writes on its own time
(`Rig::settled`): the history check, `driver.started`, and a `secrets.*` row, all after the last `server.stopping`.
That is the previous run's stop, or the fixture's: the fixture's WAL holds three `server.stopping` rows. A secrets
row can come before `server.started` when the vault answers first; my first version anchored on `server.started`
and timed out, so it anchors on the stop. The SIGTERM/SIGINT test shares the helper in place of its own driver wait.

**Proved.**
- It did not reproduce from outside: 20 runs under load with the fixture's instant `op`, then 20 more with the fake
  `op` sleeping 0 to 0.9 s before it answered (a planted test change, reverted), all passed. The window is too
  narrow to hit from outside.
- New unit test `rpc::tests::secrets_that_settle_after_the_stops_last_checkpoint_write_no_row`: a gated vault, the
  watcher spawned, `finish_stop`, then the vault opened. It waits for the watcher's `secrets` phase to end. The
  watcher writes its row in the same poll, on the test's current-thread runtime, so the row is written or dropped by
  then. It then asserts no `secrets.*` row and no frame since the stop. With the planted revert (the `closed` check
  made false) it fails: `a row after the last checkpoint: ["secrets.resolved"]`.
- Fixed: the unit test 30/30 under load. `a_clean_stop_closes_the_index_and_the_next_start_repairs_nothing` and
  `a_sigterm_or_a_sigint_stops_cleanly_and_the_next_start_replays_nothing` each 20/20 under load.

## theseus-1m3s: the reaping test prints what it forbids (`7093b64`)

**Found.** It did not reproduce: 20 runs of the base test at the recipe all passed (each about 30 s).
With the new diagnostics: 10 of 10 passed at 8 busy loops (load 10 to 12). At 32 busy loops (load 34) the first run
failed in a different way: the test's own 30 s wait for ten reaped wrappers ran out (`no ten wrappers reaped in 30
s`), because the machine was too slow, and no forbidden string appeared. I stopped that run there.

What the evidence already says: the test asserts that `op inject failed` appears exactly once before it checks the
forbidden strings. A retried injection that failed with `running op inject: <io error>` would log a second `op inject
failed; reading each reference…` line (`OpReader::fetch` logs one for every `InjectFailed::Error`). The count would
then have failed first. So in the failing run, the one failed injection was the first one, which the fake `op` fails
on purpose (`exit 1` before it reads its stdin), and the error was an io error, not op's `not yet`. It is also not
`ECHILD`: the loop checks `No child processes` first. That leaves the template's write or `wait_with_output` on an
`op` that has already exited. The broken pipe there is exempt (theseus-f6f5), so another error kind must have come
from the write, or from a read of the closed pipes. That points at the injection's own write path under load, not the
reaper. I can't name the error without the line, so nothing in the product changed.

**Changed.** The assertion prints each matching log line with the three lines before it, then the tail. A failure
now carries its evidence.

**Next.** On the owner's machine, run it under load 30 until it fails, and read the printed line. If it is an io
error from the write (an `op` that exited before reading its template, other than `EPIPE`), the fix belongs in
`OpReader::inject`'s write: treat any write error from an exited child like the broken pipe, and let its status and
stderr speak. If the error names a reaped or missing child, it is the reaper's, in `theseus_kernel::children`.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree (`gate2.log`): fmt, shape, clippy, bench build, test
build, and reader rule passed. The suite ran 1,752 tests: 1,719 passed, 33 failed, 10 skipped. None of the failures
is in a test this branch touches:
- 31 sandbox tests (`theseus-sandbox::contract` clauses 01 to 12 and egress, `theseus-sandbox::bench spawn_100`,
  and 15 of `theseusd::sandbox`) fail because the VM runs as root: "the daemon runs as root, and Linux exempts root
  from RLIMIT_NPROC" (theseus-pv6i).
- `theseus-core tests_output::the_cores_output_matches_its_golden` fails on the VM's timezone. The `wake.at`
  preview prints the local offset, and the golden has `-#:#` where UTC prints `+#:#`. Run with
  `TZ=America/Denver`, it passes. The golden is not hermetic about the timezone: worth an issue (pin `TZ` in the
  test, or redact the sign).

Then I ran the phases after the suite by hand, as the preamble says: protocol types unchanged; the turn bench at
5 frames, ok; `cargo deny --offline check`, ok (the database fetched at setup); the web and cockpit lint, test, and
build, ok; the web dist unchanged. Benches were skipped (`THESEUS_GATE_NO_BENCH=1`). Note that the first gate run
failed in clippy (the unfulfilled `expect` above). I fixed that before the commits, and the green gate ran on the
final tree before I split it into five commits. I did not rerun the gate at each intermediate commit.

## For the maintainer

- **The flaky list.** None of these five tests is on `.config/nextest.toml`'s list on this base, so nothing comes
  off it.
- **Live check.** The only behaviour change is the stop's (81kk). On a scratch daemon of this build (its own
  `--config`, `--socket`, `--state-dir`), with an `op` that takes 2 s to answer `inject`:
  ```sh
  theseusd --config <scratch>/config.toml --socket <scratch>/sock --state-dir <scratch>/state &
  sleep 0.5 && theseus --socket <scratch>/sock shutdown
  theseusd --config <scratch>/config.toml --socket <scratch>/sock --state-dir <scratch>/state &
  theseus --socket <scratch>/sock health | jq '.startup[] | select(.name=="store") | .detail'
  ```
  It should show `replayed_into_index: 0` and `index_repaired: false` after every stop, whenever the slow `op`
  answers. With `THESEUS_LOG=debug`, the log shows "the stop's last checkpoint is written: a row after it is dropped" when the
  answer came after the checkpoint. The window is narrow, so expect that line only sometimes.
- **Docs.** Part III could note the rule this establishes: a row written on its own time after serving goes through
  `Core::ledger_unless_closed`. The theseus-core `AGENTS.md` traps section is the place to say so, along with the
  driver and the index tender as the remaining writers.
- **Leftovers.** `Rig::settled` in versions.rs anchors on the last `server.stopping`, which assumes the fixture's
  WAL ends with a stop (it holds three).
