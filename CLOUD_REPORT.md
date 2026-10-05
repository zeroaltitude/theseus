# Cloud report: the recall bench's bulk sizing, admission, stale rule and compaction outcomes (theseus-523y)

Branch `cloud/20261005-bench-recall-fixes`, from main at 60b43fb6 (store format 20; no store, protocol or Rust
change here). Started 20:23 UTC, done at 22:35 UTC. Everything is under `bench/recall/`: Python on the standard
library alone, no new import, no Rust touched.

| Step | Commit |
|---|---|
| 2. `ADMIT` widened, abstention checks re-derived | b9e4dd6b |
| 3. `--stale retracted` | d4a12635 |
| 4. compactions kept and counted by outcome | 5012648a |
| 1. the bulk reads sized by the compiler's rule | b0d50c47 |
| a pre-existing negative assertion in `LeftRunning`, fixed | 8e94653f |
| `SUMMARY_MAX_TOKENS` read from its Rust source too | b34cca61 |

I committed in the order 2, 3, 4, 1: 2 to 4 are small scorer changes, and 1 re-pins the smoke's digest last.

## 1. The bulk sized by the compiler's rule (b0d50c47)

**What I found.**

- **The Rust rule matches the brief, with three things it left out.** A tool result is JSON at 2.4 bytes a token
  (`TokenRates::CLAUDE`) and text 3.3; 3 tokens a message, 1 a block, 15 a tool id; the upper bound is counted +
  estimate × 1.4; the ring runs past `request_budget` (window − max_tokens − 4,096) and keeps turns under 60% of
  it; the last candidate past the budget is an overage. The three:
  - **A tool result shows at most 30,000 characters** (`default_result_max_chars`, toolrun.rs `cap`), and `fs_read`
    numbers each line (`{:>6}\t`, 7 bytes). Main's smoke log of 27,776 bytes reads at about 32.7 KB, so the model
    saw 30,000 characters, about 12,500 tokens: the live run's "about 12,300" for the turn. One read can add at most
    about 12,500 tokens. Main's full log (about 110 KB) was cut the same way, so main's full mark could not cross
    by the plan either.
  - **A summary is written only when it fits** (turn/compaction.rs `plan_summary`): the ring's kept turns at their
    upper bound plus `SUMMARY_MAX_TOKENS` (4,096) must stay under the budget, or the outcome is `ring`, with no
    summary. With a 13.5k overhead at × 1.4 that needs about 23k of budget before the read. My first sizing met the
    brief's bounds and still produced only `ring` rows on the stand-in ("a summary of up to 4,096 tokens would not
    fit beside the 23,247 the kept turns take, within the 25,904 the window leaves"). The alone bound now keeps that
    room as well as its 10% margin.
  - **The crossing is arithmetic, not a guess.** At the mark's answer the turns before it are counted at × 1 and
    only the read at × 1.4, while the ring's last candidate (the overhead and the read turn, estimated whole) is
    entirely at × 1.4. Crossing while staying alone under the budget needs `before > 0.4 × overhead` plus the
    margins. The smoke's ten turns before its mark come to 4–9k tokens (seed 7: 6,295) against 0.4 × 13,528 = 5,411.
    **For the smoke, no window lets one read do both**, whatever its size, and growing the window doesn't help:
    both bounds scale with it. In the full, one window serves three sessions whose turns before the mark differ
    (seed 7: 51.6k, 46.7k, 44.7k), and a single read capped at about 12.5k can't bridge them either.
- **Why the stand-in smoke passed on main:** theseus-sim's fake model reports `input_tokens: 40` on every call
  (fake_model.rs `start(40)`). Theseus trusts the provider's count from a compilation's second call on, so a turn's
  whole history cost about 50 tokens, and only the newest read was estimated: 52 + 1.4 × 12.5k is under 22,154.
  The stand-in never rang, so it never compacted (the README said so) and never failed. Live, the count was real,
  so the mark rang, and its last candidate, about 1.4 × (12,042 + 12,300), was an overage.
