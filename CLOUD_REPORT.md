# Cloud report: Jev's rerank of recall made live, step 32d (theseus-6fn.7)

Branch `cloud/20261004-rerank-live`, cut from `main` at 802f913 (with the task commit 9bb1c78). Started 18:14 UTC,
2026-10-04; report written about 20:50 UTC. Four commits of work, then this report:

| commit | step |
|---|---|
| 61ded07 | 1: the labels test, shadow path (theseus-mm4a) |
| e59a46a | 2 and 3: rerank's own breaker; rerank live and bounded (with WIRED's line, see step 5) |
| 604188f | 4: per-item labels, the report's per-item grading, the memory label's system label, the cockpit's buttons |
| c480647 | 5: the template's words and the two AGENTS.md files |

## Step 1: the labels test first (61ded07)

**Found.** As the brief says: `recall_end` hands the operator's labels to the rerank (`Recalled.labeled`), and with
that field set to `Default::default()` no test failed. The filter itself (`theseus_memory::rerank::eligible` with
the asker's `labeled`) was right; nothing held the hand-over.

**Changed.** `tests_rerank_live.rs::a_labeled_note_never_reaches_a_shadow_rerank`: through a real turn against the
fake Jev, the heron's note is labeled `wrong` (then, in a second core, `stale`) with `memory.label`, Jev is scripted
to answer every `helps.N` 0.97, and the test checks that the node's id and its text appear nowhere in the body Jev
was sent (`FakeJev::seen()`), that `eligible` is 2, and that the key is in none of the row's `fused_admitted`,
`reranked_admitted`, `top`. After step 3, `a_labeled_note_never_reaches_a_live_rerank` runs the same with
`[memory] mode = "live"`. tests_rerank's rig and helpers became `pub(crate)` to share them.

**Proved.**
- Planted revert, shadow: `labeled: Default::default()` in `recall_end` → `a_labeled_note_never_reaches_a_shadow_rerank`
  FAILS ("wrong: the note's text reached Jev"); the live test passes. Restored, touched, `git status` clean of it.
- Planted revert, live: `labeled: Default::default()` in `turn/rerank_step.rs` → `a_labeled_note_never_reaches_a_live_rerank`
  FAILS (same message); the shadow test passes. Restored, touched.
- The step-1 commit was checked in a worktree of main plus the test only: `tests_rerank*` 8/8 passed, fmt and
  clippy clean. The full gate ran on the next commit's tree (it contains this one).

## Step 2: rerank's own breaker (e59a46a)

**Changed.** `theseus_judge::JevJudge::with_breaker(name, packs, cfg)`: breakers of their own by name. A batch's
breaker is its first part's pack's (a batch's packs share a state, and rerank.v1's state is its own), so rerank's
failures and timeouts admit and record only on `rerank`; every other pack keeps the shared one. One client: the
in-flight permits and the shed count stay shared. `breaker_status()` is unchanged (the shared one's; route.v1 reads
it); `breaker_of(pack)` and `own_breakers()` are new. The core builds the judge `.with_breaker("rerank",
["rerank.v1"])`. `judge.circuit` rows carry `breaker: "rerank"` (no field for the shared one, whose rows and
sentences are byte-identical to before), health's `JudgeHealth` gains `breakers: ["rerank: closed"]` (serde
default), the CLI's judge line adds ` · rerank breaker closed`, the cockpit's Judgment pill shows it, and the time
machine leaves a named breaker's `judge.circuit` rows out of the shared breaker's state.

**Proved.** `five_rerank_timeouts_open_reranks_breaker_alone`: Jev slow at 1.5 s (past rerank's 600 ms, inside
loop.v1's 5 s), loop.v1 on; five shadow reranks time out, one `judge.circuit` row (`breaker: "rerank"`,
`opened`), health shows the shared breaker `closed` and `rerank: open (…)`; the sixth rerank is skipped
`circuit_open` while a loop.v1 judgment goes out and is answered.
Planted revert: the core without `.with_breaker(...)` (rerank on the shared breaker) → the test FAILS (no
`judge.circuit` row at all: loop.v1's answers, interleaved on the shared breaker, keep resetting the streak, which
is the very coupling 32d removes). Restored, touched.

