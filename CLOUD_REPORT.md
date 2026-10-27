# CLOUD_REPORT: theseus-o1jnt, two tests red on x86 and green on ARM

## Step 0: citations, checked at my base (`5342a01c`)
- `git log origin/main` head is `5342a01c`: nothing landed after my base.
- `tests_idle.rs:234` `a_repl_computing_is_not_idle`: correct.
- `tests_content.rs:230` `ten_results_are_one_decision_point`: correct.
- `9470df70` is an ancestor of the base, and `9470df70^1` is `7d39d57e`: both correct.
- `MANIFEST_FORMAT` is 52 at `store.rs:692`: correct (not bumped here).
- **No citation was wrong.**

## Host
4 vCPU Intel Xeon @ 2.10GHz, 15 GB RAM, ext4 (`/dev/vda`), kernel 6.18.44-fc (Firecracker VM, not WSL), rustc 1.98.1, nextest 0.9.150, running as root.

## The host bisect: Branch B (green idle), then a finding
`cargo nextest list -p theseus-core -E 'test(=term::tests_idle::a_repl_computing_is_not_idle) + test(=judge::tests_content::ten_results_are_one_decision_point)'` selects exactly both:
```
theseus-core judge::tests_content::ten_results_are_one_decision_point
theseus-core term::tests_idle::a_repl_computing_is_not_idle
```
- Idle, together x3: 2 of 2 pass (1.4 s and 2.3 s). Alone x3 each: pass. A further 20 alone runs of the content test passed, but one early alone run failed (3.5 s, not captured).
- `taskset -c 0,1` x5: all pass. `taskset -c 0` x5: the REPL test failed 2 of 5 (1.8 s).
- Under load (4 busy loops at nice 0, test at nice 19), base build:
  - the content test failed 6 of 6 with `ten queued in a burst join one batch`: `left: 3` or `4`, `right: 1`, at `tests_content.rs:287`;
  - the REPL test failed 1 of 6 with `Waited: its program is idle … after 882 ms`.

So both are **load and CPU-time dependent, not x86 specific**. The idle green here matches the ARM green, and a loaded or slow-fsync host such as the owner's WSL machine is the red. I did not need the `7d39d57e` bisect (Branch A). It is not a content-flags regression: the cause is in the test and in a pty race that predates it, so I did not bisect commits.

## Cause 1: `ten_results_are_one_decision_point`
Second half of the test (`tests_content.rs`, the `for i in 10..20` loop): each iteration called `item_in`, which does a **synced `store.append`** (`tests_content.rs:107-110`), inside the loop that asks the items. `drain` sleeps `COALESCE` = 50 ms (`judge/content.rs:51`, `:880`) from the first ask, so ten fsyncs plus the asks must fit in 50 ms. On a slow-fsync or loaded host they do not, and the burst becomes 3 or 4 points.
Production is right; the test's own setup was in the window.
- Change: build the ten items first, then ask them in a tight loop. The assertion is unchanged (same count of assertions).
- Commit `e02fdae`.

## Cause 2: `a_repl_computing_is_not_idle`, two defects
(a) Test bound: `waited_ms >= 900` uses the wait's own count, which starts at the *read*, after the send; python's 1 s sleep starts at the send. Under load the read starts late, so the count can be 882 ms for a full second of sleep. Now the lower bound is `sent.elapsed() >= 1000 ms`, measured from before the send (one assertion replaced by one).