- **The overhead:** a scratch daemon of this tree on the bench profile (27 tools) puts a new session's first call
  at 13,533 tokens, 13,528 less the user's "hi" (`context.compiled`'s `estimate`, method `bytes`). The first live
  smoke said 12,042: likely an older build with fewer tools. **I set `OVERHEAD_TOKENS = 13528`.** The live check
  below shows the release build's own figure; if it differs much, set it and regenerate.
- **The second compaction at turn 25** in the live smoke is most likely the second session filling: at the old
  window its turns came to more than the budget. The plan now makes a session with no mark fit whole.

**What I changed.**

- `tokens.py` (new): the census of a request (`ProviderRequest::census`, `Census::of_messages`, `Census::tokens`),
  the three rate families and `TokenRates::of`, the framing, `MARGIN_PERCENT`, the headroom, the ring's 6/10,
  `SUMMARY_MAX_TOKENS`, `RESULT_MAX_CHARS`, and fs_read's line prefix. Each constant names its source, and
  `test_generate.TheRustRule` reads each one from the Rust file (catalog.rs, provider.rs, compiler.rs, config.rs,
  fs.rs, turn/compaction.rs; the last added at b34cca61). Checked against the compiler: the stand-in's count of a real
  request equals the daemon's `context.compiled` estimate exactly (13,533 = 13,533).
- `generate.py`:
  - every turn's `est_tokens` is its messages by that rule: its text, its work's call and result (`fs_read` with
    numbered lines, `proc_run` of a script, `fs_list`, `fs_write`), and a reply of `REPLY_BYTES` (400);
  - `plan_bulks` picks the smallest window (in thousands) where four things hold. The turns before each mark fit
    at `MARGIN` (15%) over their estimate. A session with no mark fits whole. Each read turn alone, estimated
    whole beside the overhead, stays at its upper bound under `alone_limit` = min(budget / 1.1, budget − 4,096).
    At 15% under the estimate, the reads cross the budget. Where the mark's one read can't cross, the turn after
    the mark, which holds no fact or probe, reads one to three more logs (`MORE_READS`), and the crossing is
    there. Each log sits in the middle of what its bounds allow, under the 30,000-character cap;
  - `bounds_of(prog)` re-derives every bound from the progression's own bytes, and the generator prints it: each
    log's bytes and tokens, each read turn's bound and its limit, the crossing and the turn it falls in, and the
    window's budget;
  - `estimate()` (the dollars) uses the same estimates and compacts where the plan's ring would;
  - the bulk logs draw from a stream of their own (`bulk_rng`), so resizing a bulk moves nothing else.
- `standin.py` (new): a Messages API stand-in in Python with theseus-sim's rule format. It reports each request's
  own estimate as `input_tokens`, as a provider's count would come, answers a call that carries no tools (the
  summary) with a summary, and matches rules against the person's words before the recall block (recall notes
  quote earlier turns: before that, a recalled "Which file under src/ is the largest?" made p006 take a later
  filler's rule).
- `test_drive.py`: the smoke runs on `standin.py`, with rules that do each turn's work as a model does
  (`work_for`: `fs_read` of the whole log, of a file asked about, `proc_run` of a script). The timeout test stays
  on theseus-sim. New test: main's sizing (35000 window, one 27,776-byte log) is an overage at the mark on the
  counting stand-in.
- The README: the window's rule, the new files, the sizes, the stand-in, and the Claude Code arm's compaction at
  the full's window (below).

