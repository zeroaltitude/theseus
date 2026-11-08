# NOT LANDABLE

(No code change. The row's premise is false on this base. Returned as an owner call.)

- **Row:** theseus-la5oc, checklist item theseus-nsp9l.2 (typed-prompt-to-request path, 49.5 ms vs 30 ms target).
- **Branch:** cloud/p5-nsp9l2-prompt-latency. **Base cloned:** 2e6aa047 (task commit 783ac94 on top). Head: this report commit.
- **Files:** CLOUD_REPORT.md only. 1 commit.

## What I found

The 49.5 ms figure is stale. It is the 10-22 measurement (docs/status.md:220; the h2h report of 10-18 has 51.4 ms for T2). The
~20 ms gap has been removed by work already on main:
- theseus-nsp9l.2's own cost (the 100 KB tool catalog made, cloned, serialized and hashed each turn, ~25 ms) is fixed:
  `crates/theseus-core/src/tool_set.rs`, `toolrun/catalogs.rs`, with the count tests in `tests_send_path.rs`
  (a turn after the first makes no catalog and walks no tool JSON).
- theseus-nsp9l.4 moved the per-call directory fsync and the compile step's blob writes off the send path
  (`turn/compile_step.rs`, `own_bench/tests.rs:134`), and nsp9l.1 trimmed persona files per request.
- status.md still says "~21 ms over target (nsp9l.4)": that line is out of date.

## Stock measurement (the existing harness, nothing written)

`scripts/build.sh --profile release-thin`, then
`python3 bench/h2h/run.py --bin-dir target/release-thin --theseus-only --rows T2 --runs 20 --out /tmp/h2h-stock`
(Theseus arm, 20 runs per kind, stand-in 300 ms ttfb; 4-core VM, loadavg ~2-3 during the run because the nextest install
was still compiling, so these figures are if anything pessimistic).

| T2: typed prompt to request leaving | n | median | min | max |
|---|---|---|---|---|
| interactive | 20 | 11.7 ms | 9.1 | 54.8 (one outlier; the next is 15.0) |
| one-shot | 20 | 13.6 ms | 11.4 | 18.3 |

Both are well under the 30 ms target. There is no 20 ms gap to find on this base, so I did not attribute it into parts or
change code: a per-part table of an 11 ms path whose cost already moved would be invented precision. Also measured:
T1 40.5 ms median (39.2 to 43.1), first request 49.8 KB.

## Owner calls / what to fix in docs

- Close nsp9l.2 as done by its landed work; record T2 as ~12 ms (interactive) on a 4-core VM. Re-measure on the real machine
  to confirm, since this VM is not the budget machine.
- docs/status.md:220 and the "Known limits" line (~222) still carry 49.5 ms / ~21 ms: update them.
- The first request is 49.8 KB (the 10-18 report had 44.8 KB): that is the request-budget line, a separate row.

## Gate

Not run: no code changed, and the build here consumed the session's time. No tests were added, so no red test or planted revert applies.
