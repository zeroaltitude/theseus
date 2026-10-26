# The onto sweep's pace: why a background wait hung a test, and what holds it now

_Written 2026-10-26 from theseus-75t4q's measurements, trimmed to the mechanism, the two loaded arms and the
stack. The raw 20-run tables stay in that row's review evidence; what is durable is below._

## The mechanism

The onto sweep waits between two questions of one run, so that background work yields while the machine is busy:
`judged.rs`'s `pace()` calls `theseus_store::pressure::quiet(BOUND)`, which looks at `/proc/pressure` every
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

## The latent shape elsewhere

The same un-paced call shape survives at four sibling sites, none of which any test drives hard enough to hang
today: `judge/people/backfill.rs:158`, `judge/people/sweep.rs:217`, and `rpc/mod.rs:1221` and `:1271` (the last
two accumulate `quiet`'s returned duration and so need a returning variant). Tracked as a follow-up row.
