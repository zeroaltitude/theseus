# Cloud report: cloud/20261003-load-flakes

A 4-core VM, 15 GB, running as root. Started 20:00 UTC, 2026-10-03. "Under load" below means the recipe: the test at
`nice -n 19`, beside four `sh -c 'while :; do :; done'` loops at nice 0, killed by their pids (a script,
`load.sh`, ran the test binary directly, N times in a row). Where two loaded runs overlapped, the machine carried
eight loops and the two tests: that is said where it happened.

Commits, in order:

| Commit | Issue | Subject |
|---|---|---|
| `038bfdf` | theseus-46ya | spool: a completion the other consumer took reads as none, not ENOENT |
| `bcd44d7` | theseus-amr2 | push: the position-rule test waits until the board applied every frame |
| `edbb7ea` | theseus-3dsz | stops: the stop test proves one grace and a free worker by order, not stopwatches |
| `35fa942` | theseus-a2ec | config_copy: a start from the copy is proved to serve before the vault by order |
| `10b594d` | theseus-l21m | sim: the lifecycle bench's Discord binding binds, so its driver check checks something |
| `faa9447` | theseus-a2ec | config_copy: the changed-note start is held too, so its first answer is confirming |

No new dependencies; `Cargo.lock` and the package-lock files are unchanged. No doc, spec, or status edits.

## 1. theseus-46ya: a turn faults with a bare ENOENT (product race, fixed in the product)

**Found.** As the brief read it. `Spool::read_completion` did `exists()` then `fs::read(p)?`; the daemon's drain
(`Driver::drain_spool` over `Spool::drain`) accepts a completion and then removes its file. A removal between the
look and the read made `read_completion` fail with NotFound, which `ToolRuntime::job_settled` passed up with `?`,
faulting the turn. The mirror: `Spool::drain` reading a file `job_settled` had just removed counted it malformed,
tried a rename that failed silently, and warned about a move that never happened.

Checked that the `None` path settles the call: both consumers accept before they remove (`drain_spool`: accept,
then `spool.remove`; `job_settled`: `accept_completion`, then `remove`), so a file gone at the read means its
completion is already accepted, and `job_settled` then reads the settled action from `kernel.action`. A new core
test proves it on a real kernel and spool.

**Changed (`038bfdf`).** `read_completion` reads with no `exists()` and maps NotFound to `Ok(None)`. `drain` skips a
NotFound read (`continue`) instead of counting it malformed; any other read error is malformed as before. Each read
goes through a private seam (`read_completion_with`, `drain_with`, taking the read function) so a test can remove
the file at the read itself.

**Proved.**
- New tests, all passing: `theseus-kernel spool::tests::{a_completion_that_is_gone_reads_as_none,
  a_completion_the_drain_takes_at_the_read_reads_as_none, a_completion_a_turn_takes_during_the_drain_is_skipped}`
  and `theseus-core toolrun::job::tests::a_job_the_drain_settled_reads_as_settled` (a real kernel: an action
  planned and dispatched, not settled before the completion; its completion written, accepted and removed as the
  drain does it; `job_settled` then returns it `Succeeded`).
- Planted revert (`exists()` then `?` in the read, and NotFound counted malformed in the drain): the two seam tests
  fail, `a_completion_the_drain_takes_at_the_read_reads_as_none` on the unwrap of `Err(No such file or
  directory)`, and `a_completion_a_turn_takes_during_the_drain_is_skipped` with `malformed` 1, not 0. Restored,
  touched, `git status` clean.
- The burst test, `theseusd::reaping an_op_run_through_a_burst_of_jobs_keeps_its_exit_status`, 40 runs under load
  on the pre-fix build (the planted revert is the pre-fix code): **40 passed, 0 faults**. It did not reproduce this
  time; the deterministic seam test is the proof of the race.
- After the fix, 40 runs under load (beside the item 3 and 4 runs, so about eight loops): **39 passed**. The one
  failure was mine: at 21:04:36 the test's `Command::new(theseusd)` got NotFound (`tests/common/mod.rs:70`)
  while I was rebuilding the theseusd test targets, which re-placed `target/debug/theseusd`. No run logged a
  turn fault or an `internal` answer. Twenty more runs later, beside the config test's runs (eight loops):
  **20 passed**. In all, 59 of 60 after the fix, and the one failure not the race.

