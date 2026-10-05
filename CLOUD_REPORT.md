# Cloud report: cloud/20261005-smalls

Four small fixes, one commit each, on main at 60b43fb6 (store format 20, unchanged).

| step | issue | commit |
| --- | --- | --- |
| 1 | theseus-5ihy: the secret board's settle race | 65300118 |
| 2 | theseus-u8ig: musl's `time_t` (and musl's `sched_param`) | a76f3860 |
| 3 | theseus-v6yc: the TUI's message order | 2df912b6 |
| 4 | theseus-nhg4: the budget loop's `loop.ended` row | e3428929 |

Setup: the toolchain, cargo-nextest, cargo-deny, `npm ci`, the workspace build and `cargo deny fetch` all
succeeded, so the deny phase ran offline. musl-tools was installed with apt (the musl target was already there).

---

## 1. theseus-5ihy: the secret board's settle race (65300118)

**Found.** The brief's suspicion is the race, and it is proven. `SecretBoard::publish` did `tx.send_modify(…)`,
which wakes every `settle_all` waiter, and only then locked `progress` to set `rounds`, `method` and `settled`.
`status()` reads the states, then `progress`. A waiter that reads `status()` between the two halves gets this
(the planted revert's output, verbatim):

```
SecretsStatus { state: "ready", ready: ["a", "b"], resolving: [], failed: [], method: None, rounds: 0,
  started_ms: None, settled_ms: None, retry_in_ms: None, outside_vault: [] }
```

`check`'s line formats exactly that as "2 secret(s) resolved in 0 ms (nothing to fetch)". The same read feeds
the daemon's `secrets.resolved` row (rpc/mod.rs: `ms` and `method` from `status()` after `settle_all`) and
the startup log's secrets phase, so a row with `ms: null, method: null` could be stored. I found no other path
that produces the symptom.

**Changed.**
- `publish` now writes the round's progress inside the `send_modify` closure, after inserting the states and
  before the send notifies anyone. The closure runs under the watch's write lock, so the states and their
  progress land together. "Settled" is computed from the map the closure holds.
- Lock order is now watch-write → progress. `status` drops its watch borrow before it locks `progress`.
  `begin_round`, `retry_at` and `started_at` lock `progress` alone. So nothing takes them in the opposite
  order.
- `hold` and the resolution itself (`resolve_into`, the rounds) are untouched.
- theseusd's `check` line moved into `fn resolved_words(&SecretsStatus)`; the output is byte-identical.

**Proved.**
- `secrets::tests::a_waiter_woken_by_a_publish_reads_its_method_and_time` forces the interleaving.
  - A test-only, thread-local hook runs in `publish` right after the send.
  - The hook blocks until a waiter on a second thread, parked in `settle_all` on its own runtime, is woken by
    the send and has read `status()`.
  - The test then asserts `ready`, `method: "inject"`, `rounds: 1` and `settled_ms` set.
- theseusd's `tests::checks_secrets_line_names_the_rounds_method_and_time` tests the line from a board a
  publish settled, with its origin 40 ms back: `2 secret(s) resolved in N ms (inject): alpha, beta` with
  N ≥ 40. An empty board still says `0 secret(s) resolved in 0 ms (nothing to fetch): `.
- **Planted revert:** `publish` back to main's order, with the hook between the send and the progress. The
  interleaving test fails with the status above. The check-line test passes, as expected: it reads after the
  publish returns. Restored, touched, `git status` clean of the plant.
- All of secrets.rs's tests pass (14), as do theseusd's unit tests.
- **Under load:** 20 runs at `nice -n 19` beside four busy loops at nice 0, 20/20 passing.
  - The busy loops were `yes > /dev/null`, not `sh -c 'while :; do :; done'`: this environment refused a
    command that contained `sh -c` with an `rm`, so I avoided `sh -c` after that.
  - The fix can't fail this test by timing: the waiter can only wake from the send, and the progress is written
    before the send. On main's order it fails every time, because the hook holds the publish between the halves.

**Live check (the maintainer's).** On a scratch config whose `[secrets]` resolve (`env:` entries are enough):

```sh
for i in $(seq 50); do theseusd --config /tmp/scratch-5ihy.toml check 2>&1 | grep 'secret(s) resolved'; done \
  | tee /tmp/5ihy.txt | grep -c 'in 0 ms (nothing to fetch)'   # expect 0
grep -vc '(inject)\|(local)\|(inject + local)' /tmp/5ihy.txt   # expect 0: every line names the method
```

With only `env:`/`file:` entries the method is `local`; with vault references it is `inject` (or
`inject + local`).

**Left.** Nothing known. The `secrets.resolved` row's `ms`/`method` get the same fix with no code of their own.

---

## 2. theseus-u8ig: musl's `time_t` (a76f3860)

**Found.**
- The line is wake.rs:802, as the brief says, and is the workspace's only `time_t`.
- **A second musl break that was not in the issue:** `cargo check --target x86_64-unknown-linux-musl -p
  theseus-core` stopped before reaching wake.rs. theseus-store's `pressure::idle_this_thread` (b334e485,
  theseus-tood) builds `libc::sched_param { sched_priority: 0 }`, and musl's `sched_param` has four more fields:
  `error[E0063]: missing fields sched_ss_init_budget, sched_ss_low_priority, sched_ss_max_repl and 1 other
  field`.
- So on main today, bench/build.sh fails to compile, not just warns.

**Changed.**
- wake.rs: the seconds go through `(unix_ms / 1000).try_into()`, its target inferred from `localtime_r`'s
  argument, so no alias is named. A count that does not fit takes the function's existing UTC path instead of
  wrapping.
- pressure.rs: `sched_param` is zeroed (`std::mem::zeroed()`), which is `sched_priority` 0 on both libcs.
- pressure.rs is outside this task's named files and belongs to the joined background-pass work. The edit is
  four lines, and no sibling in the list touches it.

**Proved.**
- `cargo check --target x86_64-unknown-linux-musl -p theseus-core` passes with no warnings.
- **Planted revert** of wake.rs's line gives `warning: use of deprecated type alias libc::time_t: This type is
  changed to 64-bit in musl 1.2.0…` at wake.rs. Restored and touched; 0 warnings again.
- On the host, clippy `--workspace --all-targets -D warnings` is clean.
- I did **not** run bench/build.sh to the end. The VM had 2.4 GB of disk free when I started it, too little
  for a release-thin musl build beside the debug target, so I stopped it. The disk did fill once later, and I
  cleared `target/debug/incremental`. The musl `cargo check` covers the compile, including ring's C via musl-gcc
  on the way.

**Live check (the maintainer's).**

```sh
bench/build.sh /tmp/bench-bin 2>&1 | tee /tmp/u8ig.log | grep -E 'warning|error'   # expect nothing from wake.rs or pressure.rs
grep -c time_t /tmp/u8ig.log                                                    # expect 0
```

---

## 3. theseus-v6yc: the TUI's message order (2df912b6)

**Found.** As the brief says.
- `notified` asks `session.history` (`n: 5`) for another surface's `user_message`.
- `answered(Purpose::Node)` appended the node with `Detail::node`, after any reply that had streamed meanwhile.
- The pane keeps no positions, and `node.written` carries none.

**Changed** (theseus-tui only, no protocol change).
- `Detail` keeps `places: Vec<(node_id, Option<usize>)>`, in the order the `node.written`s came.
- `others_message(node_id)` marks the place for another surface's message: after every line so far.
- **The mark closes the reply's open line,** so what streams next starts below the place and no line is ever
  split. This is a choice the owner may want to hear about: a reply mid-line when another surface's message is
  written continues on a new line, as it already did when any other line (a tool line, a note) came between.
- `node` inserts the fetched lines at the mark. A later mark at the same place moves down by the inserted lines,
  so two messages keep the order they were written in.
- When lines past `KEPT` go, each mark moves up with them. A mark whose line went (`None`) puts its message at
  the end.
- A read that finds no node, does not decode, or fails (`failed`) drops its mark (`unmark`), and nothing shows.
- `history()` (a reload) clears the marks. A node answered after that, with no mark, is appended as before.
  That is today's behaviour in a rare race between a reload and a fetch.
- The TUI's own message is unchanged: it shows at once, and its `node.written` is not read again.

**Proved.** `src/tests_order.rs` (5 tests) drives the `App` alone, the order forced by hand:
- another surface's `node.written`, three reply deltas (one with a newline), the history's answer, then one
  more delta: the operator's line sits above the turn's line and the reply, and the last delta continues the
  reply's open line;
- two messages answered in reverse order keep the order they were written in;
- a not-found read and a failed read leave the pane exactly as it was, and a later message lands in its own place;
- `KEPT`: a place survives the drop of older lines and moves up with them (asserted against the line that
  preceded it), and a place that was dropped puts the message at the end;
- the TUI's own message: `you:` at once, no read, then the reply.

The TUI's suite passes: 32 tests (27 before, plus these 5).

**Planted revert:** the fetched node always appended at the end (`place.filter(|_| false)`). Four of the five
`tests_order` tests fail; the own-message test passes, as it should. Restored and touched.

**Under load:** the same 20 runs as step 1 (both sets in each run), 20/20. These tests involve no threads or
timers, so they can't fail by timing.

**Live check (the maintainer's).**
- Start a scratch daemon on a fresh state dir, Discord and the web off, with the stand-in model
  (`theseus-sim fake-model --rules`), with `--socket /tmp/v6yc.sock`.
- In tmux, run `theseus tui --socket /tmp/v6yc.sock` and focus a session (say `ses_X`).
- From another terminal, 10 times: `theseus --socket /tmp/v6yc.sock ask -s ses_X "question $i"`.
- **Expect:** in the TUI's pane, each `operator (…): question $i` line sits above that turn's `── turn` line and
  its reply, never below the reply.

**Left.** The insertion is O(lines) at most (a splice into a ≤5,000 vector), once per message.

---

## 4. theseus-nhg4: the budget loop's `loop.ended` row (e3428929)

**Found.**
- `LoopEndedOnBudget` had no `KIND`, so it sent the notification and wrote no row.
- It was recorded inside `ask_budget`, which has two callers:
  - the model call's over-budget exit (`Called::OverBudget`), after `LoopOpened` for loop `i`;
  - **a hands group over the budget** (`hands_over_budget`), checked at the top of the loop, *before* any
    `LoopOpened`.
- On that second path the fact named loop `t.loops - 1`. That loop had already ended, with its own row and
  notification, or never started at all (`t.loops == 0`). Giving the fact a row there would have written a
  second end for an ended loop.
- nhg4's second nit (the vault-wait line with the run of spaces) is gone, as the brief says. It left with the
  vault's act-gate (theseus-zmgb, a53a79ab), so there was nothing to fix there.

**Changed.**
- `LoopEndedOnBudget` gets `KIND = LoopEnded` and a row in `LoopEnded`'s shape:
  `{"loop": i, "outcome": {"loop_index": i, "provider_stop_reason": null, "tool_calls": 0, "output_chars": 0},
  "advancer": "budget", "decision": {"decision": "budget"}, "usage": <all zero>}`.
- **`outcome` and `usage` for a loop that made no call:** no stop reason, 0 calls, 0 output characters, and a
  zero `Usage` (the four always-present counters at 0; the 1-hour field is skipped at 0). The cockpit's
  summary.ts shows it as `loop 1 ended: , 0 tool calls → budget`.
- **`decision`:** `{"decision": "budget"}`, not `Decision::EndTurn("budget")` (`end_turn` with reason `budget`).
  It matches the notification's `decision: "budget"` and the brief's "decision `budget`". If the owner prefers
  `EndTurn`, it is a one-line change.
- **The span:** the loop's span still closes at `LoopCut { decision: "budget" }`, recorded right after. The fact
  still has no span of its own.
- **A row of an existing kind owes no store format bump**, and the store format stays 20.
- The record moved from `ask_budget` to the `Called::OverBudget` exit, right after `ask_budget`. The order is
  unchanged on that path (after `BudgetAsked`, before `LoopCut`).
- **The hands path now sends no `loop.ended`**, since no loop was open there. That is a visible change: a client
  no longer sees a duplicate loop end on that path. turn.rs changes by 4 lines moved, nothing else.

**Proved.**
- `tests_budget_loop::the_loop_that_asks_the_budget_question_writes_its_loop_ended_row`:
  - a $1.40 limit, the first call billed 2,000 in and 30,000 out with a tool;
  - the turn ends `budget` at 2 loops, with 2 `loop.started` and 2 `loop.ended` rows;
  - the second row equals the literal above.
- Core golden: rewritten with `TZ=America/Phoenix THESEUS_GOLDEN=write`. It moves exactly two lines in the
  budget scenario: the frame line gains one `ledger`, and the `loop.ended` row appears in that frame. Nothing
  else moved.
- **Planted revert:** `KIND` removed from `LoopEndedOnBudget`. Both `tests_output::the_cores_output_matches_its_golden`
  and the row test fail ("every loop that started ended", only loop 0's row). Restored and touched.
- **Other tests whose expectation moved:** none. The whole suite passes apart from the known L1 tests. That
  includes tests_m3's budget tests, tests_books' budget fault test, and the turn bench (5 and 9 frames: the row
  rides in an existing frame and adds none).

**Live check (the maintainer's).**
- Run a scratch daemon as in step 3, with `[kernel] spend_limit_usd = 0.01` in its config.
- Run `theseus --socket /tmp/nhg4.sock ask "hello"`: the turn asks the budget question (stop reason `budget`).
- Run `theseus --socket /tmp/nhg4.sock ledger -k loop.ended -s <SESSION>`.
- **Expect:** a row for the loop that asked, with advancer `budget`, decision `budget`, 0 tool calls, and no stop
  reason. In the cockpit's Ledger: `loop N ended: , 0 tool calls → budget`.
- Depending on the profile's reservation, the first loop may be the one that asks; then it is loop 0's only row.

---

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran on each commit's own tree. I set the later
steps' files aside with `git stash` for each of the earlier steps' gates.

- fmt, shape, features, clippy, the cockpit's lint, tests and build, and the test build all passed every time.
- The suite failed only on the 33 known L1 tests, as the brief expects (VM runs as root, no job cgroup,
  theseus-pv6i):
  - theseus-sandbox's 21 contract tests and its bench's `spawn_100`;
  - theseusd's 11 sandbox tests, among them `the_jobs_bench_l1_row`.
- Suite runs: step 1, 2,791 tests (2,758 passed); step 2, the same; step 3, 2,796 (2,763 passed); the final tree (step 4), 2,797 (2,764 passed).
- No flaky retry, and none of the brief's timing tests failed.
- The phases after the suite, run by hand each time:
  - the protocol types: `cockpit/src/protocol.gen` unchanged, since no protocol type changed;
  - the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`): 5 and 9 frames, ok;
  - `cargo deny --offline check`: advisories, bans, licences and sources ok.

**Docs for the maintainer to write.**
- Part III / status: the four fixes.
- scripts/AGENTS.md "glibc or static musl" could say the musl build is checked by
  `cargo check --target x86_64-unknown-linux-musl -p theseus-core` for libc differences (struct literals of libc
  types break there).
- I updated theseus-tui's AGENTS.md (its Tests list names `src/tests_order.rs`) in step 3's commit.
