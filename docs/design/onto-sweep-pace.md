# The onto sweep's pace: why a background wait hung a test, and what holds it now

_Written 2026-10-26 from theseus-75t4q's measurements, trimmed to the mechanism, the two loaded arms and the
stack. The raw 20-run tables stay in that row's review evidence; what is durable is below._

## The mechanism

The onto sweep waits between two questions of one run, so that background work yields while the machine is busy:
`pace()` (first in `judged.rs`, now in `pace.rs`) calls `theseus_store::pressure::quiet(BOUND)`, which looks at `/proc/pressure` every
`LOOK_EVERY` and sleeps on tokio's timer until the machine is quiet or the bound is spent.

**That wait was no input of a test.** A run asks up to 20 pairs, so on a loaded host it could spend 19 waits of
10 s — and nextest kills a test at 120 s. The pressure of the host running the suite is not something the test
chose, so the test's outcome depended on a reading it did not control. The fix makes the wait skippable in a test
build; the production path is unchanged.

**A hang, not a slow test.** Every failure was a 120 s timeout and every pass was fast, with nothing in between —
the signature of waiting on a condition rather than of missing a deadline.

## The two loaded arms

Test: `theseus-core tests_onto_names::a_chain_asked_over_two_nights_clusters`, on the owner's machine, with the
filter asserted to resolve to exactly one test before any run was counted.

| arm | result |
|---|---|
| base `78fcfb6d`, VM idle (cpu PSI avg10 ~2), 20 runs | **20/20 pass**, 0.142-0.253 s each |
| base, 6 busy loops at nice 0, test at nice 19 (cpu PSI avg10 ~50-54) | **n = 1, hung to the 120 s kill** (`rc=100`, SLOW then TIMEOUT ×2) |
| after the fix, same load, 20 runs | **20/20 pass**, 8-27 s |
| planted revert of the fix, same load | **TIMEOUT 120.009 s** |

**The asymmetry is deliberate and worth stating plainly: the base-under-load arm is a single run against an
after-arm of twenty.** It was stopped after one run because the hang reproduced to the kill whenever pressure was
over the bar, so more runs of it add nothing — but that means the row rests on the mechanism and on the planted
revert restoring the hang, **not** on a before/after rate comparison. Read it that way.

## The stack that named the cause

`gdb`, `thread apply all bt`, on the hung process: **no thread holds a lock or sits in a blocking call.** Both
tokio workers parked; one in `epoll_wait` with a 949 ms timeout (a tokio timer); FakeJev's accept thread in
`accept4`; the store-writer parked on its channel.

That is a task asleep in `tokio::time::sleep(1 s)` in a loop — i.e. `theseus_store::pressure::quiet` — and not a
deadlock. The 949 ms timer is the tell.

## The same shape elsewhere (theseus-qc5r6)

Four sibling sites kept the un-paced call and the same latent hang, none of which any test drove hard enough to
hang: `judge/people/backfill.rs` and `judge/people/sweep.rs`, which only wait, and the index's terms and shape
stretches in `rpc/mod.rs`, which add up how long they yielded and report it as `yielded_ms`. They now go through
`pace.rs`: `pace()` for the first two, and `quiet()`, the same skip returning the `Duration` (zero in a test),
for the last two, so no async caller in `theseus-core` reaches the store's wait directly.

What holds what: the **sites** go through `pace.rs` by `pace::tests::no_async_caller_in_core_waits_on_the_raw_pressure`,
which reads the crate's sources; the **skip** by `pace::tests::a_test_never_waits_out_a_busy_machine` (fake
pressure, a control that the real wait does wait there) and, for the onto sweep end to end, by
`tests_onto_names::a_chain_clusters_on_a_busy_machine`; the **wait** is in `theseus-store`, its async loop by
`pressure`'s `a_chunk_waits_while_busy_and_goes_when_quiet_within_the_bound` and its real `/proc/pressure` reading by
`tests/pressure.rs`. A daemon's own tests link `theseus-core` built without `cfg(test)`, so they wait as in
production.
