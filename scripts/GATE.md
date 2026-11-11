# The gate on a busy machine: the disk floor, the lock, the busy allowance, and how long it takes

How to run `scripts/gate.sh` on a machine that is doing other work, and what it does when the machine
cannot give it what it needs. **Every section below was moved here verbatim from `scripts/AGENTS.md` on
2026-11-10** (theseus-4vsrz, B7), in the same relocation that made `scripts/RELEASES.md`: that file is a
`Level::Project` context file with a 64 KiB ceiling (`context_files::MAX_BYTES`), it had passed the ceiling
once already, and merged onto the main of the moment it was **12 bytes OVER** it -- 65,548 against 65,536,
the gate's own `context ceiling` phase at EXIT=1 -- so this is the answer that phase's message asks for:
move depth into a sibling document that no session loads. **Not one
byte was changed, dropped or reworded in the move** -- the five sections are byte-identical to the ones
`scripts/AGENTS.md` carried, and `scripts/context-ceiling.py` holds the ceiling so a breach cannot recur
unseen.

What stayed in `scripts/AGENTS.md`: the script list, the gate's phases in order, the keel guard, the shape
budget, the flaky list, the daily deny job, and the test clock. What is here is the operational half -- the
answers you need when the gate refuses, waits, or takes longer than you expected.

This file is not an `AGENTS.md` or a `CLAUDE.md`, so no session loads it as project context and it has no
ceiling of its own. Read it before running a gate beside other work, and when one refuses or stalls.

### A disk too full to build (THESEUS_GATE_MIN_FREE_GB)

Before its first compile the gate reads the free space of the filesystem that holds `target` and refuses to start below
a floor, so a build is not begun on a disk it would fill (`disk_floor` in `gate.sh`).

- **The floor is 10 GB**, from `THESEUS_GATE_MIN_FREE_GB` (default 10). It is a whole number of GB: anything else exits 2
  with `it is a whole number of GB`. `0` turns the floor off, and the log says so.
