# Cloud report: core-gaps (branch cloud/20261006-core-gaps)

Gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran once on the tree of all five commits (not once per commit). fmt, shape, features, clippy, cockpit, test build and the reader rule passed. The suite failed only on the known L1 tests (theseus-sandbox `contract`, theseusd `sandbox`; no other failure, the output golden passed). I then checked `protocol_types` by hand (`git diff` of cockpit/src/protocol.gen is clean, no protocol type changed). The benches were skipped (NO_BENCH). The lifecycle/jobs/turn steps after the suite were not run.

## 1. theseus-udzb: a failed routed turn counted where it ran (ed456e6f)
- Found: `count_failed_turn` used the request's pre-routing target; a failing switch or detour was counted under the base model.
- Changed: rpc/methods.rs `count_failed_turn` reads `profile`, `provider`, `model` from `te.trace.attrs` and falls back to the given target. The driver's continuation (`ran_on`) goes through the same function, so it now counts the failed turn under the model it ran on too (its `ran_on` is the fallback when the failure has no trace).
- Test: tests_route_model.rs `a_failed_routed_turn_is_counted_under_the_model_it_ran_on`: a sophisticated verdict, claude fake scripted to fail with 500s; the failed `theseus.turns` point names `opus`/`claude-opus-5-5`, and the metrics hold `theseus.provider.errors` and no `claude-sonnet`.
- Plant: the three fields back to the given target: fails (`left: "sonnet", right: "opus"`). Restored and touched.
- Not covered by a test: the driver's continuation (same function, not exercised separately).

## 2. theseus-y9p4: `keep_first`'s clear (c6853c52)
- Found: the plant (removing the two lines in `keep_first`) is an **equivalent mutant today**. `recall_compiled` reads `t.recall.drops` only while `t.recall.pending` is Some, and `turn.rs`'s dispatch (`t.recall.pending = None;` before `plan_and_dispatch`) ends that before any second loop. A second compile before the call only happens on a switch, which does not go through `keep_first`. So no test can fail on that plant alone.
- Changed: new tests_route_keep.rs (module line in lib.rs; tests_route.rs not touched). Two loops: first answer billed at 34,500 input tokens against a 40,000-token catalog window on sonnet and opus (output cap 1,000), a `fs_read` as the call, two earlier user messages in the session; loop 2's compile is a compaction on the routed model's window (the review's compaction ask, done). For `chat` (kept first compile) and `sophisticated` (switch): loop 0's stored budget names `["recall"]`, loop 1's `["compaction"]`; all three requests on the routed model.
- Plants: (A) keep_first clear alone removed: passes (equivalent, as said). (B) the dispatch's `pending = None` alone removed: passes (the clear holds). (C) both removed: fails on the chat half (loop 1: `["compaction", "recall"]`). So the test guards the pair.
- For the owner: the clear could be deleted as dead code, or kept as a guard.

## 3. theseus-b38m: a call's time proved by order (8c6680d8)
- Reproduction: not reproduced on this VM. The one test `--exact` 40 times under the load recipe (4 spinners, nice 19): 0 failures; the module `tests_m3::parallel::` 12 times with 6 spinners: 0 failures.
- Changed: the test now spawns the turn and a task beside it on the test's one thread that, once a read has run, blocks the thread 1.2 s. Every ready result waits that long in the turn's task (spans in the turn: about 1,216 ms each, own times 0 ms). Assertions: each `duration_ms` is under half the stall; some tools span is at least the stall. The 20 ms wall bound is gone. tests_m3.rs is 7,870 lines (ceiling 8,050).
- Plant: toolrun.rs, the in-process arm `Ok(Ok((Ok((out, img)), took)))` taking `t0.elapsed()` instead of `took`: fails (`[1211, 1212, ...]`). Restored, touched.
- After: 30 runs under load, 0 failures.

## 4. theseus-fner (72b8b156)
- tests_audit.rs records each polling thread's name beside its nice value; none may be `learning`; the nice check stays where `base < 19`.
- Plant: learning/audit.rs `send_on_runtime` as `rt.block_on(async move {..})` on the low thread: fails at nice 0 and under `nice -n 19` (`a request polled on the low thread`, `left: "learning"`). Restored: passes at nice 19.

## 5. theseus-2kyc (9114dcff)
- tests_stack.rs: `the_turns_entry_is_a_box_not_the_turns_future` holds `size_of_val(&runner.run(req))` equal to a boxed future's (16 bytes), dropped unpolled. The stack test's doc now says what it holds.
- Plant: `pub async fn run(&self, req) -> Result<TurnSubmitResult> { self.run_body(req).await }`: new test fails (left 9640, right 16); the 1.5 MiB test still passes, as the issue said.

## Live check (the maintainer's)
Scratch daemon on a fresh state dir, routing live, the stand-in model for the base profile, an OTLP receiver, and the routed profile's provider pointed at a stand-in that answers 500: send a message the verdict switches. Expect: the receiver's `theseus.turns` point with `theseus.outcome=failed` carries the routed profile and model, and health's provider errors name them; on main they name the base's. I did not run it (no keys or daemon here).

## Left
- The driver's continuation count has no test of its own.
- Nothing else; no docs edited. Doc note: if the maintainer wants, Part III could say that `keep_first`'s clear is an equivalent mutant today.
