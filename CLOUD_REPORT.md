# Cloud report: timing-flakes (theseus-cs71, theseus-ynia, theseus-1n2y, theseus-qjd6)

Branch `cloud/20261005-timing-flakes`, from main at 60b43fb6 (plus the task commit 4ea44f54). Started 20:23 UTC,
commits pushed by 21:57 UTC. Tests only: no product code changed. Every plant was restored, `touch`ed and rebuilt,
and `git status` was clean afterwards.

How runs were made. "Under load" is the brief's recipe: the test binary, built once, run with the test's name and
`--exact` at `nice -n 19`, beside four `sh -c 'while :; do :; done'` loops at nice 0, killed by their pids. "Alone"
is the same with no loops. The scripts were `/tmp/runs/loop.sh` (one test, N runs) and `/tmp/runs/suite.sh` (a test
filter, N runs). Both are scratch files and are not committed.

## 1. theseus-cs71: a cancel's wall bound (b1dbc26e)

**Found.** On this VM it did not fail. Main's test passed 30 of 30 runs under the recipe (7.0 to 9.1 s each), and
32 of 32 in a heavier variant: four copies running at once under the same load, as a loaded suite runs them. With the
fix, every run prints its numbers. Under the recipe the round trip `took` 0.33 to 0.71 s, and the verdict's `ms` was
10 to 14 ms (26 runs at 10, 3 at 11, 1 at 14). So the stop itself is quick, and only load can push the wall time
past 1.5 s, as the issue says. No cancel read uncertain.

**Changed.** The test no longer asserts `took < 1500 ms`.
- It asserts `verdicts[0].ms < STOP_GRACE / 2` (1000 ms). Every process in this tree dies on SIGTERM. A stop that
  waits out its grace reads **1999 ms**, not 2000: the grace loop breaks once `Instant::now() >= until`, and
  `as_millis` truncates. That is why the first bound I tried, `ms < STOP_GRACE`, passed against the plant. Half the
  grace is about 70 times the largest `ms` seen under load, and still well clear of the plant's value.
- It keeps a loose wall bound: `took < STOP_GRACE + ANSWER_WAIT` (5 s). That is the daemon's own wait for its
  wrapper; past it, the cancel reads uncertain.
- It prints `cancel: took …, the stop's ms …` on every run.
- The constants come from `theseus_kernel::job`, where theseusd already depends on the kernel.

**Proved.**
- After the fix: 30 of 30 under the recipe. The test binary took 7.6 s minimum, 8.3 s median and 11.5 s maximum.
  `took` ran 0.33 to 0.56 s and `ms` 10 to 14. An earlier batch of 30 under the old 2000 ms bound also passed, with
  `took` up to 0.71 s.
- Planted revert, in `tree.rs`'s `stop_with`: the early return when the tree is empty was removed, so the stop waits
  out its whole grace. The test failed with `the tree emptied on SIGTERM, well within the grace: 1999 ms` and
  `took 4.01 s`. The loose wall bound alone would not have caught it (4.0 s < 5 s). The old 1.5 s bound would have.

## 2. theseus-ynia: sh's Ctrl-C test types ahead (ac4b747a)

**Found.** On main it failed **2 of 30 runs alone**, each at about 15.7 s. Under the recipe it failed 0 of 30: the
load changes the interleaving. Both failures show the same screen:
```
 4|ok> sleep 4242; echo after
 5|^Cecho back
 6|
 7|ok> back
 8|ok>
```
The tty echoed `echo back` before the shell's interrupt prompt, so `back` landed on the prompt's row, and
`"\nback\n"` never appeared.

**Changed.** After the existing wait for the `sleep` to die (kept, with its 10 s "did not interrupt" bound),
`ctrl_c_interrupts_a_command` reads until `"echo after\n^C\nok> "`. Only the prompt the interrupt draws, under the
tty's `^C`, puts that text on the screen. Then it types. Both assertions stay: `after` never ran, and the shell took
the next command (`\nback\n`). I chose this over the issue's other option, matching `back` wherever it lands,
because that option keeps typing ahead into a shell that has not drawn its prompt. The test would then accept a
screen it does not mean, and pass for the wrong reason. This follows theseus-y6zr's precedent: read until the
prompt.

**Proved.**
- Alone: 50 of 50.
- Under the recipe: 30 of 30, 1.27 s minimum, 1.45 s median, 1.63 s maximum.
- Planted revert, in `term/mod.rs`'s `Terms::send`: byte 0x03 was dropped, so Ctrl-C is sent as nothing. The test
  failed at `Ctrl-C did not interrupt the sleep` after 10 s. (`keys.rs` returning no bytes was tried first;
  `term.send` refuses an empty send, so the plant went where the bytes are written.)

