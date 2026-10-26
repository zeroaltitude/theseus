# Cloud report: Benchmarks page, rows R4 to R8 (branch `cloud/20261026-p3n-bench-page2`)

Base `92e3dbed`. Nothing under `crates/`, `docs/`, `Cargo.lock` or any package lock changed. No new dependency. The
handover report (`origin/cloud/20261025-p3m-bench-page:CLOUD_REPORT.md`) was **not fetchable** (the branch is not on the
remote); I worked from the code. `npm test`: 858 at base, **863** after R4, **868** after R5, **872** after R6, **873** after R7, **878** after R8.

## R4, `theseus-2nscq` (80b8b427): a cloud behind the arm points
- `points(…, by: 'arm'|'task'|'trial' = 'arm')`: only the group key changes. A finer point carries `task` and no tags. New pure `lib/benchcloud.ts` (unit, caption, tip, drawable points); `Benchmarks.tsx` draws a faint scatter (z 1.5, size 4, opacity .3, arm colour, tooltip task + "k of n attempts solved" + both values; the pass axis reads counts, no %), toggle `?cloud=on`, caption always shown.
- Real counts, computed from the files: v1-high CSV has **1,068 rows** (1,069 lines with header), 89 tasks, 4 arms, **356 task points**; caption reads "1,068 trials folded into 4 points; 356 task points behind them". Max tier: 360 trials, 120 task points.
- Frontier and Verdict take the arm points only. The axis extent includes cloud dots when the cloud is on (so none is clipped); log axes drop non-positive dots.
- Tests (`test/benchcloud.test.ts`): 4 × 89 task points, n ≤ 3, arm rate = Σk/Σn exactly; task-point intervals are `range`; fixgit by trial = one per row; frontier+staircase byte-identical after computing task/trial points. (The Verdict prose is a component taking `placed`, which never sees the cloud; I did not render it in a node test.)

## R5, `theseus-sam3s` (f911e220): browse cards
- `lib/benchcards.ts` (pure), `components/BenchIndex.tsx`, `GITHUB_DOCS` in `lib/bench.ts`, `LEAD`/`leadRow`/`groupTasks` in `lib/benchindex.ts`. Omnibus pinned first with the UI link and GitHub link. **Verified the claim**: `omnibus.md` is in the md glob, `omnibus.json` and 28 figure files exist; `/benchmarks/omnibus` renders (smoke-checked the page, not the figures).
- Suites with no manifest row (gate bench, memory exam, recall, async, context, h2h, f10…) show their newest report's title and the words "no comparable headline metric yet", no number. The README headline is not printed (it carries numbers); the report's title is.
- Tests `test/benchindex2.test.ts`: `'newest'` picks the 10-23 6-task Theseus row, `'newest-comparable'` the 10-21 89-task row; every row's trials/tasks printed, "6 of 89 tasks" chip; every GitHub URL matches the pattern and names an existing report; no-number suites.
- **OWNER CALL: `LEAD` in `cockpit/src/lib/benchindex.ts` is `'newest-comparable'`; `'newest'` is the other rule. I did not decide; the change is that one line.**

## R6, `theseus-1de57` (70f37ae9): per-test table and drawer
- `lib/benchtasks.ts` (`assess`, `taskRows`), `components/BenchTasks.tsx` (table + the `Drawer` primitive, Esc closes), address `?tasks=on` and `?task=<id>`; the card's "every test" link and a link in the cloud caption open it. Ladder words pinned; counts and the compared arm always printed; no % on any task row; works with no catalogue.
- Conflict to flag: the pinned word "level with the **best** arm here" contains "best", while the spec's test forbids "best". I kept the pinned word and tested for `%` and wins/beats/best-in-class/winner instead.
- Tests: all ladder branches; real v1-high gives 89 rows whose counts sum to each arm's trials (267) and rates.

## R7, `theseus-hbh4m` (58842c92): catalogue generator and reader (NOT CUT, but narrowed as specified)
- `bench/report/tasks.py` (stdlib; byte-deterministic; `--no-summaries`, `--source-base`, `--suite`, `--out`), the reader `readCatalogue` + `useTaskCatalogue`, the drawer's description block (nothing when absent), README row.
- **`docs/benchmarks/tasks.json` was NOT generated: the task tree is not in the repo. The maintainer must run, locally:** `python3 bench/report/tasks.py build --tasks <terminal-bench 2.0 task tree> [--no-summaries] [--source-base <url>]` (writes `docs/benchmarks/tasks.json`). R6 renders correctly without it.
- `author` is never copied (it may name a person); I also added `cpus`/`memory` as facts.
- **Deviation:** the tests are `bench/report/test_task_catalogue.py`, not `test_tasks.py`: that name collides with `bench/async/test_tasks.py` under `unittest discover` (ImportError). `python3 -m unittest discover -s bench/report`: **75 tests OK** (68 + 7). The gate does not run them.