## Step 3: live, bounded (e59a46a)

**Changed.**
- `[memory] rerank_wait_ms`, default 200, 1 to 600 (`MAX_RERANK_WAIT_MS`, the call's own deadline), in the template
  with its range; the config test checks 0 and 601 refused and the default 200.
- The recall step's one call: in `recall_live`, the `scene` + `manifest` pair became
  `self.recall_reranked(t, mode, &begun, answer).await` (`turn/rerank_step.rs`). It reads `rerank_mode()` first
  (before cloning any candidate): `off` → today's `manifest`; `shadow` → `at_recall` off the path (the shadow
  32c rerank, marked `shadow`); `live` → `Memory::manifest_ranked` (the manifest, candidates, and each candidate's
  source ranks), then `JudgeService::at_recall_live`.
- `at_recall_live` starts its clock (tokio's `Instant`, so the paused clock tests it) before anything else, picks
  the eligible notes, mints and marks (`mode: live`), and spawns the same `judge_rerank` task with a oneshot sender.
  The turn waits `timeout_at(start + wait, rx)`; when the wait ends it `close()`s the receiver and `try_recv()`s,
  so an answer sent before the end is taken, and one after it finds the turn gone: the task's `send` fails and
  its row says `late: true`. `applied` and `late` cannot both hold. The task hands over Jev's order (the eligible
  notes re-sorted, the rest after, as `reorder` gives) or the fallback's reason; a prepare that sends nothing hands
  `budget`, `state` or `client`.
- No wait at all, read before the dispatch: rerank's breaker `Open` (`breaker_open`), or the day's budget already
  paused (`budget`, `ShadowBudget::paused`, a read of what the process knows). Those reranks still go out
  (counted, and a `circuit_open` row for the breaker) but nobody waits.
- In time and answered: `Memory::refill(scene, m, candidates, ranks, order)` packs again with
  `theseus_memory::rerank::repack` (same filters, place rule and labels first, same budget as the manifest's,
  sources' ranks kept), so the node holds Jev's order and the row's `reranked_admitted` equals the request's.
- Recorded: a `judge` span of kind `wait` (pack, judgment, `applied`, `why`, `wait_ms`); the `recall.ran` manifest's
  new `rerank` (`RecallRerank`: judgment, applied, why, waited_ms, wait_ms; a new JSON field, no format bump);
  the judgment row's `rerank.live/applied/late` and `context.live`; `theseus memory recalled` prints
  `in Jev's order (jdg_…): rerank.v1 answered after 115.0 ms of the 200 ms wait` or
  `in recall's own order (jdg_…): <why>`.
- The call keeps its 600 ms deadline, the reservation stays on the judge's day budget (§2.7's session pay waits
  for 26b), and the judgment's mode is `live`.

**Proved** (`tests_rerank_live.rs`):
- `a_live_recall_carries_jevs_order_into_the_request`: the heron's note, third in the fused order, is the one the
  request renders (and the kettle's is absent); manifest `applied`, span `applied`, row `live/applied/!late`,
  row's repack = the turn's, health `rerank.v1: live`.
- `a_slow_jev_leaves_the_request_as_judge_offs_and_its_late_row_lands`: Jev at 400 ms, wait 200: the provider
  request equals a judge-off core's (session ids and the testimony's minute-dates normalized; the two rigs'
  differ), why `timeout`, waited ≥ 200, and the row lands `applied: false, late: true`.
