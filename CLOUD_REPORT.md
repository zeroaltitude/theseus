# Cloud report: the learning ledger, step 25c (theseus-0j2.9)

Branch `cloud/20261004-learning-ledger`, cut from `4bae0f7` (main as cloned, with 23b, 24, 25a and 25b on it).
Started 14:29 UTC; the work ran on a 4-core VM as root, in UTC.

## Commits

| Commit | What |
|---|---|
| `f3ba424` | `judge: the learning ledger's labels, system labels, report and tender, step 25c` |
| `fb6e632` | `cockpit: the Judgment section's label buttons and learning report, step 25c` |

## 1. Labels, system labels, the report, holdouts, the tender (`f3ba424`)

### What I found

- 24's press already writes `judge.label` rows (`lbl_…`, scoped `judge:security`, operator, 1.0, `risky: true`,
  note "should have asked"). I kept that kind and shape and added one field, `rule` (null for a person's label;
  the rule's name for a system label, which is part of its key). No 28b label rows are on main.
- `learn.rs` has every function the report needs. `learn::sufficient` stops at the first shortfall, so the report
  asks it once per deciding question and once per acting class and joins them: the operator sees every shortfall.
- Only one pack has a Choice class that acts in M5: `loop.v1`'s `work_state: progressing` (the nudge). role.v1's
  classes are the roles table's, known per state, so its minimum counts its deciding question alone. The table is
  `learning::report::ACTING`.
- loop.v1 only judges turns that ended with `no_tool_calls`, so a `budget_exhausted` turn is never judged by it.
  The rule is still written down: a judgment whose baseline decision is not `no_tool_calls` takes no system label,
  and no rule writes "should have stopped".

### What I changed

- **Protocol** (`crates/theseus-protocol/src/learning.rs`): `JudgeLabelParams/Result`, `LearningReportParams`,
  `LearningReport`, `PackReport`, `QuestionReport`, `Holdout`, and the rest; methods `judge.label` and
  `learning.report`; ledger kind `judge.report`. The TypeScript is regenerated. `lib.rs`'s ceiling goes from 2,646 to
  2,651 for the two method names (`scripts/long-files.txt`, with its reason).
- **What a label holds** (module doc of `learning.rs`, and `learning/labels.rs`): a Noul's label is `true`/`false`;
  a Choice's is the right option's id, or `{"not": "<option>"}` when only a wrong one is known; a Score's is a level.
  `right` and `wrong` grade an answer's own lean on any kind; on the whole judgment (no question) they grade every
  answer, `wrong role` is role.v1's press (a `wrong`), and `noise`/`useful` are recorded for 26a's
  `labels_per_day` rule and grade nothing. The CLI's strings are normalised by the core (`"yes"` → `true`,
  `"not:complete"` → `{"not":"complete"}`, `"2"` → `2`) and checked against the pack's questions and options.
  Where one question has several labels, the heaviest counts, and the newest of equal weight (operator 1.0 beats
  system 0.5). Counts in the report are of judgments, unweighted; the weight decides which label counts.
- **`judge.label`** (`rpc/learning.rs`): `judge_act(Act::JudgeLabel)` (the owner, from a private place; refused
  rows are `approval.refused` with `act: judge.label`), then one row keyed `lbl_<uuid>`, scoped as its judgment,
  with the call's correlation id when the judgment is a gate's, and its sentence. In `OPERATORS`, so
  `theseus judge label` is refused in a job's shell.
- **System labels** (`learning/system.rs`), derived by each run, weight 0.5, keyed
  `lbl_<sha256(judgment, question, rule)>`, so a second run finds them and writes none:
  - `loop.v1` `continuation`: the session's next operator `UserMessage` after the turn's last node, within 10
    minutes, that begins with "continue", "go on", "keep going" or "you didn't finish" ("you did not finish", curly
    apostrophes) and has at most six words → `work_state: "progressing"`.
  - `loop.v1` `false_completion`: for a task's turn (`context.class == "task"`), another task session whose first
    message (its brief) has `learn::similarity ≥ NEAR_IDENTICAL` (0.9) to this task's brief and began within 24 h
    after the turn → `work_state: {"not": "complete"}`. Tasks are read with `list_sessions` + `first_node`, once a
    run, only when a task judgment needs them.
  - `security.*` `declined`: the call's action (`context.call`) was declined by someone (an expiry is no one's) →
    `risky: true`; `approved`: its confirm is bound and no operator label is on the judgment → `risky: false`.
  - `classify.v1` `task_create`: `should_promote` is whether a `task.create` ToolCall node is in the message's
    turn, once the turn has ended (a later node in the session, or an hour gone).
  - None for role.v1, continue.v1, nudges (26a) or slash commands (25a judges none), as the brief says.
