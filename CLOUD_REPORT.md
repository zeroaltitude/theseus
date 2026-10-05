# Cloud report: `theseus judge prove` from the ledger (theseus-0j2.18, row 50)

Branch `cloud/20261005-prove-wire-in`, from `c71179bb` (main at `80ef1dea` plus the task commit). Started 08:35 UTC
and finished 10:35 UTC, inside the 4-hour deadline. Five commits:

| commit | what |
|---|---|
| `e83d87b0` | learning: the prove's records, built from the ledger (`learning/prove.rs`) |
| `b133c40c` | rpc: `judge.prove`, the generator's report over those records |
| `c59813e0` | cli: `theseus judge prove` |
| `c92b7bea` | learning: the classification part (step 4) |
| `9d54a5ab` | docs: theseus-core's AGENTS.md names the prove |

No new dependency. No store format bump: nothing stored changed, and the method writes nothing. No long file grew:
lib.rs is still 2,709 lines, because `JUDGE_PROVE` joins `JUDGE_REPLAY`'s line and that line's doc comment was
reworded in place. Protocol types are in `judge_runs.rs`, and `ts.rs` gained names on an existing line.

## Step 1: the records (`e83d87b0`)

**What I found, and the differences from the design (the code wins):**
- **A task.** One record per `task.ended` row: an execution opened by `task.create`. A second row naming the same
  task counts once.
- **`task.closed` makes no record and changes none.** Its report close (`task_graph::closed_by_report`) repeats the
  same end: `done` for `complete`, `failed` for `failed` or `budget_exhausted`. A close by hand closes a graph
  record, not an execution.
- **A task opens no tasks** (the kernel's `TaskDepth`). So `turns` (its session's `turn.ended` rows) and
  `spend_micros` are the task's own, with no children to count.
- **Who pays for judgments.** Every judgment today, canary included, is paid from the judge's own day budget:
  `prepare_loop` → `reserve`, and the sink writes `budget: "shadow"`. None is in the execution's
  `budget.spent_micros`.
  - So `spend_micros` = the execution's `spent_micros` + the judge calls whose row says `budget: shadow`.
  - `judge_micros` = `cost_micros` of every `judge.call` (any pack) whose context names the session.
  - If 26b moves canary judgments onto the execution (design §2.6), the record already skips non-shadow rows, so
    nothing counts twice. A `max(…, judge_micros)` keeps the generator's rule that the judge's share is within the
    whole.
- **`arm`.** The `pack_arm` of the judgments whose `pack == "loop.v1"` and whose context names the session. Tasks
  are left out and counted by reason:
  - `never_judged`: no such judgment;
  - `no_arm`: only `all`, outside a canary or before 26a;
  - `both_arms`: judgments in both arms;
  - `cancelled`: a cancel or `/stop` ended it, so it has no outcome of its own;
  - `unreadable`: the execution is missing.

  A `loop.v2` candidate's judgment in shadow is ignored.
- **`stops`.** One per judgment in the task's arm, oldest first.
  - **Control's decision:** the baseline's, from the context's `decision`: `no_tool_calls` stops.
  - **Canary's decision:** Jev's, by §2.8a's rule. It continues when `work_state` acts on `progressing`, or when
    `announced_unfinished` acts true. Otherwise, or when Jev didn't answer, the baseline's decision stands.
  - **Until 26b builds the nudge, the turn ends either way.** The canary's "continue" is what Jev would have done.
  - **`should_stop`** is the resolved `work_state` label: `complete` → true; another class or `{"not":"complete"}` →
    false. A label that only rules out another class (`{"not":"progressing"}`) says nothing about completeness, so it
    gives `null`, as does no label. That is a choice: the brief's "otherwise false" would read `not:progressing` as
    "should not stop".
- **`false_completion`.** The task's last judgment with a `work_state` answer is where something "called it
  complete", provided its acting decision there was stop. The field is that judgment's resolved label read as "not
  complete". It is `null` when the decision there was continue, or when no label says.