**Left.** The burst test never showed the fault on this VM (0 in 40 before), so the loaded runs show no regression
rather than the fix. The race itself is shown deterministically by the seam.

## 2. theseus-amr2: the snapshot and the events agree under the position rule (the test read too early)

**Found.** Reproduced: **2 of 20** loaded runs failed exactly as reported (`tests_push.rs:237`, `every turn ended;
left: "turn 1", right: "ready"`). The product is right and the test read too early. The kernel's observer hands
each frame to the board's queue as its commit returns, on the committing thread, so the board applies frames in
the order their commits returned, not by position. With sixteen turns committing at once, a frame of one turn at
a lower position can reach the queue after another turn's frame at the highest position. The test waited for the
board's position (a max) to reach the kernel's highest, which says the highest frame was applied, not the lower
one queued behind it, and then read a board still missing that session's last change. A watcher is not misled:
`session.wait` and a client both apply views per execution, by that execution's own position, and the change
arrives a moment later. Nothing in the product reads the board's position as a watermark.

**Changed (`bcd44d7`).** After every racing turn has returned, the test runs one more turn on one of its first
four sessions. Its frames are the last committed and the last queued, and its last frame is the highest, so once
the board's feed reaches that position every earlier frame has been applied. It waits on the feed
(`watch::Receiver::wait_for`, 5 s timeout), not a sleep-and-poll. The assertions are unchanged (20 sessions; the
client agrees with the board view by view; every turn ended).

**Proved.**
- After the fix: **20 of 20** under load.
- Planted revert, deterministic: the board stops publishing a change whose view is `ready` (`Push::apply`), i.e. a
  watcher never hears a session's last change. The fixed test failed **3 of 3**: `exe_…: left (747, "running",
  "turn 2"), right (753, "waiting", "ready")`. Restored, touched, clean.
- A first plant, the seed dropping the frames that queued while it read, did not fail the test in 10 runs: the
  seed is quick, and few frames queue during it. Not a proof; reported for completeness.

**Left.** No deterministic test of the interleaving itself (it would need a seam in the observer's send). The
board's `position` is a max, not a "everything up to here applied" mark; health's `push.position` and the
snapshot's `position` say so only implicitly. Worth a line in the push's docs if anyone ever reads it as a mark.

## 3. theseus-3dsz: three jobs that ignore SIGTERM, one grace, health free (two stopwatches)

**Found.** Reproduced, but first on the test's *other* stopwatch: the stop had to take under 3.5 s (one 2 s grace
and a margin). **14 of 20** loaded runs failed there (3.6 to 5.4 s; these runs overlapped another loaded run, so
about eight loops), before the health assertion was reached. The health bound (500 ms) is the reported failure.

**Changed (`edbb7ea`).**
- One grace, not three, is now proved by order. Each stubborn job's shell traps SIGTERM and writes when it came to
  `job-<i>.term`, instead of ignoring it; its `sleep`s die and its loop goes on, so only SIGKILL ends it, as
  before. The test asserts the three SIGTERMs came less than one grace (2 s) apart: stopped one after another,
  each next job would be signalled only after the last one's grace. The request must still take at least the
  grace.
- Health: the slowest answer during the stop must be under 1,500 ms (was 500). A health that waited on the stop
  would answer once, at about 1.8 s, and the test also requires at least five answers during the stop.
- Off nextest's flaky list.
- A first version compared the request's time with the sum of the verdicts' `ms` (overlapping stops take less than
  their sum). It failed 1 of 8 under load: the request took 5.9 s with verdicts of 2006, 1966 and 1866 ms; the
  time outside the stop windows is not bounded, and a verdict's `ms` is clocked by the wrapper. Dropped before
  committing.

**Proved.**
- After the fix: **20 of 20** under load (beside the item 1 runs, so about eight loops).
- Planted revert A, health waits while a stop is in flight (a counter around `execution_stop`, and `health_now`
  waiting on it): fails, `health was asked during the stop: [1.869060369s]` (one answer, at 1.87 s, which the
  1,500 ms bound also fails).
- Planted revert B, the jobs stopped one after another (`terminate_all` runs a `Stopping` per job, each to its
  end): fails, `one grace, not three: the jobs' SIGTERMs came 4.10521527s apart`.
- Both restored, touched, clean.

