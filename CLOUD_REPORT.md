# Cloud report: step 42a, `budget.list` and `policy.explain` (theseus-ext.7)

Branch `cloud/20261004-budgets-policy`, from `main` at 35784d7 (with the task commit bea2e0f on top).
Session started 13:02 UTC and ended about 14:30 UTC, well inside the 4-hour deadline.

| commit | subject |
|---|---|
| fabe50b | rpc: budget.list and theseus budgets, where the money is (theseus-ext.7) |
| 2a0c60d | toolrun: the gate's order in one place, and policy.explain on it (theseus-ext.7) |

No new dependencies. Cargo.lock and the package-lock files are unchanged. No store format change: both reads only
read.

---

## Step 1: `budget.list` and `theseus budgets` (fabe50b)

### What I found
- An execution's `Budget` (theseus-kernel `types.rs`) has the limit, spent, reserved, held unknown, resets, the
  question with `question_needs_micros`, and `pinned`. A task is `pinned` with `parent` set. Its carve is the
  parent's reservation `task:<task exe>` (`tasks::carve_key`), which shrinks as the task spends. A task's settled
  spend is also added to the parent's `spent`.
- 38a's `place_limit` pins a conversation while a place's ceiling caps it, and unpins it when the cap goes. So a
  pinned execution with no parent is either a place's (when `TurnRunner::view_of(session).ceiling` has a
  `spend_limit_micros`) or its own (`open_execution` with a limit, for example an MCP client's session).
- The `budget.reset` row has `execution_id`, `by`, and `spent_before_usd`. `ledger.tail`'s indexed page by kind and
  session tag (`Store::ledger_page`) costs about its answer, and returns `None` while the index's shape is built
  after a start.
- The judge is on main (`runner.judge.health()` gives the day, spend, limit, and paused).

### What I changed
- `theseus-protocol/src/budgets.rs`: `BudgetListResult`, `BudgetRow` (with `tasks: Vec<BudgetRow>`),
  `BudgetResetInfo`, `BudgetQuestionInfo`, `BudgetTotals`, `JudgeDayBudget`, and a shape test. lib.rs gets only the
  `mod`/`use` lines and the `BUDGET_LIST` method line.
- `theseus-core/src/rpc/budgets.rs`, `Core::budget_list`:
  - Rows: `Kernel::open_executions` (state terms), each session record by key, and the place view.
  - `limit_from`: `carve` (with the parent's session), `config`, `place` (with the place's name), or `pinned`.
  - `carve_held_usd`: the parent's reservation for the task now.
  - The last reset comes from one ledger page of the newest 8 `budget.reset` rows of the session, skipping rows of
    an earlier execution of that session. While the index is unshaped, `last_reset_unread` says why, and nothing
    is scanned.
  - Tasks sit under their open parent. A task whose parent has ended is listed on its own.
  - Totals: limit, spent, reserved, held, and available add the top rows only, summed in micro-dollars. A task's
    spend is its parent's too, and its carve is the parent's reservation, so adding the task rows would count
    them twice. Lifetime adds every session's `cost_usd`, since each session counts its own.
  - The judge's day budget is a line of its own.
- The server dispatch arm. `methods::ledger_tags` became `pub(super)`.
- CLI: `crates/theseus/src/budgets.rs` (the table, each task under its parent with `└`, then carve and question
  lines, the totals, and the judge's line), `Cmd::Budgets` in main.rs, and a help line.
- `scripts/long-files.txt`: the protocol's lib.rs ceiling went 2629 → 2634 in this commit (2639 after step 2), with
  the reason.
- `crates/theseus-core/AGENTS.md`: a paragraph on the two reads.

### How I proved it
- `tests_budgets.rs` (core), each comparing every row with `kernel.execution()`'s record (limit, spent, reserved,
  held, available, resets, question, parent) and the totals with the sum of the top rows:
  - `budget_list_after_a_carve_a_reset_and_a_place_limit_agrees_with_each_record`:
    - two carves ($3 and $2), the parent's limit `config`, and its reserved $5;
    - a question (`needs_usd`), then a reset by `cli` whose `at_ms` equals its row;
    - then a `#pier` ceiling of $40 (`place_spend`) shows `place`/`#pier`/$40, and without the cap it is `config`
      at $100 again.
  - `budget_list_follows_a_changed_config_limit_and_a_carve_keeps_its_own`: the 3pj rule across a restart (the
    core rebuilt on the same store with `spend_limit_usd` 100 → 60). The conversation follows to $60 `config`, and
    its task keeps its $4 `carve`.
  - `the_last_reset_is_read_by_one_page_however_much_history_follows`: the reset is found behind 5,000 later rows of
    other kinds and sessions. A window scan of the newest rows, which `ledger.tail` falls back to, would not reach
    it. A newer reset row of an earlier execution of the same session is skipped for the execution's own.
- **Reading no history: how it is shown.**
  - Structurally, the only reads are the open executions by state terms, `execution(parent)` by key, the session
    record by key, the outbox's place by key, and `Store::ledger_page` with tags and `limit: 8`.
  - When the page is unavailable the code returns `last_reset_unread` instead of calling any scan.
  - The 5,000-row test shows the answer does not depend on how much history follows the reset.
  - I did not build a read counter into the store. That would be the stronger proof, and the store has none today.
- Planted revert: every row's `limit_usd` taken from `self.cfg.kernel.spend_limit_usd`. Both carve tests failed,
  for example `left: 100.0, right: 3.0` on `exe_lamp1` and `left: 60.0, right: 4.0` on `exe_buoy3`. Restored,
  `touch`ed, and `git status` was clean.
- CLI goldens: `budgets.txt`, `budgets_json.txt`, `budgets_none.txt` (`tests/golden.rs`
  `budgets_prints_each_task_under_its_parent_and_the_totals`).
- Protocol: `budgets::tests::a_budget_list_keeps_its_shape_on_the_wire`. The TypeScript was regenerated and added.

---

## Step 2: the gate's order in one place, and `policy.explain` (2a0c60d)

### What I found
The gate's decision for a planned call was a chain inside `toolrun::gate`'s closure:
1. `places::refusal`, then `Ceiling::refusal`.
2. `sandbox::decide`: L1's decision with the egress step, or `ToolPolicy::decide_with`, and inside it the broker
   (`ToolRuntime::brokered`).
3. `lsp::gate`.
4. `places::private_fetch`.
5. `Ceiling::floor`.
6. `external::exempt`, `held`, and `gate` (T1's hold).
7. `mcp_server::floor`.

A second copy of that order for explain would drift.

### What I changed
- **`toolrun/order.rs`**:
  - `At { place, held, mcp }`: what the gate reads of where a call runs. The hold and the MCP floor are closures,
    so a read still costs no record read.
  - `ToolRuntime::refusal`: the place's refusal, then the ceiling's.
  - `ToolRuntime::order(at, tool, plan, input, seen)`: the whole chain after the refusal. It hands `seen` the
    decision after each `Layer` (Policy, Grant, Lsp, SharedFetch, Floor, Hold, McpClient).
  - `toolrun::gate` now builds `At` from its `TurnCtx` and calls these, with a watcher that does nothing.
  - `sandbox::decide` became `sandbox::unbrokered` (the same body without the final `brokered`), because the
    broker is the order's next layer.
  - `toolrun::mcp_floor_of(kernel, execution_id)` is shared by the gate and explain.
  - Nothing the gate decides changed: policy.rs, external.rs, places.rs, broker.rs, and ceiling.rs are untouched,
    and the core's output golden (`tests_output`) is byte-identical (it passes under a non-UTC TZ; see the gate
    section).
- **`theseus-protocol/src/explain.rs`**: `PolicyExplainParams { session_id?, tool? }`, `PolicyExplainResult { places,
  roots }`, `PlaceExplain`, `ToolExplain`, `ExplainLayer { layer, says, setting?, result, raised }`,
  `ExplainCondition { layer, when, entries, then }`, and a shape test.
- **`theseus-core/src/rpc/explain.rs`**, `Core::policy_explain`:
  - For a session it builds the place the way the turn's gate reads it: `runner.view_of`, `external::held`, and
    the execution's MCP floor.
  - Without a session it explains the CLI, then each bound place, with the hold of the session the place runs on
    (`outbox.place_session`).
  - For each tool it builds a probe plan: one resource at the first workspace root (the first public path for a
    file tool in a shared place), with the tool's class as its access.
  - It runs `rt.refusal` and `rt.order` on the probe, and each row is the decision the order handed its watcher.
  - Rows: `place`, `ceiling`, `class` (L0 or L1, for job tools), `posture` (the config's setting: `[policy.tools]`,
    `[policy.mcp]`, `[policy.aws]` class, or enforcement), `tightening`, `grant`, `lsp` (lsp tools), `floor` (the
    ceiling's), `hold`, and `mcp_client` (MCP-client sessions).
  - The call-dependent layers are conditions with their entries: `public_paths`, `floor` (floor paths, plus the
    floor argv and an AWS guardrail where relevant), `approve_paths`, `outside_roots`, `private_address`,
    `approve_argv`, `allow_argv`, `grant` (programs granted secrets), `destructive` and `aws` (`[policy.aws]` lines),
    `l1` (`l1_argv`), and `lsp_start`.
  - A tool the place does not offer has `offered: false`, `refused` with the gate's words, and result `refused`.
- **CLI**: `theseus policy explain [--session <id>] [--tool <name>]` (`crates/theseus/src/policy_explain.rs`).
  - Without `--tool`: one line per tool, giving its result and the layers that set it.
  - With `--tool`: every layer, the conditions, and the gate's reason.
  - `theseus policy` (the short list) is unchanged.
- Small edits outside the new files:
  - `tests_places.rs`: `rig_setup` (a store hook before the core is built) and `Rig.root` made `pub(crate)`.
  - `cmd.rs`: the `Explain` arm.
  - core AGENTS.md: the invariant now names `order.rs`, and the "Grants in L1" text names `sandbox::unbrokered`.
  - long-files: 2639.
- I also changed step 1's `the_last_reset…` test to write its 5,000 rows ten frames at a time. Written a frame per
  row, it timed out at nextest's 120 s under the load recipe.

### How I proved it
- `tests_explain::policy_explain_agrees_with_the_gate_for_every_tool_and_place`:
  - The setup: the template's config (`Config::example`, 19 built-in tools) plus one MCP tool (`mcp:fake/echo`)
    from a stored list on the fake server's config.
  - Four places: the CLI, `#lab` (private, `posture_floor = "approve"`), the owner's DM, and `#pier` (shared).
  - Each in three states: as configured, then with `proc.run` and `fs.write` tightened, then with every session
    holding external text.
  - For every tool, a real input inside the roots and the public paths is planned by the tool's own `plan`. Its
    decision through `rt.refusal` + `rt.order`, with the place read by `view_of` and the hold by `external::held`,
    must equal the explanation's `result` and the gate's words after the summary.
  - Every tool but `term.send` is compared in every place (76 of 80 rows each pass). `term.send` needs a terminal
    `term.open` gave, and the test asserts it is the only one left out.
  - It also checks:
    - the floor's row is raised in `#lab`;
    - `proc.run` and the MCP tool are refused in `#pier`;
    - the tightening row is raised;
    - no read's hold row is raised;
    - a held `fs.write` waits.
- `tests_explain::each_call_a_turn_records_agrees_with_its_sessions_explanation`: whole turns through the real
  `toolrun::gate`, with each call's recorded `GateRecord` compared with the session's explanation of that tool.
  - The places: `#lab` (floored), the owner's DM, and `#pier` (shared). In `#pier` the `fs.read` of `notes.md` is
    outside the public paths, which explain lists as a condition, so it is left out there.
  - Then a CLI session after `tighten proc.run`, and a session holding external text, where `proc.run` waits for
    approval.
- Planted revert: explain's `At` built without the place's ceiling (`PlaceView { ceiling: None, ..s.view }`).
  - The table test failed: `#lab: fs.edit explains notify but the gate decides approve (… — approve (#lab's ceiling
    sets a floor of approve))`.
  - The turn test failed on `#lab`'s `proc.run` (explained `notify`, recorded `approve`).
  - Restored, `touch`ed, and `diff` was clean.
- CLI goldens: `policy_explain.txt`, `policy_explain_tool.txt`, `policy_explain_json.txt`.
- Protocol: `explain::tests::an_explanation_keeps_its_shape_on_the_wire`.
- Under load (AGENTS.md's recipe: the tests at `nice -n 19` beside four busy loops at nice 0, `yes > /dev/null`
  killed by their pids):
  - The filter `tests_explain|tests_budgets|tests_ceilings|tests_places|budgets|policy_explain` (30 tests) passed
    5 runs out of 5.
  - Before the frame fix, `the_last_reset…` timed out 3 times out of 3. After it, it passed every time.
- The existing gate tests stayed green: `tests_ceilings`, `tests_places`, `tests_lsp`, `tests_grants`,
  `tests_egress`, `tests_sandbox` (in-process), `tests_term`, and `tests_external`. So did the whole of
  theseus-core, theseus, and theseus-protocol: 969 tests, 969 passed under `TZ=America/Los_Angeles`.

---

## The live check (the maintainer's)

I ran this myself on this VM, in `/tmp/rig42`, with debug binaries, and every step showed what is described below.
These are the exact commands, with `$T` the build's binary directory and `$D` a fresh directory.

```sh
T=target/debug; D=/tmp/rig42a
$T/theseus-sim discord rig --dir $D --theseusd $T/theseusd
cat > $D/state/bindings.toml <<'EOF'
[[guild]]
id = "900000000000000001"
name = "proof"

[[channel]]
guild = "900000000000000001"
id = "900000000000000010"
name = "lab"
users = ["900000000000000101"]
mention_only = false
private = true

[channel.ceiling]
posture_floor = "approve"

[[dm]]
user = "900000000000000101"
name = "ana"
EOF
cat > $D/rules.json <<'EOF'
[{"when": "SPLIT NOW", "text": "Splitting.", "calls": [
   {"name": "task_create", "input": {"brief": "count the lines of the tide notes", "budget_usd": 3, "fidelity_ack": true,
     "arrangement": {"pieces": [{"quote": "the tide work into two tasks please", "role": "objective"}]}}},
   {"name": "task_create", "input": {"brief": "count the words of the tide notes", "budget_usd": 2, "fidelity_ack": true,
     "arrangement": {"pieces": [{"quote": "the tide work into two tasks please", "role": "objective"}]}}}]},
 {"when": "count the", "text": "Counting.", "calls": [{"name": "proc_run", "input": {"argv": ["wc", "-l", "tide.txt"]}}]},
 {"when": "READ-NOTES", "text": "Reading.", "calls": [{"name": "proc_run", "input": {"argv": ["cat", "tide.txt"]}}]}]
EOF
echo "high water at noon" > $D/projects/tide.txt
sed -i 's/^external_programs = \["gh"\]/external_programs = ["gh", "cat"]/' $D/config.toml
# each in its own shell:
$T/theseus-sim fake-discord --addr 127.0.0.1:9447 --gateway 127.0.0.1:9449 --guild $D/guild.json --log $D/fake.log
$T/theseus-sim fake-model --addr 127.0.0.1:9448 --rules $D/rules.json
PATH=$D/bin:$PATH OP_SERVICE_ACCOUNT_TOKEN=proof-not-a-token $T/theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state
```

1. **Budgets.** Send the split message as ana in `#lab`:

   ```sh
   $T/theseus-sim discord say --fake 127.0.0.1:9447 --channel 900000000000000010 --user 900000000000000101 --name ana "SPLIT NOW: the tide work into two tasks please"
   ```

   `#lab`'s floor makes each `task.create` wait. Approve the parent's two in turn:

   ```sh
   $T/theseus --socket $D/sock confirm
   $T/theseus --socket $D/sock confirm --no-wait <id>
   ```

   The parent's are the ones in `#lab`'s session. Leave the tasks' own questions unanswered:
   - The arrangement renders the whole quoted message, so each task's own turn also meets the `SPLIT NOW` rule.
     Its `task.create` waits at the inherited floor, which keeps the task open for the read.
   - Answered, it would be refused at depth one.

   Then:

   ```sh
   $T/theseus --socket $D/sock budgets
   $T/theseus --socket $D/sock --json budgets
   ```

   `budgets` should show `#lab`'s session (`$100.00 config`, reserved about $5) with two rows under it:
   `└ … $3.00 carve of …` and `└ … $2.00 carve of …`. Then the lines saying "its parent holds $… of its $3.00 carve"
   and "… $2.00 carve", and the DM's session as a second top row.

   In the `--json` form, `totals.spent_usd`, `reserved_usd`, and `limit_usd` should equal the sums over the top rows
   (`executions[]`), and `totals.lifetime_usd` the sum over every row, tasks included. Here it showed spent 0.00084
   = 0.00084, reserved 4.9996 = 4.9996, limit 200 = 200, and lifetime 0.00084 = 0.00084.

2. **A tightening and the floor.**

   ```sh
   $T/theseus --socket $D/sock policy tighten proc.run
   $T/theseus --socket $D/sock policy explain --tool proc.run
   ```

   The CLI section should show `posture notify … (enforcement = notify)`,
   `tightening approve ↑ tightened by the CLI …`, and `→ approve`. The `#lab` section should show the same, plus
   `floor approve #lab's ceiling sets a floor of approve (#lab's posture_floor)`.

3. **T1's hold.** Undo the tightening first, so `cat` runs:

   ```sh
   $T/theseus --socket $D/sock policy untighten proc.run
   $T/theseus --socket $D/sock ask --no-stream "READ-NOTES please"
   $T/theseus --socket $D/sock policy explain --session <the ask's session id, printed on its last line>
   ```

   The heading should say `holds external text (proc.run cat)`. The acting tools (`fs.edit`, `fs.patch`,
   `fs.write`, `proc.run`, `task.create`, `term.open`, `term.send`) should show `approve … hold:
   [policy] external_text = ask`. The reads (`fs.read`, `fs.glob`, `fs.grep`, `git.*`, `term.read`, `text.diff`)
   should keep their `[policy.tools]` posture with no hold.

Stop with `$T/theseus --socket $D/sock shutdown`, then kill the two fakes by their pids.

---

## The gate

I ran `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Both times it failed only in the suite phase.
Everything else passed: fmt, shape, features, clippy, cockpit, test build, and the reader rule. After the suite I
ran the remaining phases by hand:
- protocol types: ok after `git add`;
- `theseus-sim bench turn --check --runs 5 --burst 0`: `frames_plain` 5/5 and `frames_tool` 9/9, ok;
- `cargo deny --offline check`: advisories, bans, licences, and sources ok (`cargo deny fetch` succeeded).

The failures (second gate: 2143 run, 2108 passed, 35 failed, 17 skipped):
- **theseus-sandbox::contract (19) and theseusd::sandbox (13), plus theseus-sandbox::bench `spawn_100`.** The VM
  runs as root, and L1 refuses: "Linux exempts root from RLIMIT_NPROC" (theseus-pv6i, known).
- **theseus-core `tests_output::the_cores_output_matches_its_golden`.** It fails on this VM's timezone and is not
  caused by these changes:
  - The golden has a wake's time with a negative UTC offset (`-#:#`), and this VM is UTC (`+#:#`).
  - It passes with `TZ=America/Los_Angeles`, at both commits.
  - This one is not on the known list. The golden depends on the machine's local offset being negative; a fix
    would normalise the sign in `tests_output`'s number masking.
- **theseusd::stops `a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`.** It failed
  once, in the second gate's suite, at `stops.rs:249` (`t.lines().next().unwrap()` on an empty `job-N.term` file).
  - That looks like a race in the test: the file is read before the job has written its line.
  - It passed 3 out of 3 alone, and these changes do not touch the stop path.
  - It is not on the flaky list. Worth a look.

---

## What is left, uncertain, or for the owner

- **Totals semantics.** Money totals add the top rows only, because a task's spend and carve are already in its
  parent's figures. The lifetime total adds every session. The design said "the totals" without saying which; the
  CLI prints this rule on a line under the table.
- **`limit_from: place`** is read from the place's ceiling as the binding last told it (`view_of`). A pinned
  conversation whose place has since lost its cap, before the binding restarts, reads as `pinned`.
- **Explain's probe** is "a call inside the roots that no condition matches":
  - Its resource is the first root, or the first public path for a file tool in a shared place, with an empty argv
    and a null input.
  - So the `class` row for a job tool is the default class (L0, unless `[sandbox] default = "l1"`), and `l1_argv`
    is a condition.
  - Results for call-dependent layers are listed, not decided, as the task asked.
- **The `posture` row's text in L1** mirrors `sandbox::l1_decision`'s own-line rule (a `[policy.tools]` line, else
  L1's notify). It is a description only: the row after it carries the order's real decision. If the two ever
  drifted, the rows would disagree with each other, but the result would stay right.
- **Without `--session`**, each bound place uses the hold of the session it runs on now, and the CLI row has no
  session, so no hold. `--session` needs the full session id: there is no short-id lookup as `policy trust` has.
- **MCP client floor**: an `mcp_client` row appears only for a session an MCP client opened. It was not
  live-checked here.
- **Docs for the maintainer to write:**
  - Part III's item for 42a.
  - The spec's version line, and `docs/status.md` (42a landed; the roadmap row).
  - m7-surface.md §2.6 could note that `policy.explain` also takes `tool`, the layer names as built, that the order
    now lives in `toolrun/order.rs`, and that the totals rule is the top rows.
- **Files I touched that others may also touch:**
  - `crates/theseus-core/src/toolrun.rs`: the gate's closure moved to `order.rs`. A concurrent change to the gate's
    chain should land in `order.rs`.
  - `crates/theseus-core/src/sandbox.rs`: `decide` became `unbrokered`.
  - `tests_places.rs`: `rig_setup`.
  - `crates/theseus/src/main.rs` and `cmd.rs`: the new commands.
  - The protocol's `lib.rs`, `ts.rs`, and `server.rs`: one line or arm each.
  - `scripts/long-files.txt`: the protocol lib.rs ceiling, 2629 → 2639.