**What the smoke at seed 7 now is:** window 44000 (budget 28,904); two logs of about 4,100 tokens (9.8 KB each),
`logs/build-1.log` at turn 10 and `logs/build-1b.log` at turn 11 ("Now read logs/build-1b.log too: which of its
steps took longest?"). Each read turn alone has a bound of 24,754 / 24,757 against a limit of 24,808. The
crossing falls at turn 11: 28,952 against 28,904 at 15% under the turns' estimate. The full at seed 7: window 94000
(budget 73,904); mark 120 crosses with one log, and marks 320 and 520 with a second at the turn after. Budget:
$0.53 an arm for the smoke and $11.73 for the full at Sonnet 5.5.

**The re-pin** (d30943acf7bfd1ac → 1dd614218e7d91c1 at step 2 → 07e95754f01394f0 here). Against main's smoke of
seed 7 with step 2's checks, compared field by field:

- unchanged: every fact's id, subject, value, turn and salience; every probe;
- moved: `context_window` (35000 → 44000), the logs, every `est_tokens`, and turn 11 (a filler → the second read);
- re-drawn once: the texts of turns 13–29, one fact's script name (f001's `verify-` → `audit-jacana-archiver.sh`),
  and the names list. Main's bulk drew its lines from the main stream, so any resize moved every later turn. The
  bulks now have their own stream, so this happens once.

**How I proved it.**

- `python3 -m unittest discover -s bench/recall`: 60 tests at the branch's head (59 at b0d50c47), OK, with this workspace's debug binaries (the Theseus
  driver tests run, not skipped).
- The stand-in smoke end to end, seed 7: every turn exits 0; compacted at turn 11, outcome `compaction` (a summary
  written: `first` 18, `last` 489, 38 messages); 9 of 9 facts delivered; every probe right; nothing left running.
  By hand, the same at seeds 8, 11 and 12 (compacted at 11, outcome `compaction`, every turn 0).
