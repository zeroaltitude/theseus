# CLOUD_REPORT: theseus-75t4q, `a_chain_asked_over_two_nights_clusters` hangs

Branch `cloud/20261026-p3p-onto-names-hang`, base `78fcfb6d`. Store format and kernel SCHEMA untouched.

## Verdict

**Not a deadlock and not a race in the code. A test that took the host's CPU pressure as an input.**
The sweep pauses between two paid questions while the machine is busy
(`theseus_store::pressure::quiet`, up to `BOUND` = 10 s per pause, looking every 1 s;
PSI `some avg10` CPU >= 20 % or IO >= 10 %). The chain test asks **20 pairs** on night 1, so 19 pauses:
up to **190 s**, past nextest's 120 s kill. The spread (1.6 s ... 58 s, then the kills) is the
host's pressure at each look: the pauses end the moment the PSI average falls under the bar, so
the wait is "however long this host stays busy", capped at 190 s. "Alone" runs on the owner's machine
were not alone for PSI: other agents' work and 8 parallel test processes raise `avg10` over the bar,
which also is why the rate did not change under a separate load of 28.

Mechanism sites (base `78fcfb6d`): `judge/onto/renamed.rs:212` (`same_kinds`, `if asked > 0 { quiet(BOUND).await }`
between the 20 `same_kind` asks), also `renamed.rs:127`, `:306`, `judged.rs:484`, `:545`;
`crates/theseus-store/src/pressure.rs:82-94` is the loop.

## Measurements (raw numbers in `MEASUREMENTS-75t4q.md`)

- Base, VM idle (cpu PSI avg10 ~2): **20 of 20 pass**, test time 0.14-0.25 s.
- Base, 6 busy loops at nice 0 and the test at nice 19 (PSI avg10 ~50-54): **the first run hung to the
  120 s kill** (SLOW, TIMEOUT x2). One run was enough: with PSI over the bar every time, the outcome
  is deterministic, not 1 in 4. (The wall of 267 s includes a gdb attach that paused the process.)
- **Stack of the hung process** (gdb `thread apply all bt`): no thread in a lock or a blocking call.
  Both tokio workers parked; one in `epoll_wait(timeout=949)` (a tokio timer);
  `FakeJev`'s accept thread in `accept4` (`fake.rs:147`); `store-writer` parked on its channel.
  That is a task asleep in `tokio::time::sleep(1 s)`: `quiet`'s loop.
- After the fix, same load (6 loops nice 0, test nice 19), the original test, **20 of 20 pass**,
  test time 8.0-27.0 s each (CPU starvation of a debug build at nice 19; the pauses are gone; the
  wall of ~90-260 s per run is cargo and nextest start-up under the same starvation). None came near 120 s.

## Hypotheses

1. Mutex held across blocking I/O in `FakeJev::serve`, 2-worker runtime: **ruled out.** `serve_one` takes
   `Shared` only for short counted sections (`fake.rs:364-408`) and `lock()` is not held across reads,
   writes or sleeps (the `Held` loop re-locks every 5 ms); the hung stack shows no thread in a lock.
2. Lost wakeup in `onto_sweep_night`: **ruled out** by the stack (timer pending, not a missed notify)
   and by the code (`onto_sweep_night` has no notify/wait pair; its awaits are the model calls and `quiet`).
3. Order-dependent 21-pairs/asks-20 selection: **ruled out as a defect.** `kind_pairs` (`mint.rs`)
   iterates `BTreeMap`/`BTreeSet`s and sorts by `(Reverse(union), a, b)`: fixed order; 20 idle runs all
   passed. No second defect, no new row needed.
4. Leaked `FakeJev` listeners: not a factor (one per test, nextest runs each test in its own process).

## What I changed (code or test? the test's side)

The pause is a product feature (a nightly background sweep may take minutes), so the product keeps it.
- `judge/onto/judged.rs`: `pub(super) async fn pace()`; the five sites call it. In a test build
  (`#[cfg(test)]`) it does not wait, so no test of the sweep takes the host's pressure as an input.
- `tests_onto_names.rs`: `a_chain_clusters_on_a_busy_machine` runs the chain test again in user and
  mount namespaces with a fake `/proc/pressure` saying IO is busy (`tests_activation_pace::namespaced`,
  the existing rig), and requires `... ok`. Where namespaces can't be made it says so and passes (as its
  rig does; here they can).
- Planted revert (the `#[cfg(not(test))]` line off): the new test **TIMEOUT [120.009s]**
  (nextest kill); restored and `touch`ed: both tests pass in 0.30 s.
- Caveat to hear: the pause itself is not exercised by any test now (`pace` is skipped in test builds);
  `theseus-store`'s `tests/pressure.rs` still holds `quiet` itself. Deterministic: yes, the pin test
  does not depend on the host's pressure.
- `crates/theseus-judge/src/fake.rs` was **not** touched.

## What would have falsified it

Any timeout or run over ~30 s in the 20 loaded runs, or the busy-machine test passing with the skip
planted off. Neither happened. A rate would not convince me here: the base under load is not a rate, it
hangs every time the PSI is over the bar.

## Checks

- `cargo nextest list -p theseus-core --lib -E 'test(a_chain_asked_over_two_nights_clusters)'` -> exactly
  `theseus-core tests_onto_names::a_chain_asked_over_two_nights_clusters`. (Filter by `test(substring)`,
  not `test(=name)`, which matches nothing.)
- Both tests together: `2 tests run: 2 passed`.
- `cargo clippy -p theseus-core --all-targets`: clean.
- Keel: `python3 scripts/keel-guard.py --base 78fcfb6d` -> `keel: ok (78fcfb6d..the working tree, 4 files changed, 0 findings acked)`.
  (Without `--base` the clone's stale `origin/main` (`2d1d8d6f`) makes the guard judge 15 commits that
  are not mine; none touches my files.) `.config/nextest.toml` is unchanged; no `#[ignore]`, no `#[allow]`,
  no assertion lowered or deleted, no timeout raised. `dm_flow.txt` untouched.
- `cited file:line` at base: `tests_onto_names.rs:279/280` test + attribute, `:273` `PAIR_EF`, `:293-312`
  awaits and asserts, `fake.rs:126-155` `start`, `:157` `lock`, `:181` `script_when`, `:213` `seen` all as the
  task said.

## Live check for the maintainer

`cd` to a checkout, then under any CPU load (or on the owner's busy machine):
`cargo nextest run -p theseus-core --lib -E 'test(a_chain_asked_over_two_nights_clusters) | test(a_chain_clusters_on_a_busy_machine)'`
should pass in well under a minute. At base, the first fails 3 times in 10 on that machine.

## Disk

The VM's allowance ran out twice during gate runs (a first run's link died `Bus error`, a second
`No space left`). I removed `target/debug/incremental` and stale old-hash outputs in `target/debug/deps`,
and ran the gate with `CARGO_INCREMENTAL=0`.
