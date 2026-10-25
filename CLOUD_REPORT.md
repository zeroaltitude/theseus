# CLOUD_REPORT: p3h-burn-parent (theseus-o1h3f.5)

Branch `cloud/20261025-p3h-burn-parent`, from `0fda4d57`. Code commit `4f3bdbf8`.

## 1. For the maintainer
- Store format 51 and kernel `SCHEMA` 2: both untouched. Nothing new is stored; the windows are in memory.
- Ceilings: `day_ceiling.rs` 789 to 838 (no listed ceiling, under 2,500), `tests_runaway.rs` +164. `kernel.rs`, `turn.rs`, `overdraw.rs`, `tasks.rs` untouched.
- **OWNER CALL: the parent's hard multiple at carve time** (section 6). Nothing changed.
- No limit, cap, window, multiple or default was raised or loosened. This only makes the burn line see more.

## 2. Where the parent's record happens: (a), at the day effect
`Kernel::apply_day` records the settled cost with `burn_record` for the execution and for each ancestor, from `burn_ancestors` (day_ceiling.rs).
- Chosen over (b): the effect is queued in a transaction and dropped by `tx.rs::day_rewind` on failure, so nothing is booked for a frame that failed. (b) would have needed an effect of its own anyway, and a `kernel.rs` edit.
- Lock order: `burn_ancestors` only calls `Kernel::execution`, which is a lock-free store index read (`latest_by_key`). It takes no execution lock, and an execution's parent never changes, so it cannot deadlock under a transaction's held locks (apply_day runs at tx.rs:445 under them, after the commit). The ancestors are read at apply time rather than carried on `Effect`, so `Effect` and its builders are unchanged.
- Bound: 8 steps (`BURN_ANCESTORS`) plus a seen-set, so a cycle ends. Tasks are depth one today (tasks.rs:194), so the real chain is one step.
- Memory: a helper adds its own ring plus the one shared parent ring. The retain rule at `burn_record` (512) is untouched and still bounds the idle rings.

## 3. What is counted once
Only `burn_record` is used for the ancestors, never `settle` or `book`. In `the_day_counts_a_childs_cost_once`, two children settling 600 each give `Today.spent == 1_200` and `held == 0`, written as literals. The module doc now separates the day's total (once) from the burn window (the execution and each ancestor).

## 4. The stop
The parent's next provider call is refused with `KernelError::BurnRate`; no new stop path. The words are "the session spent $X in the last 10 minutes, its $Y burn-rate line ... the session waits for the owner to resume it". For a parent stopped by its children, that is true of the aggregate but does not say the spend came from its tasks. It is not misleading. `turn.rs` was not edited; a follow-up could add "including its tasks".

## 5. Resume
Chosen: a resume clears only the resumed execution's own window. Resuming a parent does not touch its children's windows (each has its own line and its own resume). Resuming a child never clears its parent's window, since that would loosen the aggregate line. Spend already in the parent's window before the resume is cleared with it, so it does not come back; only new settles count. Pinned by `a_resume_clears_only_the_resumed_executions_window`.

## 6. The hard multiple at carve: OWNER CALL
- A carve does not consult the parent's hard multiple. `open_task_in` (tasks.rs:209-212) takes `want.min(parent.budget.limit_micros)` under notify, and `carve_task` adds it to `parent.budget.reserved_micros` (tasks.rs:556) with no check. `hard_stop` (overdraw.rs:54-75) is called only from the plan path (kernel.rs:1745) on the calling execution's own budget.
- So a fan-out can carve and spend past `multiple × limit`. Each helper's calls are checked only against its own carve times the multiple. The parent's own next provider call is refused (its `spent + reserved` includes the carves), but the carves and the helpers' spend are not. The burn window from this row bounds the rate ($200 per 10 min), not the total.
- OWNER CALL: refuse a carve whose `spent + reserved + limit` passes `hard_multiple × limit`. It is a pure tightening with no new key, but it needs a carve-specific error (`HardStop`'s words say "this call was not made", which is wrong for a carve) and it limits self-tasking, which the owner wants light. Not done.

## 7. Tests (tests_runaway.rs), all 12 pass; kernel suite 265/265
| Test | Plant | Result at the plant |
|---|---|---|
| two_children_trip_their_parents_burn_line | P1 drop parent record | red (and 3 more) |
| a_settled_cost_reaches_every_ancestor_to_the_top | P2 one step | red (alone) |
| the_day_counts_a_childs_cost_once | P3 `book` the parent's copy | red (alone) |
| a_failed_frame_books_nothing_in_any_window | P4 record in `day_after`, before the commit | red (with 3 others, which double count) |
| a_plain_execution_writes_one_ring | covered by P1/P4 | |
| a_resume_clears_only_the_resumed_executions_window | pins section 5 | |

The tests are red at the base because `family` and the assertions need the new records. The grandchild test writes the helper's execution record by hand, since tasks nest one level only. After every restore I ran `touch` and `git status`.

## 8. Keel findings expected
None. No existing test was rewritten.

## 9-11. Gate, keel, DM golden
- `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`: no output, exit 0.
- `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no diff.
- Gate: see the end of this file.

## 12. Join notes
Touched: `crates/theseus-kernel/src/day_ceiling.rs` (module doc 41-47; `BURN_ANCESTORS` const; `burn_len` and `burn_rings` test helpers; `burn_ancestors` and the loop in `apply_day`), `crates/theseus-kernel/src/tests_runaway.rs` (appended tests). No file of the other four rows was touched.

## 13. Follow-ups
The carve check (section 6). The stop wording could name the tasks.

## 14. Data floor
Nothing is sent to a model or routed; costs already recorded go into one more in-memory window.

## Gate result (interim)
`scripts/gate.sh` fails in its keel phase on history before my commits: five acks whose signatures do not verify on this VM, and old erosions (persona_cmd.rs tests, keel-guard.py) in the range from 2d1d8d6f. None of it is mine: `keel-guard.py --base 0fda4d57 --head HEAD --quiet` is clean. The door then reports "main's gate failed on this tree". I ran the later phases by hand: `cargo fmt --check`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run -p theseus-kernel -p theseus-core` (TZ=America/Phoenix). The kernel suite alone passed 265/265. The workspace results were still running when this was written; see the final line below if one was added.
