# Cloud run, branch cloud/20261018-v1-fixes (theseus-pngho.1)

Commits (all on the branch, pushed): the owner_ask view, the pane's one block, the bench's directive.v1 answer,
the four flaky tests, the two AGENTS.md lines, and a clippy line-limit fix in whole_replay.rs.

## What each fix was proven by
- **owner_ask choices on the task's own view** (`push/work/views.rs`): the planted revert reproduced the live bug
  exactly (an approval with approve and decline). CLI and Discord hints pass; both have planted reverts that fail.
- **A pane prints the block once** (`interactive.rs`): the card was printed by two first-read paths as well as the
  block; both now skip an owner_ask. `a_question_block_prints_once_whatever_prints_below_it` fails with the re-say
  planted back.
- **bench turn --judge** (`theseus-sim/perf/judge.rs`): the fake Jev answered directive.v1 with a standing capture
  (two extra frames). It now answers none. The real bench passes in every arm; the planted revert reproduces the
  4-vs-5 frame failure.
- **Flaky tests**: s8jod (a product bug: a sample could end mid-line; `last_line_from` fixed), w68w5 (scores read at
  one instant), glisu (a refused/failed split, with the reason on the wire as `WholeCall.why`), q1vlq (reviewed; the
  test waits for the store; 60 of 60 under load, the old read failed at least 5 of 60).

## The gate
Run with `THESEUS_KEEL_BASE=70937771` (the clone-time main). Against the current origin/main the keel guard reports
71 erosions that are other branches' work, not this branch's; against the clone-time main it reports none.
fmt, shape, features, clippy, clippy-probe, cockpit and the test build pass. The suite's failures were:
- `theseusd::sandbox` (about 30 tests) and `theseus-sandbox` L1 tests: they need a non-root user and this VM runs as
  root. Not caused by this branch; they need a run on the owner's machine.
- Discord task-thread and war-room tests and the acceptance journey: failed only while the disk was full; all pass
  alone (26 of 26, 7 of 7).
The gate was not rerun to the end after these were cleared, because the sandbox tests cannot pass here. Please rerun it
on a normal machine before merging.

## Not done
- The final gate pass above, and the benches (`THESEUS_GATE_NO_BENCH=1`), and the docs commit (Part III item, version
  line, status.md).