- **Below the floor the gate exits with status 73**, its own (nextest's is 100, the gate's 1 and 2, a moved tree's 75).
  The log says `gate: REFUSED: only <n> GB free on <where>, below the disk floor of <floor> GB (exit status 73)`. A
  `df` it cannot read refuses with 73 too: a gate that cannot show it has room has not shown it.
- **It measures the filesystem holding `target`**, following a symlink; a lane that has not built yet has no `target`, so
  the nearest directory that exists stands in for the disk it will be made on. It runs after fmt and before the first
  phase that resolves or compiles.
- **It deletes nothing.** Pruning is the lane tooling's, and a gate that clears caches changes the tree it judges. Free
  space yourself and rerun, or run on purpose with a smaller whole number or `0`.

### The lock

(theseus-rx91, theseus-lew7.) The suite's timing-sensitive tests and the benches need the machine to themselves, so one
gate's tests never land in another's bench: they run under a machine-wide lock, `~/.cache/theseus-gate.lock`
(`$THESEUS_GATE_LOCK_FILE`). The gate takes it itself, only around the locked part (step 2 above), and every gate runs
this way, the chain's join gate on `main` included.

- **Why.** Until 2026-10-02 the caller held the lock for the whole gate, so every gate's `fmt`, `clippy`, and test
  build (minutes) kept every other gate waiting too: that morning six lanes' gates and the chain's join gates queued
  behind each other's compiles, and three join gates lost about 25 minutes. The gate then took the lock itself in an
  inner mode, and the old way stayed as the default outer mode, which the chain's join gate used under
  `theseus-quiet.sh` (it paused the lanes' compilers while the gate ran, and its paused processes wedged gates more than
  once: theseus-xfr1, theseus-e6xj). On 2026-10-03 the outer mode went (cut-list Tier 5.3): one mode, everywhere.
- **`THESEUS_GATE_LOCK`.** `inner` is accepted and changes nothing; `outer` is refused (exit 2), as is any other
  value. A worktree cut before theseus-lew7 still has the two-mode gate, whose default, outer, takes no lock unless its
  caller does: give that gate `THESEUS_GATE_LOCK=inner` until the worktree rebases.
- **Never wrap the gate in `flock`, or in `theseus-quiet.sh`.** The wrapper would hold the lock that the gate then waits
  for, and the gate would wait for itself forever. The gate checks: it refuses to start (exit 2, in a second) when a
  process above it holds the lock.

How the gate takes the lock: after its compile phases the gate runs itself again, as `flock -o LOCK scripts/gate.sh
--locked-part …` (an internal option, never a caller's). `-o` closes the lock's fd before that part starts, so nothing
its tests start (a daemon that outlives its test) can keep the lock past the part (theseus-e6xj), and no function of the
gate ever holds a lock fd. While the part runs, the one process with the lock open is its `flock`; once the gate ends,
none is (the proof: scan `/proc/*/fd`). The part hands its phase times back in a file, so one table covers both halves,
and its exit status is the gate's.

The log says what queued behind what, and for how long, and the table has a `lock wait` row after `test build`:

```text
gate: the compiles are done; taking the shared gate lock (~/.cache/theseus-gate.lock) for the tests and benches
gate: waiting, the lock is held by:
  pid 2943171, in ~/projects/theseus-wt/<another lane>
gate: queued for it ahead of this gate, in order:
  pid 2951376, in ~/projects/theseus-wt/<a third lane>
gate: lock taken after waiting 19 s
…the locked part's output…
gate: lock released after holding it 128 s
```

The wait is the one cost the gate cannot shorten, so read it first when a gate is slow. To stop a gate, stop its
process group (`timeout` and the harness do): a signal to the gate's own pid ends it at once, but its locked part runs
on to the end under the lock, and a signal to its `flock` alone frees the lock under a running part.

### The busy allowance

(theseus-lew7; the owner's "business wiggle room", 2026-10-03.) The chain's join gate no longer pauses the lanes' compilers,
so its benches can measure a busy machine, and a busy machine slows the daemon's starts and stops. There, and only
there, the timing budgets get an overage allowance.

- **When.** Only when the settle step before the bench waited its whole 2 minutes without a quiet window (IO pressure
  under 10 %, CPU pressure under 20 %, and the load under three quarters of the cores, 12 of 16, all at once). A quiet
  window, a machine without PSI, or `THESEUS_GATE_BENCH_ALLOWANCE=0` keeps every budget strict, as before. Each settle
  decides for the run after it, so a rerun after a miss decides again.
- **The bar and the wait** (the owner, 2026-10-03 14:20; theseus-lf1n). Until then the load bar was the core count and the
  wait 5 minutes. With the lanes no longer paused, the strict misses cluster at loads of 12 to 16 (at normal priority,
  a phase missed in 41 % of the runs at 12 or more, 10 % from 8 to 12, 6 % under 8), a band the old bar called quiet,
  so a second miss there failed a join. Now a gate there waits, then benches with the allowance, which was calibrated
  on that band; and the shorter wait keeps a busy machine from holding the shared lock for 5 minutes.
- **What.** The timing budgets only: the lifecycle bench's phases, and the jobs bench's L1 start. A phase over its
  limit by no more than the allowance, a percentage of the limit, passes. A count never gets one: the turn bench's 5
  frames are held exactly. The run's other checks (the socket served before the secrets, the swap's job adopted, and
  the rest) are never excused.
- **How much.** `THESEUS_GATE_BENCH_ALLOWANCE`, a whole percentage, by default 65, from the bench history on
  2026-10-03. It had 22 runs at a load of 12 or more at normal priority (the join gates' benches, and the lanes'
  benches run alone), the code otherwise healthy. In 21 of them every phase was within 63 % over its limit, and in all
  22 within 75 %. Left out: an IO storm's two runs (several phases at once over twice their limits), and four
  one-sample stalls over twice a limit (one fsync waiting on a neighbour), which no allowance should cover: the rerun
  does. Strictly, 9 of those 22 runs missed a phase, against 4 of the 68 runs on a quiet machine (a load under 8),
  which the rerun covers, as before.
- **A RUST TEST CANNOT REACH IT, so it is never the fix for a timing test in a crate** (theseus-4vsrz
  test-hygiene F3, 2026-11-10). `THESEUS_GATE_BENCH_ALLOWANCE` is read in exactly one place — `gate.sh:67`, into a
  shell variable the gate passes to `theseus-sim bench` as `--allowance` — and it appears in **no `.rs` file in
  the workspace**: `grep -rn THESEUS_GATE_BENCH_ALLOWANCE --include=*.rs crates/` returns nothing. So it covers the
  lifecycle and jobs benches and nothing else, and a `#[test]` or `#[tokio::test]` asserting on a duration has no
  path to it at all. **Two checklist lines on `theseus-4vsrz` prescribed it as "the right fix" for exactly that,
  and both were written around a mechanism that does not exist.** Nothing was broken by the mistake — one of those
  lines needed no fix and the other used a retry — but a reader who believes the prescription will spend an
  afternoon looking for a knob. The remedies that do exist for a wall-clock bound inside a crate's tests are: a
  retry, `tokio::time::pause` where the clock is the subject, a bound wide enough to hold on a loaded machine with
  a comment saying what it was measured at, or moving the assertion into a bench the gate already times. Never a
  loosened bound without a reason, and never `#[ignore]`.
- **Seen.** The settle line says the allowance is in force (`lifecycle: still busy after 2 minutes (…); measuring
  anyway, with the busy allowance: +65% over a timing budget's limit`). A phase it carries prints the bench's own
  `MISSED`, the strict verdict, and then `lifecycle: busy: allowance +65% applied to clean shutdown (measured 112.0 ms,
  limit 104 ms)`. Its history row keeps `passed` as the strict verdict (`false`), and the allowance in its last column,
  `allowance`; `bench history` counts those runs ("N of them passed on the busy allowance") and marks each phase the
  allowance carried. A run of them is either a regression the busy machine hid, or a machine that stays busy: read
  them before raising the allowance.

### How long it takes

On a warm target, a few minutes: the suite is about 90 s and the bench about 7 s of it, plus any wait for a quiet
machine. A lane's niced gate on a warm target took 135 s once it had the lock (2026-10-01: the suite 105 s at 4
threads, a 15 s wait to settle, the bench 7 s). A fresh worktree's first gate also compiles the whole workspace,
dependencies at opt-level 2 included, which takes far longer: about 22 minutes at nice 19 beside busy neighbours.
That build runs without the lock, so it holds nobody up. Waiting for the lock behind another gate's tests,
and for a quiet machine, can each add minutes, so give the gate's command a timeout of 30 minutes or more (a
`proc.run` call can ask for up to its `proc_timeout_max_secs`).

