# The Ship of Theseus, chapter 20: Part III, A4's Items 129 to 138 ([index](README.md))
### Item 129. The learning ledger: the operator's labels, system labels from the record, a report per pack version with its holdout frozen in, and a tender that runs it each night (theseus-0j2.9; roadmap row 42, step 25c; the fifth cloud batch's learning-ledger session, launched 07:25 and fired 07:29 from 4bae0f74, after the four judging joins, Opus 5.5; f3ba424b and fb6e6320; reviewed 08:57 to 09:31 by the batch-5 harvest wake itself, with three fixes at the join; joined 09:48 at 084ccbbb, a signed merge onto 8f79c753, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** By 07:25 every M5 pack wired so far judged in shadow (23b to 25b, Item 119 to
Item 122), and Item 16's `theseus_judge::learn` held every statistic a report needs (precision and
recall, calibration, the holdout split, percentiles, similarity), but nothing graded a judgment: 24's press
(Item 120) wrote `judge.label` rows that nothing read. Step 25c is the ledger: labels from the operator,
labels the record itself implies, and a report per pack version, run nightly, with a holdout frozen into it, which the
ladder (26a) is to cite before it promotes anything. It sits on M5's longest chain (25c, then 25d and 26a, then the
canaries), so the session launched the moment the last of its four prerequisites joined (the chain log, 07:25), and
the harvest reviewed it itself rather than queue it behind the two local reviewers (overnight decision 22).

**What landed** (the branch: 48 files, +3,661 −13 against its base without the CLOUD files; the merge: 48 files,
+3,799 −19 against 8f79c753; no new package; **no store format change**, a ledger kind and a row field, so the store
stayed at 12).
- **What a label holds** (`learning/labels.rs`, the module doc of the protocol's `learning.rs`). A Noul's label is
  `true` or `false`; a Choice's is the right option's id, or `{"not": "<option>"}` when only a wrong one is known; a
  Score's is a level. `right` and `wrong` grade an answer's own lean on any kind, and on the whole judgment (no question
  named) they grade every answer; `wrong role` is role.v1's press (a `wrong`); `noise` and `useful` are recorded for 26a's
  `labels_per_day` rule and grade nothing. The core normalises the CLI's strings (`"yes"` to `true`, `"not:complete"` to
  `{"not":"complete"}`, `"2"` to `2`) and checks them against the pack's questions and options. Where a question has
  several labels, the heaviest counts, and the newest of equal weight (an operator's 1.0 beats a system 0.5): the weight
  chooses, it does not scale. Counts in the report are of judgments, unweighted.
- **`judge.label`** (`rpc/learning.rs`): through `judge_act(Act::JudgeLabel)`, so only the owner, from a private place
  (a refusal is an `approval.refused` row with `act: judge.label`), then one row keyed `lbl_<uuid>`, scoped as its
  judgment, with the call's correlation id when the judgment is a gate's, and its sentence. It is in `OPERATORS`, so
  `theseus judge label` is refused in a job's shell. 24's press rows keep their kind and shape and gain one field,
  `rule` (null for a person's label, the rule's name for a system label, which is part of its key).
- **System labels** (`learning/system.rs`), derived by each run at weight 0.5 and keyed
  `lbl_<sha256(judgment, question, rule)>`, so a second run finds them and writes none:
  - loop.v1 `continuation`: the session's next operator message within 10 minutes of the turn's last node that begins
    with "continue", "go on", "keep going" or "you didn't finish" (also "you did not finish", and curly apostrophes) in
    at most six words gives `work_state: "progressing"`;
  - loop.v1 `false_completion`: for a task's turn, another task whose brief is near-identical
    (`learn::similarity` at least `NEAR_IDENTICAL`, 0.9) and began within 24 hours after it gives
    `work_state: {"not": "complete"}`;
  - `security.*` `declined` (the call's action was declined by someone; an expiry is no one's) gives `risky: true`, and
    `approved` (its confirm bound, and no operator label on the judgment) `risky: false`;
  - classify.v1 `task_create`: `should_promote` is whether a `task.create` call is in the message's turn, once the turn
    has ended (a later node in the session, or an hour gone).

  None for role.v1, continue.v1, nudges or slash commands. A turn the budget ended (`budget_exhausted`, which loop.v1
  never judges) takes no label, and no rule writes "should have stopped".
- **The report** (`learning/report.rs`), per pack version and question, from the `judge:<pack id>` scopes alone (plus,
  for the system rules, the judged sessions' nodes, their calls' actions and the task list): calls (answered, failed,
  skipped), labeled judgments, band shares, per-class precision and recall, calibration of a Noul's p and of a Choice's
  or Score's top confidence (10 bins: Brier, ECE and the reliability table), agreement with the baseline (the share of
  answered judgments that 23b's `disagrees` does not flag), cost, and latency p50, p95 and p99 per workload class, every
  number from `learn`. `learn::sufficient` stops at the first shortfall, so the report asks it once per deciding
  question and once per acting class and joins the answers: the operator sees every shortfall. Only one class acts in
  M5, loop.v1's `work_state: progressing` (the nudge), so `learning::report::ACTING` lists it alone.
- **The holdout**: `Window::latest(local midnight of the report's day, 14)`, split by `learn::holdout_split` on each
  judgment's row time, frozen into the report with its judgment ids, the ids of the labels that count, the train and
  later counts, the labeled counts per question and per acting class, its own numbers, and `insufficient:` with every
  shortfall below 200 labels per deciding question and 30 per acting class. The 14 days are a constant
  (`learning::HOLDOUT_DAYS`), with no config key.
- **Writing**: one frame per run holds the new system labels, a `judge.report` row per pack version (keyed
  `rpt_<date>_<pack>`, scoped `judge:<pack id>`; a second run the same day supersedes by key) and the META mark
  `learning.last_run`; then `<state dir>/learning/<date>.json`, written whole and renamed. A run with no judgments and
  nothing to label writes nothing. `learning.report {date}` rebuilds a day's report from its rows (and rewrites a
  missing file); without a date it runs the report now (refused with the judge off). It is not an operator's act, since
  everything it writes is derived.
- **The tender** (`learning/tender.rs`, started from `theseusd`'s `after_serving` as `core.learn_after_serving()`): not
  started with `[judge] enabled = false`; a missed night (no run since the latest `learning_hour`) is due at once,
  otherwise the next hour; never a run before start plus 10 minutes on tokio's clock; each run on a thread of its own
  named `learning` at nice 19 (`setpriority` on its tid), sleeping 19 times each pack's work (about 5 % of a core), and
  holding the core only by `Weak` between runs. A run that wrote nothing is remembered in memory, so a missed night runs
  once.
- **Config, CLI and the cockpit**: `[judge] learning_hour = 3` (0 to 23, checked; its template line and test);
  `theseus judge label <id> <label> [--question Q] [--note N]` and `theseus judge report [--pack P] [--date D]`; in the
  cockpit, label buttons in a judgment's detail (`JudgmentLabels.tsx`: the whole judgment right, wrong or noise; each
  question right or wrong, a Noul's true or false, a Choice's options; each confirmed first) and a Learning panel
  (`LearningReport.tsx`: the day's stored report, per-class rows, a reliability strip per question, latency, agreement,
  the holdout's line, and a confirmed "run now"), reading only while shown. The protocol gained `judge.label`,
  `learning.report` and the ledger kind `judge.report`, with its TypeScript regenerated.

**How it is proven.**
- **The session's tests**: 13 learning tests in theseus-core (and theseus's 134, among them
  `a_jobs_process_cannot_label_a_judgment`): a label is the operator's from a private place (a shared Discord place
  refused with its row, nothing written; bad labels and unknown judgments invalid); a label and a report through the
  socket's dispatch (an unnamed connection's label `REFUSED`; a dated report equal to the stored one, and its file
  rebuilt when removed); **the report's numbers equal `learn`'s on the same pairs** (120 synthetic loop.v1 judgments,
  90 labeled, each also carrying a lighter, disagreeing system label: Brier, ECE and every bin, precision and recall for
  every class, the latency percentiles, agreement, the counts); the holdout frozen (train, holdout and later by day; a
  later judgment changes neither the stored report nor a second run that day); loop.v1's system labels from real turns
  ("Go on." after a reply gives `continuation`; "continue" after a `budget_exhausted` turn gives none; a near-identical
  task gives `false_completion`; a second run writes 0); security's and classify's from three real waiting `proc.run`
  calls and a turn that calls `task_create`; and the tender on tokio's paused clock (a run due at once waits exactly 10
  minutes; a missed night is due now; the run's thread is `learning` at priority 19). Under load (four busy loops at
  nice 0, the tests at nice 19), three runs of three.
- **Planted reverts, 5 of 6 caught at the review.** The session's two, re-run: system labels written again without the
  written-once filter (`left: 2, right: 0`), and the holdout's window left open at its end (`jdg_today` inside it). The
  review's: `judge.label` without `judge_act` (both protocol tests fail, `REFUSED` expected); the tender's 10 minutes
  gone; a turn the budget ended taking a continuation label (the history test fails on `jdg_b`). **The sixth, the
  continuation's 10-minute window removed, was not caught**: every scripted message is seconds apart (theseus-e1ei).