- `no_wait_when_the_breaker_is_open_the_budget_spent_the_judge_off_or_rerank_in_shadow`: breaker (five 3 s
  timeouts, then a recall: why `breaker_open`, waited < 300 ms against Jev's 3 s, its row skipped `circuit_open`
  and not `late`); budget (limit 0: the first recall's why is `budget` from its own reservation, the second's
  from the pre-check, waited < 300 ms; Jev never connected); judge off and rerank in shadow (no `rerank` in the
  manifest, no wait span; the shadow one's row lands with mode `shadow`).
- `the_turn_goes_on_at_the_wait_exactly` (`start_paused`, Jev a channel through `JudgeService::rerank_with`, a
  test-only seam in the rerank task): an answer 199 ms after the call reached Jev is applied (waited 199.0 ms,
  the heron admitted); one at 250 ms is not: waited exactly 200.0 ms, why `timeout`, the fused order's note
  admitted, and the row `late`.
- Planted revert, the wait unbounded (`(&mut rx).await` for the `timeout_at`): `the_turn_goes_on_at_the_wait_exactly`
  FAILS (250 ms: applied true, waited 250.0) and the slow-Jev test FAILS ((true, None) for (false, "timeout")).
  Restored, touched.
- The labels test on the live path (above).
- The turn bench, judge off: `target/debug/theseus-sim bench turn --check --runs 5 --burst 0`: frames_plain 5
  (budget 5), frames_tool 9 (budget 9), unchanged; `a_reranked_turn_keeps_its_frame_budget` still passes.

## Step 4: per-item answers graded (604188f)

**Changed.**
- `judge.label` with a per-item question: `rpc/learning.rs` looks the question up among the judgment's own answers
  (`learning::items::asked_about`), checks the label as a Noul's (`labels::check_item`: true, false, right,
  wrong), and writes `about` (the item's key) on the row (`fact::judge::JudgeLabel.about`, written only when set,
  so every other label row is byte-identical) and on `JudgeLabelResult.about` (protocol; TS regenerated). A
  per-item question the judgment did not answer (`helps.7`, `helps_more.1` when it asked three) or the
  definition itself (`helps`) is refused, naming the ones it did answer (`items::refuse_unasked`); nothing is
  written. `labels::check` is unchanged. `LabelRow.about` reads it back. The CLI's label line says
  `helps.3 (about <node>#0)`.
- The report: `learning/items.rs`, called from `report::pack_report` in three one-line places: per-item
  definitions (`helps`, `helps_more`) become questions of kind `noul`, each answer graded by the label that
  `resolve` picks for its own question name (`helps.3`) with report.rs's `graded` (made `pub(super)`, with
  `band_name` and `calibration`): answered, labeled, bands, calibration. A judgment with a graded item counts in
  the pack's `labeled`; the holdout gets the item questions, their `labeled_per_question`, and their label ids.
  report.rs's `question`, `answer`, `questions_of` and `ACTING` are untouched (route.v1).
- The system label: `learning/rerank.rs`, rule `memory_label`, `SYSTEM_WEIGHT`. Each `memory.label` row (scope
  `memory`) of `useful`/`should_have` (true) or `wrong`/`stale` (false) grades the answer about its node: the
  rerank whose `context.recall` is the label's `recall_id`, else the newest answered rerank whose row precedes the
  label (by WAL position) and asked about the node. Keyed `system_key(judgment, "helps.N", "memory_label@<label's
  position>")`, so each memory label writes one, once; the row has `question`, `about`, `rule`.
  `SystemLabel.question` became a `String` (with `about`) for it. `system_labels` returns these for pack `rerank`.
- The cockpit: `JudgmentLabels.tsx` gives each per-item answer a row (`helps.3 · <key> · 97%`) with true and false
  buttons; the done line names the item.

**Proved** (`tests_rerank_labels.rs`, through a real shadow rerank against the fake Jev):
- `a_per_item_answer_takes_its_own_label_and_the_report_grades_it`: `helps.3 true` written with the heron's key;
  `helps.7`, `helps_more.1`, `helps` and `helps.2 maybe` refused with their reasons, nothing written; the report's
  `helps`: kind noul, answered 3, labeled 1, calibration n 1, Brier 0.03² (p 0.97, true); pack labeled 1; a second
  label → labeled 2.
- `a_memory_label_writes_a_system_label_once_and_the_owners_beats_it`: `useful` on the heron naming its recall →
  `helps.3` true at 0.5; `wrong` on the kettle naming none → the newest rerank's `helps.1` false at 0.5; both keyed
  to the one judgment; `system_written` 2, then 0 on a second run; the owner's `helps.3 false` (1.0) then wins:
  Brier = (0.10² + 0.97²)/2.