(b) **Production race, found after (a), once the test was loaded further (the message was `idle … after 1003 ms` and the screen had no `slept`).** `Pty::idle` (`term/pty.rs`) is "program in front and waiting for input". A program that prints then goes back to its read is waiting before the reader thread has taken its output off the master, so `until_idle` returned "idle" with the output **missing from the screen the read returns**. That is a real bug a model can hit: a command's last output is cut from an idle read on a loaded host.
- Change: `Pty::idle` also requires `drained()`: `FIONREAD` on the master is 0 and a new `Shared::feeding` flag (set by the reader once it has a chunk, cleared after the screen has it and the version is bumped) is false. +26 lines in `pty.rs`.
- **Residual window (honest):** between the kernel handing the reader its bytes and the `feeding` store there are a few instructions in which both checks read "clear". It is much narrower than before, not closed. A full close needs the reader to publish a monotone counter, or an `idle` that is confirmed on two looks; I did not do that because it adds latency to `until_idle` against the `< 100 ms` bounds in `tests_idle.rs`. OWNER CALL: whether that is worth it.
- Commit `e6badfc` (this and the test's bound).

## Proof
- Red at base (load, 4 busy loops, `nice -n 19`): content test 6/6 fail (`left: 3|4`); REPL test 1/6 fail (882 ms), and once with the screen missing `slept` (1003 ms, from `/tmp/g1.log`, with the test's bound already fixed).
- Content fix, load: 8/8 pass (7 s each). Planted revert (old test restored, rebuilt): 6/6 fail again (`left: 3|4`, 31 s). Restored, `git status` showed only my three files.
- Both fixes, load: **20/20 pass** (2 tests each). `taskset -c 0` x10: all pass.
- Planted revert of `&& self.drained()` only (test fixes kept, rebuilt): REPL test 14/15 pass, 1 fail: `Waited: its program is idle, back in front, after 922 ms` with screen row 2 blank, the same symptom. The restored file was verified with `git status`.
- `cargo nextest run -p theseus-core -E 'test(/^term::/) + test(/^judge::/)'`: 102 run, 102 passed. `term::` alone: 42 passed, 3 ignored (already ignored).
- `cargo fmt --all --check` clean; `cargo clippy -p theseus-core --all-targets -- -D warnings` clean.
- I returned to my branch (no detached head was used; I never checked out `7d39d57e`).

## The red-first caveat
The REPL race has no deterministic test: it needs the reader thread to lag the program, which only load makes. The evidence is the load runs above, with the planted revert showing the same symptom. I did not add a new test; adding a deterministic one would need a hook in the reader.

## What the owner's host should run next
```
cargo nextest run -p theseus-core -E 'test(=term::tests_idle::a_repl_computing_is_not_idle) + test(=judge::tests_content::ten_results_are_one_decision_point)'
```
Three times idle, then each alone. On the red host the content test should now pass (its items are made first) and the REPL test should pass, with or without load. If the content test is still red there, run `fsync` timing (`dd if=/dev/zero of=x bs=4k count=1000 oflag=dsync`): ten synced appends over 50 ms is the old cause. If the REPL test is still red, the residual window above is the next suspect.

## Keel
`python3 scripts/keel-guard.py` prints findings (test-removed, assert-removed, assert-weakened, allow-added, keel-file) for files I did not touch: `data.rs`, `tests_bg_gate.rs`, `courier.rs`, `tests_reach.rs` and others. My diff is `judge/tests_content.rs`, `term/pty.rs` and `term/tests_idle.rs` only, so those findings come from the guard's range (probably older than my base), not from my commits. **Keel findings expected: none from my commits.** One assertion in `tests_idle.rs` was rewritten (not removed): `waited_ms >= 900` became `sent.elapsed() >= 1000 ms`.

## Gate
Not run in full: the gate needs the cockpit build (`npm ci`) and a workspace build, which I did not set up (I built `theseus-core` only). The suite subset, fmt and clippy above are green. I did not run `cargo deny`. The speed benches were not run.

## Integrator notes
- `e02fdae` (content test) and `e6badfc` (pty fix and REPL test): both for main.
- `session started` (`4df67ea`), `CLOUD_TASK.md` and this file: not for main.
- No store format change, no config change, no new dependency, no names in the diff, `dm_flow.txt` untouched.

CLOUD REPORT COMPLETE