### Running it beside other work

- **The lock** keeps one gate's tests out of another's timing bench (see "The lock"). Whoever takes
  it takes it with `flock -o`, which closes the lock's fd in the gate, so nothing it starts can keep the lock after it.
  Never take it with `exec N>lock; flock N` in a shell that starts anything long-lived.
- **A lane's gate runs niced**, with its own `CARGO_TARGET_DIR`, and `CARGO_BUILD_JOBS=4` and
  `NEXTEST_TEST_THREADS=4` exported (one or the other, never also nextest's `--test-threads`, which it then refuses
  as given twice): `THESEUS_GATE_NO_BENCH=1 nice -n 19 ionice -c3 scripts/gate.sh`, with no `flock`. The gate
  resolves where cargo builds itself (`resolve_target_dir`, on first use: `CARGO_TARGET_DIR`, else `cargo
  metadata`'s `target_directory`, else `$PWD/target`), so the benches and the bench-binary check find the
  binaries wherever the target dir is, and a worktree needs no `target` symlink for them. Until theseus-1eemp
  they read `target/debug` by a relative path, which is why every self build - whose tree builds into
  `<state>/self/target/<item>`, outside the checkout - failed `test build` before its tests ran.
  `scripts/gate.sh --bench-bins <cargo message file>` runs that check alone over a planted layout, and is
  what `bench/recall/test_bench_bins.py` drives; it builds nothing and takes no lock.
- **A lane's gate skips the bench** (`THESEUS_GATE_NO_BENCH=1`). The bench's settle step waits for a quiet machine
  while holding the shared lock, so every other agent's gate queued behind it; the join's gate on `main` benches
  instead. A lane whose work touches the start path runs `target/debug/theseus-sim bench lifecycle --runs 10
  --check` alone, at normal priority, once before its join, and quotes it.
- **A niced gate whose only miss is the bench**, beside busier neighbours, counts as green when the bench rerun alone
  at normal priority passes. Quote both runs.
- **A gate that sits at 0% CPU** is waiting on a lock: this one (held by another gate, or by an orphan that
  inherited it: scan `/proc/*/fd` for it, since `/proc/locks` hides a dead owner), or cargo's package cache. The gate
  names the holders in its log (`gate: waiting, the lock is held by:`, with each one's worktree). Find the holder
  before waiting longer, and never kill another agent's process.

## Making load for a measurement: `load-rig.py`

(theseus-1eemp, B5, 2026-11-11.) A builder's inline `for i in 1 2 3 4 5 6 7 8; do (while :; do :; done) & done; sleep 11;
...; exit 0` ended with its shell and left eight immortal busy loops, which held `/proc/pressure/cpu`'s `some avg10`
at 94 % for 11 h 48 m and so shut the self loop's start guard with nothing saying so. Load for a measurement or a test
starts with `python3 -I scripts/load-rig.py [--spinners N] [--cpus 0-3] [--nice N] [--settle S] -- COMMAND...`, and
nowhere else.

- **How it cannot orphan.** Each loop, and COMMAND, is a child that asked the kernel to kill it when the rig dies
  (`PR_SET_PDEATHSIG`, SIGKILL): that holds for a normal end, a crash and `kill -9`, which a `trap` and a process group
  do not survive. SIGTERM, SIGINT and SIGHUP run the same cleanup and reap every child. Each loop also stops itself at
  `--max-secs` (300), so a bug in the first two costs minutes. `scripts/tests/test_load_rig.py` holds each path: it finds
  survivors by a tag in the rig's environment (`/proc/<pid>/environ`), because `pgrep -f` and `ps | grep` also match the
  shell that asks.
- **What it cannot do.** It does not stop a COMMAND's own grandchildren: a COMMAND that forks must clean up after
  itself, or run under the rig's own `--max-secs`.
- **Reading the load.** System-wide, PSI's `some` is the share of time at least one task waited, averaged over the CPUs
  that were busy at all; it reads 94 % for loops pinned to four of sixteen cores and about 56 % at a load of 40 across
  all sixteen. `full` is a constant zero for the CPU system-wide. The self pipeline's CPU hold reads the five-minute load
  average a core instead (`crates/theseus-core/src/rsi/capacity.rs`, `CPU_LOAD_BUSY`).