- Planted revert: the `questions.extend(items::questions(...))` line removed → both tests FAIL ("the per-item
  definition's report"). Restored, touched.

## Step 5: live by default (e59a46a's WIRED line; c480647's words)

`WIRED` gives rerank.v1 `PackMode::Live`. It rode in e59a46a, not c480647: the config can only lower a mode
(`mode_of` = min of WIRED's, `max_mode` and the pack's line), so the live path could not be reached or tested
without it. That commit also moved the four tests that list health's packs (`tests_judge` ×2, `tests_continue`,
theseusd's `tests/judge.rs`) and tests_rerank's to `rerank.v1: live`. c480647: the template's `[judge]` words
(one pack live, the arms rule, the wait, the pack line's modes; the template test un-comments prose, so no line
there may begin with a `[`), theseus-core's and theseus-memory's AGENTS.md. Health's pack list follows WIRED by
itself.

## The live check (the maintainer's)

A scratch daemon on a fresh state dir, `theseus-index` beside `theseusd` (the build's own, both from
`target/release-thin` after `scripts/build.sh --profile release-thin`). Ports and paths are examples.

```sh
S=/tmp/rr32d; mkdir -p $S/bin $S/state
cp target/release-thin/{theseusd,theseus,theseus-index} $S/bin/
cat > $S/theseus.toml <<'TOML'
[server]
state_dir = "/tmp/rr32d/state"
[web]
enabled = false
[discord]
enabled = false
[index]
enabled = true
[memory]
mode = "live"
[judge]
enabled = true
[secrets]
jev_api_key = "op://…"            # or env:/file: as the machine has it
anthropic_api_key = "op://…"      # the model key the default profile names
TOML
$S/bin/theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &
T="$S/bin/theseus --socket $S/sock"
$T health | grep -E '^judge|^memory|^index'
```
The `[secrets]` names are the template's. Health's judge line should read `… rerank.v1: live · … breaker
closed · rerank breaker closed …` (or `idle` before the first judgment).

1. ```sh
   $T ask "Remember: the grey heron nests by the old weir at Millbrook."            # session A
   $T index status                                # wait until the index has read through A's turn
   B=$($T --json ask "Where does the grey heron nest?" | jq -r .session_id)        # session B
   $T --json ledger --kind turn.ended -n 1        # B's trace: a judge span, kind wait, pack rerank.v1, mode live, applied true
   $T memory recalled $B                          # "in Jev's order (jdg_…): rerank.v1 answered after N ms of the 200 ms wait"
   $T judge log --pack rerank.v1 -n 1             # mode live; rerank.live true, applied true, late false
   ```
   With one note in the index, Jev's order and recall's admit the same note: `applied` still says the turn used
   Jev's order. A few more notes make `order_changed` visible.
2. Add `rerank_wait_ms = 1` under `[memory]`, restart (`$T shutdown`, start again), and `$T ask` B's question
   again (a new session): `memory recalled` says `in recall's own order (jdg_…): Jev had not answered within the 1 ms wait`,
   and `judge log` shows that judgment answered later with `late: true, applied: false`.
3. ```sh
   $T judge label <jdg from 1> true --question helps.1   # "… helps.1 (about <node>#0): true (operator, weight 1)"
   $T judge label <jdg from 1> true --question helps.9   # refused: "this judgment did not ask helps.9: …"
   $T judge report --pack rerank                         # rerank.v1: a `helps` row, kind noul, labeled 1
   $T memory label <A's node> useful --recall <rcl from memory recalled B>
   $T judge report --pack rerank                         # the run's system labels: 1 written (rule memory_label, weight 0.5)
   ```
   The owner's label on `helps.1` outweighs the system label on the same question; a second report writes none.
4. `$T shutdown`.

## What is left, uncertain, or for the owner

- **A whole-judgment `right`/`wrong` stands for every per-item answer** (`labels::resolve` takes a label with no
  question for any question, and I left that). For rerank I think it should not: "the rerank was wrong" says the
  order was wrong, not that each of twenty Nouls flipped its lean; read so, one press grades twenty answers and
  swamps the item labels' calibration. My suggestion: `resolve` skips whole-judgment `right`/`wrong` for an answer
  with `about`. Not done: it changes the meaning of existing labels.
- **The system label's "newest rerank before it"** is by WAL position (the judgment row's, written by the sink up
  to 2 s after the call), among answered reranks that asked about the node. A label with a `recall_id` whose
  rerank did not ask about the node writes nothing; it does not fall back to an older rerank.