## 3. theseus-1n2y: Python's REPL test (3981a621)

**Found.** On main it did not fail here: 30 of 30 alone and 30 of 30 under the recipe. The cause was found with a
pty probe instead (`/tmp/runs/probe.py`, a scratch file). It runs `python3 -q` (3.11), types
`while True: pass\r\r`, and sends Ctrl-C:
- with Ctrl-D in the same write as Ctrl-C, Python did not exit in 3 s and sat at a fresh `>>> `, in 2 runs of 2;
- with 1 s between Ctrl-C and Ctrl-D, it exited.

A Ctrl-D typed before Python's REPL reads its next line reaches it as nothing. The test sent Ctrl-D as soon as
`KeyboardInterrupt` showed, before the `>>> ` that follows it. When that loses the race, Python waits on, and the
5 s quiet read fails at about 5.9 s, the failure the suite saw.

**Changed.** Each key now goes at the state it is meant for, never after a sleep:
- `while True: pass` and one Enter, then a read until `"while True: pass\n... "`, then the second Enter;
- Ctrl-C once the loop runs, which the test reads as Python's time on a CPU rising by 20 ms (bounded at 30 s,
  "the loop never ran"). The time comes from `/proc/<pid>/schedstat`, not `stat`'s utime and stime: those are
  sampled ticks. A first version on ticks passed its check about 100 µs after the Enter in half the runs. A probe
  showed a process whose schedstat said 10.3 ms reading 0 ticks;
- Ctrl-D only after `"KeyboardInterrupt\n>>> "`, the fresh prompt.

With schedstat, the rise is seen at the first look after `term.send` returns, because the send waits for its screen.
A check that a block that ends at once fails: typing `if True: pass` instead fails at "the loop never ran" (a scratch
build, not committed).

**Proved.**
- Alone: 30 of 30.
- Under the recipe: 30 of 30 with the ticks version (0.68 s minimum, 0.95 s median, one passing run at 15.7 s, cause
  not seen). Then 30 of 30 with the final schedstat version and per-step timing printed: every `until` was met
  within 117 ms. The 15.7 s run did not recur.
- Planted revert (the same 0x03 drop): the test failed at `no "KeyboardInterrupt"`, its 15 s wait.

## 4. theseus-qjd6: a trace counted twice (9c98b540)

**Found.** On main it failed **30 of 30 under the recipe**, every time with `left: 4, right: 3`. Runs took 9.3 to
19.2 s under load, against 0.1 to 0.14 s alone, where it passed 10 of 10. At nice 19 beside a nice-0 loop on every
core, the test's runtime gets a sliver of a CPU. A post's answer comes after the tuning's 2 s timeout, the exporter
retries once, and the receiver records both.

**Changed.** The test now counts distinct `traceId`s among the spans posted to `/v1/traces` (`spans_of`) and expects
3: three turns, three traces, however often each was posted. I did not give the test a longer tuning timeout. Under
this recipe the test is 70 to 170 times slower than alone, so any timeout only moves the line the load has to cross.
Counting ids states what the test means. `telemetry/tests.rs` is now 2,491 lines, under the 2,500 limit for unlisted
files.

**Proved.**
- Alone: 30 of 30.
- Under the recipe: 30 of 30, 2.7 s minimum, 6.7 s median, 7.4 s maximum.
- Planted revert, in `export.rs`'s `trace`: the second half of each turn's spans got a second trace id. The test
  failed with 4 ids.

**Is the duplicate a bug?** It is the exporter's at-least-once delivery. A timed-out post may have arrived, and OTLP
does not deduplicate, so a backend shows that trace's spans twice. With the default `export_timeout_secs = 10` it
needs a receiver that answers after 10 s. I'd call it acceptable, not a bug. If the owner wants it gone, there are
two choices: retry only on a refused connection or a 429/5xx, never on a timeout after the body was sent (that drops
a trace that may have been lost), or accept duplicates. `export.rs` is unchanged.

## Suites under load (5 runs each, final build, recipe)