- **The report** (`learning/report.rs`), per pack version and question: calls (answered, failed, skipped),
  labeled judgments, band shares, per-class precision and recall (`learn::precision_recall`), calibration of a
  Noul's p and of a Choice's or Score's top confidence (`learn::calibration`, 10 bins: Brier, ECE, the reliability
  table), agreement with the baseline (the share of answered judgments that 23b's `disagrees` does not flag: the
  rule behind `theseus.judge.disagreements`), cost, and latency p50/p95/p99 (`learn::percentile`) per workload
  class (`context.class`; `inbound` for inbound packs). It reads only the `judge:<pack id>` scopes (plus, for the
  system rules, the judged sessions' nodes, their call's action, and the task list).
- **Holdouts**: `Window::latest(local midnight of the report's day, 14)`, split by `learn::holdout_split` on each
  judgment's row time, frozen into the report with its judgment ids, the ids of the labels that count, train and
  later counts, labeled counts per question and per acting class, its own question numbers, and
  `insufficient: <every shortfall>` below 200 per deciding question and 30 per acting class.
- **Writing**: one frame per run holds the new system labels, a `judge.report` row per pack version (keyed
  `rpt_<date>_<pack>`, scoped `judge:<pack id>`; a second run the same day supersedes by key), and the META mark
  `learning.last_run`; then `<state dir>/learning/<date>.json`, written whole and renamed. A run with no judgments
  and nothing to label writes nothing. `learning.report {date}` rebuilds the report from its rows and rewrites the
  file when it is missing. Without a date, `learning.report` runs the report now (refused with the judge off): it is
  not an operator's act, since everything it writes is derived.