- **At the review**: 1,207 of 1,207 on the merged tree before fix 3 (theseus-core, the CLI, theseus-protocol,
  theseus-judge and theseusd's judge tests), then the 15 learning tests (both new tests among them) and 98 of 98 on the
  final tree.
- **Live, at the review**, on a scratch daemon of the merged tree (a transient unit, a fresh state dir), with GLM
  (glm-5.3-flash) and the real Jev, the judge on (GLM about $0.0015, Jev about $0.0004 for 9 judgments). Three turns in
  one session gave 9 answered judgments (loop.v1, classify.v1 and role.v1 per turn). Four labels went in through the
  CLI; a bad Choice label was refused with the options listed (-32602), and a label with `THESEUS_SESSION` set was
  refused by the CLI. `theseus judge report` read `labels 4 operator, 3 system (3 new)`: loop.v1's `continuation` on turn
  2's judgment ("go on" 5 s after it) and classify.v1's `should_promote: false` on turns 1 and 2 (turn 3's still open),
  with every holdout `insufficient: …`. A second run read `(0 new)`; `--date` returned the stored report, and a date with
  none `NOT_FOUND`. It also showed **fix 3's bug**: role.v1 `noise: recall 0% (1 labeled)`. After the fix, on the same
  state dir and the rebuilt binaries, role.v1 read `0 labeled` with no `noise` class.

**What the review found** (three fixes, applied in the merge by resolve.py rather than sent back: the report is
advisory until 26a, the third fix is in the reader with no stored row changed, and a relaunch would have cost an hour on
the critical path).
1. **A semantic conflict with 28b that no merge marker showed** (Item 126). `judge.label` had two
   writers (decision 20 said so): 28b's answers to Jev's categorize.v1 proposals are `judge.label` rows with `label:
   "accepted"` or `"rejected"` beside the judgment's `answer`. Read as written, "accepted" on categorize.v1's Choice
   question `topic` is a class no answer has, so every accepted proposal would have graded Jev wrong and a phantom class
   would have entered the report. `learning::label_row` now reads an accept as the answer's class and a reject as
   `{"not": answer}`; its test goes through `ProposalLabel`'s own `row()`, so a change in the writer's shape fails it.
2. **A fold**: `dispatch` (rpc/server.rs) measured 102 lines with the branch's two arms, over clippy's 100; its
   narration-off refusal moved into `narration_off()` beside `or_empty` (98), as mcp-server's and mcp-prompts' joins had
   folded it.
3. **The branch's own bug, found live**: `truth()` let a whole-judgment `noise` or `useful` fall through to a Choice's
   `Value::String(c) => Truth::Class(c)`, so a noise press counted against role.v1's recall, and on loop.v1 would have
   graded `work_state` wrong. The cockpit puts a "noise" button on every judgment, so it would have been common. Both
   words now give `None` (test `noise_and_useful_grade_nothing`).

**The join** (09:48, by the batch-5 harvest wake, which reviewed it in its own tree beside rerank-arm's gate). The
review commit 084ccbbb is the merge (8f79c753, the branch, and resolve.py). Six files conflicted, all keep-both: the
generated index (`PackReport`, then 39a's `ParkedTask`), theseus-core's guide (28b's bullet, then `learning/`), the
template's test (32c's rerank example, then `learning_hour`), the facts' registry (28b's `ProposalLabel`, then
`JudgeReport`), `OPERATORS` (thirteen: 28b's two proposal answers and `judge.label`), and `scripts/long-files.txt` (the
protocol's `lib.rs` at 2,668 lines, past main's 2,663, raised with its reason). `CLOUD_TASK.md` and `CLOUD_REPORT.md`
dropped. Its gate (exit 0 at 09:48:05): 2,287 of 2,287; lifecycle OK after a 25 s settle; turn frames 5 and 9, plain p50
77.0 ms and tool-call 173.7 ms, main's level again, which settled decision 21: rerank-arm's high numbers at 09:05
(Item 128) were load, and no A/B was needed. Pushed 09:48; theseus-0j2.9 closed. It released 25d (row 43)
and 26a (row 44).

**FAST.** `learn_after_serving` returns at once with the judge off, which is how the lifecycle and turn benches run;
with it on it is one tokio task that reads one META row and sleeps until the later of the due hour and start plus 10
minutes. No turn path: `learning.report` runs under `spawn_blocking`, and `judge.label` is a read, a check and one
append.

**The install** (14:09, at bddfd407, install #1). Eddie's judge is on (every pack shadow, as configured), so the tender
runs on his daemon at `learning_hour` 3, never within 10 minutes of a start. Health after the restart: the config from
`/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on, lsp on, startup 73.1 ms; the store from format 6
to 14 on start.

**Divergences.** `learning.report` without a date runs and writes (derived rows only), so it is not in `OPERATORS`
and a job's shell may run `theseus judge report`; reading by `--date` writes only a missing file. A whole-judgment
`right` or `wrong` stands for every question. A Choice's metrics count unweighted judgments. The holdout's numbers sit
in the report beside the all-time ones (the ladder should cite the holdout's). Per-item questions are not graded:
rerank.v1's `helps` and `helps_more` (32c) show 0 answered, since the report reads whole answers only (Eddie's decision
4, at 10:11, added grading them to 32d, Item 141). The system rules' windows ("the next human message",
classify's turn end) wait for their window, so a label can appear a night after its event. Not built, as the brief
said: the learning channel's digest (theseus-0j2.10, which has no roadmap row yet and waits for Eddie's channel), the
canary part of the report, nudge labels, the `control` label, and an audit label writer (rows with `source: audit` are
read and counted).

**Known gaps.** theseus-e1ei (P3: no test that a continuation more than 10 minutes after a turn takes no label; the code
is right). theseus-cf5c (P3: each run re-derives system labels over every judgment in history, and reads every judged
session's nodes for loop and classify, though every rule settles within about a day). The label buttons and the Learning
panel were built, linted and unit-tested (39 of 39), but neither the session nor the review loaded them in a browser.
The learning loop that uses these labels to change Jev's prompts (theseus-0j2.12) was approved by Eddie at 10:21 and
built later (Item 164).

### Item 130. `budget.list` and `policy.explain`: where the money is, and why a call waits, read from the gate's order written once (theseus-ext.7; roadmap row 73, step 42a; the fifth cloud batch's budgets-policy session, launched 05:58 and fired 06:01 from 35784d7c, Opus 5.5; fabe50b5 and 2a0c60d7; reviewed 08:49 to 09:43 by local reviewer R2, merged onto 94304c3b; joined 10:00 at 4c3a7008, a signed merge onto 084ccbbb, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** M7's design (`docs/design/m7-surface.md` §2.6) gives step 42a two reads: `budget.list`, each open execution's
money (its limit, spent, reserved, held unknown and lifetime cost, its carves, the last reset, the totals) from records
with no scan of history; and `policy.explain`, each offered tool's posture layer by layer with the result, "why did this
wait?" on one screen. The second has a trap the session named: the gate's decision was a chain inside `toolrun::gate`'s
closure (the place rule's refusal and the ceiling's; the call's own policy with L1's decision or L0's, and the broker
inside it; an `lsp.*` call's start; a shared place's word on a private address; the place's floor; T1's hold; an MCP
client's floor), and a second copy of that chain for explain would drift from it.

**What landed** (the branch: 44 files, +3,456 −57 against its base; the merge: 42 files, +2,970 −61 against 084ccbbb,
with the join fixes; no new package; **no store format change**, both are reads, so the store stayed at 12).
- **`budget.list`** (`rpc/budgets.rs`; the types in the protocol's `budgets.rs`). Rows from `Kernel::open_executions`
  (state terms), each session record by key, and the place view. Each row says where its limit comes from
  (`limit_from`): `carve` (with the parent's session), `config`, `place` (with the place's name: 38a's `place_limit`
  pins a conversation while a place's ceiling caps it) or `pinned` (its own limit, as an MCP client's session has),
  with spent, reserved, held unknown, available, the session's lifetime cost, a waiting budget question, and
  `carve_held_usd` (what the parent holds now of the task's carve). The last reset (its time and who) comes from one
  ledger page of the newest 8 `budget.reset` rows by kind and session tag (`Store::ledger_page`, Item 91's index),
  skipping rows of an earlier execution of the same session; while the index's shape is being built after a start the
  row says `last_reset_unread`, and nothing is scanned. Tasks sit under their open parent (a task whose parent has
  ended is listed on its own). **The totals add the top rows only**, in micro-dollars, since a task's spend is its
  parent's too and its carve is the parent's reservation; the lifetime total adds every session's `cost_usd`. The
  judge's day budget is a line of its own. `theseus budgets` prints the table, each task under its parent with `└`, the
  carve and question lines, the totals and the rule under them.
- **The gate's order written once** (`toolrun/order.rs`). `At { place, held, mcp }` is what the gate reads of where a
  call runs (the hold and the MCP floor as closures, so a read still costs no record). `ToolRuntime::refusal` is the
  place rule's refusal, then the ceiling's; `ToolRuntime::order(at, tool, plan, input, seen)` is the rest, handing
  `seen` the decision after each `Layer`: `Policy` (L1's, or L0's floor, lists and posture with its tightening),
  `Grant` (a granted secret's posture), `Lsp` (a language server's first start, by an `lsp.*` call or, after the join,
  by an edit), `SharedFetch`, `Floor` (the place's), `Hold` (T1's) and `McpClient`. Each layer after the first only
  raises the posture. `toolrun::gate` builds `At` from its turn and runs both, with a watcher that does nothing.
  `sandbox::decide` became `sandbox::unbrokered` (the same body without the broker, which is the order's next layer);
  `toolrun::mcp_floor_of` is shared by the gate and explain. policy.rs, external.rs, places.rs, broker.rs and
  ceiling.rs are untouched, and the core's output golden is byte-identical.
- **`policy.explain { session_id?, tool? }`** (`rpc/explain.rs`; the types in the protocol's `explain.rs`). For a
  session it builds the place as the turn's gate reads it (`runner.view_of`, `external::held`, the execution's MCP
  floor); without one it explains the CLI, then each bound place with the hold of the session it runs on. Per tool it
  plans a probe (one resource at the first workspace root, or the first public path for a file tool in a shared place;
  an empty argv; the tool's class as its access), runs `refusal` and `order` on it, and each row is the decision the
  order handed its watcher: `place`, `ceiling`, `class` (L0 or L1, for job tools), `posture` (the setting that says it:
  a `[policy.tools]`, `[policy.mcp]` or `[policy.aws]` line, or enforcement), `tightening`, `grant`, `lsp`, `floor`,
  `hold` and `mcp_client`, each with `raised`. The call-dependent layers are listed as conditions with their entries
  (`public_paths`, `floor`, `approve_paths`, `outside_roots`, `private_address`, `approve_argv`, `allow_argv`, `grant`,
  `destructive` and `aws`, `l1`, `lsp_start`), not decided. A tool the place does not offer has `offered: false` and
  the gate's words. `theseus policy explain [--session <id>] [--tool <name>]`: a line per tool, or with `--tool` every
  layer, the conditions and the gate's reason; `theseus policy` is unchanged.

**How it is proven.**
- **The session's tests.** `tests_budgets.rs` (3), each row compared with `kernel.execution()`'s record and the totals
  with the sums of the top rows: two carves ($3 and $2) under a `config` parent with $5 reserved, a budget question, a
  reset by the CLI whose time equals its row, then a `#pier` ceiling of $40 shown as `place` and, uncapped, `config`
  at $100 again; the limit following a changed config across a restart (100 to 60) while a task keeps its $4 carve;
  and **the last reset found by one page behind 5,000 later rows** of other kinds and sessions, which the window scan
  `ledger.tail` falls back to would not reach (the rows written ten a frame: a frame per row timed out at nextest's
  120 s under the load recipe). `tests_explain.rs` (2): **explain agrees with the gate for every tool and place**, the
  template's 19 built-in tools plus one MCP tool, in four places (the CLI, a floored private `#lab`, the owner's DM,
  a shared `#pier`), each as configured, tightened and holding external text, every tool planned with a real input by
  its own `plan` (76 of 80 rows each; `term.send`, which needs a terminal, the only one left out, and asserted so); and
  **each call a whole turn records agrees with its session's explanation**, through the real `toolrun::gate`. Six CLI
  goldens and two protocol shape tests. Under load (the tests at nice 19 beside four busy loops), the 30-test filter
  passed 5 runs of 5; theseus-core, theseus and theseus-protocol, 969 of 969.
- **At the review**: 210 of 210 on the merged tree over the branch's and the gate's suites (209 of 210 before join fix
  4), then, since every tool call's gate now runs through `order`, **all of theseus-core, theseus and theseus-protocol,
  1,068 of 1,068**.
- **Planted reverts, 4 of 5 caught.** Every limit taken from the config (both carve tests fail, `100.0` against `3.0`
  and `60.0` against `4.0`); explain's place without its ceiling (`notify` explained where the gate decides `approve`);
  explain reading no hold; an earlier execution's reset taken as the last (the 5,000-row test fails). **The fifth, R2's
  own join fix 1 reverted (L3 out of `order.rs`), was not caught**: main's L3 test calls `lsp::edits::gate` directly
  and its turn test's postures hide the raise, so nothing checks that the gate's chain runs L3, on main as on the
  branch (theseus-x1jj).
- **Live, at the review**, on the report's own rig with stand-ins only (`theseus-sim discord rig`, a fake Discord, a
  fake model, invented secrets, the judge off; nothing spent). The split message as a user in `#lab` gave two tasks:
  `theseus budgets` showed `#lab`'s session at `$100.00 config` with $4.9996 reserved, `└ … $3.00 carve of …` and `└ …
  $2.00 carve of …` under it, "its parent holds $2.9998 of its $3.00 carve", the DM's session as a second top row, the
  totals with their rule, and `judge: off`; in `--json`, spent 0.00084, reserved 4.9996 and limit 200 each equal to the
  top rows' sums, and lifetime 0.00084 the sum over every row (the report's numbers, to the digit). After `policy
  tighten proc.run`, `policy explain --tool proc.run` showed `posture notify (enforcement = notify)`, `tightening approve
  ↑ tightened by the CLI`, `→ approve`, and in `#lab` also `floor approve  #lab's ceiling sets a floor of approve`.
  After a `cat` of a scratch file, `policy explain --session` said `holds external text (proc.run cat)`: every acting
  tool `approve` with `hold: [policy] external_text = ask`, every read at its own posture with no hold.

**What the review found: a semantic conflict that no build shows.** Main's lsp-diagnostics (Item 127)
had added L3, an edit that starts a language server (`lsp::edits::gate`), to the old inline chain, after L2's
`lsp::gate`; the branch's call into `order` had no such line, so taking it alone would have dropped L3 from the gate in
silence. Join fix 1 runs it inside the `Lsp` layer, after L2, with the place's class, and the module's and `Layer`'s docs
say so; main's chain and the branch's base differ by that one line alone (diffed). The other join fixes: 43a's two new
`McpServerConfig` fields (`egress`, `frozen`) in tests_explain.rs (E0063); `dispatch` back within clippy's 100 lines
(main's was at exactly 100, the branch's two arms made 102: the branch's two reads and 23b's two judge reads each share
one arm through one helper, `rpc_reads`); inputs for main's four new tools in the agreement test (39a's `task.update`,
`task.split`, `task.close` and 43a's `extend.propose`, whose plans refused a name-made input); and the protocol's
`lib.rs` ceiling.

**The join** (10:00, by the batch-5 harvest wake, prepared in its own tree while learning-ledger's gate ran). rerere
replayed four files from R2's tree (the generated index, theseus-core's guide, toolrun.rs, the protocol's lib.rs), and
R2's resolve.py the fifth, `scripts/long-files.txt`, a new conflict beside 25c's raise (the protocol's `lib.rs` ceiling
2,678), then its join fixes. `CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. It changes the gate's path, so it joined
alone, and its tool-call bench was the one to read. Its gate (exit 0 at 09:59:30): 2,296 of 2,296; lifecycle OK; turn
frames 5 and 9, plain p50 75.6 ms, **tool-call p50 171.2 ms against 084ccbbb's 173.7** (p95 186.7 against 177.2, the
single slowest of 10 runs; the daemon's p50 142.0 against 143.0): the gate's path at main's. Pushed 10:00;
theseus-ext.7 closed. extensions-load (43b, Item 133) joined second and carried its load floor into
`order.rs`, as R2's review of it said.

**The install** (14:09, at bddfd407, install #1). `theseus budgets` and `theseus policy explain` read Eddie's daemon
from then on. Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready,
judge on (every pack shadow), lsp on, startup 73.1 ms; the store from format 6 to 14 on start.

**Divergences.** The design said "the totals" without saying which: they are the top rows' (the CLI prints the rule).
`policy.explain` also takes `tool`, and its layers are the order's as built, not the design's list. `limit_from: place`
reads the place's ceiling as the binding last told it (`view_of`), so a pinned conversation whose place lost its cap
reads `pinned` until the binding restarts. Explain's probe is "a call inside the roots that no condition matches", so a
job tool's `class` row is the default class (L0 unless `[sandbox] default = "l1"`) and `l1_argv` is a condition. The
`posture` row's text in L1 mirrors `sandbox::l1_decision`'s rule as a description; the row after it carries the order's
real decision. `--session` needs the full session id (no short-id lookup).

**Known gaps.** theseus-x1jj (P2: no test runs an edit's language-server start through the gate's order; dropping it
passes). theseus-t2xr (P3: explain has no row or condition for an edit's L3 start). The MCP client's floor row appears
only for a session an MCP client opened, and was not live-checked. There is no read counter in the store, so "no scan"
is shown by the reads' structure and the 5,000-row test, not counted. The session also met a race in theseusd's
`a_stop_of_three_jobs_that_ignore_sigterm_…` test (an empty `job-N.term` read before the job wrote it; 3 of 3 alone),
not on the flaky list. 42b, the Budgets, Ledger and Policy tabs that read these two, came later, in the cockpit
(theseus-ext.15, Item 165).

### Item 131. Compaction: a `Summary` where the ring would cut, the latest summary as the floor of every recompile, `context_overage` before any call, and the assembled strategy (recall, then the summary, then the tail), with Eddie's `session` default on top (theseus-6fn.4; roadmap row 54, step 30c; the fifth cloud batch's compaction-roots session, launched 02:59 and fired 03:03 from 760553f7, Opus 5.5; 407ea057 and be3d7bb7; reviewed 08:37 to 09:56 by local reviewer R1, finished by its relaunch after R1's run failed at about 09:15; merged at 10:09 as f2695593, a signed merge onto 4c3a7008, with 802f9135, decision 3's default, on top; joined 11:10 by the batch-5 harvest wake, store format 13; installed 14:09 at bddfd407, install #1)

**Why.** M6's design (`docs/design/m6-memory.md` §2.5) names two context strategies beyond the ring: compaction,
where the ring would drop a session's leading turns, a profile summarizes the dropped range into a `Summary` node; and
the assembled strategy, a recall section first, then the summary, then the tail. §5.5a's baseline includes summaries,
and M5 keeps CONTINUE's `recompile_compaction` in shadow until M6 builds them. Step 30c builds both, and gives the
overflow the ring cannot fix a named outcome: the design's "named outcome, never a thinner prompt".

**What the session found.**
- The ring (`compiler.rs`, `compile_with`) picks its candidates over **every** renderable node of the session, not
  over the current compilation's prefix. A summary as "one more node" would be selected again, beside its own range,
  at every later recompile (a model change, a system change, a manual transcript, a second ring). Hence a **floor**:
  the latest `Summary` stands for its range at every recompile (`compiler::compaction::visible`), the ring rings over
  the nodes after that range and leaves the summary out, and the ring stays the plain fallback with nothing of
  compaction in it.
- A summary node is written after the turn's new message, so "first in the prefix whatever its position" has to be a
  rule of the render (`summaries_first`). Without it the summary renders after the newest message and the request is
  still byte-stable, so only a test of the order catches it (the session's planted revert 2).
- `budget_report` already computed an `overage` when the ring's last cut still did not fit; nothing read it, and the
  request was sent anyway. That became `context_overage`.
- 30b's scene counts the whole transcript as "in context". After a compaction the summarized range, recall notes
  included, is out of context, so the assembled section's scene counts only what lies past the new summary's range
  (the session's own compaction test caught the first version admitting nothing).

**What landed** (the branch: 29 files, +2,214 −43 without the paperwork; the merge: 33 files, +2,347 −157 against
4c3a7008; 802f9135: 6 files, +123 −27; no new package). Steps 1 to 4 are one commit, since they share the turn's compile
step, the compiler's floor rule and the facts module; step 5 (the assembled strategy) is the second.
- **Config** (`config/memory.rs`, the template): `[memory] summary_profile` (`"off"` keeps the ring) and
  `assembled_budget_tokens` (4,000), both validated (an empty profile or 0 refused) with commented template lines. A
  profile name that does not resolve is not refused at load: it rings at run time, and the row says why.
- **The `Summary` body** (`node.rs`): `first` and `last` (WAL positions), `nodes`, `text`, `profile`, `model`,
  `cost_usd`, and `header`, the testimony header frozen when it is written (`[Summary of 6 earlier messages,
  2026-10-04, written by glm]`, or a date range when it spans days), as 30b's `RecalledRef.header` is frozen, so the
  render never reads other nodes. It renders first in the prefix and nothing in a tail; the index indexes its text and
  not its header; `node.info`, `publish` and `preview` know it.
- **Compaction** (`turn/compaction.rs`, 838 lines; `compiler/compaction.rs`; `fact/compaction.rs`). After a loop's
  compile rings (by the estimate, or by the provider's word), the dropped range goes to the summary call, a kernel
  action (`PROVIDER_TOOL`, `purpose: compaction`) planned with its reservation and dispatched in one frame, and settled
  at its real cost in a frame that also writes the `Summary` node and the `context.compacted` row; its dollars join the
  turn's books. The compilation is `strategy: compaction`: the summary, then the ring's kept turns, as of the summary's
  frame, thinking stripped. A `Recall` node in the range is dropped as `recall_note` with tier `compaction`; the range
  is reported with reason `summarized`. A second compaction folds the first summary in (its text goes to the call, and
  the new range starts at the first one's `first`). The ring runs as before, with a `context.compacted` row giving
  `outcome: ring` and the reason, when the profile does not resolve, its provider is not configured, its model is
  unpriced, the ring dropped nothing new, a summary of up to its cap would not fit beside the kept turns, the range is
  past the summary model's window, the spend limit refuses it, the call fails or answers with no text, or the result
  does not fit. The fact: the row, a `compaction` span, the `theseus.compactions{outcome}` count and the
  `theseus.compaction.tokens` histogram (the instruments 24 to 26), and the narrative line "Compaction summarized 6
  messages into 120 tokens with glm, for $0.0014." Limits the session chose: a summary of at most `min(profile cap,
  4096)` output tokens (`SUMMARY_MAX_TOKENS`), and each node cut at 6,000 characters for the call (`NODE_CHARS`; long
  tool results say how much they left out).
- **`context_overage`** (`OVERAGE_CLASS`). When the ring's last cut, or a request with nothing to drop, still has an
  overage, the turn fails before any call and nothing is persisted. The error names the model, the window, the
  estimate, its upper bound, the limit and how far over it is; a `context.overage` row carries the `BudgetReport`; a
  narrative line says the same; `session::Failing` parks it on the next message as `context_window` is parked (no
  retry). Both classes are kept: `context_window` is the provider's verdict after a send, `context_overage` the
  estimate's before one, and they differ in what was sent and paid for.
- **The assembled strategy.** A task's first compile, and a compaction, put a recall section first in the prefix, then
  the summary, then the tail. The section is 30b's pipeline at `assembled_budget_tokens`, following the session's arm,
  read before the compile under the recall deadline with 30a's place filter. In front of the model it is a `Recall`
  node the compilation records as its new `recall_id`, rendered first in the prefix wherever it was written and never
  in the tail; in shadow or the control arm only the row is written. At a compaction, the first loop's pending recall
  becomes the section; a section that will not fit is dropped and the compaction tried again without it.
- **The store's format**: 8 to 9 on the branch's base, for the `Summary` body and a compilation's `recall_id`;
  **renumbered to 13 at the join** (main's 12 plus one), with every pin. The COMPILATION layout 8 (30b's, before
  `recall_id`) joined `tests_layouts`, written by hand in that build's layout.
- **802f9135, Eddie's decision 3 on top** ("Jev makes the call", 09:43): until Jev routes the summary (route.v1,
  theseus-0j2.11, Item 139), `summary_profile` defaults to a new word, `session`: the turn's own `Target`
  (profile, provider and model) is cloned for the summary call, so no second provider reads a session's text by
  default, and the row, the node and its header name its profile. A key of `[profiles]` still sends the range to that
  profile, and `off` keeps the ring. The branch's default had been `glm`. Once routing is live, the setting is to be
  `jev`.

**How it is proven.**
- **The session's tests** (11 new): `tests_compaction.rs` (8): the summary's range equals every message the ring
  dropped, by position and count, the summary heads the request, and the row, the action's reservation, the settlement
  at glm's prices (`9000×0.15 + 120×0.50` micros) and the execution's budget agree with nothing left held; the next
  request begins with the compacted request's bytes after a restart; a second compaction folds the first; the ring runs
  when the summary call fails; `off` keeps the ring; the newest exchange past the window fails with `context_overage`
  (no call, the numbers, the row, no compilation kept, parked, and the next small message answers); a task's first
  compile and a compaction assembled with recall first. Unit tests for the header, the floor and the order. Planted
  reverts: the ring's last cut sent instead of the overage (the turn answered on a 67,694-token request against a
  33,904 limit), and `summaries_first` removed (three tests fail, among them `first_text(&compacted).starts_with("[Summary
  of ")`). Under load, the compaction and compiler set (32, then 34 tests) and the recall suites (14) passed 5 runs of 5.
  Frames: plain 5 and tool-call 9, unchanged; a compaction adds 2 frames to its own turn (the summary call's plan and
  its settlement).
- **At the review**: on b332bfd7, 1,128 of 1,129 (theseus-1n2y's pty test, alone 3 of 3); re-checked by the relaunch on
  8f79c753, 1,147 of 1,148 (`tests_pages::a_filtered_page_equals_the_scans_answer` timed out at nextest's 120 s at a
  load of about 20, and passed alone in 21.3 s: theseus-hohs). **Planted reverts, 4 of 4 caught on both trees**: the
  floor never found; `summary_profile = "off"` not honoured; a failed summary call left unsettled; the summary settled
  at zero rather than its real cost.
- **Live, at the review, the overage half only** (the stand-in model, a fresh state dir, nothing spent): with a 20,000
  window, "estimated at 10,449 tokens (14,629 at the estimate's upper bound) against the 13,904 the window leaves after
  the output cap, 725 over … Nothing was sent", exit 1; with 40,000 and a 232 KB attachment, 79,162 over, one
  `context.overage` row and no new `provider.call`. The compaction itself was out of reach: the stand-in reports fixed
  token counts, and the compiler counts the request from the last call's real usage.
- **Live on GLM, after the join** (11:14 to 11:30, a check-only subagent on a scratch daemon of 802f9135 with
  independence's prepared merge, f7999ac7: GLM's glm-5.3-flash, `summary_profile` unset, the window cut to 24,000;
  **five checks of five passed, $0.0136**, GLM alone). At the brief's sizes no compaction could happen (finding 1,
  below): turn 2 rang, and its row said why. With smaller reads, turn 4's second loop compacted: a `Summary` of 12
  earlier messages (turns 1 to 3, each a message, a call, a result and an answer), on the session's own profile and
  model, 655 summary tokens from 5,536 in, **$0.0011579, settled at 1,158 µ$ of 1,665 reserved**, matching the catalog
  (5,536 × $0.15/M + 655 × $0.50/M); the kernel action `provider.messages` succeeded, planned and dispatched in one
  frame and settled 19.5 s later; `theseus budgets` then read reserved and held $0.0000. Turn 5 appended on the same
  compilation (counted 10,166, the compacted request's 10,122 plus its answer) and named four secret words, three of
  them now only in the summary. After a restart, turn 6 appended on the compilation the new process loaded from the
  store, with no second summary call (still two `context.compacted` rows; one action whose result is a summary), and a
  probe turn quoted the first line of its context: the summary's header.

**What the live check found.** (1) At a small window one big tool result at the ring point always drops the earlier
turns unsummarized: the plan check sets aside the summary's whole cap (2,000 here; 655 used) on the kept turns' upper
bound (× 1.4), so at 24,000 with a 6.8k base a compaction needs the kept part at most about 11,360 tokens. Not a defect
at GLM's real window (1,048,576), but a real limit for a small-window model. The ring's earlier drop was recovered: the
later compaction's range began at turn 1. (2) GLM's cache reads are not byte evidence: each recompile's request read 0
from cache though all three compilations carried the same system and tools digests and the same 25,519-byte header, a
pattern like backends that do not share a cache (route.v1's GLM cache check may show the same). (3) The summary's
reservation is priced on the input estimate's `tokens`, not its `upper` (1,665 µ$ implies about 4,433 input tokens
against GLM's 5,536; a summary of more than about 1,670 tokens would have settled above its reservation):
theseus-6fn.8. (4) `theseus sessions` totals mix two rules: the summary's tokens are only in its row, its dollars in the
session's (theseus-6fn.9). (5) With the live profile `glm`, `session` and the old default give identical rows; a
compacting turn under a one-message model override would tell them apart, and has no test (theseus-6fn.9).

**The join** (by the batch-5 harvest wake, from 10:08). Prepared in main's tree, since another wake held the cloud
tree for independence's review. rerere replayed all twelve conflicted files from R1's resolution (main's `Arrangement`
arm beside `Summary` in node.rs, recall.rs and recall/render.rs; the format, main's 12 plus one, with every pin;
`INSTRUMENTS` 24 + 2 = 26; CONTINUE's `signals` and the branch's `assembled` in `CompileInput` and every test's
literal; the compile step taking CONTINUE's signals, the compaction, the overage check, and task-record's view,
**attached before the overage check so its tokens count**; memory-arm's own sources in `recall_live`; rerank-arm's
`candidates.clone()` kept), and resolve.py edited the layout sample's note to 13. joinfix.py: **`TurnRunner::compile_step`
moved whole into `turn/compile_step.rs`** (turn.rs 3,538 to 3,417 lines against its 3,523), compiler.rs listed at 2,540
lines (with the reason "split it: the render and the estimate are each a module of their own"), `render_messages` (106
lines) marked with the codebase's `too_many_lines` expectation, and the semantic fixes (E0061, E0063 and E0004: memory-
arm's `recall_begin` sources, `Compiled`'s `signals` in the ring's fallback, `Summary` in arrangement's `text_of` (not
a quote source, as a recall is not) and in the exam's `kind_of`, test literals). Eddie's default went on top as a second
signed commit, gated with the merge, so main never carried 30c with `glm` as the default. **Gate run 1, red at
10:50:58** (2,303 of 2,308): four `tests_overflow` tests, caused by the new default (the ring's suite never configured
glm's provider, so its compactions used to fall back to the ring; on the session's own model they summarized with the
rig's scripted model), fixed by pinning that suite to `"off"`, folded into 802f9135; and one theseus-discord outbox
count under load 30 (10 of 10 alone; theseus-0bq1). **Run 2, exit 0 at 11:09:41**: 2,308 of 2,308; lifecycle OK, every
line under its strict budget though measured with the busy allowance (CPU pressure 69 %, IO 20 %, load 13); jobs OK (L1
start p95 15.15 ms); turn frames 5 and 9. The turn bench read plain p50 107.4 ms and tool-call 269.5 ms at load 12.9,
against 4c3a7008's 75.6 and 171.2 at load 4.6; every phase rose alike, start, SIGKILL restart and swap included (+44 to
+49 %), which 30c does not touch, with frames and resident memory (59.8 MB) unchanged, so the harvest read it as load
and pushed (overnight decision 25); independence's gate at 11:44:55 measured 84.6 and 171.4, main's level, and no A/B
was needed (Item 132). Pushed 11:10; theseus-6fn.4 closed. It released roadmap rows 61 (33, tiering) and
62 (35a, situations), and route (25e) and rerank-live (32d) launched from 802f9135, held for it because both edit the
compile step or recall_step.rs, which the join moved or changed.

**The install** (14:09, at bddfd407, install #1): the store went from format 6 to 14 on start, through this join's 13.
Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on (every
pack shadow), lsp on, startup 73.1 ms.

**Divergences.** One field more than the task listed (`header`). The overage uses the estimate's upper bound, as the
ring does, so a request whose real count would fit but whose upper bound does not now fails before sending (stricter at
the edge). A `/stop` does not cut a summary call (bounded by the provider's timeouts). The summary's tokens go in its
row, not the turn's usage; its dollars join the turn's and the session's cost. A summary written and then found not to
fit stays as the floor, rendering nothing. A promoted first-loop recall keeps its 1,500-token pack as a compaction's
section; only a section recalled for the compaction, or at a task's first compile, gets the 4,000. Triggers are
deterministic (the estimate, or the provider's word); CONTINUE (25b) is not touched. The join's two choices, neither
covered by a test: the task view counts toward the window, and the assembled section at a compaction follows memory-
arm's arm rule (a live arm's own sources; `live` with arm `none` asks nothing). The design's "cheap profile" became,
by Eddie's decision, the session's own model by default; 802f9135 amended m6-memory.md §2.5 and its config sketch to
say so.

**Known gaps.** theseus-6fn.8 and theseus-6fn.9 (P3, above). The cockpit's compaction view (a session's compactions
with header, text, range, profile, model, tokens and cost; each ring fallback with its reason; the strategy badge and
the assembled order in the context view; a `context_overage` with its numbers) is still to come. The gate flakes met on
the way: theseus-1n2y (a pty test under the suite's load), theseus-hohs (a store property test near nextest's 120 s
under load) and theseus-0bq1 (a Discord outbox count), all P3.

### Item 132. Check tasks: `check_of` opens a task that sees the checked task's claim and nothing else of its session, with a 12-word overlap flag and the basis on every surface (theseus-vug.3; roadmap row 48, step 28a; the fifth cloud batch's independence session, launched 08:20 and fired 08:24 from 78d749cb, after task-record's join, Opus 5.5; 20dadc1d; reviewed 09:55 to 10:39 by the batch-5 harvest wake itself, with three join fixes; joined 11:45 at e27405a2, a signed merge onto 802f9135, by another batch-5 harvest wake, store format 14; installed 14:09 at bddfd407, install #1)

**Why.** M5's design (`docs/design/m5-judgment.md` §2.11) makes independence a property of the compiler: a **check
task** admits the checked task's arrangement pieces (objective, acceptance) and its report, rendered as a claim, and
excludes every other node of the maker's session, its messages, tool calls and thinking. Since DD7 a task sees only its
brief, so the risk is the brief itself, which the parent writes after reading the maker's report: hence a flag for a
brief that copies 12 words or more of the maker's session, and a basis recorded and shown beside the check's report.
The session waited for task-record's join (39a first, since it is on 39b's path; overnight decision 8), and launched
a minute after that join pushed (08:19), from its merge.

**What the session found.**
- 39a's `task.create` opens a task session through `open_task`'s one frame (brief node, `Arrangement` node, edges, the
  outbox record and the `TASK` record), and `compile()` reads only the child's own nodes, so what a check sees is what
  that frame writes, plus the task graph's view. A task's report is its last assistant message, which the parent's
  next turn relays as a `[Report from task …]` node with a `derived_from` edge (via `report`) to it.
- `TurnCtx` has no config, so a check's `profile` could not be resolved where `task.create` runs: `ToolRuntime::profiles`
  (name to provider and model, from `Config::all_profiles`) is new, and a check's session gets that `last_target`.
- **39a's task-graph view reaches every task, checks included.** It shows each record's title, objective and
  acceptance (the parent's words), but a checked task that writes notes or evidence on its own record (`task.update`,
  `task.close`) puts its working there, outside the exclusion. The session left 39a alone, as its brief asked, and
  named it for the owner (theseus-w8ys).

**What landed** (one code commit; the branch: 39 files, +2,406 −28 against its base with the two CLOUD files; the
merge: 40 files, +2,093 −30 against 802f9135; no new package).
- **`check_of` and `profile` on `task.create`** (its input, schema and description only gain fields; the logic is in the
  new `check.rs`, 733 lines with the review's fix). `check_of` resolves only among the tasks this conversation started.
  A task with no report is refused, saying why ("is waiting and has not reported"; "cancelled (…), and has no report to
  check"). **`profile` is a check's alone**: any other task naming one is invalid input, and an unknown profile is refused
  with the configured names; a check without one runs on its parent's target. **27's rule is met by the claim**: the
  checked task's standing `objective` and `acceptance` pieces stand for the check's own, so a check needs no
  arrangement, and may add pieces that resolve as 27's do (a checked task with no objective piece needs the check to
  bring one). The fidelity check does not apply to a check.
- **The admission.** The check's `Arrangement` node gains an optional `claim` (`check::Claim`: the task, its report
  node, when it was written, and the report's text up to 16,000 characters), with a `derived_from` edge to the report
  (`graph::VIA_CLAIM = "claim"`). `check::render` renders the pieces, a short framing paragraph, then `--- Claim:
  claimed by task a1b2c3, as of <UTC> ---` and the report; the compiler, recall's render and `node_info` use it (the
  same text as `arrangement::render` when there is no claim).
- **The exclusion on the check's own pieces.** A piece whose node copies the report (the parent's relayed node) becomes
  the claim, admitted with role `claim` from `own`. A piece whose node derives, by `derived_from` edges followed back
  (a breadth-first walk, at most `WALK_NODES` = 20,000 nodes), from any other node of the checked session is refused,
  class `excluded`, naming the node and its source.
- **The overlap flag** (`check::overlaps`, `OVERLAP_WORDS = 12`): the brief and each of the check's own admitted
  pieces, against every node of the checked session but its report, brief and arrangement (those are admitted, or the
  parent's own words). A word is a maximal run of letters and digits, lowercased; a node's words are its message or
  reply text, its thinking and tool inputs, a call's input strings, or a result's content. Each maximal run of shared
  12-word windows (hashed) is one flag: its source (`brief` or `piece N`), the node holding its first window, its length
  and the span as written (cut at 200 characters). The task still runs.
- **The basis**: `theseus_protocol::TaskCheck` (new `check.rs`, with `CheckPiece` and `CheckOverlap`), stored on the
  check's session record as `TaskOf.check`: the checked task, its report node and time, the excluded sessions, the
  admitted pieces (each `from`: `checked`, `own` or `claim`), the profile, provider and model, the overlaps, and
  `at_ms`. Rows `task.check_opened` (the whole basis) and `task.check_refused` (class, `check_of`, reason), with their
  facts in `fact/check.rs`. One wording, `TaskCheck::line()`: `🔍 check of task a1b2c3 · independent (excluded
  ses_…a1b2c3, <model>)`, plus `· overlap: N spans` when anything is flagged, on `task.list` (`TaskInfo.check`),
  `theseus tasks` (a line under the task, one per flag), the report's outbox post and Discord's report head, Discord's
  `/tasks` line, the report node the parent reads, the `task.create` result (each flagged span listed, the basis in its
  meta), and the cockpit's task view (`lib/check.ts` mirrors `line()`; each flag on hover).
- **The store's format, 12 to 13 on the branch's base** (`TaskOf.check`, `Body::Arrangement.claim`, both absent when
  unset, so older records keep their bytes), with two literal format-12 samples in `tests_layouts` (a task session with
  its arrangement; an arrangement node with a trusted piece, a superseded piece and the fidelity ack); **14 at the
  join**, since compaction-roots' 13 landed first (Item 131).

**How it is proven.**
- **The session's tests**: `tests_check.rs` (5, through the whole core): a check's compilation holds nothing of the
  checked session but its claim (the brief, then the objective, the acceptance and the claim, in order; the maker's
  brief, its call and the words its tool read absent; exactly one edge into the maker's session, to the report, via
  `claim`); the basis recorded and shown (the record, the row, the call's result and meta, `task.list`, the report's
  post and node); a copied span of twelve words raises the flag and eleven does not; a check of a task with no report
  refused (a waiting task, a cancelled one, an unknown name, and `profile` without `check_of`); the exclusion on the
  check's own pieces (the relayed report becomes the claim; a parent node with a `derived_from` edge into the maker's
  tool result refused as `excluded`; an unknown profile refused). Unit tests in `check::tests` and the protocol's, the
  CLI's render test, the Discord report test, and the cockpit's `check.test.ts`. A focused run of 72 tests passed, and
  under load (four busy loops, the tests at nice 19) 14 of 14 in each of five runs. Planted reverts: the checked
  session's last tool result appended to the claim (`"forty two herring gulls" reached the check`); the flag at 13
  words (`left: 0`).
- **At the review**: the suites (theseus-core, the CLI, theseus-protocol, theseus-discord, theseus-store, theseusd's
  versions and tasks) ran 1,275 tests twice: first 1,274 passed and 1 failed, the store pin of join fix 3; then, with
  all three fixes, 1,274 passed and 1 failed, `term::tests::python3s_repl_computes_on_the_screen`, theseus-1n2y's known
  load flake in code the branch does not touch (alone at load 35.7, two passes in three). **Planted reverts, 5 of 5
  caught**: the exclusion gone; the flag at 13 words; `profile` open to every task; the claim left out of the check's
  compilation; and join fix 2 reverted.
- **Live, at the review**, on a scratch daemon of the merged tree (a transient unit, a fresh state dir, GLM's
  glm-5.3-flash, the judge, memory, the index, web and Discord off; about $0.012 of GLM). The maker: asked to count the
  words of an 11-word file as a background task quoting the message as its objective, GLM opened the task, which counted
  11, closed its record with evidence and reported. The check: asked for an independent check with `check_of` and
  profile `glm`, **GLM's first call named the task by its 39a record id (`tsk_…`) and was refused** (`task.check_refused`,
  class `unknown`); its second opened the check. The check's compilation held two nodes, the brief and the arrangement
  (the maker's objective piece, the `[Check: …]` paragraph, and `--- Claim: claimed by task …, as of 2026-10-04 17:05
  UTC ---` with the report), nothing else of the maker's session; the check read the file itself (`fs.read`, then `wc`
  and `od -c` through `proc.run`) and reported "the claim holds: 11 words". `theseus tasks` showed the 🔍 line with the
  excluded session and glm-5.3-flash; the `task.check_opened` row held the basis (the objective from `checked`, the
  claim, profile glm, provider zai); the parent read the check's report with the 🔍 line after its head.

**What the review found.**
1. **The branch's own miss, found by the suites**: theseus-core's `store::tests::a_write_moves_an_older_store_to_this_builds_format`
   still pinned format 12 after the branch's bump, and fails on the branch's own tree. The session's gate had failed
   it too, but its list of failures was printed with `awk '{print $5,$6}'`, which on that padded line gave the position's
   two fields, so the test's name never showed, and the report counted all 35 failures environmental (the root VM's 34
   sandbox tests and the time-zone golden). The pin now follows main's format plus one.
2. **The branch's own gap, found live**: `check_of` refused the 39a record id that the task graph's view and
   `task.create`'s result show. `check::claimed` now first looks for a task of this conversation whose record id
   (`task_graph::of_session`, 39a's own rule) equals the name, then falls back to `task::resolve`, which the older task
   tools keep using unchanged; new test `a_check_names_its_task_by_its_record_id`, and its plant fails.
3. **A conflict only the build shows**: 25c's `tests_learning.rs` builds a `TaskOf` and gains `check: None`.

The review was finished by a script (`finish.sh`, detached), which was to mark it complete only if the plants and the
suites passed. It blocked on two findings, and neither held: its "a plant's anchor was not found" matched the word
`skipped` in nextest's summaries (the script's bug), and its one failing test was theseus-1n2y's flake. The wake
overrode it at 10:39 with those reasons, and accepted. Semantic checks against what joined after the branch's base
found nothing more: 28b's `tasks.parked`, 25c's system labels (a check's `task.create` counts as `should_promote`, as
any task's does), 42a's `budget.list` (a check is a task, its carve under its parent) and the gate's order
(`task.create`'s layers unchanged), and 32c's recall (an arrangement renders through `check::render`).

**The join** (by the batch-5 harvest wake that took its lock at 11:03). Prepared in the cloud tree while
compaction-roots' gate ran, with main untouched until that lock's done line (11:10:34); then main fast-forwarded to the
prepared merge at 11:11 and was warmed and fixed there. Five files conflicted, all new beside 30c and all keep-both:
`fact/mod.rs` (`check`, then `compaction`), theseus-core's `lib.rs` (the two test modules), `recall.rs` (the branch's
`Arrangement` arm with its claim, then 30c's `Summary` arm), theseus-store's `store.rs` (format 13's doc line, then the
branch's) and `scripts/long-files.txt`. The review's resolve.py then **renumbered the format 13 to 14** (store.rs, theseusd's
versions test, the core's store pin, the session layout's label, node.rs's and session.rs's doc lines), applied its three
fixes and raised the protocol's `lib.rs` ceiling to 2,685. Join fixes: compiler.rs held at 30c's new 2,540-line ceiling
by folding the branch's one comment line into the line above, rather than raising it; the arrangement sample's label
and the core's guide say 14; and **two semantic conflicts with 30c that the warm build found**: `check.rs`'s
`working_text`, which matches every `Body`, met 30c's `Summary` (E0004) and **reads the summary's text**, since a summary
is the model's account of the maker's own turns and a check that copies it is copying the maker's working (`None`, as
for an arrangement or a recall, whose words are others', would have let a check paraphrase-copy through a summary
unflagged; new test `the_overlap_flag_reads_a_summarys_text`); and 30c's `tests_compaction` builds a `TaskOf` (`check:
None`, E0063). The first warm (11:11, the merge as f7999ac7) was red on those two; the merge was amended to e27405a2,
signed. The second warm (11:22): the test build, clippy, theseus-protocol 29 of 29 with the TypeScript unchanged, 130
of 130 targeted tests, shape ok; theseus-core's tests where the branch meets 30c and the new format, 44 of 44. Its gate
(started 11:33 after 306 s waiting behind another job's release build; exit 0 at 11:44:55): 2,320 of 2,320 (1 slow);
lifecycle OK in 20.6 s under the busy allowance (CPU pressure 91 %, Terminal-Bench running beside it); jobs ok; turn
frames 5 and 9, plain p50 84.6 ms (p95 108.6) and tool-call p50 171.4 ms (p95 192.8), against 4c3a7008's 75.6 and
171.2: **overnight decision 25's re-measure**, which showed 30c's high reading at its own join was load, and the
branch's one `Option` match per compile shows nothing. Pushed 11:45; theseus-vug.3 closed.

**The install** (14:09, at bddfd407, install #1). Its format, 14, is the one Eddie's store reached: from 6 to 14 on
start, one way, as every bump. Health after the restart: the config from `/etc/theseus/theseus.toml`, 9 secrets ready,
discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms.

**Eddie's call.** theseus-w8ys (P2) asked whether a check's task-graph view should stay as it is, leave out the
checked task's record, or show it by title only; the harvest asked without a recommendation. At 10:45, under Eddie's
"Take all of these excellent recommendations" (overnight notes, the 9am review), the call made for him: in a check's
view, the checked task and its subtasks show title and state only, no notes and no evidence; the check keeps the
graph's shape and gets the objective and the claim through its own basis (hiding the record outright would leave
`check_of`'s target missing from the model's view). It was built in smalls-tasks and closed there
(Item 146).

**Divergences.** The basis is `TaskOf.check` on the session record at a store format bump, not the design's SESSION
schema 3 to 4; the claim lives on the `Arrangement` node; `profile` is a check's alone; the fidelity check is skipped
for checks; the design's Observatory is the cockpit's task view. The exclusion follows `derived_from` edges only, so a
parent's reply that paraphrases the maker's working without an edge is caught only by the overlap flag, at 12 words or
more, as the design intends. `working_text` joins a node's text parts with newlines, so a window can span two strings
of one tool input: it can flag a little more, never less. The cockpit's time machine does not fold the basis for a past
moment. Not built, as the design files them: the transmission-ancestor rule inside JUDGE_STOP, and a `verify.v1` pack.

**Known gaps.** theseus-lczw (P3: the exclusion's walk stops silently at 20,000 nodes and admits what it did not reach;
a supersession is lost when its replacing piece is skipped). theseus-1n2y (P3, the pty flake). A check's
`task.create` loads the checked session's nodes once, walks its edges and hashes every 12-word window of its working
text, all on the call that opens the check and never on a turn.

### Item 133. 43b: an acked extension loads from its frozen copy, restarts with the daemon, and is revoked; `/extensions` and the cockpit's Extensions card (theseus-ext.8; roadmap row 76, step 43b; the fifth cloud batch's extensions-load session, launched 06:11 and fired 06:15 from d3cad454, extend-propose's join, Opus 5.5; 70b6a83f, 6a5eff6e and 29a80d8c; reviewed 09:29 to 10:51 by local reviewer R2, on 8f79c753 and replayed on 4c3a7008; joined 11:57 at bb210c2d, a signed merge onto e27405a2, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1)

**Why.** §3.21's planks: the agent may add a tool to itself as an MCP server in L1, loaded only on the operator's ack.
43a (Item 118) built the proposal, the frozen copy, the trial and the ack, and its ack loaded nothing:
the board's `servers` was a map fixed at build, so a server added later could not be offered. 43b is the rest of
`docs/design/m7-surface.md` §2.7: on the ack the extension loads, it comes back after a restart, and one command takes
it away.

**What landed** (the branch: 47 files, +3,070 −129 against its base; the merge onto e27405a2: 45 files, +2,650 −132; no
new package, no `Cargo.lock` or `package-lock.json` change; **no store format change**: the new record is a META key of
its own, `extensions`, as 36b read the rule for `mcp.tools.<server>`, and no stored record gained a field).
- **The ack's own frame** (`extend/answer.rs`, with `Core::load_of` in `extend/load.rs`, 309 lines) also stages: the
  META record `extensions` (the name to its `Loaded`: digest, description, command, frozen copy, source, tools,
  capabilities, who acked and through what, when, the question, the proposing session, that session's place and its
  ceiling at the ack, and the digest it replaced); the `extend.loaded` row, naming `replaced`; the server's stored list
  `mcp.tools.ext-<name>`, built from the trial's tools, so a new version never offers the old version's list; and a
  replaced version's manifest, now in state `replaced`. After the frame, `Core::board_load` puts the server on the
  board. A mutex (`Extensions::writes`) keeps a load and a revoke from interleaving their read-modify-write of
  `extensions`.
- **The board** (`mcp/ext.rs`, 111 lines, and about ten in `mcp/mod.rs`): `extensions`, a map of loaded servers beside
  `servers`, which `rebuild`, `status`, `stop`, `restart` and `start` include. A load on a started board spawns 36b's own
  `tend` for it under the list's lock, so a load racing `start` is tended exactly once. A newer version replaces the
  older and then ends it; an unload takes the server off the board, rebuilds the catalog, aborts its tending and sends
  SIGTERM to its process group.
- **What runs** (`Loaded::server_cfg`): 43a's trial configuration (the frozen copy, L1, the acked network as its
  egress, no secret), plus `external = !network.is_empty()` (the design's Q22: outside text only with a network) and the
  configured default `call_timeout_secs = 110`. A turn's tools are fixed once per turn (`TurnRunner::request_spec`
  calls `definitions_for` once, before the loop), so a load is offered from the next turn's start and never mid-turn.
- **The gate.** An `ext-` server's tool is `notify` ("an extension's default"), the stricter of notify and the
  enforcement, so an `approve` enforcement still asks; a `[policy.mcp]` line overrides either way. Its class is `Run`.
  A configured server named `ext-…` is refused at the config's check. **Never wider:** the proposing place's ceiling at
  the ack is recorded in `Loaded`, and that ceiling's **floor** holds every call of the extension's tools, wherever the
  call comes from (`Floors::floor`, one line in the gate's chain after the place's own floor; at the join, in
  `toolrun/order.rs`'s `Floor` layer). The recorded ceiling's `tools` list is not applied: the proposing place could
  not list `mcp:ext-<name>` before the ack, and applying it would make the extension unusable where it was asked for.
  Its tools are offered where MCP tools are: private places, within each place's own ceiling.
- **Restart** (`Core::seed_extensions`, in `Core::build`): one META read (`extensions`) and one stored list per
  extension; each is loaded without tending, as 36b's configured servers are, and `start`, after serving, tends them.
  A record that does not read is logged and skipped; it never fails the start.
- **Revoke** (`extend/revoke.rs`, 77 lines; the RPC wrapper in `rpc/mcp.rs`): `extension.revoke { name, author?,
  discord? }`. A name that is not loaded is refused, saying so; otherwise it is judged as an answer
  (`judge_act(Act::Revoke { name })`; a refusal is ledgered `approval.refused` with `act: extension.revoke`), then one
  frame writes the record without it, its manifest `revoked` and `extend.revoked`; the floor is cleared and the board
  unloads it. The frozen copy stays on disk. The method is in the CLI's `OPERATORS` as `theseus extend revoke`.
- **Surfaces.** `extend.list` gains `loaded` (each with its board state, calls, errors and last error); `theseus extend
  list` prints the loaded ones first, each with a `revoke:` line; health's `extensions.loaded`, and `ext-` servers in
  health's `mcp[]`; narrative lines for `extend.loaded` and `extend.revoked` (`extend.acked` no longer says "nothing
  loads in this build"); MCP telemetry unchanged (it keys on the `mcp:` prefix). The protocol: the method,
  `ExtendLoadedInfo`, `ExtensionRevokeParams` and `ExtensionRevokeResult`, the two ledger kinds, the TypeScript
  regenerated.
- **Discord's `/extensions`** (6a5eff6e; `runtime/extensions.rs`, 394 lines, and 15 in runtime.rs, 3,447 of its 3,500
  ceiling): registered globally, answered to the presser alone, each loaded extension with its short digest, state,
  network, tools, who acked it and when, and a Revoke button (five to a row, 25 at most). A press is `extension.revoke`
  with the presser's ids, judged by the core; the message is then updated with what happened above the list as it is.
- **The cockpit's Extensions card** (29a80d8c; `components/Extensions.tsx`, pure parts in `lib/extensions.ts`), in the
  Systems view once health counts any proposal, with no new route (`main.tsx` is a join file): each loaded extension
  (digest, state, server, network, tools, who acked it and when, the manifest's tests and description, files and
  bytes, the command, the frozen copy, calls, errors, the replaced digest, the last error), a Revoke button confirmed
  first, and then the proposals that are not what runs, with the `theseus confirm` id for a waiting one.
  `ExtendInfo` gains `files` and `bytes`.

**How it is proven.**
- **The session's tests.** theseus-core's `extend::` 16 of 16 (6 new in `extend/tests_load.rs`): an ack landing inside
  a turn's first model call offers that turn no `mcp__` tool, and the next turn calls `mcp__ext-wordcount__echo` from
  the frozen copy in L1 with no egress, no env and `external = false`, at notify under an open enforcement; a new core on
  the same store offers the tool at once, stopped, and starts it once after `start`, from the frozen path; a revoke from
  a shared guild channel is refused and the CLI's revoke empties the catalog, ends the fake, keeps the frozen copy and
  marks the manifest `revoked`; v2 proposed but not acked leaves v1 running, and acking it replaces v1 once; a shared
  place's turn is never offered an extension's tool; `[policy.mcp]` wins, and a load under a place with `posture_floor
  = "approve"` makes its calls approve, naming the place, until the floor is cleared. theseus-discord 99 of 99 (the
  list with 7 buttons in 2 rows; a shared channel's press refused and the owner's DM press revoking); the voice test
  that counts the binding's commands now counts ten (`/extensions` the tenth), the one edit outside the step's area.
  The cockpit's lint, 34 tests (2 new) and build.
- **Real processes** (theseusd's `tests/extend.rs`): as root, L1 refuses the trial and the test checks the refusal; as
  uid 65534, 2 of 2: the ack's `extend.loaded` and `mcp.list` at once; the process tree `mcp-sandbox`, `job-sandbox`,
  `theseus-sim fake-mcp`; a turn's call; `shutdown` ending all three; a new daemon starting it after `server.serving`;
  a `kill -9` of the daemon leaving none of its processes; `theseus extend revoke` refused from a job's shell
  (`THESEUS_SESSION` set) and done from the operator's. Under load (four busy loops at nice 0, the tests at nice 19):
  theseus-core's filter 5 of 5 runs, theseusd's test 5 of 5.
- **The session's planted reverts:** a load applied to the running turn's tools (fails "offered mid-turn": the second
  loop listed the five tools); a restart from the workspace's directory, not the frozen copy (the start ran from
  `…/work/tools/wc`).
- **The VM's gate**, at each commit: fmt, shape, features, clippy, the cockpit, the test build and the reader rule
  passed; the suite failed only on the root VM's 33 L1 tests (theseus-pv6i) and the core's golden under UTC; the turn
  bench by hand (5 and 9) and `cargo deny` ok. The VM's disk filled once mid-gate (18 GB of incremental cache).
- **At the review** (R2, on origin/main 8f79c753, tree cloud-r2): 281 tests on the merged tree, 281 of 281 first.
  **Planted reverts, 5 of 6 caught:** the mid-turn load; the run from the workspace's directory; a revoke that skips
  `judge_act` (the shared place's revoke went through); the tools at the enforcement's posture (`Open` against
  `Notify`); a configured `ext-…` server accepted. The sixth, the load's floor taken out of the gate's chain, **was not
  caught**: its test calls `Floors::floor` on a `Floors` of its own, so nothing checks that the gate applies it
  (theseus-sh9w, P2; the code is right as merged). R2 also ran the report's live check offline on the operator's
  machine, as its user, on the real L1 path: theseusd's real-process test, 10 of 10 runs after join fix 5. Not run: `/extensions` and
  its button pressed by the owner, and the card in a browser. No model, no Jev, nothing spent.

**What the review found.** Join fix 5 is the branch's own race: its real-process test failed in 3 of 7 runs, since the
board records `mcp.started` on a spawned blocking write and marks the server ready without waiting, so the test's one
ledger read could find the first run's row. The daemon does start it after serving; the test now reads until this
start's row is there, for at most 5 s, as `tests/mcp.rs` does, and a start before serving would still fail. A revoke
reads whether the name is loaded before `judge_act`, so a refused caller learns whether it is (nothing else).

**The join** (11:11 to 11:57, the batch-5 harvest wake, lock `cloud-extensions-load-join`). Prepared in a second tree
while independence's gate ran (first on f7999ac7, again on the amended e27405a2), with main untouched until that lock's
done line. rerere replayed all seven conflicted files (`fact/answer.rs`, `rpc/confirms.rs`, `rpc/mod.rs`,
`toolrun.rs`, the protocol's `ts.rs`, the CLI's `client.rs`, `long-files.txt`), and R2's `resolve.py` applied the join
fixes: 43a's `extend.list` and the revoke share one arm of `dispatch` through a helper (`rpc_ext`), so it stays within
clippy's 100 lines; **past budgets-policy (Item 130) the load's floor moves into `toolrun/order.rs`'s
`Floor` layer**, after the place's floor and before `seen(Layer::Floor, …)`, so the gate keeps it and `policy.explain`
runs it (on such a main `toolrun.rs` takes 42a's call into the order, and the line would otherwise be dropped in
silence); the restart test's read; the protocol's `lib.rs` ceiling at 2,687. Core::build runs 28b's judge attach, then
the seed; learning-ledger's `Act::JudgeLabel` sits beside `Act::Revoke`. No conflict or compile error from 30c or 28a.
`CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. It changes the gate, `Core::build` and the board, so it joined alone.
Warm: the test build, clippy, theseus-protocol 29 of 29 (`protocol.gen` unchanged), 85 of 85 targeted, shape. **Its gate
(exit 0 at 11:57:07):** 2,329 of 2,329, one test passing on its retry (theseus-sim's seeded-faults coverage check, the
known theseus-81ig); lifecycle OK in 25.4 s under the busy allowance (IO pressure 17 %); turn frames 5 and 9, plain p50
79.7 ms (p95 91.6), tool-call p50 193.1 ms (p95 234.3), against 171.4 at independence's gate twelve minutes before. The same
run's disk probe read fdatasync p50 12.9 ms against 6.4 to 6.8 at the last three gates, and the branch's cost for a
tool that is not an extension's is one prefix check, so the harvest read it as IO load and pushed (overnight decision
27), with an A/B if the next gate stayed up. Pushed 11:57; theseus-ext.8 closed; plan row 76 joined.

**The A/B** (12:08 to 12:18, decision 29). Hands-network's gate read 190.5 ms with fdatasync still 11.8 ms, so frozen
builds were compared: each arm's own theseusd, theseus-sim and theseus-index, A = e27405a2 and B = 452b0b9d (mgw.9 adds
nothing to a turn), `theseus-sim bench turn` in palindrome order, each round in one exclusive hold of the gate lock after
a PSI settle. Round 1 (4 against 4, 10 turns a run): tool-call median +9.4 ms, exact p 0.49. Round 2 (8 against 8, 20
turns a run, fdatasync 6.3 to 7.4 ms throughout): **+2.3 ms median, +2.4 mean, p 0.65; plain turns equal, 82.6 against
82.5**. 43b costs nothing the bench can see; both arms read about 196 ms in that window, so 171.4 was a low draw (the
day's gate readings span 171 to 206).

**The install** (14:09, at bddfd407, install #1). Health after the restart: the config from `/etc/theseus/theseus.toml`,
9 secrets ready, discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms; the store from format 6 to 14 on
start (43b adds none). The seed reads one META key at each start.

**Divergences.** "Never wider" is the floor of the proposing place's ceiling, applied everywhere; its `tools` list is
recorded, not applied (the design: "its ceiling is the proposing execution's at the ack"). The posture is notify never
looser than the enforcement. The `extensions` record holds more than the design's list (the place and ceiling, the
proposing session, the tools, the frozen copy, the replaced digest). The latest ack wins: a new version replaces the old
at its ack, so v1 acked after v2 loads in v2's place. An extension's `list_changed` is honoured as any MCP server's is,
with a notice, so it can change its own tool list from its frozen code. The design's Observatory section is a card in
the cockpit's Systems view (the cockpit replaced the Observatory, Item 86).

**Known gaps.** theseus-sh9w (P2: no test runs the load's floor through the gate). theseus-ext.13 (P2, filed from the
join by the DM thread at 12:50: only the tools the proposal named should load, and a new tool should need a new ack). A
narrow race in a revoke: an abort landing while the tending task sits between awaits can leave a client it just set, whose group is
SIGTERMed once its last clone drops, not at once. The owner's own press of `/extensions` and the card in a browser are
still to be seen.

### Item 134. Step 40's network: AWS hands in an existing VPC, never a NAT of their own, and a guard that uses another project's network and never changes it (theseus-mgw.9; step 40's network, after aws-hands; the fifth cloud batch's hands-network session, launched 05:42 and fired 05:46 from a4da5e1c, hands-cancel's join, Opus 5.5; 161225d5, 89aa52ce, 840ff07e and ab27e367; reviewed offline 10:37 to 11:23 by local reviewer R2; joined 12:05 at 452b0b9d, a signed merge onto bb210c2d, by the batch-5 harvest wake; installed 14:09 at bddfd407, install #1; its live check waits for the owner)

**Why.** At 23:24 on 2026-10-03 Eddie cleared aws-hands' live check (Item 107) on one condition: the hands
reuse the NAT gateway the account already runs for another of the operator's projects, rather than add one of their
own (about $36 a month). The aws-hands branch could not: `theseus-hands-network.yaml` always made its own VPC, with a
`NatGateway` toggle, and Fargate's discovery read the network only from that stack's outputs. mgw.9 lets the hands
network name an existing VPC, and makes Theseus's guards allow using that network and refuse changing it.

**What landed** (the branch: 34 files, +2,110 −40 against its base; the merge onto bb210c2d: 32 files, +1,648 −40; no
new package, no protocol type, **no store format change**: the store stays at 14; `config.rs` 2,770 of its 2,910
ceiling at the join).
- **The template** (161225d5; `infra/aws/theseus-hands-network.yaml`, +90). New parameters `ExistingVpcId`,
  `ExistingSubnetIds` (a list) and `ExistingSecurityGroupId`, each empty by default and pattern-checked. The condition
  `OwnVpc` covers everything of the VPC's own (the VPC, its three subnets, both route tables and their three
  associations, the S3 endpoint, the flow log with its group and role); `NatEnabled` becomes `OwnVpc` and
  `NatGateway=enabled`, covering the internet gateway, its attachment, the EIP, the NAT and both default routes; the
  hands' security group is made under `OwnSecurityGroup` in whichever VPC is in use, tagged, with no ingress. Two
  `Rules`: `NoNatBesideAnExistingVpc` (the NAT off, and subnets named, when a VPC is named) and
  `NoExistingPartsWithoutTheVpc`. The outputs keep their keys, naming the existing network, and `NatGateway` reads
  `existing`. `infra/aws/test/rules.py` gains a `network-modes` rule that evaluates the conditions and Rules under five
  parameter sets.
- **The config** (89aa52ce): `[aws.accounts.<id>.hands_network]` takes `vpc`, `subnets` and an optional
  `security_group`, each id's form checked at load; the default is none, and the stack makes its own VPC as before. The
  template carries the commented entry. `aws.stack.plan` of `theseus-hands-network` fills the three `Existing*`
  parameters from the config (`aws::hands::network::plan_parameters`), and a plan that gives any of them another value,
  or asks for `NatGateway=enabled` beside an existing VPC, is refused before anything is sent, naming the config's
  value. The session chose the refusal over the tender's reconcile: a reconcile that applied on its own could remove the
  stack's own VPC or move the hands with no plan shown, the wrong posture beside another project's network.
- **Discovery** (840ff07e; `aws/hands/network.rs`, 156 lines). With `NatGateway` reading `existing`, a Fargate launch
  first sends `DescribeRouteTables` filtered on the VPC (a read, in the work session): each subnet's table (its own
  association, else the VPC's main table) must send `0.0.0.0/0` to a NAT gateway whose route is not a blackhole.
  Otherwise the launch is refused, naming the subnet and the table, and the words never suggest a NAT: name routed
  subnets in the config's `hands_network`, or run on Lambda. A refused discovery is not cached, so the next call reads
  the routes again. `HandsEnv.existing` is `#[serde(skip)]`, and the group's record keeps only `nat` (true for a routed
  existing VPC), so the stored layout is unchanged.
- **The guard** (ab27e367): one entry, `network-not-ours`, under a new limit, `others-resources` ("another project's
  resources"). At the gate, every direct change to network plumbing that exists (deletions, modifies, replaces,
  revokes, disassociations and detaches of VPCs, subnets, route tables and routes, NAT, internet and egress-only
  gateways, ACLs, endpoints, flow logs, security groups and peering) asks at the floor; the entry is `direct`, with a
  `when` of `present` on the ids those calls name, so `ReplaceRoute` and `ModifySubnetAttribute` ask although they are
  IaC-only; creates of new things stay IaC-only. `check_call` now names every entry that hits beside a direct one. In a
  template, a member naming a resource the template does not make asks at plan (the evaluator's new `not_own`: a
  literal or a parameter's value is a yes, an import a maybe, the template's own `Ref` or `GetAtt` a no). On AWS's side
  the rule is `scp = "deny"` on `ec2:Delete*`, `Disassociate*`, `Detach*`, `Replace*`, `Modify*`, `Associate*`,
  `Attach*`, `Authorize*`, `Revoke*`, `CreateRoute`, `CreateNetworkAclEntry` and `CreateTags`, on the ARNs of VPCs,
  subnets, route tables, NATs, internet gateways, ACLs and security groups, where the resource has no `theseus:owner`
  tag and the call is not tagging at a create; security-group-rule ARNs are left out, and `CreateSecurityGroup` is left
  to the gate, so the hands' own tagged group can be made in another VPC. It is in the session guards, the boundary,
  the SCPs, and a new **`theseus-guard-deployer`**, generated from every `scp = "deny"` entry and attached to
  `DeployerRole` in the foundation (with `GuardDeployerPolicy` in the foundation's stack policy): the deployer had
  carried no session guard, and SCPs need an Organization, so "the deployer included" had no enforcer inside the
  account. The inventory and the reaper are unchanged: they list by `theseus:owner`, so the existing network is never
  theirs.

**How it is proven.**
- **The session's tests.** `infra/aws/check.sh` (cfn-lint 1.57.1): 5 templates, 0 violations, 36 rule tests.
  theseus-core: an existing VPC plans the hands' group alone, even with `NatGateway=enabled`, and nothing with a group named; the
  template scans clean in both modes; against C2's stateful CloudFormation fake, a new stack's change set is exactly
  `+ HandsSecurityGroup` with the config's three values, and another VPC or the NAT is refused with no request sent;
  discovery against part 1's fake refuses an unrouted subnet by name (no `RunTask`, one route read) and, once routed,
  runs one `RunTask` in the configured subnets with the stack's group and `assignPublicIp: DISABLED`, the hand settling
  `Succeeded`. The guard: six call cases and five template cases in `cases.toml` (hits and near misses), the gate's
  verdicts (the four plumbing calls ask; a public-IP `ModifySubnetAttribute` names both entries; `CreateRoute` stays
  IaC-only; a read is clear), a route into a literal table, a parameter or an import asking while a `!Ref` of its own
  does not, and `tests/policies.rs`: `DeleteRoute`, `ReplaceRoute`, `DeleteNatGateway`, `ModifySubnetAttribute` and
  `CreateTags` on an untagged resource refused by the session guard, the boundary and the deployer's guard, with no
  statement denying `ecs:RunTask` or `ec2:CreateNetworkInterface`. Under load, discovery's, the plan's and part 1's
  Fargate tests passed 20 of 20 runs.
- **The session's planted reverts:** the NAT not tied to `OwnVpc` (the Rust test, the rules test and six
  `network-modes` violations: the internet gateway, its attachment, the egress route, the address, the NAT and the
  private default route made beside an existing VPC); discovery's outputs merged before the parameters (fails "the
  hands VPC's NAT, which is off"); the tag condition removed from the entry (three tests).
- **The VM's gate**, before each commit: fmt, shape, features, clippy, the cockpit, the test build and the reader rule
  passed; the suite failed on the same 34 each time, none the step's (33 root-VM L1 tests, theseus-pv6i, and the golden
  under UTC, which fails on the base too); by hand, the protocol types, the turn bench (5 and 9) and `cargo deny`.
- **At the review** (R2, offline, on origin/main 4c3a7008): 237 of 237 on the merged tree and check.sh. The template
  read resource by resource against its conditions. Policy sizes: the boundary **6,016 of IAM's 6,144 characters**
  (128 left), `theseus-guard-limits` 5,535, `theseus-guard-deployer` 2,045 (main's `theseus-guard-iac` already 6,129).
  **Planted reverts, 4 of 5 caught:** the NAT beside an existing VPC; the tag condition dropped; outputs merged before
  parameters; a plan's other VPC let through. The fifth, a blackhole NAT route taken as a way out, passed: the fake's
  route tables hold no blackhole route (theseus-rx7m; the code handles it). The scrub: every AWS id the branch adds is
  a placeholder, and none of the owner's account ids appears.

**What the session found.** A bug in the way: discovery merged a stack's parameters over its outputs, so in existing
mode (`NatGateway` the parameter `disabled`, the output `existing`) the mode would have read as a NAT that is off.
Outputs now win.

**The join** (from 11:38, the batch-5 harvest wake, lock `cloud-hands-network-join`). Prepared git-only in a throwaway
worktree on extensions-load's prepared merge while that gated, with main untouched until its done line (11:57:42).
rerere replayed the one conflict, `config.rs`'s re-exports (the branch's `HandsNetwork` beside 25b's `SignalsConfig`);
R2's `resolve.py` had nothing left; no join fix. `CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. check.sh passed on the
prepared merge and again on main's tree (infra had not changed on main since R2's base). Main fast-forwarded to it at
11:58; warm 11:58 to 12:00 (the test build in 1 m 27 s, clippy), no semantic conflict with 30c, 28a or 43b; 238 of 238
targeted. **Its gate** (12:00:46 to exit 0 at 12:05:20): 2,337 of 2,337 (17 skipped, no flaky retry); lifecycle OK in
25.1 s; jobs ok; turn frames 5 and 9, plain p50 83.8 ms, tool-call p50 190.5 ms (p95 213.0) with fdatasync p50 11.8 ms.
mgw.9 adds nothing to a turn, so this reading was the re-measure of extensions-load's, and it led to that step's A/B
(Item 133). Pushed 12:05; theseus-mgw.9 closed, with the live check noted as owed.

**The install** (14:09, at bddfd407, install #1) carried the template, the config table and the guard. Nothing on AWS
changes until a plan of the network stack is approved and applied; the foundation's `GuardDeployerPolicy` arrives only
with a foundation apply. Health after the restart: 9 secrets ready, discord ready, judge on, lsp on, startup 73.1 ms; the
store from format 6 to 14.

**The live check** has not run. It spends money and applies the foundation (its plan shows `+ GuardDeployerPolicy` and
`~ DeployerRole` at the floor), so it waits for the owner's go: the Fargate half of decision 9's paid checks, queued by
the DM thread after this join (overnight decision 28). What it must show: the network in the config and a new stack's
change set of exactly the hands' group, nothing at the floor (an existing own-VPC stack instead plans the group
replaced and its VPC's parts deleted, the log group waiting as stateful; the replace's fixed `GroupName` may fail); a
plan naming another VPC or the NAT refused; a one-hand Fargate group reading the routes, then launching into the
configured subnets and settling once; a model's `ec2:DeleteRoute` on a configured subnet's table asking at the floor;
`aws.inventory` listing the hands' group and nothing of the existing network. One risk to watch: if CloudFormation tags
the group after creating it, the deployer's guard refuses the `CreateTags` (the fix: the config's `security_group`, or
`CreateTags` out of the entry's `iam`).

**Divergences.** The design's network (`docs/design/aws-toolset.md` §3.3) has a NAT of the stack's own, turned on when a
hand needs egress; beside an existing VPC Theseus never makes one. No S3 gateway endpoint there (it would change the
other project's route tables, which the guard refuses), so S3 and ECR pulls pay the existing NAT's data charge, about
$0.045 a GB; no flow logs on a VPC Theseus did not make. A Theseus security group in the other VPC is built as not a
change to it (tagged, no ingress, attached only to Theseus's tasks); the config's `security_group` names the other
project's own instead. The deployer is now bound by every `scp = "deny"` entry, not only this one: a stack that ever
needs one of those actions fails until that changes. `plan_parameters` keys on the stack's name, so the same template
planned under another name is not filled from the config.

**Known gaps.** The live check (above). theseus-mgw.13 (P2, filed at 12:50: the boundary has 128 characters left, and
the next guard entry may not fit). theseus-rx7m (P3: no test of a blackhole route; closed on 2026-10-05 by
Item 174). The AWS side leaves creates of new things inside another VPC (subnets, route tables, endpoints,
flow logs, NATs, security groups) to the gate: sessions are denied them already (IaC-only), and in a stack the
template rule asks. A floor session, allow-all alone after an approval, is not under the guards, as for every entry.

### Item 135. The cockpit without WebGL: a plain page in the Ship's place, not the crash page (theseus-9k53; the `cockpit-webgl` lane, a subagent spawned 12:50 on the brief written 12:48, Opus 5.5, in a worktree from 452b0b9d; 3193f660; the lane's review line and join at 13:14, a fast-forward onto 452b0b9d; installed 14:09 at bddfd407, install #1)

**Why.** At 12:45 Eddie's cockpit (his daemon, `localhost:7433`) showed only an error page: "THREE.WebGLRenderer: Error
creating WebGL context", with the console's "A WebGL context could not be created … GL_VENDOR = Disabled …". His
browser had WebGL off. The Ship's engine built `THREE.WebGLRenderer` unguarded, its constructor threw, and the route's
`errorElement` took the Ship's place. The cockpit opens on the Ship, and the install he was about to take brought
Judgment, the label buttons, approval cards and Extensions to it, so this fix went in first. (The DM thread found at
13:38 that the browser was Chrome inside WSL, through WSLg, not his Windows Chrome.)

**What landed** (one signed commit, cockpit only: 4 files, +170 −8; no store format change).
- `cockpit/src/ship/webgl.ts` (new, pure, so `npm test` runs it): `hasWebGL(canvas)` asks a canvas for
  `getContext('webgl2')`, then `'webgl'`, and releases a context it gets at once (`WEBGL_lose_context`); a canvas that
  throws counts as no WebGL. `tryBuild(build)` returns the engine, or the failure when its constructor throws.
  `isWebGLFailure(reason)` tells WebGL's failures from other errors.
- `cockpit/src/ship/NoWebGL.tsx` (new): `ShipFallback`, "The Ship needs WebGL": the 3D Ship needs WebGL, which this
  browser has turned off; how to turn it back on (hardware acceleration in the browser's settings, a restart, then the
  browser's GPU page, where WebGL should read "Hardware accelerated"); a Reload button, a link to each of the 11 other
  views, and a "Cause:" line. For an error that is not WebGL's, the same page reads "The Ship stopped" with the error.
  `ShipBoundary`, an error boundary around the Ship alone, shows it for any later error in the Ship's tree, so such an
  error never reaches the route's crash page.
- `cockpit/src/views/Ship.tsx`: `ShipRoute` probes once a page, before the Ship reads anything; without WebGL it renders
  the fallback and none of the Ship's reads start. Otherwise the Ship runs inside `ShipBoundary`, and its engine is
  built through `tryBuild`, since a probe can pass and the renderer still fail. `main.tsx` (the routes, a join file)
  and the shell are unchanged.

**How it is proven.**
- `npm test`: 47 of 47, 5 new (`cockpit/test/webgl.test.ts`): a stubbed `getContext` giving null after asking for
  `webgl2` and then `webgl`; WebGL 2, or WebGL 1 alone, counting as WebGL, each context released; a `getContext` that
  throws, or no canvas, counting as none; an engine whose renderer throws `Error creating WebGL context.`, as three's
  does, classed as WebGL's; an engine that builds returned, and another error not classed as WebGL's. `npm run lint`
  (no warning in the changed files) and `npm run build`; the fallback ships in the Ship's chunk.
- **Headless Chrome** (puppeteer-core, 1600x1000) against a scratch daemon of the lane's debug build serving the lane's
  cockpit, with its own state, socket and port, Discord off and invented keys:
  - WebGL off (`--disable-gpu --disable-software-rasterizer`): the fallback in the Ship's place, no canvas, no crash
    page, the rail there; Judgment opens from the fallback and from the rail; console empty;
  - `--disable-webgl`: the same;
  - all 12 views open from the rail with WebGL off, each with its panels and no crash page;
  - SwiftShader: the Ship's canvas drew (3 frames on an idle daemon), no fallback;
  - a render error planted in the Ship's tree: "The Ship stopped" in its place, the rail there;
  - **the planted revert** (`Ship.tsx` as on main): the old crash page, "THREE.WebGLRenderer: Error creating WebGL
    context.", with three's console message in the same shape as Eddie's. Restored with a fresh mtime, rebuilt, and
    the modes rerun.

  The check reads titles by `textContent`, since CSS upper-cases them in `innerText`; its first "before" run missed the
  crash page that way, and every mode was rerun after the fix.

**What the lane found.** The shell's rail was never lost: the route's `errorElement` sits inside the shell's children,
so the crash panel replaced only the outlet. But the cockpit opens on the Ship, so the crash panel was all the landing
view showed.

**The join** (the lane, lock `lane-cockpit-webgl-join` from 13:05:32). Main had not moved, so `git merge --ff-only` to
3193f660, no rebase; the commit's signature `G`. Warm outside the lock (clippy fresh in 5 s, one crate relinked in 13
s). **Its gate** (13:06:29 to 13:13:13, ok in 404 s): 2,337 of 2,337 (17 skipped); lifecycle, p50 and p95, cold start
28.8 and 35.8 ms, start from the config copy 31.9 and 35.9, clean shutdown 68.5 and 96.0, SIGKILL and restart 37.2 and
43.5, binary swap 93.2 and **250.6** against a strict 202 (one sample, with the machine still busy after the settle, IO
pressure 16 %: passed on the busy allowance, the strict miss in the history; the bench runs the web UI off, so a
cockpit change cannot move it); L1 start p50 6.08 ms; turn frames 5 and 9, plain p50 83.7 ms, tool-call 207.3 ms with
fdatasync p50 13.1 ms, about twice its quiet value. Pushed 13:14:01; theseus-9k53 closed; worktree, branch and target
removed. Main was then free for install #1.

**The install** (14:09, at bddfd407, install #1). Eddie's cockpit at `/` carries the fallback. Health after the restart:
the config from `/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on, lsp on, startup 73.1 ms. The same
afternoon his Windows browser reached the cockpit through the relay on port 17433 (the chain log's 14:19 line), since
the machine's mirrored loopback was broken both ways.

**Divergences.** None from the brief, beyond its premise: the brief said every view was unreachable, and only the
landing view was.

**Known gaps** (open, not filed). A WebGL context lost after the Ship has drawn (a GPU process crash, say) is not
covered: three stops drawing and the Ship goes still, but nothing throws, so no fallback shows. The probe runs once a
page, so a page that found no WebGL keeps the fallback until a reload.

### Item 136. 31a: the memory pass after each turn, written only between turns: a deterministic labeler, the gate's `same_entity` and `supersedes` edges, attribution, baseline's second version, and `memory.v1` and `attribution.v1` in shadow (theseus-6fn.6, with theseus-ms5m and theseus-lx3x; roadmap row 56, step 31a; the fifth cloud batch's memory-pass session, launched 02:59 and fired 03:03 from 760553f7, recall-node's join, Opus 5.5; 18d94da5, 441d9ef4, ae58b540, ed0a3e1e, eb08cbd7, 9456a4fc, 4aaaf0a9 and e79c9f63; reviewed 09:25 to 11:13 by local reviewer R1's relaunch (R1c), not accepted on theseus-ms5m; Eddie's decision 10 at 12:45; its three fixes by the memory-pass-finish subagent, 12:50 to 13:40, re-checked in the review's addendum; joined 14:04 at bddfd407 by the DM thread, the signed merge d3ac1c2a onto 3193f660 and three signed fix commits, 1058b7a1, db421a51 and bddfd407; installed 14:09 at bddfd407, install #1)

**Why.** §5.2 and `docs/design/m6-memory.md` §2.6: after each turn, a pass labels what was said, links near-duplicates
and corrections, and records which recalled notes the reply used, so later steps (31b's consolidation, 32a's retention,
32b's activation) have edges and outcomes to read. 31a is the deterministic half, with Jev's two packs beside it in
shadow.

**What landed** (the merge onto 3193f660: 53 files, +3,776 −49; the three fixes: 7 files, +535 −55; no new package;
**no store format change**: an EDGE's `kind` is a string, which an older build reads as unknown, and the three rows are
ledger rows, so the store stays at 14).
- **The pass** (`crates/theseus-core/src/memory_pass/`). `MemoryPass` is a field of `TurnRunner`; at a turn's end, with
  memory on, `after_turn` is one channel send, and the transcript read, the tender's entities and neighbours, the labels
  and the frames all run on the pass's own task. Eligible: user and assistant messages and tool results with text, not
  written by the harness, and (decision 10) a non-empty compaction `Summary`; never a `Recall` (excluded by its body,
  whoever wrote it), a tool call, or an `Arrangement` (its text is its sources'). What a pass has done is read back
  from its rows once per session per daemon, so nothing scans on the start path.
- **The labeler** (`memory_pass/labels.rs`), word tables and no model: kind (decision, then preference, the operator's
  only; transient for short acknowledgements; procedure for a code block with an instruction; fact; other), durability
  by kind and author, volatile values named with why, trust from DD5's `external`, and `about` from **the index
  tender's one extractor**: a new tender method, `index.entities { texts }` (at most 256 a call), asked once a pass, so
  the core holds no copy of the rules. Without a tender the rows say `entities_unavailable` and why. A summary is
  labeled as a reply is (`Shape::Summary`: the operator's phrase rules and the correction flag are not its), and its
  trust is its range's: `external` when a tool result from outside lies between its first and last positions.
- **The gate** (`MemoryScience::gate` over `index.neighbours`, k 10, as of the node's position, so only earlier nodes,
  of the kinds the pass labels and of other turns; a reply restating its own question is no duplicate). Thresholds are
  the science's data, readable for the row (`merge_cosine` 0.92, `supersede_cosine` 0.75). **As fixed (theseus-lx3x),
  an operator's correction whose top neighbour reaches 0.75 supersedes it, however close; otherwise 0.92 or over is a
  duplicate.** Edges are 12a's EDGE with `via = "memory"`: `same_entity` from the new node to its duplicate,
  `supersedes` from the correction to the older node, scoped into the older; `node.reach` follows neither. A node not
  yet embedded is asked again after 250 ms, doubling, for at most 4 s a pass, then left whole for its session's next
  pass; an index answering BM25 alone, or no tender, gives `unavailable` and why, no edge.
- **Attribution** (`attribution.rs`, `recalls.rs`), per item of an ended turn's `Recall`: used when one of its entities
  (of the excerpt the model saw) is in the reply or a call's input, or 8 of its words in a row are in the reply, with
  `by` saying which. Its outcome comes at the pass after the session's next input: `corrected` when the operator's next
  message is a correction overlapping the item, `ok` for any other operator message, `unknown` when the next input is
  not the operator's (a wake, a task's report). An unused item is written at once with no outcome.
- **The rows:** `memory.labeled` and `memory.gated`, keyed by node and **scoped `memory:<session>`** (30b's label set
  scans the whole `memory` scope, and a session's next pass reads its own with one scan); `memory.used`, one per item,
  scoped `recall:<session>`.
- **Baseline's second version** (theseus-memory): `Baseline.version` 2, the default, prefers the newer node; version 1
  keeps 30a's parameter line and digest (`baseline@46038939f14a4f49`, pinned), and version 2's is
  `baseline@8bc51e97cf11d435`. After the filters (the place first, unchanged) and before the rank, the older side of a
  `supersedes` drops as `superseded`, and all but the newest of a `same_entity` group as `duplicate` (chains resolve),
  each only when the newer node is a kept candidate or already in context. The links reach the pipeline in
  `Asker.links`, read from the EDGE records (scope `in:<id>`) only for a science that reads them. Two new drop reasons,
  with narrative words.
- **`memory.v1` and `attribution.v1` in shadow** (theseus-judge): `memory.v1` a Choice over the labeler's kinds (`other`
  as no match) with `corrects_earlier` and `volatile` Nouls; `attribution.v1` one `relied_on` Noul per note, at most 6;
  a new point, `memory_pass`; builders `memory` and `attribution`, whose states hold only what the session's model saw,
  through the core's scrubber; `Baseline::Rules` and `Source::Notes`. `JudgeService::at_memory_pass` adds two `WIRED`
  lines in shadow, sampled (1.0), reserved from the shadow budget and recorded by the sink (`judge:memory`,
  `judge:attribution`), with no trace marks, since no turn waits.
- **Between turns** (theseus-ms5m, `memory_pass/turns.rs`, decision 10). Every turn counts itself from the top of
  `TurnRunner::run`, before its first frame, until it returns (`Turns::begin`, a guard). The pass's frames (one per 32
  nodes or 2 s after the first waits) are written only when no turn runs anywhere in the daemon and none has for 500 ms:
  event-driven, a notify when a turn ends, a timer only for the quiet stretch and the bounds; the WAL-position poll is
  gone. A handshake of sequentially consistent atomics closes the race at a turn's start: whichever side looks second
  sees the other, so the pass steps back or the turn waits for the one frame being appended. After 120 s
  (`QUIET_BOUND`) a frame takes any moment with no turn running; after 600 s (`BUSY_BOUND`) of turns running all the
  while it is written beside them, with an INFO line: the one case a pass frame can land inside a turn. Ended turns
  queue meanwhile, and a session's next pass labels all its unlabeled nodes together. The turn bench keeps the pass on
  (`quiet_config` leaves `[memory]` in shadow, with a comment saying why).

**How it is proven.**
- **The session**: the pass's tests (only the eligible labeled, a recall never; the labels and the index's entities; at
  most one frame per 32 nodes or 2 s on a paused clock; memory off does nothing; the labeler's 17-row table; volatile
  values; trust and `about`), the gate's thresholds and its wait, attribution's run of 8 words (7 is not one) and its
  outcome, every drop reason, the newer node preferred, a correction superseding its fact through a whole core, the
  place property tests, the packs' loader rules and goldens, and with the fake Jev one `memory.v1` per node labeled and
  one `attribution.v1` per recall. Planted reverts: a recall let in; `prefers_newer` false; the gate reading the index's
  `state`. A scratch daemon with the real tender and no model files found that last bug: the gate read `state`
  (`ready`) where `bm25_only` is the `mode`, so every pass waited 4 s and left every node (4aaaf0a9). The turn bench,
  three runs, 5 and 9 frames; under load, 94 memory and recall tests 5 of 5 runs.
- **R1c's review** (on main 084ccbbb): 1,214 of 1,214 at a load of 25, 6 of 6 planted reverts, and a live check on real
  Nomic vectors with the stand-in model: a correction scored **0.954** against its fact and became `same_entity`, not
  `supersedes`, since the merge line was checked first (theseus-lx3x); recall still admitted the correction first and
  dropped the fact as `duplicate`, and `memory.used` said `used: true, by: [run], outcome: ok`. **Not accepted:** the
  turn bench's frame check passed 3 of 3 at a load of about 11 but failed 2 of 3 at about 25, each with a pass frame
  inside a measured tool-call turn ("a tool-call turn's trace counts 9 frames, and the WAL holds 10"): the 500 ms
  still-WAL guard did not hold when load stretched the gaps inside a turn (theseus-ms5m).
- **Decision 10's re-check** (on main 452b0b9d plus the merge and the fixes): the build, clippy, rustfmt and the
  cockpit clean; 1,260 tests run, 1,259 passed (the one, `tests_lsp_edits::the_block_adds_no_frame`, has memory off and
  passed 5 of 5 alone: theseus-xx6w); **13 of 13 planted reverts** (R1c's six and seven new: the turn guard's call, its
  running check and the handshake's wait dropped, no quiet stretch, lx3x reverted, a summary never labeled, its trust
  not its range's), caught by turns.rs's three paused-clock tests, the whole-core `the_pass_writes_only_between_turns`
  (a model held 3 s inside B's turn while A's frame comes due), the gate's table at 0.954 and the summary's test. **The
  turn bench with the pass on: 10 of 10 runs**, plain 5 and tool-call 9 frames each, runs 9 and 10 beside 24 CPU-bound
  processes at a load of 20.2 to 26.5. A live check of 22 turns wrote 3 pass frames, none inside a turn: A's first came
  due 2 s after A's first turn and was written 511 ms after A's last; B's came 1.8 s after its last turn.

**The join.** The finisher took the join lock at 13:37, found the DM thread's `install-wsl-restart` lock holding main
(the restart for Eddie's broken loopback) and released its own untouched; the WSL restart (13:50) ended it. The DM thread
ran its `fix/join.sh` at 13:56: the signed merge d3ac1c2a (rerere replayed six of the seven conflicted files; `graph.rs`
had a new preimage beside `VIA_CLAIM`, and the keep-both held), joinfix.py's fixes, then the three fix commits from
their patches, and a check that the tree differed from the proven one only by main's files since 452b0b9d. The join
fixes: `recall.rs` merged cleanly but wrongly beside rerank-arm (its `manifest_with` now takes the links, and returns
them, so the `+rerank` repack drops what the recall dropped with no second read); `eligible` and `shape_of` match every
body; rerank's `Reranked` delegates `gate_thresholds` and `prefers_newer`; the exam's replay takes no links; the judge's
tests beside the judging stack; and two found on today's main: independence's `edges_into` (E0004: the derived-from walk
ignores `same_entity` and `supersedes`, which never mean derived from) and the protocol's type-list test at 101 lines.
`turn.rs` stayed at 3,420 of its 3,523 ceiling. **Its gate** (exit 0 at 14:04:12, 260 s): 2,360 of 2,360 (17 skipped);
lifecycle OK in 20.2 s (cold start p50 20.3 ms, the clean shutdown 33.2, SIGKILL and restart 25.7, the binary swap 50.6
and p95 54.7); L1 start p50 5.36 ms; turn frames 5 and 9, plain p50 75.7 ms, tool-call p50 168.4 ms, fdatasync 6.5 ms.
Pushed; theseus-6fn.6, theseus-ms5m and theseus-lx3x closed; plan row 56 joined. The DM thread's review (14:21) matched
the worker's proofs to their logs.

**The install** (14:09, at bddfd407, install #1, the install's own head). Eddie's config, written to
`/etc/theseus/theseus.toml` at 13:07, runs `[memory]` live on arm `baseline`, with `[judge]` and `[lsp]` on, so the
pass labels his turns, and `memory.v1` and `attribution.v1` run in shadow. Health after the restart: 9 secrets ready, discord ready,
judge on (every pack shadow), lsp on, startup 73.1 ms; `theseus memory search` ran on `baseline@8bc51e97`, version 2,
while the vectors were still loading; the store from format 6 to 14 on start.

**Eddie's calls.** Decision 10 at 12:45, "option c, I agree": the pass writes only between turns and stays on in the
turn bench (ms5m), the correction rule runs before the 0.92 line (lx3x), and compaction summaries join the eligible
bodies (R1c had left them never labeled, as harness writing). The review's other points for him: the bounds' values
(120 s and 600 s; only the second lets a frame into a turn); `memory.v1` at sample 1.0 (with the judge and memory on,
one Jev call per labeled node and one `attribution.v1` per recall, within the shadow budget); a summary that
near-duplicates a message of its own range gets a `same_entity` edge, and baseline v2 then recalls the summary's
paraphrase over the original.

**Divergences.** The design's rows are scoped `memory`; these are `memory:<session>`. The design's EDGE payload
`{ weight, by }` is not written: `via = "memory"` names the writer. The gate checks the correction rule first. No
`contradicts` edge: `memory.v1`'s `corrects_earlier` asks in shadow. The design's outcome `ok` "when the task succeeds"
is not built: a next input that is not the operator's is `unknown`. A new point, `memory_pass`, since neither
`loop_end` nor `exchange_end` is when the pass runs. Batching keeps 32 nodes or 2 s, but only between turns.

**Known gaps.** The thresholds rest on one Nomic reading. A node later than the 4 s vector wait is gated only at its
session's next pass, so a session's last turn may wait. An exchange that ends never writes its used items' outcomes.
The pass has no stop rule, so a pass frame can land after a clean stop's checkpoint and be replayed at the next start.
The exam's replay reads no links, so memory-arm's exam scores baseline without the newer-node rule until its recording
keeps the pass's edges. The installed tender must be this build for `index.entities` (an older one answers "no method",
and the rows say `entities_unavailable`). Not run: `memory.v1` and `attribution.v1` against the real Jev (the report's
live step 3). The cockpit's Memory view, proposed by the report, is not built. theseus-xx6w (P3: the LSP block's frame
test races a 100 ms stillness wait).

### Item 137. The README's logo, animated: rolling seas, with a GIF fallback and the still logo for reduced motion (theseus-wuxa; the `logo` lane, a subagent spawned 14:27 on the brief written 14:26, in a worktree from bddfd407; 9bf8ac35; joined 15:10 at 9bf8ac35, a fast-forward onto bddfd407, by the lane; reviewed 16:18 by the DM thread; docs only, in the tree at install #2, 20:00 at 3085f71a)

**Why.** Eddie at 13:07 and 13:10: "what ever happened to my rolling seas animaged theseus logo?", and "I meant the
image also in the readme at the top-- making that an animated gif/png/etc"; at 14:24, that it did not yet appear live
on GitHub. No animated logo had ever existed: the README's top image was the static `docs/assets/theseus-logo.svg`.

**What landed** (one signed commit, 3 files, +149 −1, and a 1,745,754-byte GIF; no code).
- **`docs/assets/theseus-logo-animated.svg`** (8,511 bytes; the static logo is 6,104, unchanged): the static logo's own
  artwork, animated with CSS `@keyframes` in an inline `<style>`, with no script. One 12 s loop, every period dividing
  it, so it repeats seamlessly: the four sea rows roll aft by one wavelength each, the far row slowest (12 s) and the
  near rows fastest (4 s), for depth; the ship bobs ±3 units and rocks ±1.8° about the middle of her hull at the
  waterline, a quarter-cycle ahead of the bob, so the bow rises as she climbs a swell (6 s); the seven stars twinkle,
  staggered; the five gold planks glint once a loop, stern to bow. Each wave path runs one wavelength past both edges,
  so a row never shows its end. `prefers-reduced-motion: reduce` stops everything, leaving the static picture (4 of
  640,000 pixels differ, by at most 2 levels, from antialiasing).
- **`docs/assets/theseus-logo-animated.gif`**: the raster fallback, 400 px, 150 frames at 12.5 fps, looping, with
  transparent corners. APNG was 4.9 MB from ffmpeg, and GIF animates in more viewers. The frames were rendered in
  headless Chrome, each animation paused and seeked to the frame's time; a palette of 255 colours plus transparency
  keeps every colour that fills at least 150 px of some frame exactly (89.2 % of opaque pixels exact); a small encoder
  of the lane's own writes only the changed pixels after the first frame (about 11.6 KB a frame; ffmpeg's encoder gave
  3.6 MB, since it drops frame differencing for any frame with a transparent pixel).
- **`README.md`**: the top image is a `<picture>`: the static logo for reduced motion, else the animated SVG, else the
  GIF, at the same 200 px and with the same alt text. Only the top block changed.

**How it is proven.** Headless Chrome: by default the animated SVG at 200×200, moving (5,293 px changed in 1.5 s);
emulated reduced motion, the static logo, still; a plain `<img>` of the animated SVG under Chrome's global
reduced-motion switch, still, so the SVG's own media query works too. The loop's end against its start: 0 of 640,000
pixels differ. ffmpeg decodes the GIF's 150 frames, each matching its paletted source pixel for pixel, and the wrap from
the last frame to the first changes 12,490 px, within an ordinary step's 12,036 to 12,594. **Live on GitHub**, after
the push: GitHub keeps the `<picture>`, rewrites its relative sources to the repository's raw paths, and the browser
loads the animated SVG at 200×200 with the alt text; two shots of the page differ in 5,563 px, all inside the logo's
box; the SVG is served as `image/svg+xml` with its 18 animations running; GitHub's blobs match local main. Eddie saw
it (the DM thread's review, 16:18). The scrub found only what main's README already had.

**What the lane found.** After the commit, a renderer flaw: Chrome repaints only part of what moved when animations are
seeked, so a fresh page kept faint antialiasing from its pose at load. The renderer now waits for every animation,
seeks each in one step and repaints the root before each shot, and two builds are byte-identical. The committed GIF came
from the earlier renderer and differs from a rebuild in 0.40 % of pixels, consistently within itself; the lane judged
that not worth a second join.

**The join** (the lane, lock `lane-logo-join` from 15:00:33). A fast-forward of main to 9bf8ac35 (signature `G`) at
15:01:16, and the gate (15:01:25 to 15:10:18, ok in 533 s, 65 s of it waiting on the shared gate lock that two cloud
reviews held): 2,360 of 2,360 (17 skipped); lifecycle passed on the busy allowance, with IO pressure still 18 % after
the settle (cold start's p95 76.6 ms against a strict 57.1, recorded as a strict miss; the diff is docs only); jobs ok
(L1 start p95 9.0 ms); turn frames 5 and 9, plain p50 79.6 ms, tool-call 200.6 ms. Pushed 15:10:40; theseus-wuxa closed.

**The install.** Nothing to install: install #2 (19:59:53 to 20:00:01, at 3085f71a) carried it in the tree. Its health:
store format 16; route.v1, rerank.v1 and security.v3 live; startup serving 29.1 ms.

**Known gaps.** Not checked: a real OS-level reduced-motion reader on GitHub, GitHub's mobile app, and its dark mode
(the corners are transparent, as the static logo's are). The rebuild recipe lives outside the repo, with the operator's
notes.

### Item 138. One-command setup: `scripts/setup.sh` from a checkout to a running user service, the config's lookup ending at `/etc/theseus/theseus.toml`, `--unit` for a second daemon, and the README's "Set it up" (theseus-00me and theseus-5aqz; the `setup` lane, a subagent spawned 14:12 on its brief, in a worktree from bddfd407; 6be6bdc6 and 2e5c2cf7; joined 16:01 at d5ff8489, a signed merge onto 9bf8ac35, by the lane, on its second gate; reviewed 16:12 by the DM thread; installed 20:00 at 3085f71a, install #2, by the install script, with setup.sh's dry run on the same machine)

**Why.** Eddie at 13:05: the config leaves the vault for "a default /etc based config; you're in charge of maintaining
it", with no secrets in it and the template kept in source (theseus-5aqz: the code's default should find it). At 13:10:
one-command setup in the README, "so other agents can set up theseus trivially", and Linux only, by design, using the
kernel (theseus-00me; the kernel features went to their own survey, theseus-779n).

**What landed** (the merge: 21 files, +1,924 −160; no store format change, the store stays at 14; no new package).
- **The lookup** (6be6bdc6; `crates/theseus-core/src/config/lookup.rs`, 195 lines): `--config`, then `THESEUS_CONFIG`
  (clap reads both into one value, the flag first), then `~/.theseus/theseus.toml` if it exists, then
  `/etc/theseus/theseus.toml` if it exists; with neither, `NO_CONFIG` names both places. theseusd runs it once in
  `main`, after the job wrapper's, hand's and L1 roles' dispatch and before `install`'s, so `install --user` writes the
  found file into the unit. A path the lookup cannot examine counts as there, so its read fails saying why. In a debug
  build `THESEUS_TEST_SYSTEM_CONFIG` stands in for the `/etc` file, so no test reads the machine's; a release build has
  no stand-in. `theseusd config` (`# source: <path>`), `check` (`ok: config loaded from <path>`), health (`config:
  file <path>`), the help text and the template's header all name the file in use.
- **`scripts/setup.sh`** (2e5c2cf7; 665 lines), `[--dry-run] [--op-token-file FILE] [--prefix DIR] [--config PATH]
  [--unit NAME --state-dir DIR --socket PATH] [--no-build]`, each step printing a line per thing it checks or does
  (`ok`, `info`, `WARN`, `FAIL`) and its commands with a leading `+`:
  1. **a read-only preflight**: Linux, not root; the systemd user manager (with hints for WSL's `systemd=true`) and
     linger; cgroup v2; the pinned toolchain; node 22 or later; `op` (a FAIL when the config names `op://` references);
     the token file by `stat` alone (a regular file, the operator's, 0600 or stricter, not empty); whether the config's
     directory can be written, else sudo; a WARN when the prefix is not on PATH; a note when `~/.theseus/theseus.toml`
     exists, since the lookup reads it first. Any FAIL stops the run before anything changes;
  2. **the build**: `npm ci` and the cockpit's build, each skipped while fresh, then `scripts/build.sh --profile
     release-thin`;
  3. **the install**: each of the five binaries that differs from the build, `install -m 0755` to a temporary name and
     a rename, else "nothing to install";
  4. **the config**, written once: `theseusd example-config` cut by `config --sparse`, `projects_dir` made portable, a
     header; asserted before writing (every `[secrets]` value an `op://` placeholder, no run of 40 token characters,
     and the result loads), then mode 0640 by a rename. Its directory is made 0750, or once with `sudo install -d -o
     <you>` when the parent is not writable. An existing config is never written: the run lists the keys that are only
     the operator's, only the template's, or another value, never a value;
  5. **the check**: a placeholder left, or `op://` references with no token, stops the run for the operator (exit 3),
     naming what to do; otherwise `theseusd check`, its output masked;
  6. **the service**: with `THESEUS_CONFIG` exported, nothing to do when the unit matches its plan, is active and
     answers; `user-service.sh restart` when only the binaries changed; else `user-service.sh install --yes`. Then the
     unit is checked: no `ExecStopPost=` or `Delegate=`, and `KillMode=process`;
  7. **health**, waited for up to 40 s while the secrets resolve, its lines masked.

  Exit 0 done or nothing to do, 1 a step failed, 2 a usage error, 3 stopped for the operator. A dry run runs only the
  preflight's reads, never runs theseusd and never connects to a socket.
- **`--unit NAME`** for `theseusd install --user` and `scripts/user-service.sh`: a second daemon's own unit. Without
  it, `user-service.sh install --yes` for a scratch daemon would have taken the operator's running daemon for one
  started by hand and shut it down; a second unit is now refused without its own `THESEUS_SOCKET` and
  `THESEUS_STATE_DIR`.
- **The docs**: the README says near its top that Theseus runs on Linux only, by design ("built straight on the
  kernel's own machinery (cgroups, namespaces, seccomp, pidfds, and inotify), with no layer in between to make it
  portable"), and "Quick start" became "Set it up": a dry run, then one command with the service account's token file.
  `docs/setup.md` is new (what you need, each step, the first run and the next, where the config lives, the options, a
  second daemon, and the steps by hand); `docs/user-service.md`, `docs/README.md` and the AGENTS.md files follow.

**How it is proven.**
- **Tests**: the lookup's five unit tests (a named config wins over both files; the operator's file before the
  machine's; the machine's when the operator has none; neither, saying where one goes; an unexaminable path counting as
  there) and the real binary's walk through each step (`config`'s source line, `check`'s, and `install --user`'s
  `ExecStart` on the machine's file); `tests/setup_script.rs` (usage errors; a dry run that changes no file; a
  `--no-build` first run installing the binaries and writing the config, 0750 and 0640, then stopping with 3; a second
  run leaving the tree identical, bytes and mtimes; after the operator's edit, the check passing and the differing
  keys listed with no value); the second unit beside the first, in `user_service_script.rs` and `install::tests`.
  Targeted, 85 of 85. **The planted revert**: the `/etc` branch switched off fails three tests (the named config's
  empty-name case, the machine's file, the binary's walk at its fourth step); restored byte for byte, 8 of 8. The
  workspace suite in the lane at a load of 33 to 38: 2,369 of 2,371, the two being timing assertions in other code
  (865 ms against a 500 ms builder cap; a job's 267 ms median against 200), each passing alone.
- **A dry run on the operator's machine**: exit 0, 121 lines, and a snapshot before and after (the units, the binaries'
  and the config's and unit's sha256, the daemon's pid and start time) identical. The sudo branch, by a dry run against
  a root-owned directory: it said why and printed the one `sudo install -d`, creating nothing.
- **A scratch run with its own unit** under `/tmp`, with a stand-in token file and key, three times: run 1 built
  (`npm ci`, then a cold release-thin build in 9 m 58 s), installed 5 binaries, wrote the config (60 lines, 8 secrets,
  each an `op://` placeholder) and stopped for the operator (exit 3); after an edit putting stand-in `file:` secrets and
  a dead loopback model address in place, run 2 found nothing to install, left the config alone (14 keys listed as
  differing, names only), passed the check (7 secrets resolved in 4 ms; the L1 self-test worked), installed and started
  the unit, and showed health with the config's file and the secrets ready, the cockpit served at `/` with its four
  assets and `/cockpit/` a 308 to `/`; run 3 found nothing to do, the same daemon still up after 479 s. Torn down
  through `user-service.sh --unit … uninstall --yes`. The operator's daemon, binaries, config and unit were unchanged in
  every snapshot.

**The join** (the lane, lock `lane-setup-join` from 15:23:11). Main had moved by the logo (Item 137), and
`merge-tree` was clean, so a signed `--no-ff` merge, d5ff8489, at 15:23:46. Warm, 5 m 41 s. **Gate 1** (15:33 to 15:42)
was red in lifecycle alone, under strict budgets (the settle found a quiet window): run 1 missed the start from the
config copy (p95 57.5 ms against 57) and the clean shutdown with executions waiting and a job running (p95 150.9
against 104, p50 84.7), and the rerun missed the clean shutdown again (p95 238.9, p50 73.0). Main was reset to
origin/main. The bench history read it as the machine: the clean shutdown's p50 matched the base's gate at 15:10 (73.8),
each miss was the slowest of ten runs, and the merge touches no stop path (the bench names `--config`, so the lookup
costs it nothing). Main went back to d5ff8489; the finishing script starved behind the reviewers' steps
(theseus-6ir8), so after a 46 s re-warm, **gate 2** (15:52:26 to 16:01:12, ok in 526 s): 2,371 of 2,371; lifecycle on
the busy allowance (IO pressure 17 %), the cold start's p95 59.5 ms against 57.1 passing on it, every other phase within
its strict budget (the copy start 35.6 and 38.5 ms, the clean shutdown 69.6 and 77.4, SIGKILL and restart 42.0 and 54.4,
the binary swap 88.7 and 100.6); L1 start p50 7.70 ms; turn frames 5 and 9, fdatasync 13.9 ms; 61.5 MB resident, 85.0
MB after 30 turns. Pushed 16:01:37; theseus-00me and theseus-5aqz closed. The DM thread's review (16:12) checked the
signature, the files, the operator's untouched daemon and the scrub's counts, unchanged by the join.

**The install** (19:59:53 to 20:00:01, at 3085f71a, install #2). The install script did it, with Eddie's 18:02
profiles added to his config first; `scripts/setup.sh --dry-run` also ran on the machine: exit 0, nothing changed. His
config was already at `/etc/theseus/theseus.toml` (since 13:07), which the lookup now finds by itself. Health: store
format 16; route.v1, rerank.v1 and security.v3 live; startup serving 29.1 ms.

**Divergences.** None named by the lane or its review. setup.sh does not back up the store, which the install script does first: an
install over a store a newer build wrote should back up before it, as the lane's notes for install #2 said.

**Known gaps.** theseus-sgsf (P3: `theseusd check` passes a config with Discord on, the default, and no bot token; the
setup message and `docs/setup.md` tell the operator to turn a part off when its secret goes). The sudo branch is proven
by its dry run alone; a fresh machine's first run is its live proof. The template still names its author's own paths
(`projects_dir`, and the commented `roots` and `cwd`), which setup.sh rewrites in what it writes (noted on theseus-e663).
theseus-6ir8 (P3: the finishing script's lock waiter starves while reviewers run).