- Main's sizing on the counting stand-in (now a test): turn 10 exits 1, "its request is estimated at 26,145 tokens
  (36,603 at the estimate's upper bound) against the 22,154 the window leaves after the output cap, 14,449 over,
  with every earlier turn dropped. Nothing was sent. [class=context_overage]", then a compaction at 11, the live
  smoke's shape. The same progression on theseus-sim's stand-in: no compaction, every turn 0.
- Under load (four busy loops at nice 0, the tests at nice 19): `test_drive.TheseusDriver`, 5 runs: 5 of 5 OK (each about 575 s; nothing left running).
- Planted reverts:
  - **The bulk at four bytes a token** (`bulk_log(rng, int(r * 4))`): `Window.test_each_mark_crosses_the_budget_
    and_no_read_turn_alone_passes_it` fails (and the two digest pins). The stand-in smoke still passed at this
    window: the reverted logs stay just under the budget there. Main's whole sizing fails on the stand-in (above).
  - **The margin dropped from the alone bound** (`alone_limit` returns the budget): the Window test fails
    ("28494.4 not less than or equal to 25905"), and so does the stand-in smoke: the mark's compaction became a
    ring (`['ring'] != ['compaction']`), with no room for its summary.
  - Restored and touched; `cmp` against the saved file, and `git status` clean but for the step's own files.

**Uncertain, and for the owner.**

- **The second read is a design change** the brief didn't ask for: the turn after a mark used to be a filler that
  held nothing. It still holds no fact or probe, so a compaction at the mark or at the turn after puts every probe
  in the same bucket. The alternative was to accept a mark that doesn't cross in the smoke, which then never
  compacts in its short session.
- **The smoke's margins are at their constants and no more.** It is the smallest window, so the crossing at 15%
  under the estimate clears the budget by 48 tokens, and the alone bound sits 51 under its limit. At the turns'
  own estimate the crossing has about 1,000 tokens to spare. A live arm whose turns run more than 15% under the plan
  (replies much shorter than 400 bytes, no recall notes) can miss the crossing and compact a turn or two later, or
  not at all in session 1. A miss on the alone side is now impossible but for the overhead: a build whose system
  prompt and tools are much larger than 13,528 shrinks the room. `MARGIN` and `ALONE_MARGIN` are the knobs.
- **The full's window at seed 7 is now 94k, under the 100k that Claude Code's `--autocompact` takes.** So the
  Claude Code arm of the full now gets `/compact` after each mark (`--cc-compact auto` → `marks`), as the smoke
  does, where before it compacted on its own at 124k. Both arms then compact at the marks. If the owner wants
  Claude Code's own autocompaction in the full, the plan needs a floor of 100k (more reads per mark) or
  `--cc-compact window` at 100k.
- `REPLY_BYTES` (400) and `MARGIN` are still guesses at the live arm's replies. The first full run's ledger
  (`context.compiled` per turn) would calibrate them.

## 2. `ADMIT` widened (b9e4dd6b)

**What I found.** p003's sentence fails `ADMIT` on both counts the brief says: no `tell`, and "found no matches" /
"nothing there pins" put the negator after the verb or used verbs not listed.

**What I changed.** `ADMIT` also takes `tell`, `pins`, `pinned` and `states` as verbs after a negator. It gains
`no match(es)` / `matching` / `hits` / `results`, `found no`, and `(unable|not able) to (find|tell|say|locate|
determine|confirm|see)`. `HEDGE` gains `can't tell`, `cannot tell`, `no match(es)`, `found no`, `unable to` and
`not able to`.

**An old run meets the new rule:** the scorer derives an abstention's check from its kind each time it scores
(`score.check_of`), so a run kept from before is scored by today's rule. A direct or indirect probe keeps its
stored check. Two side effects: a newly generated progression's digest differs from an old run's, so `score.py`
refuses to score them together (as it would any two progressions); and an old report rescored can move.

**How I proved it.** Tests: the live sentence and seven short forms admit; six replies don't (a value given, "I can
tell you", "matches", "I found it pinned", "as the lockfile shows"); each new phrase hedges; an old run's p003,
whose stored check is the old rule, scores right by today's. Planted reverts: `ADMIT` without `tell` fails
`test_a_plain_abstention_is_an_admission` ("I can't tell."); `check_of` returning the stored check fails
`test_an_old_runs_abstention_is_scored_by_todays_admission`. Suite: 49 tests, OK.

## 3. `--stale retracted` (d4a12635)

**What I changed.** `score.py --stale strict|retracted`, `strict` the default. Under `retracted`, a reply that fails
its check is right when the check's first line (the new value) holds and every sentence naming the old value
carries a retraction (`RETRACT`: ignore, disregard, no longer, anymore, moved from, replaced, superseded, instead
of, used to be, previously, formerly, "was … before", earlier answer, correction, …). It is then `old_named`, never
stale or confident-wrong. Sentences split at a stop before a space and at line breaks, so a version's dots don't
split. The report's Supersession table gains *Old named*, its header says which rule scored, and scores.json
carries `stale_rule` and `old_named`.

**How I proved it.** Tests: p001's live reply is wrong by default and right under the option (old named, not stale,
not confident-wrong). "It moved from 27340 to 38013.", "38013 now; 27340 is no longer used." and "It's 38013. It
was 27340 before the move." are right under it. "The archiver is on port 27340." is stale under both, "It's 27340,
or maybe 38013." wrong under both, and "It's 38013 or 27340. Ignore my first answer." wrong under both (the
retraction is in another sentence). An unknown rule raises. Planted revert: `retracts_only` accepting any naming
fails `test_a_retracted_old_value_is_right_only_under_the_retracted_rule`. Suite: 50 tests, OK.

**For the owner:** the README's Stale bullet now names the option, keeps strict as the default, and keeps the
theseus-exam reason. "Moved from" counts as a retraction under the option, which is how a person reads it.

## 4. Compactions by outcome (5012648a)

