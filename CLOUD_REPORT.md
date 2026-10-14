# CLOUD REPORT: discord-ec2-timeout (theseus-wpv2g)

Branch cloud/20261013-discord-ec2-timeout. Test-side only (crates/theseus-discord/src/tests_process.rs, one sentence in that crate's AGENTS.md). No courier, render, fake or core change.

## Found
- The old test is flaky here too, in the EC2 shape: alone, 5 of 6 runs timed out at 21 s ("timed out: the thinking's create, held, and the reply's post"); 1 passed in 1.2 s.
- A second weakness: with the lane's batch holding both the thinking and the text op, the live path writes the answer (it takes `owed`), so the reply's post never writes it and the guarded rule is not exercised. Planting the revert (`let owed = ... && !streamed` in courier.rs `reply`) did not fail the old or my first rewrite: found by planting, fixed below.

## Changed (b495389)
- Hold on the prefix `-# 💭` (matches `thinking` and `thought for`).
- `an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: uses a test-side provider (TextAfterTheHeldCreate) that streams the thinking at once and holds the text until the fake has a held create, so the post writes the answer on any machine.
- New `an_answer_the_post_writes_after_the_fold_is_still_the_answer`: holds every write until the reply is queued (EC2 order: the fold is the first create), then asserts the same.
- Both: the wait says "the scenario: ..." (not a bare timeout); after it they assert the create is recorded `held` by the fake and its text is the scenario's (`-# 💭 thinking` / `-# 💭 thought for`); the end state asserts the answer pings, the fold is posted and outbox pending is 0 (assertions stay in the two named test fns; the keel guard passes).

## Proof
- Old test alone, 6 runs: 5 failed (21 s timeouts), 1 passed.
- New pair, 50 runs in a loop: 50/50 (each run both tests); `taskset -c 0`: 20/20; beside 4 nice-0 busy loops with the test at nice 19: 20/20; whole crate: 236 passed.
- Planted revert (`owed` also needs `!streamed`): the thinking test FAILS with "the answer pings" (silent answer); the fold test passes (there the live path writes the answer; it guards the end state and the hold, not the post rule). Reverted, file touched, `git status` clean.
- Old hold string under EC2 order: not reproduced by a literal old-test run; the fold test's create is asserted to be `thought for`, which the old string does not match.
- keel-guard: ok, 0 findings. clippy on theseus-discord --all-targets and fmt: clean.

## Other hold_writes_containing users
tests_live.rs ("held at the dock"), theseusd tests/outbox.rs and theseus-sim lifecycle.rs (FOOTER) hold on their own text, not the placeholder: no change.

## Live check for the maintainer
On the EC2 host: `cargo nextest run -p theseus-discord an_answer_the_post` alone (expect 2 passed, ~1.3 s each), then the full gate with lanegate.py.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit, build, reader rule pass; suite: 4349 passed, 33 failed, all the known L1 ones (theseus-sandbox contract 38 incl. bench 2... counted 33 as listed: sandbox::contract, sandbox::bench spawn_100, theseusd::sandbox; root VM, theseus-pv6i). Protocol types phase checked by hand (clean); benches skipped (NO_BENCH). A first gate run failed ~154 job tests because the disk filled (395 MB free, below the daemon's 1 GB floor); after freeing target/debug/incremental they passed. Keel findings expected: none.
