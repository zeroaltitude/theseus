# CLOUD_REPORT: task.not_this (theseus-o1h3f.2)

Branch `cloud/20261027-p3t-not-this`, base `9470df70`. Commits on top of the task commit and the start commit:

| step | commit |
|---|---|
| 1 RPC `task.not_this` (protocol, core, generated TS) | cc8bb40 |
| 2 CLI `theseus task not-this ID [REASON]` | 283cf70 |
| 3a cockpit key `u` (twice) on a task | b6c7257 |
| 3b 👎 reaction on a self-started task's message | 24a1f57 |

## What I checked first, and what the brief cited

- `git log origin/main` in this clone is **stale**: `origin/main` is `7d39d57`, an *ancestor* of the base `9470df7`. Because of that
  the keel guard and the gate's keel phase pick the wrong merge-base and report ~19 findings from the base's own content-flags
  merge (none mine). I ran everything with `THESEUS_KEEL_BASE=9470df7`, where the guard says `0 findings`. The maintainer's
  gate on a fresh `origin/main` is unaffected.
- Row A (`theseus-o1h3f.1`) had landed: `TaskOrigin.self_started` / `because` exist (`crates/theseus-protocol/src/tasks.rs:~108-128`). Used.
- Citations: `Ops.tsx` handler map at `:300-:301` was right (`stop_all`, `cancel` with `twice`). `Ops.tsx` is 543 lines (right).
  `TASK_LIST` constants line at `lib.rs:~300` was right; I added `TASK_NOT_THIS` to the `TASK_REHOME` line (`:304`), no new line.
  `task.not_this` existed nowhere (right). `standing.rs` / `standing_proposals.rs` exist; I reused `standing_add` through the board, not the proposals.
- Wrong or loose in the brief: (1) "writes one frame": cancel, preference and label are three writes (cancel keeps its own frames; I reused
  it as told). (2) "labels the offer that produced the task": a task carries **no link** to its offer (the offer is in memory,
  keyed by turn, and the auto-start is a named follow-up), so I label the newest acting `offer_task` judgment in the task's *origin session*;
  best effort, and `labeled` is empty when none exists. (3) `long-files.txt` ceiling for `theseus-protocol/src/lib.rs` is 2733; it is 2629, one `mod` line added.
- `docs/benchmarks`, spec, status, README: untouched. `crates/theseus-discord/src/golden/dm_flow.txt`: byte-identical (`git diff 9470df7 -- golden` empty). `turn.rs`, `runtime.rs`, `task.rs`, `tasks.rs`: untouched.

## Step 1: `task.not_this` (cc8bb40)

- **Found**: no method; `task_cancel_by` is the cancel path, `standing_add` takes `replaces` and dedupes identical text.
- **Built**: `theseus-protocol/src/task_not_this.rs` (params: task, reason, author, discord; result: task, preference, replaced, labeled; `SOURCE = "task.not_this"`),
  `theseus-core/src/rpc/task_not_this.rs` (+ `tests_task_not_this.rs`). Judged first as `Act::Standing` (owner, private place), so a stranger cancels nothing.
  Then `task_cancel_by` (ended task: answered as is), then the rolling preference, then the label.
- **The judgment call, the key**: the preference is a `Preference` for *everywhere*, `source = task.not_this`, and its text names the task's **shape**: the model's own `because`
  line (what it said it started the task for), else the task title, plus the owner's reason. Why: `because` is the one line the
  next self-started task will say of itself, so the model can tell "like this" from "not like this"; a title is often too specific and
  a category does not exist for tasks. **Rolling** = the live record with that source is passed as `replaces`; identical words leave the one record untouched (no churn).
  A lost race for the old record retries up to 3 times.
- **Dispatch**: `dispatch` in `rpc/server.rs` sits at clippy's limits (100 lines, complexity 25). `TASK_CANCEL` and `TASK_NOT_THIS` share one arm
  (`task_stop`, in my module), and `TASK_NOT_THIS` is imported by name so the arm stays one line.
- **Red at base**: the method did not exist (`unknown method`); the tests did not compile before the module. The behaviours were proved by planted reverts (below).
- **Tests** (`cargo nextest list -p theseus-core -E 'test(task_not_this)'` lists 6, and they run: 6 passed):
  `not_this_cancels_the_task_and_writes_a_preference`, `two_calls_leave_one_preference_and_it_is_the_seconds`,
  `not_this_on_an_ended_task_is_answered_not_refused`, `a_stranger_cancels_nothing_and_writes_nothing`,
  `an_unknown_task_is_not_found_and_writes_nothing`, `the_text_is_one_line_with_the_shape_and_the_reason`.
- **Planted reverts** (restored, `touch`ed, `git status` clean of the plant each time):
  (b) `replaces: old.clone()` -> `replaces: None`: `two_calls_leave_one_preference_and_it_is_the_seconds` FAILED (2 live, not 1), 5 others passed.
  (a) the `task_cancel_by` call replaced by a lookup that does not cancel: `not_this_cancels_the_task_and_writes_a_preference` FAILED (state not cancelled).
  A first draft also failed `not_this_on_an_ended_task...` (replacing an identical record changed its id): fixed by the "same words, same record" rule.
- **Not done**: the store format is not bumped (a new method, new ledger rows? none; a standing record and a judge label only), no layouts sample owed.

