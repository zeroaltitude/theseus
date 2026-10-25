# CLOUD_REPORT: p3g-bench-index (theseus-jda0z)

Branch `cloud/20261025-p3g-bench-index`, base `0fda4d57`. Commits: `5687cf5c` (manifest, `draft.py` hook, README),
`e5950997` (cockpit summary), plus the empty start commit `eb0e7180` and this report.

## 1. For the maintainer

**Step 0 — the `omnibus.json` claim is wrong for the UI.** `grep -ril omnibus cockpit/src cockpit/test` finds nothing
(rc 1). `omnibus.json` (`kind: "omnibus"`, keys `report, kind, about, figures, total_usd`) feeds only `omnibus.md`; the
Benchmarks page never reads it. `benchfiles.ts` globs `docs/benchmarks/*.json`, so the file is embedded in the build
but unused, and no view draws it. What the page showed as stale was real, though: its run list and frontier come from
the per-trial CSVs and `TASK_SETS`, with nothing saying which run is the newest for a given arm. `omnibus.json` was not edited.

- **Store format:** untouched. No wire type, no `protocol.gen/`, no Rust.
- **Ceilings / OWNER CALL / STOP:** none raised. One design point for the owner is under OWNER CALL below.
- **Scope cut, not done:** the omnibus figure generator (`make_figs.py`, `build_md.py`) is on the owner's machine,
  outside the clone. Moving it into the repo, running it in the report step, and regenerating `omnibus.json` from the
  newer runs are local, docs-only jobs.
- **Daemon serving `docs/benchmarks` (not done):** the daemon would need a read-only route (or a `bench.manifest` RPC)
  that serves `index.json` and the report files from a directory on disk, and the cockpit would read it by `fetch`
  instead of `import.meta.glob`. That means a place rule for the route, a cache-busting or watch story for new files,
  a protocol type, and a decision on which directory is served (the install has no checkout).
- **OWNER CALL:** the newest Theseus row for `terminal-bench@2.0` / high is the 10-23 timeout retest, which covers only
  6 tasks (18 trials, 22.2%, Wilson 9.0–45.2%). That is the newest measurement, but it is not comparable to the 89-task
  runs. The page shows trials and tasks beside every value. Choosing a "fullest recent" rule instead of "newest" is the
  owner's call, and I did not make it.
- **Setup/gate caveat:** the environment's disk allowance ran out twice during `cargo build --workspace --all-targets`
  (`No space left on device`). I cleared `target/` pieces and could not finish a Rust build, so the full gate did not
  run (section 8). I also made the empty start commit before the setup had finished, because the build was still in
  progress at that point.

## 2. The manifest schema (`docs/benchmarks/index.json`, version 1)

Top level: `version` (1), `about`, `runs` (sorted by `date`, `report`, `variant`; JSON `indent=1`, trailing newline).
Each row, from a dataset's top level and one `summary` entry:

| field | from | why |
|---|---|---|
| `report` | `report` (else file stem) | link to the report page |
| `benchmark` | `suite`, verbatim | the key's first part |
| `tier` | `tier`, else null | the second part; the older reports have none |
| `arm` | `family(variant)`: `theseus*` → `theseus`, else the summary key | a stable identity across Theseus's per-commit variants |
| `variant` | the summary key | which variant of the arm in this report |
| `harness` | the entry's `label` (else the key) | display name as the dataset states it |
| `model` | `model`, else null | verbatim |
| `date` | `date` | required, must be YYYY-MM-DD |
| `commit` | `commit`, else `run.code_commit`, else null | `run.code_commit` is used for the codex report |
| `dataset` | `docs/benchmarks/<report>.json` | |
| `trials`, `tasks` | entry | |
| `metrics.pass_rate` | `rate`+`wilson`+`solved`+`trials`, or `pass{k,n,rate,lo,hi}` | `{value, lo, hi, k, n}` |
| `metrics.usd_per_trial` | `usd_trial`+`usd_trial_ci`, or `cost_per_trial{mean,lo,hi}` | optional |
| `metrics.usd_total`, `usd_per_solve`, `agent_s_median` | `usd_total`/`cost_usd`/`dollars`, `usd_solve`, `agent_s_median` | optional |
| `comparison` | true when the report marks its own arms "(this run)" and this label lacks it | an arm it drew against, not measured |
| `carried_from` | the earlier report whose figures match exactly | derived by the tool, recomputed every build |

Only fields a dataset can fill are present. Two shapes of pass rate are supported (the flat tiered shape and the
nested `pass` shape); nothing else is guessed.

## 3. Coverage

48 datasets (`docs/benchmarks/*.json` besides `index.json`): **13 became rows (62 rows)**; 30 of those 62 rows are
`comparison` or `carried_from`. **35 skipped**, each printed with its reason:

- *Omnibus, not a run (1):* `omnibus.json`.
- *No `summary` object (4):* `2026-10-12-async-six-families`, `2026-10-12-gate-bench-fast-history`,
  `2026-10-12-head-to-head-speed`, `2026-10-18-head-to-head-speed`.
- *No summary entry holds a pass rate with its interval (30):* `2026-09-30-memory-exam-headroom`, `2026-09-30-memory-exam-v2`,
  `2026-10-01-context-token-estimate`, `2026-10-01-gate-bench-opt-level-2`, `2026-10-01-retrieval-embedding-engines`,
  `2026-10-01-retrieval-fusion`, `2026-10-02-gate-bench-install-builds`, `2026-10-02-gate-bench-start-path-10k`,
  `2026-10-03-gate-bench-ledger-reads`, `2026-10-03-harbor-worth-spike`, `2026-10-04-gate-bench-one-sync-per-job`,
  `2026-10-04-memory-exam-arms`, `2026-10-04-terminal-bench-first-full-run`, `2026-10-05-gate-bench-speed-io-stalls`,
  `2026-10-05-recall-smokes`, `2026-10-06-async-smokes`, `2026-10-06-gate-bench-fast-history`,
  `2026-10-06-harbor-efficiency-checks`, `2026-10-12-async-kill-family`, `2026-10-12-asyncbench-parallel-calls`,
  `2026-10-12-context-token-estimate`, `2026-10-12-f10-first-live-run`, `2026-10-12-gate-bench-stop-point`,
  `2026-10-12-memory-exam-arms`, `2026-10-12-recall-full-run`, `2026-10-12-retrieval-fusion`, `2026-10-12-scale`,
  `2026-10-18-async-kill-family`, `2026-10-18-gate-bench-v1`, `2026-10-18-recall-full-run`.

**Finding:** the datasets have no common schema. Speed, memory, recall and async reports have no pass rate with an
interval, so the Benchmarks page's summary covers Terminal-Bench (plus one `harbor` 2-trial check) only. The first
full Terminal-Bench run (`2026-10-04-…first-full-run`) is skipped because its summary is keyed by arm letter with different fields. Making the other suites
manifest rows needs a metric choice per suite.

## 4. `draft.py`'s hook

In `write_outputs`, right after the data file is written:

```python
manifest = _load("bench_report_manifest", HERE / "manifest.py")          # beside the other _load lines
...
if manifest.append(jp, out / manifest.INDEX, log=lambda m: print(m, file=sys.stderr)):
    written.append(out / manifest.INDEX)
```

Idempotent: `append` drops every row of the same `report` and adds the dataset's rows, then re-sorts and recomputes
`carried_from` over all rows, so appending twice, or in another order, gives the same bytes. A dataset with no pass
rate adds nothing (a stderr line, no failure). A dataset that has pass rates but lacks `suite` or a valid `date`
raises `ManifestError` before anything is written, so the draft fails loudly. A manifest that is not version 1 is refused with a
message to rebuild it. The manifest is written beside the data file, so the repo's own runs update `docs/benchmarks/index.json`.

## 5. The page

`Benchmarks.tsx` gets a plain `<section className="panel" aria-label="the newest measurements">` (no `Panel`, no
`help` id) between `YourWork` and the frontier. The frontier and run list are untouched. Groups are by benchmark and
tier, freshest first; one row per arm: harness name, pass rate with its interval, trials and tasks, "as of ‹date› ·
‹commit› · its report" (link to `/benchmarks/<report>`), and a 72×18 SVG line of the arm's pass rate over its
measuring runs, oldest to newest, the newest point marked in the harness colour (with a `<title>` listing every point).
A manifest that cannot be read shows one line with the reason. Row in words, from a headless render of the build:
"Claude Code 2.1.290 — 75.3% [69.8%, 80.1%] — 267 trials, 89 tasks — as of 2026-10-18 · 351159fc · its report — ⟋".
`benchfiles.ts`: the existing `*.json` glob already embeds `index.json` (as report `index`, not in `REPORTS` because that
list is built from the `.md` files), so I added only `useBenchManifest()`, not a second glob.

## 6. Newest Terminal-Bench numbers the page shows

| tier | arm | value (Wilson) | n | as of | commit |
|---|---|---|---|---|---|
| max | Theseus (`theseus-rt`) | 71.6% [61.0, 80.3] | 81 trials, 27 tasks | 2026-10-23 | fe56a164 |
| max | Claude Code 2.1.290 | 75.6% [65.8, 83.3] | 90 trials, 30 tasks | 2026-10-18 | 351159fc |
| max | OpenCode / Pi | 32.2% / 71.1% | 90 trials, 30 tasks | 2026-10-18 | 351159fc |
| high | Theseus (`theseus-rt`) | 22.2% [9.0, 45.2] | 18 trials, 6 tasks | 2026-10-23 | fe56a164 |
| high | Claude Code / OpenCode / Pi | 75.3% / 70.8% / 70.4% | 267 trials, 89 tasks | 2026-10-18 | 351159fc |
| high | Codex CLI 0.161.0 | 77.4% [60.2, 88.6] | 31 trials, 31 tasks | 2026-10-25 | b69236c1 |

