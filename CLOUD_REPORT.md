# CLOUD_REPORT: bench-efficiency-fixes (theseus-1xxi, theseus-t412)

## Step 1: theseus-1xxi (efae2398)
**Found.** As briefed: `machine()` subtracted `sampler.cpu_s` though the sampler's pid is already in `outside` from its first sample. `theseus_spend(None, None)` and `claude_code_spend`'s empty last line gave 0 calls.
**Changed.** Second subtraction dropped; fixture work now 31.5 (outside's 0.2 holds the sampler's 0.08); docstring of `machine()` and sampler.py's head corrected; both empty last lines give `model_calls`/`tool_calls` None. The sampler's CPU before its first sample and after its last summary stays in the work (small: one start-up read, one final summary). I kept the sampler's pid in outside rather than excluding it and subtracting `cpu_s`: it changes no sampler output, and `cpu_s` only starts after the cgroup's start is read, so the outside class's count is the one that matches the cgroup window.
**Tests added.** `Machine.test_the_sampler_is_counted_once`, `Theseus.test_a_trial_with_no_files_has_unknown_calls_not_zero`, `Report.test_an_old_trial_with_empty_files_has_unknown_calls_and_the_mean_is_the_rest` (arm mean 8.0 over the one trial that knows).
**Planted reverts.** Second subtraction back: `test_the_sampler_is_counted_once` and `test_the_work_is_the_containers_cpu_less_the_rest` fail. The 0 back: the unknown-calls test (harbor) and the report test fail.
**No claude_code trial reaches the 0 line with a record on main** except one with files present but empty; it is covered by the same test.

## Step 2: theseus-t412 (2c2a87ae)
**Changed.** Host test keeps its other assertions, loses the scaled bound. New in test_sampler.py: `SamplerCost` (fixture /proc, N=50 and 4N=200: per-process bound 24 us, ratio to a bare stat read under 2.4x, 4N per-process under 1.5x N's + 5 us; plus a test that a greedy read of every status and cmdline costs over 1.5x) and `InANamespace` (60 sleepers, `core_share` at 250 ms under 1.1%; skips with its reason where `unshare` is refused).
**Measured** under the load recipe (nice 19 beside four nice-0 loops): fixture 7.0 to 11.7 us per process (N and 4N alike, 15 runs each), bare stat 4.7 us; namespace 0.48 to 0.53% of a core (5 runs, 62 processes). Bounds are 2x the worst. README's "0.3% with 25, under 1% with 90" is consistent with 0.5% at 60 and left alone (I did not measure 25 or 90).
**Planted revert.** `read_procs` also reading each process's status and cmdline: `SamplerCost.test_the_cost_per_process...` fails on the ratio (12.5 us vs bound 10.7). Its absolute bound alone did NOT fail (13 us), which is why the ratio is there.
**Loaded runs of test_sampler.** 9 runs, 8 passed. One (while the cold workspace build was also running; 382 s) failed; I did not capture which test, and 4 further loaded runs passed. Not identified: report as unexplained.
**Suites.** python3.11: `-s bench/harbor` 55 tests OK, 7 skipped (Harbor not importable); `-s bench/report` 9 OK. Harbor 0.23.0 venv (python3.12): harbor 55 OK, 0 skipped; report 9 OK. The namespace test ran (did not skip) on this VM.

## Live checks for the maintainer
1. On a host of 150+ processes, under load: `python3 -m unittest discover -s bench/harbor` should be green with no stopgap.
2. In `python:3.12-slim` on cgroup v2, copy `sh` to `theseus`, spin a child ~20 s, run `python3 bench/harbor/sampler.py --out /tmp/s --names theseus --interval-ms 250` (then 20), stop it with SIGTERM, then `python3 -c "import sys,json;sys.path.insert(0,'bench/harbor');import efficiency as e;print(e.machine(json.load(open('/tmp/s/sampler.json')))['work'])"`: `cpu_s` should match the cgroup's count for the child within about one sample's cost; main's reads low by the sampler's own CPU.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, reader rule pass; suite fails only on the 33 known L1 `theseusd::sandbox` / theseus-sandbox tests (root VM, theseus-pv6i), no other failure. protocol_types check: clean tree. Benches skipped (NO_BENCH); no Rust changed.

## Docs to change at review
bench/README.md's sampler cost line could cite 0.5% at 60 processes (namespace, 250 ms).