## Step 2: CLI (283cf70)

`theseus task not-this ID [REASON]` in `crates/theseus/src/task_start.rs` beside `pause`/`resume`, the method in `client::OPERATORS` (count 110 -> 111), so a job's shell is refused.
Unit test `not_this_names_the_task_the_preference_and_what_it_replaced`. `cargo nextest run -p theseus-protocol -p theseus`: 967 passed (the protocol's generated-TS test wrote `protocol.gen`, committed in step 1).

## Step 3: cockpit and reaction (b6c7257, 24a1f57)

- Cockpit: `not_this` handler beside `cancel` (`twice`), verb row in `ops/verbs.ts`, key `u` in `ops/keys.ts`; I also had to add the verb's row to the owner's table in
  `cockpit/test/ops-verbs.test.ts` (`EXPECT`): an addition. `npm test` 886/886, `tsc -b` clean, `oxlint` only existing warnings.
  **Not** touched: `TaskGraph.tsx`, the rest of `Ops.tsx`.
- Reaction: `crates/theseus-discord/src/runtime/task_react.rs` (not `runtime.rs`): `Verb::NotThis` on 👎, acted on only when `started_itself(task)` (`TaskOrigin.self_started`);
  otherwise `task_reaction` returns false and the reply rating (`route.rs`, `initiative.rate`) takes the 👎 as before. The acting function is `not_this` ("the task this turn started");
  the call site is one match arm. No 👎 is pre-added to messages (that would change the DM controls and risk the locked golden).
- Tests: `a_thumbs_down_on_a_task_that_started_itself_says_not_this`, `a_thumbs_down_on_a_task_the_owner_asked_for_does_nothing` (in `tests_task_react.rs`; `nextest list` shows both). Discord package: 670 passed.
  Planted revert (guard disabled with `if false &&`): the owner-asked test FAILED (task cancelled); restored and `touch`ed.

## Live check for the maintainer

On a scratch daemon (own `--config/--socket/--state-dir`):
```
theseus --socket $S task start "survey the north quay" --budget 1        # prints a short id, say a1b2c3
theseus --socket $S task not-this a1b2c3 "too noisy"                      # "Not this: a1b2c3 is cancelled. Preference std_… will keep Theseus from starting work like it."
theseus --socket $S task not-this a1b2c3 "too noisy"                      # same preference id, no second one
theseus --socket $S standing                                              # exactly one live preference with source task.not_this
theseus --socket $S task not-this a1b2c3 "different reason"               # replaces it: `(it replaces std_…)`; still one live
```
In the cockpit's Ops view, focus a running task and press `u` twice: it cancels and a preference appears in `theseus standing`. In Discord (owner, DM or private channel) react 👎 to a *self-started* task's thread message: the task is cancelled, a ✅ lands. 👎 on an owner-asked task does nothing here.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=9470df7 scripts/gate.sh` (before the reaction commit; the reaction commit then ran
fmt, shape, discord clippy, the whole discord suite (670), the keel guard (0 findings) and the golden diff by hand):
fmt, shape, recall tests, features, clippy, clippy-probe, cockpit, test build, reader rule: **pass**. Suite: 8749 run, 8713 passed, **36 failed**:
- 34 known L1 failures (root): `theseus-sandbox::contract` (19), `theseus-sandbox::bench spawn_100`, `theseusd::sandbox` (14).
- `theseus-core tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` (theseus-b38m, timing): passes alone.
- **`theseus-core tests_route::a_switched_turns_compilation_names_its_recall_drops`: not on the known list.** It failed in the gate and on the first run alone right after a rebuild,
  then passed 3 of 3 afterwards and passes on the base (stash test). I could not explain it and did not change it; it looks like recall's 250 ms deadline on a cold/slow VM
  (recall-in-deadline), but that is a guess. Treat as a finding.
- Phases after the suite (`protocol types`, lifecycle, jobs, turn, size benches) were not run as one: `protocol types` needs the generated TS staged, which it now is; benches are the owner's machine's.
- `cargo deny`: not run (no advisory db fetched in the time); `Cargo.lock` unchanged.
- **Environment note**: the VM's disk allowance filled once (a linker bus error); I cleared `target/` and rebuilt with `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0`. That failure was not a code failure.

## Keel

`python3 scripts/keel-guard.py` with the default base reports findings from the stale `origin/main`; with `THESEUS_KEEL_BASE=9470df7`: `keel: ok (… 0 findings acked)` for every commit.
**Keel findings expected**: none.

## Left open

- **OWNER CALL**: the rolling preference is global (everywhere) and one only: saying "not this" about a *different kind* of work replaces the preference about the first. That is "rolling means one" as written; a per-shape set would be a different feature.
- The offer auto-start (`judge/offer.rs`, `reply_step.rs`, `turn.rs`) is untouched, as the brief says. When it lands it should record the offering judgment on the task so `label_offer` can label that exact judgment instead of the newest one of the origin session.
- Docs to change at review: `docs/spec` Part III item for o1h3f.2; `crates/theseus/AGENTS.md` (the `task not-this` verb in the `task_start.rs` entry); `crates/theseus-core/AGENTS.md` (the `rpc/task_not_this.rs` entry); `crates/theseus-discord/AGENTS.md` (the 👎 control in `task_react.rs`).

CLOUD REPORT COMPLETE