**Left.** None for this test. Note for the reviewer: the job script changed from `trap '' TERM` to a trap that
writes a file, so the shell now runs the trap and its `sleep` dies on the tree's SIGTERM; the job still outlives
the grace and is SIGKILLed, and the actions still settle `cancelled` / `termination_verified`.

## 4. theseus-a2ec: the copy serves the next start (stopwatch replaced by order)

**Found.** Reproduced: **20 of 20** loaded runs failed at the `took < OP_MS` assertion (620 ms to 1.4 s).

**Changed (`35fa942`).** The fake `op` also waits while a `hold` file exists. The test creates it before the
start from the copy, takes health's first answer, then removes it: the vault could not have answered first. The
answer must still say `confirming` and `started_from = copy`, then `confirmed`. The first start's lower bound
(`took >= OP_MS`: it read the vault first) is unchanged. Off nextest's flaky list.

**Proved.**
- After the fix: the order assertion held in all **52** loaded runs (20, 12, then 20 beside the burst test).
- Planted revert, the start from the copy awaits the vault's read before serving (`theseusd` main, `Start::Copy`):
  the start stalled on the held vault until the daemon's own 10 s `op` deadline, then served with the config
  `held`; the test failed at `config_copy.rs:244` (`left: "held"`, expected `confirming`). Restored, touched,
  clean.

**A second race of the same kind, fixed in a follow-up commit (`faa9447`).** Two of the last 20 of those runs
(beside the burst test, so about eight loops) failed at `config_copy.rs:314`: after the note changes, the start
from the old copy must first answer `confirming`, and the 500 ms vault had answered first (`left: "restarting"`).
The vault is now held over that start too and released once health has answered. With both holds: **20 of 20**
under load (the environment as before, `RUST_BACKTRACE=1`).

**Left: a "theseusd did not stop", probably this VM's environment, not fixed.** In the first 20 loaded runs after
`35fa942`, 4 failed later in the test at `r.stop` (`config_copy.rs:152`, "theseusd did not stop" within 10 s),
after the start from the copy (line 254, three times) or the comment-only start (line 360, once). Each time the
daemon's log tail (the last 30 lines, all the rig prints) was a backtrace through `theseus_core::harness::drive`.
This shell has `RUST_BACKTRACE=1`, which the tests and the daemons inherit: `anyhow` then captures a backtrace for
every error, and an error logged with its backtrace in a debug build, under load, is slow enough to plausibly push
a shutdown past 10 s; the tail looks like such a log line, not a panic (no panic message was in the 30 lines).
With `RUST_BACKTRACE=0`, 32 loaded runs (12, then 20 beside the burst test) had no "did not stop". Not proved: a
capture of the whole log in one of those failures would settle it. If it is seen on the owner's machine, the rig
printing the whole log would show the error and whether it is a panic.

## 5. theseus-l21m: the lifecycle bench's bindings, and a driver check that checked nothing

**Found.** As the brief read it: `BENCH_BINDINGS`' `guild_id = "1"` and `user = "2"` fail the binding's load, so
the binding never waited for its token and `driver_before_token` passed whatever the driver did.

**Changed (`10b594d`), in the rig and its check only** (`crates/theseus-sim/src/lifecycle.rs`; nothing in
`print`, history, or `scripts/gate.sh`):
- `BENCH_BINDINGS`: invented ids of Discord's length; the guild is `fake_discord::DEFAULT_GUILD`, which the fake
  gateway's READY names.
- With the bench's own config, `run` starts `FakeDiscord::start_with_gateway()` and `on_fake_discord` points
  `[discord] rest_proxy` and `gateway_proxy` at it (127.0.0.1). A run with an operator's `--config` is unchanged.
  The `inflight` phase keeps its own fake as before.
- Each cold start, and the shutdown phase's first start, waits until the binding has bound its DM
  (`bindings[0].places[0].session_id`) and reads when its token resolved: the end of its `discord.token` startup
  phase, by the daemon's clock (`Start::token_ms`, serialized only when present). A binding that does not bind
  within `resolver_ms` + 30 s fails the bench with its state and the log's tail.
- `driver_before_token` also requires each such start's token at or after `resolver_ms` (the binding waited) and
  the driver before the token.
- A unit test: the bindings' ids are 15 to 21 digits, the guild is the fake's, and the config's REST and gateway
  are the fake's.