- **`success`.**
  - **False** on a `failed` or `budget_exhausted` execution, or when the last judgment's resolved `work_state` says
    not complete. That covers:
    - the system's near-identical rule (`false_completion`, 0.5);
    - a continuation re-ask;
    - an audit (0.5);
    - an operator's label. **I read the operator's "wrong" as** `wrong` on a judgment leaning `complete`, or any
      class other than `complete` / `not:complete` on `work_state`. `labels::resolve` settles them heaviest first,
      so an operator's `complete` (1.0) outweighs the system's rule and an audit.
  - **True** once a learning run has read the 24 hours after the task's end (`learning.last_run.at_unix_ms >=
    ended + FALSE_COMPLETION_MS`). Waiting for the run, not just the clock, matters: the system label is written only
    by the nightly run, so a clock-closed window the run hasn't read could hide a near-identical task.
  - **`null`** until then.
  - **"The audit says done" needs no audit:** where none ran, nothing says not done. An audit's `complete` counts
    like any label.
- **Nudges.** `nudges` and `unnecessary_nudges` are 0: no nudge is recorded before 26b. The method says this in a
  note, not as data.

**Proof** (`tests_prove.rs`). The store is seeded with ten invented tasks; one task.ended row is written twice.
- `the_ledger_gives_exact_records` checks every field of six records, in both arms:
  - judge spend from two packs;
  - an operator's `complete` over an audit's `not complete`;
  - a Jev continue followed by a stop;
  - a failed execution;
  - a `loop.v2` judgment in the other arm;
  - an open window.

  It also checks the four left-out reasons.
- `an_open_window_is_no_outcome_yet`: before the run has read the window, success is `null`; a near-identical label
  or a failed execution is still `false`.
- `the_records_file_gives_the_same_report`: the JSONL parses back to the same records and the same Markdown.
- `known_cohorts_give_exact_rates`: 40+40 tasks give completion 0.9 / 0.6, completions per USD 2.25 / 1.2, and
  `canary_better`. A control arm of 12 gives "insufficient" with `control arm: labeled tasks: 12 of 30`.
- `judge_spend_counts_in_the_whole`.

**Planted reverts.** Each file was restored and `touch`ed afterwards, and `git status` was clean.
- Judge calls left out of `spend_micros` (`spent.max(judge)`) → 4 tests fail: `judge_spend_counts_in_the_whole`,
  `known_cohorts_give_exact_rates`, `the_ledger_gives_exact_records`, and `the_method_is_the_generator_over_its_records`.
- An open window read as success (`closed = true`) → 3 fail: `an_open_window_is_no_outcome_yet`,
  `the_ledger_gives_exact_records`, and `the_method_is_the_generator_over_its_records`.
- The arm read from another pack's judgment (the `pack != LOOP_PACK` filter dropped) → 2 fail:
  `the_ledger_gives_exact_records` and `the_method_is_the_generator_over_its_records` (task g becomes `both_arms`).

## Step 2: the method (`b133c40c`)

- **The call:** `judge.prove { since?, until?, min_tasks?, min_labeled?, records? }`.
  - It builds on the blocking pool and runs `theseus_judge::prove::prove` with `ProveMinimum`.
  - It answers: `report` (the generator's JSON), `markdown`, `verdict`, `arms`, `left_out`, `tasks`, `notes`,
    `window`, `since_ms`/`until_ms`, `elapsed_ms`, and the JSONL `records` when asked.
  - The protocol carries the report as JSON, since it depends on no crate of ours.
- **Writes nothing (found in testing).** The first version took the default window from `ladder().rows_of()`. On a
  daemon whose ladder wasn't warm (the judge off, so no `warm_ladder`), that first load writes the adoption table's
  three `pack.mode` rows. The method now reads `pack.mode` rows straight from `pack:loop`'s scope, and the no-frame
  test holds it.
- **Default window:** since `loop.v1`'s latest non-declined move to `canary`, by that row's time; else everything.
  I think that's right for one canary. If a canary is rolled back and promoted again, the window restarts at the new
  move, and earlier canary tasks need `--since`.
- **Days:** `--until` is the whole local day: up to the next local midnight, DST-safe through `local_midnight`.
- **Dispatch:** `rpc/server.rs`'s `dispatch` was already at clippy's 100 lines. Rather than add a line, I replaced
  the `LEARNING_REPORT` arm with one arm for both reads (`m @ (LEARNING_REPORT | JUDGE_PROVE) => self.rpc_ledger(…)`,
  in `rpc/judge_prove.rs`). push-once also edits server.rs, so expect a small conflict on that line.
- **Reads:** `task.ended` by kind tag with the window. Per task: its execution (kernel), and its session's
  `judge.call` and `turn.ended` by kind-and-session tag. Plus `judge:loop`'s scope (`read_scope`), and
  `learning.last_run`.
- **Time on 10,000 tasks** (`ten_thousand_tasks_read_in_time`, `#[ignore]`, run by hand): **1,837 ms** in a debug build
  on this 4-core VM, each task with a turn, a judgment and its end. Nothing is on the start path.

