# Cloud report: proc.run steps, and fs.patch's recount (theseus-7gir.3, theseus-inw)

Branch `cloud/20261005-proc-steps`, cut from main at `80ef1dea` (store format 17 there, not the brief's 16: the
files lane had joined). Three commits on top of the task commit, and this report:

| Commit | Step |
|---|---|
| `e92f5d04` | tools: fs.patch recounts each hunk header from its body (theseus-inw) |
| `0be84690` | gate: proc.run takes steps, and the gate judges a batch as its strictest step (theseus-7gir.3) |
| `32335372` | toolrun: a proc.run batch runs its steps in turn, stopped at the first that fails, with one result (theseus-7gir.3) |

The brief's steps 2 and 3 (schema and plan; the gate) are one commit, since the gate's tests need a plan with steps
and the plan's field had nowhere to be read without the gate. Step 4 (the run) is its own commit; between the two, a
batch that passes the gate fails at its launch saying its jobs are its steps'.

## Differences from the brief, as the code had them

- Main was at store format **17** (`AttachmentContent::File`, theseus-c9l6), so the bump is to **18**. The
  maintainer renumbers at the merge. The sample I added is a format-17 `proc.run` tool call; `theseusd/tests/versions.rs`
  and `theseus-core/src/store.rs`'s format tests moved one number with it.
- `crates/theseus-tools/src/fs.rs` is not on `scripts/long-files.txt` and was at 2,491 lines of the 2,500 an unlisted
  file may have. The recount lives in a new module, `recount.rs`, and fs.rs grew 4 lines (2,495).