**What I changed.** For each turn that compacted, `drive.compaction_row` keeps every `context.compacted` row's
`outcome`, `why`, `messages`, `first` and `last` (run.json `compaction_rows`, now `[]` from a Theseus run's
start). **The bucket rule:** both outcomes move a probe (`score.MOVING_OUTCOMES`), because a ring drops the same
leading turns, only with nothing in their place. A run with no rows (Claude Code's, or an older one) counts its
list as `compaction`. The report's *Compacted at* names each outcome ("12 (ring), 22 (compaction)"), a
*Compactions* table counts them apart, and scores.json carries `compactions_by_outcome`.

**How I proved it.** A fixture run with a `ring` row at 12 and a `compaction` at 22: p005 and p007 both land in
the compaction bucket, the counts are one of each, and the report and the scores say so. The stand-in smoke checks
the rows end to end. Planted revert: `MOVING_OUTCOMES = ("compaction",)` fails
`test_a_ring_moves_a_probe_as_a_summary_does_and_is_counted_apart` (p005 `topic_shift`). Suite: 51 tests, OK.

## A finding: `LeftRunning`'s negative assertion (8e94653f)

Under load, `test_drive.LeftRunning` failed 3 of 12 runs (logs kept). Two were its positive check
(`assertIn(named.pid, found)`): the scan ran between the fixture's fork and its exec, when its command line was
still python's. **One was the negative check, `processes_naming(d) == []`, which found `(24157, 'sh')`.** The
fixture `sh -c "sleep 30" <run>/daemon/sock` forks its `sleep`. `p.kill()` killed the shell only, and its child,
caught between fork and exec, still carried the shell's command line naming the run. So the test leaked a
`sleep 30` each run, and the "nothing left" scan could see it. That is the test's fixture, not the driver: the
driver's own runs kill a turn's whole process group (`run_group`) and stop the daemon. I fixed the fixture (8e94653f): each fixture runs in a session of its own and its whole group is killed, and the
scan waits until each fixture runs its program (`exec_done`). Under the same load: 12 of 12 pass. The evidence of
the failures: `AssertionError: 23659 not found in {23657}`, `AssertionError: 23872 not found in {23870}`, and
`AssertionError: Lists differ: [(24157, 'sh')] != []`.

## The live check (the maintainer's: a key, a release build of `theseus`, `theseusd`, `theseus-index`, a throwaway container)

```bash
scripts/build.sh --profile release-thin    # or cargo build --release -p theseus -p theseusd -p theseus-index
rm -rf /tmp/rf-smoke /tmp/rf-th /tmp/rf-report /tmp/rf-report-r
```

1. `python3 bench/recall/generate.py --seed 7 --size smoke --out /tmp/rf-smoke`. It should show digest
   `07e95754f01394f0`; "scratch context window 44000, a request budget of 28,904 (the window less its output cap
   11,000 and 4,096); the system prompt and tools estimated at 13,528"; and a mark-10 line: `logs/build-1.log`
   9,788 bytes and 4,098 tokens, `logs/build-1b.log` 9,786 and 4,097, alone bounds 24,754 and 24,757 (at most
   24,808), crossing at turn 11 at 28,952 (over 28,904), and the turn before it 20,875. Then "$0.53" an arm.
2. `ANTHROPIC_API_KEY=… python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir
   target/release-thin --model anthropic/claude-sonnet-5-5 --progression /tmp/rf-smoke --out /tmp/rf-th` (or
   `target/release`). It should print "ran 30 turns; 9 of 9 facts delivered; compacted at turns [10] or [11] …;
   left running: nothing", and exit 0. Then check:
   - `python3 -c 'import json; [print(json.loads(l)["index"], json.loads(l)["exit"]) for l in open("/tmp/rf-th/turns.jsonl")]' | awk '$2 != 0'`
     prints nothing: every turn exited 0;
   - `python3 -c 'import json; print(json.dumps(json.load(open("/tmp/rf-th/run.json"))["compaction_rows"], indent=1))'`:
     the first row at turn 10 or 11 with outcome `compaction` and its cut's `messages`, `first` and `last`. A
     later `ring` row is possible (see uncertain, above), and the scorer counts it;
   - the overhead: while the run is on (after its first turn), from another shell, `target/release-thin/theseus
     --socket /tmp/rf-th/daemon/sock --json ledger -k context.compiled -n 500 | python3 -c 'import json,sys; v=json.load(sys.stdin); r=v.get("rows", v); print(min((x["data"]["estimate"]["tokens"], x["data"]["estimate"]["method"]) for x in r if x["data"]["estimate"]["method"] == "bytes"))'`.
     A new session's first call (method `bytes`) less its user message should be near 13,528. If it is off by more
     than about 1,000, set `OVERHEAD_TOKENS` in generate.py and regenerate;
   - `ps -eo pid,args | grep rf-th | grep -v grep` prints nothing.
