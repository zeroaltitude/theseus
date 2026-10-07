# Cloud report: recall-fair (branch `cloud/20261006-recall-fair`)

bench-bounds' two-sided check (`past_cushion` as `abs(measured - planned) > OVERHEAD_CUSHION`, and
`test_a_daemon_under_the_plan_is_refused_like_one_over_it`) is in the clone. Only bench/recall changed.

## 1. theseus-a5we part 2: Pi planned at its own overhead (88fd6162)

**Found.** One progression is read by every arm and planned at Theseus's 13,640; Pi's system prompt and tools are
about 2.2k, so its context ran about 11.4k short at every turn and on the smoke it never compacted. I built the
"other way" the brief asks for (one progression, Pi's threshold planned at its own overhead): nothing in the code
stopped it, so score.py's one-digest rule is untouched and the plans' digests (`SMOKE_7`) are unchanged
(test_generate passes).

**Changed.** `drive.py`:
- `PI_OVERHEAD_TOKENS = 2233` and `--pi-overhead`.
- `pi_threshold(prog, window, pi_overhead) = window - (planned_overhead(prog) - pi_overhead)`, so Pi's context
  crosses where the plan's does. The reserve is the model's window less the threshold (smoke seed 12: threshold
  36,660 of 48,000).
- `keepRecentTokens` is `min(20000, threshold // 4)`. It is taken from the threshold, not the window, because the
  threshold is the context Pi holds when it compacts, and the quarter rule exists to leave something to summarize
  in that context.
- Pi's overhead is measured as Theseus's is: the first turn's first answer's input (input + cacheRead +
  cacheWrite, `pi_turn`'s new `first_input`) less the turn's words at the model's rates.
- run.json's `overhead` has the Theseus record's shape plus `threshold`; `pi_compact` gains `threshold`.
- The refusal is the same: off Pi's plan by more than the cushion either way exits 3, names both numbers and
  `--pi-overhead <measured>`.
- `--allow-overhead` is now a top-level flag both drivers read.
- `overhead_record` takes an optional `planned`, and `overhead_refusal` an `arm`.
- Theseus's wording and check are unchanged.

**Measured.** Pi 1.0.4 itself (npm, under /tmp), `drive.py --arm pi --api-base <standin.py>` on smoke seed 12,
`--allow-overhead`: measured 2,233 (so the constant). The stand-in counts by the generator's own rule, so the
offline difference between rule and count is 0. How far the rule's rates are from the provider's on Pi's request is
only known on the real model (live check). Pi never compacted in that run, as expected, since the stand-in has no
rules and the context never grew.

**Proof.** `python3 -m unittest discover -s bench/recall`, system python 3.11 and `python3.12`, before (77 tests,
4 skipped for lack of binaries) and after (78 tests OK, 0 skipped, `target/debug` built). New or changed tests on
`PI_STANDIN` (which now reports `STANDIN_FIRST_INPUT` on the first answer): reserve and keep at the planned
threshold, the record's shape and threshold, `pi_threshold`, Pi 51 off its plan (both directions) refused with both
numbers and `--pi-overhead`, 50 off runs, `--allow-overhead` runs on, and `--pi-overhead` moves threshold and plan.
Plants: `pi_threshold` returning the window fails 3 tests; the refusal raise removed fails the refusal test.
Each file was restored from a saved copy and touched.

## 2. theseus-p6kd q4: an aborted answer (hash in `git log`: "a Pi turn whose last answer was aborted")

The test now has an aborted answer, a `stop` answer, no answers and a nonzero exit. Plant: `("error", "aborted")` cut
to `("error",)` fails `test_a_turn_ends_failed_when_its_last_answer_is_a_providers_error`
("print mode exits 0 on an aborted answer too").

## 3. theseus-a5we part 1: Pi offline ("Pi runs offline")

Pi 1.0.4's docs/environment-variables.md: "`PI_OFFLINE` | Disable automatic network activity, including model
catalog refreshes"; docs/cli.md `--offline`: "Disables automatic network activity, including model catalog
refreshes. Equivalent to `PI_OFFLINE=1`"; docs/models.md: "Pi starts with its bundled catalog and can overlay newer
catalog data from pi.dev. Cached catalog data remains available offline". In dist, `process.env.PI_OFFLINE` is read
for the model runtime's catalog refresh (core/model-runtime.js), the version check, tools-manager, package-manager
and the llama extension, and not on a provider call path. `PI_OFFLINE=1` is set beside `PI_SKIP_VERSION_CHECK` and
`PI_TELEMETRY`; the stand-in logs the three and the driver test asserts them. Plant: `PI_OFFLINE` dropped fails
`test_a_session_id_per_session_...`.

## 4. theseus-n6p5: effort ("every arm runs at effort medium")

`--effort` (default `medium`; low, medium, high, xhigh, max) sets the scratch config's `profiles.bench.effort`,
Claude Code's `--effort` (in `claude --help`) and Pi's `--thinking`; run.json has `effort`. Tests: the Claude Code
and Pi argv through their stand-ins, `--effort high` for Pi, `theseus_config` for every level, and the Theseus
driver's config.toml and run.json end to end. Plants (each arm's flag dropped): Claude Code, Pi and Theseus each
fail a test.

## Live check for the maintainer

```
python3 bench/recall/generate.py --seed 12 --size smoke --out /tmp/rc-smoke12
python3 bench/recall/drive.py --arm theseus --bin-dir target/release --progression /tmp/rc-smoke12 --out /tmp/rc-th
python3 bench/recall/drive.py --arm claude-code --progression /tmp/rc-smoke12 --out /tmp/rc-cc
python3 bench/recall/drive.py --arm pi --progression /tmp/rc-smoke12 --out /tmp/rc-pi
python3 bench/recall/score.py /tmp/rc-th /tmp/rc-cc /tmp/rc-pi
```
(check score.py's usage for the exact invocation). Each should show: one report (same digest); every run.json
`effort` is `medium`; Pi's `overhead.measured` within 50 of its `planned` (2,233; else move `PI_OVERHEAD_TOKENS`),
`pi_compact.threshold` 36,660 and `compactions` holding the mark's turn (10, or 11). Then
`drive.py --arm pi ... --pi-overhead <measured + 150>` must exit 3 naming both numbers.

## Left / uncertain

- The provider's count for Pi's first call is unmeasured offline (above).
- Pi's `keepRecentTokens` at the smoke's threshold is 9,165: if Pi 1.0.4 still skips the compaction at the mark,
  that number is the first thing to look at.
- A "bash <pid>" was seen left running at the end of the Pi stand-in measure run (Pi's own tool shell,
  gone moments later); not investigated.
- Docs to change at the review: bench/README.md could point at the recall README's new Pi paragraph and `--effort`
  (not touched here, bench-fair's file).

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run before the first commit and again at the end
(Python-only commits, so nothing it builds changed). Only the known L1 root-VM failures failed in the suite
(33: theseus-sandbox contract tests and `spawn_100`, theseusd `sandbox` tests; theseus-pv6i). fmt, shape, features,
clippy, cockpit, test build, reader rule and the protocol-types check passed. Final run: 3,041 tests, 3,008 passed, 33 failed, all of them the sandbox ones named above (0 outside them); the phases after the suite (protocol types) have no diff to catch, and the benches were skipped (`THESEUS_GATE_NO_BENCH=1`). Both runs gave the same 33.
