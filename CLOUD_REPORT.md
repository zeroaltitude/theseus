# CLOUD_REPORT: recall-plan-fix (theseus-cs8k, with theseus-iec1 and theseus-hau2)

Branch `cloud/20261010-recall-plan-fix`. Commits on top of the task commit (5219706):

- `cae4ee4` recall bench: `--stale retracted` closes three false rights, and its unguarded arms get tests (theseus-hau2)
- `6ae5de9` recall bench: plan at the overhead the daemon measures, and Pi's threshold from Pi's own measure (theseus-cs8k, theseus-iec1)

Only `bench/recall` changed. No Rust, no `crates/`, no `bench/report`, `bench/harbor`, spec or status files.

## Step 1: the overhead is measured at run time (theseus-cs8k)

**Found.** `generate.OVERHEAD_TOKENS` (13,640) was outgrown. On this VM's build of main the daemon measures **13,961**
(the brief said 13,943; the number moves with the build, which is the point). The first real turn reads 13,954 to
13,958, 3 to 7 under the probe: the rounding of the first turn's words. So `first_turn` is not equal to `measured`;
the tests allow 10.

**Changed (`6ae5de9`).**
- `generate.OVERHEAD_TOKENS` is gone. `build(seed, size, overhead)` and every bound function take it, with no default.
  `generate.py --overhead` is required. `planned_overhead(prog)` raises for a file with none recorded (it used to fall
  back to the constant).
- `drive.py`: for the Theseus arm, `measure_theseus` starts a **throwaway daemon** of the run's own config (same
  binaries, profile, memory arm, effort), sends one short exchange (`PROBE_TEXT`) in a session of its own, and reads its
  first `context.compiled` less that message with `overhead_of`. A daemon of its own, not the run's, because recall
  draws on every earlier session and a probe in the run's daemon would be in the run's memory. The window does not matter
  (the overhead is the prompt, not the output cap); the probe uses 100,000.
- The progression is then generated at that measure: `--seed N --size smoke|full`. The generator checks the plan's bounds
  there and at the cushion either side (`plan_misses`, unchanged). `--overhead N` with `--seed` **pins** a plan, and a
  `--progression` file's recorded overhead is a pinned plan too: a measure more than the cushion off it exits 3 **before
  the first turn**, naming both numbers (it used to refuse after turn 0). `--allow-overhead` runs on.