- **`late`** means the turn had moved on when the outcome arrived, whether Jev answered or the call itself failed
  (a 600 ms timeout after a 200 ms wait is `late: true` with `fallback: "timeout"`).
- **A rerank skipped at the open breaker or the paused budget** is still dispatched (so it is counted, and the
  breaker's half-open probe can go), but not waited on; its row (the breaker's) has `late: false`.
- **The batch's breaker is its first part's pack's.** Fine for rerank (its state is its own); a future pack with a
  breaker of its own that shares a state with another would split oddly.
- **The exam's `+rerank`.** I left the exam's arms. To wire it: the exam's daemons run the judge off and
  refuse `arm = "+rerank"`; the arm is now "`[memory] mode = live`, arm baseline, `[judge] enabled`, rerank.v1
  live", so the exam would run a `+rerank` daemon with the judge on (a key, or the fake Jev) and every other pack
  `off`, and score its recalls' admitted sets (from `recall.ran`, which now says whether Jev's order was used, so a
  timed-out rerank is scored as baseline and counted). `rerank_wait_ms` should be the exam's to set, and the
  report should give the applied share.
- **§2.7's session pay** waits for 26b, as the brief says; a live rerank is still paid by the judge's day budget
  (its `budget: "shadow"` column, which now covers a live pack too: worth renaming at 26b).
- Docs for the maintainer: the spec's Part III item for 32d; `docs/design/m6-memory.md` §2.7 (the wait is 200 ms by
  `rerank_wait_ms`, the arms rule, `late`), §2.9's table (rerank's system label from memory labels), §2.14 (the
  manifest's `rerank`, the `wait` span); `m5-judgment.md` §2.2 (breakers of their own) and §2.9 (per-item labels and
  the report's per-definition rows); `docs/status.md`.

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on each of e59a46a, 604188f, c480647: fmt, shape,
features, clippy, cockpit (lint, test, build), test build, and the reader rule pass; the suite fails only the 33
sandbox tests that need a non-root user (theseus-pv6i: 19 `theseus-sandbox::contract`, 1 `theseus-sandbox::bench
spawn_100`, 13 `theseusd::sandbox`), 2284 passed. The phases after it, run by hand: protocol types clean (the
regenerated TS is committed), the turn bench (5 and 9 frames, at budget), `cargo deny --offline check` ok
(advisories, bans, licences, sources).

- `TZ`: under the VM's UTC, `tests_output::the_cores_output_matches_its_golden` fails at a wake line's offset
  (`+00:00` where the golden has a negative one): the golden was written in a western zone. Under
  `TZ=America/Los_Angeles` it passes. Not this change's; worth making the test zone-independent.
- A stale-build trap I hit: verifying step 1 in a second worktree with the same `CARGO_TARGET_DIR` left the main
  tree's theseus-protocol fingerprinted to the worktree's sources, and the next gate's test build failed with
  "no field rerank"; touching the sources rebuilt it. Worth a line in scripts/AGENTS.md: a worktree never shares
  the main tree's target dir (AGENTS.md says a lane's must not; this is the same trap).

## Runs under load

AGENTS.md's recipe: four `while :; do :; done` loops at nice 0 (from a script file; killed by their pids), the tests
at `nice -n 19`, binaries built first. On the final tree (c480647), `tests_rerank_live` and `tests_rerank_labels`
(9 tests): five runs, 9/9 passed each (about 31 s a run).

Earlier rounds found two bounds of mine too tight under load, both in
`no_wait_when_the_breaker_is_open_the_budget_spent_the_judge_off_or_rerank_in_shadow`, and I fixed the test, not
the code: the whole turn's wall time under 600 ms (a starved turn takes longer whatever the rerank does; the wait
itself is what is bounded, and asserted), and `waited_ms < 50` for the pre-checked cases (under load the
eligibility pass and the dispatch take ~51 ms; Jev, at 3 s there, is never waited on, and the breaker case now
also checks its row is not `late`). Before those fixes it failed 5 of 5, then 1 of 5, loaded; after them, 0 of 5.
The paused-clock test is not a timing test and passed every run.