3. `python3 bench/recall/score.py /tmp/rf-th --out /tmp/rf-report`, then `python3 bench/recall/score.py /tmp/rf-th
   --stale retracted --out /tmp/rf-report-r`. In `report.md`, *Compacted at* reads "11 (compaction)" (or 10),
   and the Compactions table counts each outcome. p003's kind of answer ("I can't tell … found no matches") now
   scores right under both rules. Under `retracted`, p001's kind of answer ("… Ignore the "27340" …") is right
   with Old named 1/1 and Stale 0/1, where strict says Stale 1/1. Read as a person would, the first smoke's arm
   was recall 6/6 and abstention 2/2.
4. The full on Theseus alone first: `python3 bench/recall/generate.py --seed 7 --size full --out /tmp/rf-full`
   (window 94000, budget 73,904; marks 120 one log, 320 and 520 two), then `drive.py --arm theseus … --progression
   /tmp/rf-full --out /tmp/rf-th-full` (about $11.70). Check that the first compaction of each session is at 120
   or 121, 320 or 321, 520 or 521, with outcome `compaction`, and that no turn exits nonzero. Only then Claude
   Code's arm, which at 94k gets `/compact` after each mark (see above).

## Docs the maintainer may want to touch

- docs/benchmarks.md, if it quotes the recall bench's windows (35000 / 124000) or $0.48 / $11: now 44000 / 94000
  and $0.53 / $11.73 at seed 7.
- Part III's record of the recall bench: the stand-in's 40-token count hides the ring and the overage (and is why
  main's smoke never compacted). And the arithmetic: crossing at a mark while its turn alone fits needs the turns
  before it to outweigh 40% of the system prompt and tools.

## The gate

Two gates, `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: one before the first commit, one before
the summary-room test and this report (the branch changes nothing the gate builds).

- **First** (on main's Rust, 20:59): fmt, shape, features, clippy, cockpit, test build and the reader rule pass. The
  suite: 2,789 run, 2,756 passed, **33 failed, all the known L1 tests** (theseus-sandbox's contract tests, its
  bench's `spawn_100`, and theseusd's sandbox tests: a root daemon's job with no job cgroup, theseus-pv6i). I ran
  the phases after it myself: protocol types ok; the lifecycle and jobs benches are skipped under
  `THESEUS_GATE_NO_BENCH`; the turn bench passes (frames 5 and 9, at their budgets); `cargo deny --offline check`:
  advisories, bans, licences and sources ok. Counted green. The core's golden passed under `TZ=America/Phoenix`.
- **Second** (22:20): the same phases pass. The suite: 2,789 run, 2,755 passed, 34 failed: the same 33 L1 tests,
  and **theseus-core's `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`**,
  once: "assertion `left == right` failed: the pool thread took the idle thread's policy, left: 0, right: 5"
  (tender.rs:340). It passed 8 of 8 alone after. It's not on the brief's list, and it's in learning/, which I
  didn't touch. It asserts the fault it documents reproduces: a blocking-pool thread started from the SCHED_IDLE
  thread inherits SCHED_IDLE (5). Left 0 means the `spawn_blocking` ran on a pool thread that wasn't started
  there, SCHED_OTHER. Under the suite's load, something on the fresh runtime may have started a pool thread
  first. It's for the learning lane (learning-fixes) to look at; the full output is the second gate's log. The
  phases after the suite: protocol types ok, the turn bench ok (5 and 9), deny ok.

**One trap I met:** after a planted revert of `tokens.py` (4096 → 4000) and its restore, a run used the planted
bytecode. A pyc keeps its source's mtime in whole seconds, and the planted file and the restore had the same size
and the same second, so `touch` in that second didn't help. Clearing `bench/recall/__pycache__` fixed it. The
earlier planted reverts each had a full green suite after their restore, which a stale pyc would have failed. For a
Python planted revert, `touch -d '+2 seconds'` the restored file, or clear `__pycache__`. That might be worth a
line in AGENTS.md's planted-revert principle.