- `policy.explain` explains a tool, not a call (it runs the order on a probe plan), so it cannot "answer a batch"
  call by call. What it does now: its `proc.run` row carries a `steps` condition saying how a batch is judged, and
  the function the gate runs (`ToolRuntime::judge`, the place's refusal and then the order, per step for a batch) is
  the one `tests_explain`'s `gated` helper runs, so the test that holds explain to the gate holds the batch rule too.
- No system-prompt text was added or changed; the tool's description gains one sentence ("Several programs in a row
  go in one call as `steps`."), and `steps` has its own description.

## Step 1: fs.patch recounts (theseus-inw), `e92f5d04`

**Found.** `split_patch` cuts the patch into file sections, and diffy parses each; a header whose lengths disagree
with its body is a parse error ("hunk header does not match hunk"). diffy's `apply` already checks the context and
the removed lines against the file (with GNU patch's offset search, no fuzz).

**Changed.** `recount.rs`: before diffy parses a section, each `@@ -a,b +c,d @@` gets its lengths from its body
(context on both sides, `-` on the old, `+` on the new, `\ No newline` on neither); starts and the section heading
after `@@` are kept. A section whose headers all match comes back byte for byte, and its result reads as today. A
section with any recounted header adds a line to the result, `recounted 2 hunk headers in src/a.rs` (the patch's own
name for the file).

- **An empty body line** is a blank context line whose space was dropped (written back as `" "`), when another body
  line follows it in the hunk. The empty lines that end a hunk (before the next `@@` or the section's end) are its
  end, not its body, unless the header counts them: then as many as it counts are context, and the header stands.
- **`-- ` beside `++ `**: a removed line `--- x` followed by an added line `+++ y` still reads to `split_patch` as a
  file header and cuts the section there. Today the patch then fails (the second "file" is not found) and nothing is
  written; a test holds that (`a_removed_dashes_line_beside_an_added_pluses_line_splits_the_section`). Not widened.
  A fix would have split_patch read hunk bodies by their (recounted) lengths, or not start a new section inside a
  hunk; it is a change to split_patch, which the brief left alone.

**Proved.**
- `theseus-tools`: 42 tests, all pass. New: `recount::tests` (4) and `tests_patch` (7): counts off by one either
  way (and the new side alone) apply and say "recounted"; two hunks recounted are named once with their count; right
  counts read exactly as before; an empty line inside a hunk is blank context; a recounted hunk whose context does
  not match still fails ("the patch does not apply to src/a.rs") and writes nothing; all-or-nothing across files
  holds with a recount; the `-- `/`++ ` case. The old `patch_applies_atomically_across_files_or_not_at_all` passes.
- Planted reverts (each restored, `touch`ed, `git status` clean after):
  - recount off (`recount` returns its input): 7 fail, among them `counts_off_by_one_either_way_apply_and_say_recounted`,
    `two_hunks_recounted_are_named_once_with_their_count`, `an_empty_line_inside_a_hunk_is_a_blank_context_line`.
  - a recount that skips the context check (context lines dropped from the body and the counts): 8 fail, among them
    `a_recounted_hunk_whose_context_does_not_match_still_fails` (diffy then finds `-two` at an offset and applies).

## Steps 2 and 3: the schema, the plan, the gate (theseus-7gir.3), `0be84690`

**Changed.**
- `proc.run`'s input: `steps: [{argv, cwd?, timeout_secs?}]` beside `argv`, exactly one of the two, checked in
  `plan()`: "give argv (one program) or steps (programs run in turn), not both"; "argv must name a program (or
  steps, programs run in turn)"; "steps must hold at least one step"; "step 2's argv must name a program"; "steps
  holds 17 steps, more than 16: split the batch". **The cap is 16** (`proc::MAX_STEPS`, also the schema's `maxItems`).
  A step has no `env` or `sandbox` (unknown fields are refused): those are the call's, for every step. The call's
  `cwd` and `timeout_secs` are each step's default. The schema has no top-level combinator: `argv` is no longer in
  `required`, and plan() holds the one-of.
- The plan: its summary lists every step whole ("run 3 steps in turn, stopping at the first that fails: 1. `printf
  one` in /w; 2. `false` in /w; 3. `touch /w/never` in /w"), its resources each distinct directory, `argv` none, and a
  new field `Plan.steps: Option<Vec<Vec<String>>>` with each step's argv. Store format 17 → 18, with a format-17
  `proc.run` tool-call node as a literal in `tests_layouts.rs`. `cockpit/src/protocol.gen/Plan.ts` regenerated.
- The tool contract (`theseus-tools/src/lib.rs`) gains two defaulted methods: `Tool::steps` (each step as the call it
  would be alone: its argv, cwd and timeout or the call's, and the call's `env` and `sandbox`) and `Tool::jobs` (each
  step's job, every directory checked before the first starts; one job for any other tool).
- The gate (`toolrun/batch.rs`, `ToolRuntime::judge`): for a batch, each step's plan, then the place's refusal, then
  the whole order (`order.rs`, unchanged), per step. A refusal of any step refuses the batch ("step 2 of 3 (`x`): …").
  Else the strictest, by (floor, posture): the floor over approve over notify over open, a tie keeping the earlier
  step; its reason, and its notice's rule, start "step 2 of 2 (`op whoami`): ". `granted` joins every step's grant.
  The job's class (L0/L1) is the strictest step's; `sandbox` is the call's, so every step's is the same.
- One approval binds the batch: the proposal is the whole input, so its digest covers every step, and a batch that
  differs in any step is another proposal and asks again.
- Outside text: `external::Listed::of` reads each step's argv, so a batch with a `gh` step marks its result as a
  listed program's (before, it read only a top-level `argv`).
- The kernel deadline (`deadline_ms`) is the sum of the steps' timeouts plus 30 s.

**Proved.** `tests_steps` (gate part): `a_batch_takes_its_strictest_steps_posture` (all steps on `allow_argv` run
open; one that is not runs at the tool's notify, its reason naming step 2); `a_step_on_the_floor_makes_the_batch_wait_as_the_floor`
(`op whoami` as step 2 at enforcement open: approve, `floor: true`, reason names the step, step 1 not run);
`a_batchs_deadline_covers_every_steps_timeout`. `proc::tests::steps_or_argv_exactly_one_and_the_plan_lists_every_step`
(both, neither, empty steps, an empty step argv, an unknown step key, 17 steps, the summary, the per-step inputs,
the jobs and a missing step directory). `tests_explain::a_batch_is_judged_as_its_strictest_step` (the batch's
posture and reason are its strictest step's alone, prefixed; explain's row has the `steps` condition), and the
existing `policy_explain_agrees_with_the_gate_for_every_tool_and_place` (its probe input for proc.run is now given,
since argv is not required). `tests_layouts::every_old_layout_on_disk_still_reads` with the new sample.

Planted revert, the gate judging only the first step (`.take(1)`): 3 fail, `a_step_on_the_floor_makes_the_batch_wait_as_the_floor`
(the batch ran `op whoami` at open), `a_batch_takes_its_strictest_steps_posture`, `a_batch_is_judged_as_its_strictest_step`.

## Step 4: the run, `32335372`

**Design.** In the core (`toolrun/steps.rs`, beside `run_job`), not the wrapper. `run_job`'s launch (env, broker,
wrapper args, the stop checks, the launch, `tool.job_started` and the grant rows) is now its own function, `launch`,
which the single job and each step share; the single-job path is otherwise as it was. Every step runs as a job under
the call's **one** action and correlation id:
- a `/stop` or a cancel finds the running step's pid in the spool under that id, and `launch`'s check of the action
  (`told_to_stop`) starts no more steps once one came;
- the turn waits on the id from before the first launch to the end of the batch, so the drain leaves every step's
  completion to the turn: a step that exits 0 before the last is taken off the spool by the turn, never by the
  kernel, and the action stays `dispatched` between two steps (the reconciler sees a dispatched action as it would
  for one job);
- a race I found and closed: a step's wrapper writes its completion, then removes its pid file. The next step's
  launch writes the same pid file, so the core waits (at most 2 s, on tokio's timer) until the finished wrapper is gone
  or lingering before it launches the next. The finished step's raw output is unlinked, never truncated, so a process
  it left holding the file writes on to its own inode;
- the step that ends the batch (the last; one that exits non-zero or times out; one a stop killed) is looked at as
  one job is (`look_at_job`) and settles the call with its completion, in one frame with the result.

**The result.** One result: the steps before as blocks, `[step 1 of 3: `printf one` in /w]`, `[exit code 0, 1 ms]`,
the end of its output; then the ending step's block as one job's result is (`[step 2 of 3: …]`, `[exit code 1]`,
its output, its cap lines); then each step not run, `[step 3 of 3: `touch never` in /w: not run]`. `meta.steps`
has every step's row (`step`, `argv`, `cwd`, `ran`, and for those that passed `exit_code` and `duration_ms`).
**The room:** the steps before the ending one share at most a quarter of `result_max_chars`, each keeping the end of
its output (saying how much it left out); the ending (failing) step gets the rest, and the whole is then capped by
`toolrun::cap` as one job's result is.

**The wait.** The batch's: `proc_sync_secs` (capped at the steps' timeouts plus 5) from the first launch. A step
still running at its end goes on in the background as one job does: the answer lists the steps done, says "Still
running as background job act_… after N seconds … A batch stops at a step that goes on in the background: the steps
after it will not run.", and names the rest as not run. That step's completion settles the call (the drain's, or a
restart's), and its late result opens "[the batch's step that went on in the background; the steps after it were not
run]". The steps after a background step never run: continuing them from the drain would need state the drain does
not have (a design choice the owner should hear about; see below).

**A restart mid-batch** settles the call as one job's does: the wrapper of the running step runs on and reports,
and the restart takes that completion. Its result is that step's alone (the steps before were held in the turn's
memory); the steps after it never start.

**The shell-fallback ratio.** A batch is one `proc.run` call, so it counts once as one escape to the shell, however
many steps it has (the ratio counts calls). Two `tool.job_started` rows are written for a two-step batch, both with
the call's correlation id.

**Proved.**
- Core, `tests_steps` (8 tests, `InlineLauncher`: the real wrapper on a thread): the brief's batch (`printf one`,
  `false`, `touch never`): one result, error, marker absent, each step named, two `tool.job_started` rows under one
  correlation id, the action failed once; every step passing runs all three; `one_approval_runs_every_step_and_a_changed_batch_asks_again`
  (one card, the `tool.confirm_requested` row's input lists both steps, one approval launches both, a batch differing
  in its last step asks again); `a_step_past_the_wait_goes_to_the_background_and_the_rest_are_not_run`
  (`proc_sync_secs = 1`, a `sleep 3` step: Background, step 1 done, the background id, step 3 not run, and its
  marker still absent 4 s later); `a_stop_during_a_step_starts_no_more` (result Cancelled, step 1's block in it, two
  launches, no marker).
- Daemon, `theseusd/tests/steps.rs` (2 tests, real wrapper processes): `a_stop_during_the_second_step_kills_it_and_starts_no_third`
  (execution.stop during step 2: its process is gone, `never` absent, action `cancelled`);
  `a_restart_with_a_step_running_settles_the_call` (kill -9 of the daemon during a 2 s step 2, restart: the action
  settles `succeeded` from step 2's own report, `never` absent).
- Under load (AGENTS.md's recipe, but `yes > /dev/null` as the four nice-0 busy loops: this environment refused a
  `sh -c 'while :; do :; done'` command line as a possible removal, so I took that route instead; killed by their pids):
  `tests_steps`, `tests_jobs`, `theseusd::steps` and the explain batch test at `nice -n 19`, 5 runs: 13/13 each time
  (and 5 earlier runs of the core ones alone: 11/11 each).
- Planted revert, a failed step that does not stop the batch (any completion taken as passed): `a_failed_step_stops_the_batch_and_its_one_result_names_each_step` fails (the third step ran).
- The request's system blocks: no system text is touched; `tests_output`'s golden and the compiler's tests pass
  unchanged. The tools part of the request changes (proc.run's description and schema), so its `tools_digest`
  moves, as for any tool change.

## The live check (the maintainer's)

I ran these on a scratch daemon here, and each showed what is written. A fresh dir, the stand-in model, Discord,
the web and the index off, the key from the environment:

```bash
L=$(mktemp -d); mkdir -p $L/projects $L/state; printf 'tide\nebb\nflow\n' > $L/projects/notes.txt
cat > $L/config.toml <<EOF
[model]
api_base = "http://127.0.0.1:9448"
[secrets]
anthropic_api_key = "env:LIVE_FAKE_KEY"
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
[tools]
projects_dir = "$L/projects"
[policy]
enforcement = "notify"
EOF
cat > $L/rules.json <<EOF
[
 {"when": "run the batch", "calls": [{"name": "proc_run", "input": {"steps": [{"argv": ["printf", "one"]}, {"argv": ["false"]}, {"argv": ["touch", "$L/projects/never"]}]}}]},
 {"when": "patch short", "calls": [{"name": "fs_patch", "input": {"patch": "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,2 +1,2 @@\n tide\n-ebb\n+EBB\n flow\n"}}]},
 {"when": "patch wrong", "calls": [{"name": "fs_patch", "input": {"patch": "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,3 +1,3 @@\n tidal\n-EBB\n+ebb\n flow\n"}}]}
]
EOF
theseus-sim fake-model --rules $L/rules.json &   # note its pid
LIVE_FAKE_KEY=sk-fake theseusd --config $L/config.toml --state-dir $L/state --socket $L/sock &
```

1. `theseus --socket $L/sock ask "run the batch"`, then `theseus --socket $L/sock history`: one result, `← proc.run
   error · exit 1`, reading `[step 1 of 3: `printf one` in …] ⏎ [exit code 0, … ms] ⏎ one ⏎ [step 2 of 3: `false` in
   …] ⏎ [exit code 1] ⏎ (no output) ⏎ [step 3 of 3: `touch …/never` in …: not run]`; `ls $L/projects` has no
   `never`. `--json history` shows the plan's `steps` and its summary with all three, and the result's `meta.steps`.
2. `theseus --socket $L/sock shutdown`, set `enforcement = "approve"`, start again, `ask "run the batch"`: it parks
   (exit 6) with one card, `theseus --socket $L/sock confirm` lists it once with the whole input; `theseus --socket
   $L/sock confirm <act_…>` runs steps 1 and 2 (two `→ proc.run` lines) and stops. `theseus --socket $L/sock --json
   ledger`: one `tool.confirm_requested` row whose `input` has the three steps, and a `tool.job_started` row for each
   step that ran, both with the call's correlation id. (Set `enforcement` back to `notify` for 3.)
3. `ask "patch short"`: `notes.txt` reads `tide EBB flow`, and the history's result says `Applied to 1 file: ⏎
   …/notes.txt +1 -1 ⏎ recounted 1 hunk header in notes.txt`. `ask "patch wrong"` fails, "the patch does not apply to
   notes.txt: error applying hunk #1", and the file is unchanged.

Stop with `theseus --socket $L/sock shutdown` and kill the fake model by its pid.

## Left, uncertain, and for the owner

- **The steps after a background step never run.** The brief's form; continuing a batch from the drain would put
  the loop where the turn is not (the drain would need each step's spec, durably, and to launch from the heartbeat).
  Likewise a restart mid-batch settles the call from the running step alone: the earlier steps' blocks were held in
  the turn's memory and are not in its late result. Steps' blocks could be made durable (a row per step) if wanted.
- **The approval card's reason** names the strictest step and that step's own summary ("step 1 of 3 (`printf one`):
  run `printf one` in …: proc.run — approve …"); the card's input, and the plan's summary in the record, carry every
  step. The surfaces that render the card from the reason alone (Discord's, the cockpit's) may want the plan's summary
  beside it. Not changed here (Discord's render.rs is at its ceiling).
- **The cockpit's call view** (not changed, only `Plan.ts` regenerated): for a batch it should list `plan.steps`
  (each argv, from the summary each directory) where it lists `plan.argv` today, and render the result's
  `meta.steps` as a row per step (exit, time, ran or not).
- **The narrative's subject** for a batch is its directory (the plan has no `argv`), as `proc.run in /w`; a subject
  naming the steps ("3 steps") would read better. The CLI's `→ proc.run [argv] pid` lines already come per step.
- `Plan.steps` carries argv only; each step's directory is in the summary, not a field (a field would need a new
  protocol type, and `ts.rs`'s list is at its line limit).
- Docs the maintainer may want to touch: the spec's `proc.run` section (§3.23) for `steps`, the cap of 16, and how
  the gate judges a batch; `docs/technical-overview.md` where it describes a job's call; `fs.patch`'s recount where
  the tools are listed.

## The gate

Each commit's gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`) failed in its suite phase only on
the cases the brief names, then the phases after it passed by hand (protocol types unchanged from the commit,
`theseus-sim bench turn --check --runs 5 --burst 0`: 5 and 9 frames at the p95, ok; `cargo deny --offline check`:
ok):

- `e92f5d04`: 2,573 tests, 33 failed, all L1 (theseus-sandbox's contract tests and `spawn_100`, theseusd's
  `sandbox` tests: a root VM with no job cgroup, theseus-pv6i).
- `0be84690`: 2,578 tests, the same 33.
- `32335372`: 2,585 tests, the same 33, and `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`
  (a pty's `back` not on the screen in 15 s, under the suite's load; it passes 3 of 3 alone). Not on the brief's
  list; my change does not reach terminals.
- An earlier gate of the whole tree also failed once on `theseusd::crash a_panic_leaves_a_crash_file_and_the_next_start_reports_it`
  (`this_start` was true on the third start, under load); it passed 3 of 3 alone. Not on the brief's list.
- `the_deadline_stops_the_whole_tree_too` (theseus-g11i) did not fail in any run.

No new dependency; Cargo.lock and the package locks are unchanged. Nothing under bench/ changed.
