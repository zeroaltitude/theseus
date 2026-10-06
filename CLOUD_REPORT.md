# Cloud report: bench-recall-plan (theseus-5dey, theseus-dp3y)

Branch `cloud/20261005-bench-recall-plan`, from main at 4a449460 (store format 22, untouched). Python and Markdown
under `bench/recall/` only: no Rust, no new imports (standard library only), nothing in bench/async, bench/harbor,
bench/report or `bench/theseus-bench.toml`.

| Step | Commit |
|---|---|
| 1. The retraction governs the old value (5dey) | f277682 |
| 2. The planned overhead recorded and checked by the driver (dp3y, part 1) | 812aa08 |
| 3. The written bounds checked (dp3y, part 2) | 27c5417 |

## 1. The retraction governs the old value (theseus-5dey), f277682

**Found.** As the issue says: `retracts_only` checked only that each sentence naming the old value held a `RETRACT`
word. All four of the issue's replies scored right under `retracted` (shown again with the old rule beside the new:
`True False` for each).

**Changed** (`score.py`): `RETRACT` is replaced by `RETRACT_PREFIX`, `RETRACT_SUFFIX` and `WAS_BEFORE`, with `REACH`,
`NOT_REACH` and `CANCEL`; `governed()` decides one occurrence; `retracts_only(text, kind, old, new)` asks that every
occurrence of the old value (in any of `value_patterns`' forms) be governed and that the new value appear at least once
ungoverned. `score_probe` passes the new value. `strict` is unchanged. README's Stale bullet and the module docstring
say the rule.

**How far a phrase reaches.** One value: the nearest value of the asked kind on its side (any value of the kind,
found by `KIND_REGEX`, plus the old and new values' forms), at most **four words** off, inside one **clause**
(a sentence, cut again at a semicolon or a dash). Four words reach past an article, an adjective and a noun
("instead of the old port 27340"); farther, the phrase is usually about something else in the sentence. A clause
because a semicolon or a dash usually starts a new assertion ("38013 is no longer used; it's 27340"). "Nearest value
on its side" is what makes "moved from 27340 to 38013" govern only 27340, and "It's 27340, previously 38013" govern
only 38013. Three shapes:
- prefix: moved/migrated/changed/switched (away) from, ignore, disregard, forget, previously, formerly, used to be,
  instead of, rather than, replaced/replaces/supersedes/superseded (not followed by "by"), no longer, not any more,
  wrong about, the old, the former, and `not` (at most one word off: "not port X");
- suffix, the value its subject: X is/was/'s/has been/isn't … no longer, not current/used/valid/right, anymore,
  replaced, superseded, retired, deprecated, dropped, outdated, obsolete, stale, wrong, gone, (the) old; X used to be;
- around: "was/were X … before/originally/at first/until" (each side within the reach).
A prefix right after said/mentioned/noted/wrote/told you or don't/do not/never/didn't is cancelled ("as I said
previously, it's 38013", "don't forget 27340").

**Proved.** `python3 -m unittest discover -s bench/recall`: green at each commit (step 1: 62 tests). New tests in
`test_score.py`:
- `test_a_retraction_word_that_governs_the_new_value_retracts_nothing`: the issue's four replies plus "It's 27340,
  no longer 38013.", "It's not 38013 but 27340.", "Don't forget 27340; 38013 is no longer used." are wrong, stale and
  not old-named under both rules;
- `test_a_retracted_old_value_is_right_only_under_the_retracted_rule` (extended): p001's live reply right under
  `retracted`; "It moved from 27340 to 38013." right under `retracted` and stale under `strict`; nine more right
  shapes ("27340 was replaced by 38013.", "38013 replaced 27340.", "It's 38013, not 27340.", "As I said previously,
  it's 38013, not 27340.", …); the existing cases unchanged;
- `test_a_retracting_phrase_reaches_one_value_four_words_off`: the reach, the semicolon and dash bounds, versions in
  both forms, dates in two forms.

Planted revert: `retracts_only` given back its old body (a retraction word anywhere in the sentence). Three tests
fail: `test_a_retraction_word_that_governs_the_new_value_retracts_nothing` (`(True, False, True) != (False, True,
False)`: a reversed reply scored right and not stale), `test_a_retracted_old_value_is_right_only_under_the_retracted_rule`
and `test_a_retracting_phrase_reaches_one_value_four_words_off`. Restored, touched, `git status` clean but for the work.

**Ambiguous shapes, scored conservatively (not right under `retracted`, so stale):**
- "It's 38013 now; earlier it was 27340." (no "before", and "earlier" is not a phrase: "you said earlier it's
  38013" is a citation);
- "38013 (was 27340)", "27340 → 38013", "from 27340 to 38013" with no verb;
- "Ignore the port from my first answer, 27340" (six words off), "not the 27340 one" (`not` two words off);
- "Don't use 27340" (no phrase for it).
The owner may want some of these widened; each is a one-line addition to a pattern, with a test.

## 2. The planned overhead recorded and checked (theseus-dp3y, part 1), 812aa08

**Changed.**
- `progression.py`: `overhead_tokens: int | None`, beside `context_window`. `to_json` leaves the key out when it is
  None, so **an older file loads with None and keeps its digest** (its canonical bytes are what they were). A file
  without it is held to today's `OVERHEAD_TOKENS` (`generate.planned_overhead`), and run.json says
  `planned_recorded: false`.
- `generate.py`: `build(seed, size, overhead=OVERHEAD_TOKENS)`, threaded through `plan_bulks`, `fit_bound`,
  `cross_bound`, `_alone`, `bounds_of` (by default the progression's own), and `estimate`; `--overhead`; the summary
  names the planned overhead and the cushion. `OVERHEAD_CUSHION = 50` beside `OVERHEAD_TOKENS`: under the 71 tokens of
  growth that rang the smoke.
- `drive.py`: after the first turn, before any probe, `overhead_of` reads the earliest `context.compiled` row's
  `est_tokens` less that turn's user message at the model's rates (`ledger -k context.compiled`, as `compactions`
  reads its rows). run.json's `overhead` holds `planned`, `planned_recorded`, `measured`, `cushion`, `past_cushion`,
  `allowed`. Past the plan by more than the cushion, the driver stops the daemon cleanly and exits 3 with (the test's case) "the
  daemon's system prompt and tools are 13,599 tokens, past the 13,099 the progression was planned at by 500 (more
  than the cushion of 50): its marks may ring or fail. Generate it again with --overhead 13599, or run it as it is
  with --allow-overhead". `--allow-overhead` runs on. Only the Theseus arm measures (Claude Code's prompt is not ours).

**Digest.** The smoke's digest moves only by the key: with `overhead_tokens` removed, today's smoke hashes to the old
pin 25df5ff56f522723 (checked). New pin a6a203b842f48d21.

**Proved.** The scratch daemon of this build (debug, on standin.py) measures **13,599**, as the issue said.
- `test_a_progression_planned_under_the_daemons_overhead_is_refused`: planned at 13,099, exit 3 after one turn, the
  message names 13,599 and 13,099, run.json records both, nothing left running.
- `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing`: planned at the measured overhead, it runs:
  every turn exits 0, all facts delivered, the first compaction's outcome `compaction`, a perfect arm scores 100%.
- The timeout test runs on with `--allow-overhead` from a plan of 1,000 (`past_cushion`, `allowed` true).
- `Overhead` (no daemon): the earliest row less its user message; the refusal names both numbers; an older file.
- `test_progression`: the key round-trips, an older file keeps its digest; `test_generate`: `--overhead` is recorded.

Planted revert: the refusal skipped (`if False and rec["past_cushion"] …`). `test_a_progression_planned_under_the_daemons_overhead_is_refused`
fails ("0 != 3: drive: theseus ran 30 turns; … compacted at turns [11]"): the under-planned progression ran whole.
(The class's measuring helper fails with it, so the end-to-end test fails too.) Restored and touched.

## 3. The written bounds checked (theseus-dp3y, part 2), 27c5417

**Choice: verify the written plan, not plan with slack.** `plan_bulks` now yields candidates (the smallest window
first, in it the fewest reads first); `build` writes each and keeps the first whose written bytes pass `plan_misses`:
`bounds_of`'s rule at the planned overhead **and at `OVERHEAD_CUSHION` more** (fit, the crossing at the mark or the
turn after, each read turn alone under `alone_limit`, no log past one result, a session without a mark fitting).
Why: the miss is rounding between tokens and bytes, which a slack only makes rarer, while a check of what was
written makes it impossible to ship; and a plan that already held is kept byte for byte (no digest churn for the
sound ones). Checking at the cushion too makes the driver's refusal threshold exactly what the generator guarantees.

**What moved.** At 13,700 the smoke of seed 7 is byte for byte step 2's (digest unchanged, a6a203b842f48d21). The
cushion check moves the smoke of seed 8 from 41000 to 42000 and seed 11 from 50000 to 51000 (their first plan held
at 13,700 but not at 13,750); seed 12 and every full plan (7: 94000, 8: 101000, 11: 97000, 12: 108000) are unchanged.
The issue's case (13,599, smoke, seed 7) now plans 45000 with two logs of 10,694 bytes: alone 25,381 and 25,385
(limit 25,558), crossing at turn 11 at 29,929 over 29,654, the turn before 20,946.

**Proved.**
- `test_a_thin_plan_is_planned_again_until_its_written_bounds_hold`: the issue's case holds; the first candidate
  is the 44000 that missed, and it was not kept.
- `test_every_overhead_from_13528_to_14000_holds`: the smoke at every overhead 13,528–14,000 and the full at every
  59th, seeds 7, 8, 11, 12 (about 35 s).
- Offline, the full sweep: smoke and full at **every** overhead 13,528–14,000, four seeds, 3,784 plans: 0 misses.
- `Window`'s checks are now a helper reading each plan at its own recorded overhead.
- Before the fix, at 13,528, 13,599, 13,700 and 14,000 the miss at the plan's own overhead was the issue's one; at
  +50, five more (smoke 7 at 13,528 and 13,599, 8 at 13,700, 11 at 13,700, 8 at 14,000).
- The stand-in smoke end to end is now generated at the measured overhead (`build(7, "smoke", 13599)`, exactly the
  issue's case) and passes: every turn exits 0, the first compaction is `compaction`.

Planted revert: `if True or not plan_misses(prog)`. `test_a_thin_plan_is_planned_again_until_its_written_bounds_hold`
fails ("overhead 13,599, mark 10: the reads cross …") and the sweep fails at 13,578 (a read turn alone past its
limit at the cushion). Restored and touched. Note: the shipped bad plan still ran end to end on the stand-in (its
cross missed only at the 15%-under estimate), so the drive test alone would not have caught it; the generator's
tests do.

## Suites

- `python3 -m unittest discover -s bench/recall` with this workspace's binaries (target/debug): 69 tests, OK (64 s).
- Under load (four busy loops at nice 0, the suite at nice 19): `test_drive` 11 tests OK in 403 s. The whole suite
  at nice 19 starved (the CPU-bound sweep got 1% of a core) and I stopped it after 15 minutes; the generator and
  scorer tests have no timing.
- No Harbor code touched, so no venv run.

## The live check (the maintainer's)

In a throwaway container, with `ANTHROPIC_API_KEY` and a release build of `theseus`, `theseusd`, `theseus-index`:

1. `python3 bench/recall/generate.py --seed 7 --size smoke --out /tmp/rp-smoke`
   Shows digest a6a203b842f48d21; "scratch context window 45000, a request budget of 29,654 …; the system prompt and
   tools estimated at 13,700 (the driver refuses a daemon's past it by more than 50)"; mark 10: logs/build-1.log
   10,518 bytes, 4,402 tokens and logs/build-1b.log 10,524 bytes, 4,404 tokens, alone 25,420 and 25,427 (at most
   25,558), crossing at turn 11 at 29,858 over 29,654, the turn before 21,047. `jq .overhead_tokens
   /tmp/rp-smoke/progression.json` gives 13700.
2. `python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir target/release --model
   anthropic/claude-sonnet-5-5 --progression /tmp/rp-smoke --out /tmp/rp-th`
   Then `jq .overhead /tmp/rp-th/run.json`: planned 13700, measured near 13,599 on today's main (a release build
   and the live date may differ by a few tokens), `past_cushion` false. If it exits 3 instead, the message names both
   numbers: regenerate with `--overhead <measured>` and drive again. `jq -r '.exit' /tmp/rp-th/turns.jsonl | sort |
   uniq -c` gives 30 zeros; `jq .compaction_rows /tmp/rp-th/run.json` shows the first row at turn 10 or 11 with
   outcome `compaction`.
3. `python3 bench/recall/score.py /tmp/rp-th --out /tmp/rp-report --stale retracted`
   The report's first lines say Stale is `retracted`. In the Supersession table, Old named (p001) is 1/1 only when the
   reply states 38013 as current and names 27340 only under a retracting phrase ("Ignore the 27340", "moved from 27340",
   "27340 is no longer used"); it must be 0/1 (and Stale 1/1) for a reply like "It's 27340, previously 38013." or
   "Port 27340 replaced 38013." Score it again without `--stale` to compare: `strict` counts any 27340 as stale.

## Left, uncertain, and for the owner

- The cushion is 50: a growth of 51+ tokens over a progression's plan stops a live run until it is regenerated with
  `--overhead`. The planned 13,700 is 101 over today's 13,599, so today's main runs; a daemon far *under* its plan
  (say 300 tokens) is not refused. It makes the crossing thinner (the reads may not cross at 15% under the estimate);
  only passing the plan is checked, as the task asked. A lower bound is a one-line addition if wanted.
- Generating at the measured overhead is now the honest default for a published run (`--overhead 13599`), rather than
  13,700. I left the constant at 13,700, as main has it.
- The retraction patterns are English and hand-written (AGENTS.md: detection written by hand can't be finished). The
  report says which rule scored; `strict` stays the default.
- Docs for the maintainer: docs/benchmarks.md (if it describes `--stale retracted` or the plan's overhead) should say
  that a retraction governs the nearest value within four words in its clause, that a progression records its planned
  overhead, and that the driver refuses a daemon more than 50 tokens over it (`--allow-overhead`).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before the first commit and after the last code commit:
fmt, shape, features, clippy, cockpit and the test build pass; the suite runs 2,830 tests, 2,797 pass, **33 fail**,
exactly the known L1 set both times (theseus-pv6i: the VM runs as root with no job cgroup): theseus-sandbox's 19
contract tests and its bench's `spawn_100`, and 13 of theseusd's `sandbox` tests. No timing test failed, and no
flaky retry was needed. The phases after the suite, run by hand: protocol types unchanged; the turn bench 5 and 9
frames (budgets 5 and 9); `cargo deny --offline check` advisories, bans, licenses and sources ok (the fetch at setup
worked). The lifecycle and jobs benches are skipped by `THESEUS_GATE_NO_BENCH`, as a lane's gate does.