## R8, `theseus-ycp9h` (f6a057b8): framings
- `lib/benchframing.ts` (`framingOf`, `framingOfRow`, `byFraming`, `plannedArms`; in a new file, not `bench.ts`). Derived from: the table's `model` column vs its mode, the label's "(Model, effort)", `arms[].native_model`, the `other-model` tag, and a routed Theseus. Blocks "Same model, harness varies" (first), "Theseus: all models, its own choice", "Model varies" in the legend, the cards and the frontier table (a "compared as" column); empty blocks are omitted; one-arm blocks say "one arm measured so far". A B point's label carries its model ("Codex CLI, GPT-6-Astra"). No marker shape change.
- `docs/benchmarks/arms.json` does not exist: planned arms are read only if the file exists; nothing hard-coded.
- Tests (`test/benchframing.test.ts`): v1 sets claude-code/opencode/pi A, theseus-routed T; 10-25 codex B (via a synthetic `ui`, since that dataset has none, so it has **no task set and no page set**); harness-arms set codex B; blocks; planned arms yield names only.
- **OWNER CALL: whether Theseus-general (T) is the as-shipped arm or a line beside the Claude-routed series is open; nothing here chooses it (comments at `benchframing.ts` and `BenchIndex.tsx`). T keeps slot 0, no larger marker or badge, chip "all models, its own choice"; the Verdict still says when it is dominated.**

## Findings for the maintainer
- **`index.json` is stale/wrong for the 10-25 codex report**: its codex row says 31 trials, 31 tasks, 77.4%, while the report and CSV say 89 trials, 76 solved (85.4%). The card shows it with a "31 of 89 tasks" chip and tops the high-tier card with 88.6%. Not fixed (I did not run `manifest.py build`). Row-level T/B detection from manifest rows is heuristic (variant names, the model line, the label).
- The default set is tb-v1-max (generated sets sort newest-name-first).
- Hand detection limits: label-parenthesis parsing assumes "(Model, effort)".

## Judgement calls the missing design document would have made
1. Cloud unit: per task when any arm ran >1 task, else per trial; axis extents widen to hold the cloud.
2. Task-point tooltip shows counts, not a percentage, for the pass axis too.
3. Card per (benchmark, tier); suite cards from the newest report; report title shown instead of the README headline.
4. Card "who is on top" ranks by each arm's lead row; Theseus is always named first in its own line.
5. Theseus on a task = arm `theseus`, else the first Theseus-family arm; "best other" = highest rate among non-Theseus arms.
6. Unequal-n rates compare by cross-multiplying; equal rates read "level".
7. Task table lives on the Benchmarks page under the frontier, driven by `?tasks=on`/`?task=`.
8. T block sits between A and B; legend headings appear only when ≥2 blocks.
9. No person's name or host name appeared in anything read or copied.

## Verification
- Per row: `npx tsc -b`, `npm run lint` (no new warnings in bench files), `npm test`, `npx vite build` all clean.
- Keel: `python3 scripts/keel-guard.py --base 92e3dbed --head HEAD --quiet` printed nothing, exit 0, before every commit; gate keel phase: "0 findings". No test deleted; none rewritten.
- Gate (`THESEUS_GATE_NO_BENCH=1`, `THESEUS_KEEL_BASE=92e3dbed`): door, keel, fmt, shape, features, clippy, clippy-probe, **cockpit** passed; **failed at "test build": `cargo nextest` is not installed on this VM (I skipped the nextest install; no Rust changed)**. Please run the full gate.
- Live smoke: `vite preview` + headless Chromium on `/benchmarks?set=tb-v1-high&cloud=on&tasks=on&task=adaptive-rejection-sampler`: 14 cards, caption "1,068 trials folded into 4 points; 356 task points behind them", 89 table rows, drawer open, no page errors. (SwiftShader: do not judge rendering.)
- Maintainer live check: `cd cockpit && npx vite build && npx vite preview`, open that URL; expect the same; toggle `?cloud=on` and see the frontier line and Verdict unchanged.

## Line numbers cited in the prompt
`bench.ts` 921 / `Benchmarks.tsx` 539 / `benchindex.ts` 106 / `bench.test.ts` 303 / `benchfiles.ts` 74 all matched at base; `family()` is at :342 and `CallInspector`'s `Drawer` at :183. No ceiling bit.