The rivals' rows in the 10-21 and 10-23 reports repeat or re-cut the v1 figures, so they are `comparison` rows and
do not move the 10-18 date. Theseus max history: 10-18 (v1) → 10-21 → 10-23. The untiered `terminal-bench` and
`terminal-bench@2.0` and `harbor` groups also show, older and sorted below.

## 7. Tests and plants

- Python: 60 → 68 tests (8 new in `test_manifest.py`), `python3 -m unittest discover -s bench/report`: Ran 68, OK.
- `npm test`: 792 → 798 (6 new in `test/benchindex.test.ts`): `# tests 798  # pass 798  # fail 0`. `npx tsc -b` clean.
  `npm run lint`: no finding in a file I touched (the output has existing warnings elsewhere, none in my files).
  `npm run build` OK.
- Plants (each restored, re-`touch`ed, `cmp` against the backup):
  - **P1** drop the sort → 2 fail (`…second_build_is_byte_identical`, the append test).
  - **P2** append blindly → 1 fail (the idempotent append test).
  - **P3** let an unparseable file through → 3 fail (skips, no-pass-rate/half-row, drafted-run).
  - **P4** `draft.py` hook stashed → `test_a_drafted_run_writes_its_manifest_row` fails (index.json not written).
  - **P5** take the first row → 2 fail (newest-wins, real manifest).
  - **P6** trust the input → 4 fail (newest-wins, keys, validation, real manifest).
- The new files do not exist at the base, so their tests are trivially red there. No existing assertion was changed.

## 8. Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` was **not run to completion**: this VM's disk allowance ran
out in `cargo build --workspace --all-targets` twice (`No space left on device` building `theseus-core` tests), after
the first attempt's incremental cache filled it. No Rust file changed on this branch. What I did run, all passing:
`cargo fmt --all -- --check` (rc 0), `scripts/shape.sh` (rc 0), keel guard (below), the cockpit's `npm test` (798/798),
`npx tsc -b`, `npm run lint` (no finding in my files), `npm run build`, and `python3 -m unittest discover -s bench/report`
(68, OK). Not run: clippy, the nextest suite, protocol-types, `cargo deny` (the advisory fetch did not finish). The
maintainer's join gate is the proof for those. A pending-gate note for the cargo-deny phase: no dependency was added.

## 9. Keel

`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` → no output, rc 0. **Keel findings expected:** none.

## 10. DM golden

`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` → rc 0, no diff.
`Cargo.lock` and `cockpit/package-lock.json`: no change.

## 11. Join notes

Files touched: `bench/report/manifest.py` (new), `bench/report/test_manifest.py` (new), `bench/report/draft.py`
(+1 `_load` line near 70, +4 lines in `write_outputs` near 163), `bench/README.md` (a table row near 30 and one paragraph
before `--arm takes…`), `docs/benchmarks/index.json` (new), `cockpit/src/lib/benchindex.ts` (new),
`cockpit/src/lib/benchfiles.ts` (two imports at the top, `useBenchManifest` at the end), `cockpit/src/views/Benchmarks.tsx`
(imports, `<Newest />` after `<YourWork />`, the `Newest`/`NewestRow`/`Spark` block after `plainArms`),
`cockpit/test/benchindex.test.ts` (new). Nothing under `cockpit/src/ops/`, and none of `views/Ops.tsx`, `views/Fleet.tsx`,
`lib/explainers.ts`, `lib/threadboard.ts`, `lib/taskgraph.ts`, `lib/fullgraph.ts`, `components/TaskGraph.tsx`.

## 12. Follow-ups

- Docs for the maintainer: `docs/benchmarks/README.md` should mention `index.json` and `manifest.py` (I may not edit
  it); `cockpit/AGENTS.md`'s Benchmarks paragraph should name `benchindex.ts` and `useBenchManifest`; `bench/README.md` is done.
- Add the other suites (gate bench, memory exam, recall, async) to the manifest with a metric per suite.
- The `index.json` rebuild is pure from the datasets: `python3 bench/report/manifest.py build` should be a no-op in review.
- My first commit message says "14 datasets"; the correct count is 13.

## 13. Data floor

Nothing new is sent to a model or routed anywhere, and no benchmark was run. No model call, key, spend or AWS.