- **The tender** (`learning/tender.rs`, started by `theseusd`'s `after_serving` as `core.learn_after_serving()`):
  not started with `[judge] enabled = false`; `due()` says a missed night (no run since the latest
  `learning_hour`) is due now and otherwise the next hour; `tend()` never begins a run before `start + 10 min` on
  tokio's clock; each run goes to a thread named `learning` at nice 19 (`setpriority` on its tid) that sleeps 19
  times each pack's work (about 5% of a core), and holds the core only by `Weak` between runs. A run that wrote
  nothing is remembered in memory so a missed night runs once.
- **Config**: `[judge] learning_hour = 3` (0 to 23, checked), with its template line and template test.
- **CLI**: `theseus judge label <id> <label> [--question Q] [--note N]`, `theseus judge report [--pack P]
  [--date D]` (`crates/theseus/src/render/learning.rs`).
- `crates/theseus-core/AGENTS.md`'s judge section has a bullet for `learning/`.

### How I proved it

- Under load (AGENTS.md's recipe, four busy loops at nice 0, the tests at nice 19), the 13 learning tests passed
  three runs of three.

- New tests, all passing (`cargo nextest run -p theseus-core -E 'test(/learning/)'`: 13 tests;
  `-p theseus`: 134 tests, including `a_jobs_process_cannot_label_a_judgment`):
  - `tests_learning::a_label_is_the_operators_from_a_private_place`: written keyed, scoped, weight 1.0; a shared
    Discord place refused (`Refusal`, `approval.refused` row, nothing written); bad labels and unknown judgments
    invalid.
  - `tests_learning::a_label_and_a_report_through_the_protocol`: over the socket's dispatch, the CLI surface's label
    goes and an unnamed connection's is `REFUSED`; `learning.report` with no params runs, with a date reads the
    stored one equal to it, and the file is equal and rebuilt when removed.
  - `tests_learning::the_reports_numbers_equal_learns_on_the_same_pairs`: 120 synthetic loop.v1 judgments, 90
    labeled (each also carrying a lighter, disagreeing system label): Brier, ECE and every bin for a Noul and the top
    choice, precision/recall for every class, latency percentiles per class, agreement, label counts, all equal to
    `learn`'s on the same pairs.
  - `tests_learning::a_holdout_is_frozen_into_the_report`: train/holdout/later by day, the window's start inside and
    its end outside, the labels frozen, "insufficient" with every shortfall; a later judgment changes neither the
    stored report nor a run again the same day.
  - `tests_learning::loops_system_labels_come_from_the_history_and_are_written_once`: real turns ("Go on." after a
    reply; "continue" after a turn whose judgment says `budget_exhausted`; a task and a near-identical task) give
    exactly `continuation` on the first and `false_completion` on the task, weight 0.5; a second run writes 0.
  - `tests_learning::security_and_classify_labels_come_from_the_record`: three real waiting `proc.run` calls,
    declined / approved / approved-then-pressed → `declined: true`, `approved: false`, none; a turn whose model
    calls `task_create` → `should_promote: true`, a plain chat → `false`.
  - `learning::tender::tests::the_tender_never_runs_within_ten_minutes_of_a_start` (tokio's paused clock: a run due
    at once waits until exactly 10 min; the next waits its 3 h), `..._a_missed_night_is_due_now_and_a_kept_one_waits`,
    `..._the_run_takes_a_low_priority_thread_of_its_own` (thread `learning`, `getpriority` = 19).
  - `learning::labels::tests` (resolution order, the operator's forms against loop.v1 and role.v1, the key) and
    `learning::system::tests::continuation_phrases_and_their_near_misses`; `config::judge` tests for
    `learning_hour`.
- **Planted reverts** (each restored with `cp` from a copy, then `touch`ed, `git status` checked):
  - `system_labels` without its `!scope.label_ids.contains(&l.id)` filter: 
    `loops_system_labels_come_from_the_history_and_are_written_once` failed, `left: 2, right: 0` (the second run
    wrote both again). Restored: 13/13 pass.
  - the holdout split on `Window { end_ms: u64::MAX, ..window }`: `a_holdout_is_frozen_into_the_report` failed,
    `left: ["jdg_in1", "jdg_in2", "jdg_edge", "jdg_today"]`, `right: [..."jdg_edge"]`. Restored: 13/13 pass.
- **A scratch daemon** of this build (fresh state dir, `[judge] enabled = true`, a dummy key and
  `api_base = "http://127.0.0.1:9"`, `theseus-sim fake-model` as the model): two `ask` turns made real `judge.call`
  rows (failed: network, and one `circuit_open`); `theseus judge label <loop.v1 id> wrong --note "live check"` wrote
  its row (`ledger --json -k judge.label` shows it, `source: operator`, `via: cli`, `rule: null`);
  `theseus judge report` printed every pack with its calls, "0 labeled" (failed judgments grade nothing), and
  "insufficient: announced_unfinished: labeled 0 of 200; work_state: labeled 0 of 200; progressing: labeled 0 of
  30"; `state/learning/2026-10-04.json` existed; `judge.report` rows were in the ledger; with
  `THESEUS_SESSION=ses_0000aa1b2c3` the label was refused by the CLI; `--date 2026-01-01` said no report is stored.

## 2. The cockpit's label buttons and report page (`fb6e632`)

- 23b's Judgment section is on main, so: a judgment's detail has **label buttons** (`components/JudgmentLabels.tsx`:
  the whole judgment right / wrong / noise; each question right / wrong, a Noul's true / false, a Choice's "was…"
  options), each confirmed first, each `judge.label`; and a **Learning** panel (`components/LearningReport.tsx`)
  that reads the stored report of the day in `?report=<date>` (today by default), with its questions, per-class
  precision and recall, a reliability strip per question, latency, agreement, and the holdout's line, and a
  "run now" button (confirmed, since a run writes rows). It reads only while shown.
- `src/lib/learning.ts` (pure) and `test/learning.test.ts`: `npm test` 39/39, `npm run lint` clean for these files,
  `tsc -b` clean, and the gate's cockpit build passed. **Not done here**: loading the view in a browser from a
  scratch daemon (no Playwright in the cockpit's modules on this VM). The maintainer should open Judgment, pick a
  judgment (the label buttons show under its fields), and press "show the report", and check the console is clean.

## Live check for the maintainer (real Jev key)

On a scratch daemon of this build with a fresh state dir, a GLM profile, `[secrets] jev_api_key`, and
`[judge] enabled = true` (and, to see the tender, `learning_hour` set to the current hour: it runs 10 minutes after
the start):

```bash
S=/tmp/ll/sock
theseus --socket $S ask -P glm "Say hello in one line."
SES=$(theseus --socket $S sessions --json | jq -r '.sessions[0].session_id')
theseus --socket $S ask -P glm -s $SES "Run echo hi with proc.run."      # proc.run at notify in this config
theseus --socket $S ask -P glm -s $SES "Name three herons, then say you will name more."
theseus --socket $S ask -P glm -s $SES "go on"                            # within a minute of the reply
theseus --socket $S judge log                      # loop.v1, security.v1/v3, classify.v1, role.v1 judgments, answered
theseus --socket $S judge label <loop id> --question work_state progressing
theseus --socket $S judge label <loop id 2> wrong
theseus --socket $S judge label <security id> --question risky false
theseus --socket $S judge label <classify id> --question kind new_ask
theseus --socket $S judge label <role id> noise
theseus --socket $S policy tighten proc.run --call <the echo's correlation id>   # 24's press: risky true rows
theseus --socket $S judge report
ls /tmp/ll/state/learning/            # <today>.json
theseus --socket $S ledger --json -k judge.label
THESEUS_SESSION=ses_x theseus --socket $S judge label <loop id> wrong     # refused by the CLI
theseus --socket $S judge report --date <today>                           # the stored one, as printed above
```

`judge report` should show, per pack version, the calls with answered/failed, "N labeled", band shares, the
labeled questions' calibration and per-class rows, latency per class, and every holdout "insufficient: …". Labels
should read 5 + the press's 2 operator, and at least one system: the "go on" after the third reply gives loop.v1's
`continuation` (weight 0.5) on that turn's judgment, once its 10 minutes have passed if the report runs sooner than
that, the next message is already there, so it is written at once. A second `judge report` writes no system label
again (`(0 new)`). In the cockpit, Judgment → a judgment shows the label buttons; the Learning panel's "show the
report" shows the same numbers.

## What is left, and choices the owner should hear about

- **Docs** (not edited, as the brief says): Part III's item for 25c; `docs/status.md`; design §2.9 could say what a
  label holds per kind (the forms above), that weights choose and do not scale, that `noise`/`useful`/`wrong role`
  are whole-judgment words, and that `ACTING` lists only loop.v1's `progressing`; §2.13's Protocol row matches.
- **Choices**: `learning.report` without a date runs and writes (derived rows only), so the CLI's `judge report`
  writes; it is not in `OPERATORS`. A Choice's metrics count unweighted judgments. The holdout's metrics are in the
  report beside the all-time ones (the ladder should cite the holdout's). Label resolution lets a whole-judgment
  `right`/`wrong` stand for each question. System label windows: "next human message" and classify's turn end
  wait for their window, so a label can appear a night later than the event.
- **Not built** (by the brief): the learning channel's digest, the canary part of the report, nudge labels, the
  `control` label, audit labels' writer (rows with `source: audit` are read and counted).
- The holdout window is fixed at 14 days (`learning::HOLDOUT_DAYS`); no config key, by the few-knobs rule.

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Fmt, shape, features,
clippy, deny (offline, database fetched), the cockpit's lint, test and build, the test build, and the reader rule
passed. The suite: 2,210 tests, 2,177 passed, 33 failed. Every failure but one is the root VM's sandbox
(theseus-pv6i): the 21 `theseus-sandbox::contract`/`bench` tests and 12 `theseusd::sandbox` tests. In the first
gate, also `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` failed once (a terminal screen
read raced its prompt's repaint under the suite's load; the terminal toolset, not this step): it passed 5 of 5
alone, and passed in the second gate. In the second gate `theseus-sim::sim
the_kernel_holds_its_invariants_under_seeded_faults` failed once and passed on its retry. I then ran the phases
after the suite myself: the protocol types (ok) and the turn bench (5 and 9 frames, at budget: ok); the lifecycle
and jobs benches are skipped in a lane's gate.

**Why TZ**: on this VM (UTC), `tests_output::the_cores_output_matches_its_golden` fails on main as cloned: the
golden's wake line was written in a zone west of UTC (`-#:#`), and UTC prints `+#:#`. It passes with
`TZ=America/Los_Angeles`, so both gates ran with it. Not this step's; the golden could pin a zone or mask the sign.