**Proved.**
- `theseus-sim bench lifecycle --runs 10` (the gate's command, without `--check` and `--record`): every cold start's
  binding bound; tokens resolved at 1,010 to 1,022 ms after the process began (resolver 1,000 ms); the driver
  started at 12 to 28 ms; `driver_before_token` true. The daemon's log: `discord state="waiting" … waiting for the
  secret discord_bot_token to resolve`, `continuation driver parked`, about a second later `resolved
  secret=discord_bot_token`, `discord state="connecting"`, `discord slash commands registered`; no "not a Discord
  id".
- Planted revert, the driver awaits the binding (in `harness::drive`, before the driver's phase: wait until every
  binding is neither `waiting` nor `connecting`): with this branch's rig, `--phases cold --runs 3`: driver at
  2,018 to 2,026 ms, tokens at 1,008 ms, `driver_before_token` **false** ("a driver WAITED past the binding's
  token"). The same plant with the rig before this change (the old bindings): driver at 13 to 14 ms, no token,
  `driver_before_token` **true**: the vacuous check. Restored, touched, clean.
- That bench run's budget verdicts missed `shutdown` (p95 209 ms) and `kill` (p95 239 ms) on this VM, which ran other
  loaded tests at the time; the budgets are the owner's machine's, and were not judged here.

**Left.** The rig adds up to about one `resolver_ms` (1 s) to each cold run and to the shutdown phase's first start
while it waits for the binding to bind: about 11 s on a bench of 10 runs. The shutdown, kill and swap phases now
stop a daemon whose binding is connected to the fake gateway (as the operator's is), so their timings may move on
the owner's machine; worth one bench run there at review.

## The gate

`THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `10b594d` (21:35 UTC), and again on the tree of
`faa9447` (22:03 UTC) with the same result. `cargo deny fetch` had succeeded at setup, so the deny phase ran
offline with advisories.

- fmt, shape (no file over 2,500 lines but the 9 listed; `lifecycle.rs` is now 1,992), clippy (`--workspace
  --all-targets -D warnings`), the reader rule's registry test (9 of 9): passed.
- The suite: **1,692 run, 1,690 passed, 2 failed**, both on the brief's known list and nothing else:
  - `theseus-core tests_output::the_cores_output_matches_its_golden`, all three tries (theseus-6a7o, the byte the
    elapsed time prints);
  - `theseus-sandbox::contract clause_09_limits` (theseus-pv6i, root is exempt from `RLIMIT_NPROC`).
  No test needed a retry to pass.
- The phases after the suite, run by hand as `machine_checks` and the rest of the gate run them: the protocol
  types (no diff in `web/src/protocol.gen`), lifecycle and jobs benches (skipped, `THESEUS_GATE_NO_BENCH`), the turn
  bench (`frames_plain: 5 frame(s) at the p95, budget 5: ok`), `cargo deny --offline check` (advisories, bans,
  licenses, sources ok), web lint and build, cockpit lint, test (5 of 5) and build, and the web dist (unchanged):
  all passed.

So the commit counts green by the brief's rule. Each commit before the last was checked with fmt, the workspace's
clippy, and its own tests as it landed, not with a whole gate of its own: on this VM the after-runs held the
machine, and the gate at `10b594d` covers the first five; `faa9447` had its own. (Running clippy on `theseus-core` alone, `-p theseus-core`,
reports five unfulfilled lint expectations in files this branch does not touch; the workspace clippy the gate runs
is clean.)

## Docs the maintainer may want to change

- `.config/nextest.toml` lost the theseus-3dsz and theseus-a2ec entries; `scripts/AGENTS.md`'s "The flaky list" names
  no test by name that I found, so nothing there.
- `crates/theseus-sim/AGENTS.md` (if it describes the bench's rig): the bench's binding now binds at an in-process fake
  Discord with a gateway, and the cold check reads the `discord.token` phase.
- Part III / `docs/status.md`: the five items above, at review.
- Possibly an issue for item 4's "theseusd did not stop" under load with `RUST_BACKTRACE=1`, if it is ever seen
  without it.
- `crates/theseus-sim/src/lifecycle.rs`: `NOWHERE`'s comment still calls it "the bench's Discord REST and gateway";
  since `10b594d` it is that only in `bench_config`'s output before `on_fake_discord` (and for the turn bench, and
  the inflight phase's gateway). A one-line comment fix I left out to keep the commits to their issues.