- `term::tests` (14 tests): 5 of 5 green, 1.8 to 2.6 s each.
- `job_approval` (4 tests): 5 of 5 green, 9 to 14 s each.
- `telemetry::tests` (that filter takes in 46 tests, `tests_resumed` among them): 4 of 5 green. Run 2 failed two
  tests that are not this task's, and the four fixed tests passed in it:
  - `telemetry::tests::a_hanging_receiver_never_slows_a_turn`: `slowest hand-over 152.146538ms`, a wall bound under
    load (tests.rs:1295).
  - `telemetry::tests_resumed::a_confirmed_call_and_a_background_job_are_counted_once_by_their_runs`:
    `theseus.tool.calls` `ok` was "1", expected "2" (tests_resumed.rs:236). The test runs a 0.4 s sleeper with
    `proc_sync_secs = 1`, and at nice 19 the first job likely outlasts its sync window and changes path. This is a
    count assertion, so I'm flagging it rather than calling it a flake. It is in telemetry3/telemetry-resumed's area,
    so I left it alone. The full log is kept on this VM at `/tmp/runs/keep-tel-run2.log`, which is lost when the
    session ends.

## Live check for the maintainer (on a 16-core machine)

Build once per tree (main, then this branch) and find the binaries:
```sh
cargo build --workspace --all-targets
CORE=$(ls -t target/debug/deps/theseus_core-* | grep -v '\.d$' | head -1)
JA=$(ls -t target/debug/deps/job_approval-* | grep -v '\.d$' | head -1)
```
1. Each of the four, 200 times at nice 19, beside one busy loop per core:
```sh
loops=(); for i in $(seq $(nproc)); do sh -c 'while :; do :; done' & loops+=($!); done
for t in term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one \
         term::tests::python3s_repl_computes_on_the_screen \
         telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is; do
  f=0; for i in $(seq 200); do TZ=America/Phoenix nice -n 19 "$CORE" "$t" --exact >/tmp/one.log 2>&1 || { f=$((f+1)); grep -A3 panicked /tmp/one.log; }; done; echo "$t: $f failed of 200"
done
f=0; for i in $(seq 200); do nice -n 19 "$JA" a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too --exact --nocapture >/tmp/one.log 2>&1 || { f=$((f+1)); grep -A3 panicked /tmp/one.log; }; grep -h "cancel: took" /tmp/one.log; done; echo "cs71: $f failed of 200"
kill "${loops[@]}"
```
What each should show:
- On main, failures (if any) read as the issues do: `no "\nback\n"` with `ok> back` on screen (ynia); `Waited: quiet
  for 5000 ms` instead of `its program ended` (1n2y); `left: 4, right: 3` (qjd6); `a verified kill is quick` (cs71).
- On this branch: 0 failed of 200 for each, and each `cancel: took` line with `ms` in the tens.

2. The whole suite with no retries, at the gate's load:
```sh
TZ=America/Phoenix cargo nextest run --workspace --retries 0
```
None of the four should appear among the failures.

## Left, uncertain, and for the owner

- cs71 and 1n2y never failed on main here, so their before-counts are 0 of 30 (cs71 also 0 of 32 in parallel). Their
  fixes rest on the issue's numbers (cs71) and on the pty probe (1n2y). The plants show that each fixed test still
  guards its behaviour.
- The `ms` bound is half the grace, where the stop takes tens of ms. A machine where a SIGTERMed `sleep` takes over
  1 s to be seen dead would trip it; that would be worth knowing in any case.
- `term.send` returns only once its screen settles. That is why the CPU rise is already there at the first look; the
  check is still a real gate, as the `if True` probe shows.
- No docs changed. Part III or status could note that these four left the "known under load" list, and that
  `tests_resumed`'s count above was seen once under load.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the final tree before the commits:
- fmt, shape (no file over 2,500 lines but the 10 listed), features, clippy, cockpit and test build: ok.
- reader rule: 9 of 9.
- suite: 2,789 run, 2,756 passed, **33 failed**. All 33 are the known L1 failures as root (theseus-pv6i): 19 of
  theseus-sandbox's contract tests, its bench `spawn_100`, and 13 of theseusd's sandbox tests. Nothing else failed,
  and none of the four failed.
- The phases after the suite, run by hand:
  - protocol types: ok (no `protocol.gen` change);
  - lifecycle and jobs: skipped (`THESEUS_GATE_NO_BENCH`);
  - turn: `frames_plain` 5 against a budget of 5, `frames_tool` 9 against 9, ok;
  - deny, offline: advisories, bans, licenses and sources ok.

A VM note: partway through, the disk's allowance filled (714 MB left). A daemon test then failed with "no
wrapper.pid in 30 s" on a clean tree. I cleared `target/debug/incremental` and my own scratch binaries, and it passed
again. Every result above was measured with room on the disk, apart from the two term plants: they ran just before
the cleanup and failed exactly where expected.