**Proof:**
- `the_method_is_the_generator_over_its_records`:
  - the Markdown equals `markdown(prove(parse_records(records)))`, byte for byte;
  - the JSON report is equal;
  - `last_position` is unchanged, so no frame was written;
  - arms 3/3 and four left out;
  - "insufficient" with `canary: 3 tasks, 3 labeled, 3 successes` and `canary arm: labeled tasks: 3 of 30`;
  - the caller's minimum is applied.
- `the_default_window_is_the_canarys`: a task that ended before the move isn't read, one after it is, and `--since`
  overrides.
- `every_method_has_its_dispatch_arm` passes. TypeScript regenerated.

## Step 3: the CLI (`c59813e0`)

`theseus judge prove [--since D] [--until D] [--min-tasks N] [--min-labeled N] [--records <path|->] [--json]`.
- **stdout** is exactly the generator's Markdown, so `theseus judge prove > a.md` equals `theseus-judge prove <file>
  --markdown -`.
- **stderr** carries what the daemon read: the window, records by arm, left out by reason, the classification line,
  the note, and the time.
- `--records -` writes the JSONL to stdout instead of the report. `--json` is the whole answer.
- It is a read, so it isn't in `OPERATORS`.
- Test: `judge_prove::tests::what_was_read_is_said`.

**I ran live checks 1 and 3 myself** on a scratch daemon of this build: fresh state dir, `[discord]` and `[web]` off,
`[judge] enabled = true`, unset `env:` keys.
1. `theseus judge prove` → "## Verdict: insufficient", with canary 0 and control 0 and "labeled tasks: 0 of 30" for
   each arm.
2. Then `--records /tmp/scratch/prove.jsonl` (empty) and `theseus-judge prove … --markdown -`: `cmp` says identical.
3. `theseus packs promote loop.v1 --canary 0.5` gave "forced short of the bar", and the window line then said "since
   loop.v1's move to canary 0.5 on 2026-10-05".

## Step 4: classification (`c92b7bea`)

`classify_quality`: decision quality on `should_promote`.
- **Comparison:** `classify.v1`'s lean against its baseline, the model's own `task.create` in that turn. The baseline
  is read from the system's `task_create` label by its key, not derived again.
- **Truth:** operator and audit labels only, resolved heaviest first. The system label is the baseline itself, so it
  can't also be the answer.
- **Counting:** labeled-but-not-yet-system-labeled judgments are counted, not compared.
- **Verdict:** McNemar's test on the discordant pairs at 95% (`jev_better` / `baseline_better` / `no_difference`), or
  `insufficient (compared n of min)`.
- **Where it shows:** in the answer as `classification`, and on the CLI's stderr.

`kind` is not compared: its baseline (`conversation`) makes no class, and slash commands are not judged (25a). Comparing
it needs a recorded baseline class per message: `control` for a slash command, `new_ask` when the turn called
`task.create`.

Test: `classification_is_jev_against_the_models_own_task_create` checks exact counts, the window, a system-only label
not taken as truth, and both verdicts. Planted revert (system labels allowed as truth) → that test fails.

## The live check for the maintainer

Use a scratch daemon of the install build with a GLM key and Jev's: fresh state dir, `[discord]` and `[web]` off,
`[judge] enabled = true`.
1. `theseus --socket $S judge prove` → stdout "## Verdict: insufficient" with "canary: 0 tasks" and "control: 0
   tasks"; stderr "records by arm: canary 0, control 0" and "every task that ended: loop.v1 has not moved to canary".
2. `theseus --socket $S packs promote loop.v1 --canary 0.5` (its line says `forced`). Then three `theseus --socket $S
   ask "<a request the model should make a task of>"` messages that make the model call `task.create`. Wait for the
   tasks to end, then `theseus --socket $S judge prove`:
   - stderr: "since loop.v1's move to canary 0.5 on <today>", and "3 finished tasks read; records by arm: canary a,
     control b" with a+b ≤ 3. A task whose turn wasn't judged (sampling, or Jev unreachable) is `never_judged`.
   - stdout: "insufficient", with "N tasks, 0 labeled": success stays `null` until a learning run has read 24 hours.
3. `theseus --socket $S judge prove --records /tmp/prove.jsonl > a.md && theseus-judge prove /tmp/prove.jsonl
   --markdown - > b.md && cmp a.md b.md` → no output, exit 0.

## Left open, and choices the owner should hear

- **26b.** When the nudge lands, it must record the nudge (a node or row naming the judgment) so `nudges` and
  `unnecessary_nudges` can be filled, and say which budget pays a canary judgment. The record skips non-`shadow`
  rows' costs, on the assumption that the execution then pays them.
- **Success waits for a learning run** that has read the 24 hours. With the judge on, the nightly run does that. On
  demand, `theseus judge report` writes `learning.last_run` too.
- **`should_stop` from a `not:<other class>` label is `null`**, not false (see step 1).
- **The cockpit's Judgment view** (not built here) should show:
  - the verdict, with its reasons;
  - per arm: tasks, labeled, successes, spend (judge share), completion per task and per USD with intervals, and
    the spend ratio;
  - left out by reason;
  - the window;
  - the classification row.

  Every field is in `JudgeProveResult` and `report`.
- **Docs to change** (the maintainer's):
  - the spec's Part III item for row 50;
  - `docs/status.md`: the roadmap row and the recently landed step;
  - m5-judgment.md §2.9, "The prove": the definitions above (who pays; the run gates success; `task.closed`; the
    left-out reasons) and the classification baseline.
- **Disk.** This VM's disk filled once mid-session (the target dir's incremental cache, 17 GB), which made one gate
  run fail ~90 job tests with ENOSPC. I deleted `target/debug/incremental` and ran the later gates with
  `CARGO_INCREMENTAL=0`.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` was run before every commit. The last run, on `9d54a5ab`:
2,571 tests, 2,538 passed, 33 failed, 20 skipped.
- **The 33 failures are all the known L1 tests** (VM runs as root, theseus-pv6i):
  - theseus-sandbox's `contract` clauses and its bench's `spawn_100`;
  - theseusd's `sandbox` tests.
- **Flaky:** `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` (theseus-81ig, the put-back check
  on the flaky list) passed on try 3.
  - On the gate before `c59813e0` it failed all three tries: "no series was put back", all invariants held. It then
    passed alone and in the next gate.
  - The branch touches nothing the sim links.
- **Also from that gate:** `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` failed once (15.8 s, at load).
  It passed alone in 0.56 s and in every other gate.
- **Phases after the suite, run by hand:** "protocol types" clean, and `cargo deny --offline check` OK (advisories,
  bans, licenses, sources). The benches are skipped by `THESEUS_GATE_NO_BENCH`.
- **Under load** (four nice-0 loops, the test at nice 19): the prove's 8 core tests passed.