- The first real turn is read again (`first_turn`) and held to the plan the same way (exit 3 after that turn).
- run.json `overhead`: `planned`, `measured` (the probe's), `first_turn`, `pinned`, `cushion`, `past_cushion`,
  `allowed`, `source`, `probe_cost_usd`. (`planned_recorded` is gone: `pinned` replaces it.)
- `score.py`: report.md has a new "System prompt and tools" table (planned, measured, first turn, pinned per arm), and
  scores.json `arms[...].overhead` carries the record.
- Claude Code and Pi read the progression the Theseus arm kept: `--progression <its run directory>`. `--seed`, `--size`
  and `--overhead` are for theseus; the others refuse them with a message.
- README: the numbers (seed 7 smoke at 13,943: window 46,000, budget 30,404; $0.54 and $12.02), the commands, "The
  overhead is measured, never assumed", the run directory, the tests list. The corrected figures are the ones
  `generate.py` prints at 13,943.
- Fixed beside it: a run refused after its first turn left `turns.jsonl` open (ResourceWarning); it is closed.

**Cost of the probe.** Theseus: one call with about 14,000 input tokens; at the catalog's $2.50/M cache write that is
about $0.035 (the stand-in's record says $0.029 at its counts). Nothing on the stand-in. Pi: about 2,500 input tokens,
about a cent.

**Which seeds were checked at today's measure.** `test_every_plan_holds_at_the_daemons_measure_of_2026_10_09`: smoke
and full at seeds **7, 8, 11, 12**, planned at 13,943: `plan_misses` is empty and every mark's bounds hold there and at
the cushion either side. The existing sweep (smoke at every overhead 13,528 to 14,000; full every 59th; seeds 7, 8, 11,
12) still passes, and the 13,961 this VM measures is inside it for the smoke. Stand-in smokes of seeds 7, 8, 11 and 12
on a scratch daemon (below) planned at 13,961 and ran to the end.

**Proof.**
- `python3 -m unittest discover -s bench/recall`: **87 run, 0 failing** (main as cloned: 78 run, 2 failing; the two named
  tests, `test_mains_sizing_fails_the_mark_on_the_counting_stand_in_as_it_did_live` and
  `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing`, now pass: the first plans at the measured
  overhead, the second no longer asserts a constant). At `cae4ee4` alone the suite is 80 run, 2 failing, the same two.
- Smoke on a scratch daemon with `standin.py` (`drive.py --arm theseus --seed N --size smoke`, no pin), each: 30 turns,
  9 of 9 facts delivered, every turn exit 0, compacted at turn 11 (mark 10), outcome `compaction`, nothing left running.
  run.json `overhead` for seed 12:
  `{"allowed": false, "cushion": 50, "first_turn": 13955, "measured": 13961, "past_cushion": false, "pinned": false,
  "planned": 13961, "probe_cost_usd": 0.029172, "source": "probe"}`. Seeds 7, 11, 8: planned 13,961, measured 13,961,
  first_turn 13,955, 13,955, 13,954; compaction at 11.
- Planted reverts, each restored with `touch`, `git status` clean after each:
  - measure skipped (plans at a fixed 13,640): `test_a_run_plans_at_the_overhead_it_measures_and_records_it` fails.
  - the constant back in `generate.py`: `test_the_planned_overhead_is_recorded_and_can_be_set` fails.
  - pinned-plan refusal skipped: `test_a_pinned_plan_is_held_to_the_cushion_before_any_turn` and
    `test_a_progression_planned_under_the_daemons_overhead_is_refused` fail.
  - first-turn hold off (Theseus): passed the suite until I added `test_a_first_turn_reading_far_from_the_probes_stops_the_run_after_that_turn`
    (patches the probe 100 under the real overhead); with the hold off that test fails.

## Step 2: Pi's overhead from Pi's own measure (theseus-iec1)

**Found.** The 2,233 was an offline reading; the one live reading says about 2,475, 242 over, so the first real Pi smoke
would have been refused at turn 0.

**Changed (`6ae5de9`).** `PI_OVERHEAD_TOKENS` is gone. `Pi.prepare()` runs before turn 0: a probe turn in a throwaway Pi
session (its own `pi-probe-agent` and `pi-probe-sessions`, in the run's workspace, so nothing of it is in the run's logs,
compaction, or memory), the first answer's whole input less the probe's words at the model's rates. `pi_threshold` is set
from that, the settings are written after it, and the record (`planned`, `measured`, `first_turn`, `pinned`, `threshold`)
goes in run.json. `--pi-overhead N` still pins a plan; a probe more than the cushion off a pinned plan exits 3 before any
turn. The first real turn is held to the plan too. README's worked example is rewritten: seed 12's smoke at 13,943 has a
window of 49,000, and Pi measured at 2,475 gets 49,000 - (13,943 - 2,475) = 37,532.

**Proof.** `PiDriver` tests, on a stand-in `pi` whose probe and first-turn counts are set separately: a reading of 2,475
plans at 2,475 and runs (threshold follows); 1,900 and 2,233 too; a pin of 2,233 with probes at +/-50 runs, +/-51 exits 3
before any turn (empty turns.jsonl) and names `--pi-overhead <measured>` and `--allow-overhead`; a first turn 51 off its
probe stops after that turn; the probe's session is not in the run's logs. Plants: Pi planned at a fixed 2,233 fails
`test_pis_plan_is_its_measure_unless_pinned_and_a_pinned_one_is_held_to_the_cushion`; the pinned refusal off fails the
same; the Pi first-turn hold off fails `test_a_pis_first_turn_reading_far_from_its_probes_stops_the_run_after_that_turn`.
No real Pi on this VM: not run against the real binary.

## Step 3: three false rights in `--stale retracted` (theseus-hau2)

**Found.** All four replies and the three plants reproduced.

**Changed (`cae4ee4`, `score.py`).**
- `the old X` governs only beside a retraction word about X: `RETRACTS_ELSEWHERE` became `RETRACT_WORD` with a nearest-value
  test (`_retracted_here`: no other value of the clause nearer the word). Bare `instead` was dropped from it ("use the old
  port X instead" chooses X; `instead of` stays, as a prefix).
- A `no longer` ahead of wrong/stale/outdated/obsolete/old/retired/deprecated/dropped/gone is a negation in the prefix too
  (`NEGATED`, shared with the suffix guard).
- A list member that `is/was/are/were/'s` follows at once starts a clause and takes no prefix from before the list
  (`STARTS_CLAUSE`, on the backward walk only: the forward walk's verb is the suffix phrase).
- `CANCEL`'s curly-quote alternatives removed: `sentences()` folds first, so they were unreachable; a test shows both
  curly styles and both ASCII ones still cancel.
- README "The rule's reach" updated.

**Proof.** `test_the_false_rights_of_the_retracted_rule_are_closed`: the six wrong replies (the four from the task plus two
more verb forms) are scored wrong under both rules (confirmed right under the old `score.py`); the six right ones (q1's
sentence, one each for `after that`, `later`, `next`, and two list shapes that must stay right) are scored right under
`retracted` and stale under strict. `test_a_citing_prefix_in_either_quote_style_governs_nothing`. Strict is unaffected: the
suite's strict assertions pass unchanged, and each new sentence is asserted stale under strict. Plants, each caught by
that test: `the old` arm off (q1), `after that` dropped, `later` dropped, `next` dropped (q7), the nearest-value rule off,
bare `instead` put back, the negated-`no longer` guard off, the starts-clause guard off.

## Live check for the maintainer (the owner's machine, real model)

```bash
cargo build --release -p theseus -p theseusd -p theseus-index   # or scripts/build.sh --profile release-thin
python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir target/release \
  --model anthropic/claude-sonnet-5-5 --seed 12 --size smoke --out /tmp/rc-th12
python3 -c 'import json;r=json.load(open("/tmp/rc-th12/run.json"));print(r["overhead"],r["compactions"],r["compaction_rows"])'
python3 bench/recall/drive.py --arm pi --model anthropic/claude-sonnet-5-5 --progression /tmp/rc-th12 --out /tmp/rc-pi12
python3 -c 'import json;r=json.load(open("/tmp/rc-pi12/run.json"));print(r["overhead"],r["pi_compact"],r["compactions"])'
python3 bench/recall/score.py /tmp/rc-th12 /tmp/rc-pi12 --out /tmp/rc-rep12   # report.md has "System prompt and tools"
```

Expect: Theseus `overhead.planned == measured` (about 13.9k, owner's build), `first_turn` within a few tokens, a
`compaction` outcome at turn 11 (the mark is turn 10; the mark's turn or the next); `progression.json` records the plan.
Pi: `overhead.measured` about 2,475 (the provider's count; the live reading is the thing to compare), `planned == measured`,
`pi_compact.threshold` = window - (planned_theseus - measured_pi), a compaction in the logs near turn 11, `first_turn`
within a few tokens of `measured`. If Pi's real compaction does not fall at the plan's turns, say so in the report: the
stand-in cannot show it. Expected cost: about $0.52 for the Theseus arm (the generator's budget) plus the probe, about
$0.04; about $0.4 to $0.5 for Pi plus a cent for its probe. About $1.1 for both. A pinned rerun: add `--overhead <the
earlier planned>` to the Theseus command (`--pi-overhead` for Pi).

## Gate

- `scripts/gate.sh` (with `THESEUS_GATE_NO_BENCH=1`, `TZ=America/Phoenix`, and `THESEUS_KEEL_BASE=5219706`, see below):
  keel ok, keel tests ok, fmt ok, shape ok, features ok, clippy ok, cockpit ok, test build ok, reader rule ok, then the
  suite: 3,682 run, 3,649 passed, **33 failed**, 44 skipped. The 33 are the known L1 ones: 20 in `theseus-sandbox` and 13 in
  `theseusd::sandbox` (the VM runs as root; theseus-pv6i). No other failures. The phases after the suite
  (`protocol_types`, `compiled_under_lock`, and the benches, which `THESEUS_GATE_NO_BENCH` skips) were not run: this
  branch changes no Rust. The tree was not changed after the gate ran, apart from splitting it into the two commits.
- The recall suite is the gate for these commits: 87 run, 0 failing (see above).
- The gate does not run bench's Python; I ran it myself before each commit. Harbor's tests were not touched or run.

**Keel findings.** Of mine: none. A rewritten Pi test is matched by the guard as the same test (renamed and rewritten), not
a finding. Two tests of mine first lost assertions (a net 9 to 7 and 23 to 22): I rewrote them to keep the count (more
assertions on the new `pinned`, `source` and absence of a run daemon). **Keel findings expected: none.** Note for the
maintainer: this branch was cut from a lane whose base (2fd1f65) is ahead of this clone's `origin/main` (a9ad950), so the
guard's default range (`merge-base(HEAD, main)..HEAD`) judges that older range too and reports 5 findings that are not
mine (a tui test's assertions, two `allow`s in theseus-index and theseus-protocol, two `long-files.txt` ceilings) and
fails the gate at the keel. I ran the gate with `THESEUS_KEEL_BASE=5219706` (the task commit), where the guard reports
`0 findings`. On the owner's machine, where `main` is current, the range is mine alone.

## Left, uncertain, and for the owner

- **bench/report/draft.py is bench-parity's, so I did not touch it.** Its "Reproduction" block still shows the old
  commands (`generate.py --seed <seed> --size <size> --out <prog>`, then `drive.py ... --progression <prog>`): the
  generator now needs `--overhead`, and the Theseus arm takes `--seed --size`. It also does not print the overhead the
  scorer now puts in scores.json (`arms[...].overhead`). Both are a few lines for whoever owns it.
- **Docs that should change at the merge:** the spec's Part III item for the recall bench and any text that gives 13,640 or
  Pi's 2,233 as the plan; `docs/benchmarks.md` if it quotes the recall commands. The 2026-10-05 recall smokes' data
  (`docs/benchmarks/2026-10-05-recall-smokes.json`) holds `planned_recorded`, which this change no longer writes (the
  scorer does not read it).
- **A design choice.** The probe is a separate throwaway daemon, not a first exchange in the run's own: it costs one
  extra daemon start (about a second on the stand-in) and about $0.04 on a real model, and in exchange the run's memory,
  its first turn, and its first-compile reading are exactly as before. If the owner would rather the run's own first
  turn be the measure, the plan could not be fixed before it (the progression is generated from the measure), so that
  would need the full progression to be regenerated mid-run: not done.
- The stand-in smokes ran for the smoke at seeds 7, 8, 11, 12. The full size was not run on the stand-in (about 600
  turns); its plans are checked by the generator's tests at 13,943 and across the sweep.
- Pi's own overhead on a real model, and whether Pi's real compaction falls where the threshold puts it, are the live
  check's to show.
