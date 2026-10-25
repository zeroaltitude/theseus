# CLOUD_REPORT: p3g-soft-retire (theseus-99l9z)

Branch `cloud/20261025-p3g-soft-retire`, from `0fda4d57`. Cockpit only.

## 1. For the maintainer
- Store format: untouched. No Rust file, no wire type, no `protocol.gen/`. No ceiling raised. `NODE_CAP` stays 400.
- No `OWNER CALL:` and no STOP. One scope note: besides `retire.ts` I added **`cockpit/src/ops/RetireBar.tsx`** (new file): the hook and the checkbox row that the three views share. It is a new file, so it cannot conflict.
- The cutoff `days` and the idle/stale split are my reading of the brief (section 2); the owner may want other numbers.

## 2. The five classes (`cockpit/src/ops/retire.ts`, `classOf`)
One class per item, first rule that fits:
- live (needs you, working: `bucket` is `needs` or `working`): never hidden, any age.
- **ended**: `bucket === 'ended'`, any age (`row.bucket`, via `rowItem`; for a Fleet session `bucketOfSession`, for a task record `bucketOfCard`). No state string appears in `retire.ts` (a test reads the source for that).
- **retired**: the session's own `retired` flag (`r.session?.retired`, `s.retired`).
- recent (touched within the cutoff): shown.
- **at rest**: `bucket === 'rest'` and older than the cutoff. Its own class, not folded into ended; `COLUMN_OF` untouched.
- a scheduled card whose `dueMs` is still ahead: shown (waiting with work).
- **idle**: open and quiet (`scheduled`, `paused`, `todo`), untouched for more than the cutoff and up to `STALE_FACTOR` (4) cutoffs, i.e. 7 to 28 days by default.
- **stale**: the same, untouched for more than 4 cutoffs (over 28 days).
The line between idle and stale is `STALE_FACTOR * days`, one constant in `retire.ts`.

## 3. Default view and cutoff
Default: live plus anything touched in the last `DEFAULT_DAYS = 7` days (`retire.ts`). Ended and retired are hidden at any age. A person changes it with the "recent" select beside the checkboxes (1, 3, 7, 14, 30 days) or `?older=<days>`.

## 4. Persistence
URL: `?show=ended,rest` (checked classes, or `none`) and `?older=14`. Storage: `cockpit.retire.<view>` with `view` of `fleet`, `graph` or `taskgraph`, JSON `{show:[…],days}`. The address wins, field by field, where it speaks; else this browser's copy; else the default (`readChoice`). Every change writes both. Other parameters are kept (`show` and `older` are not in `OWN_KEYS`, so the Ops address round trip leaves them).

## 5. Nothing deleted
Hiding is a layout/drawing filter only. In the Graph lens it feeds the existing `keep` of the layout (the hole mechanism: slots kept, nothing moves) and never touches `rows`; in the Fleet it filters the table and graph sessions, while the "all N" and state chips still count `sessions`; in the task graph it filters the tree rows, not `records`, and the heading counts are untouched. Ancestors of a drawn node, the focused node and picked nodes are spared (`hiddenIds`). The chip `+N older` is `hiddenIds.size`. Test 4 proves `countersOf` over the same rows is identical before and after the split, the set is unchanged, and the checkbox numbers add up to the hidden count.

## 6. The Actions panel
Chosen: **fullscreen** through `useFullGraph`'s `?graph=full`. Why: `Actions.tsx` (not in my scope) renders `TaskGraphView` below the page, so in place would need a change there; fullscreen is where the click was and is the shared helper. Before: the button only set `?taskgraph=1`, and the graph appeared in a separate panel further down. After: `writeTaskGraph` (`lib/taskgraph.ts`) sets `taskgraph=1&graph=full`, closing removes `taskgraph`, `task` and `graph`. `?taskgraph=1` alone still opens the panel, and it now scrolls into view. The tree panel also got the checkbox group (records by `bucketOfCard`, touched `updated_at_ms`). The title and `help=` are untouched.

## 7. Tests
`npm test` in cockpit: 792 tests before (at base, after `npm ci`), **799 after** (7 new in `test/retire.test.ts`), 0 failed. Lint and `npx tsc -b` clean (the lint warnings are the base's). Plants (each reverted, file touched, `git status` clean):
- P1 default shows ended: red (tests 1 and 5).
- P2 rest folded into ended: red (tests 1, 2, 3).
- P3 bucket replaced by a state-string list in `rowItem`: red (test 3).
- P4 split mutates its input (filters before the counters see it): red (tests 2 and 4).
- P5 storage write dropped: red (test 5).
- P6 base's parameter-only toggle: red (test 6); the message names that the parameter alone renders nothing in that panel.
No existing expectation moved.

## 8. Gate
See the section after 13.

## 9. Keel
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`: no output, exit 0. Keel findings expected: none.

## 10. DM golden
`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no diff.

## 11. Join notes
Files: new `ops/retire.ts`, `ops/RetireBar.tsx`, `test/retire.test.ts`; edited `views/Fleet.tsx` (import; a block after `titleOf`; the table's `drawn`; the bar in the header row; the graph's sessions), `components/TaskGraph.tsx` (body only), `lib/taskgraph.ts` (appended `writeTaskGraph`).
`ops/GraphLens.tsx`, all additive at the edges (line numbers at my head): two import lines at 27-28; a six-line block at 87-92 (the hook, `retiredOut`, `kept`, `retireCounts`) replacing the one `memory.lay` line, which now reads `keep: kept`; `keep` becomes `kept` at 98 and 100 (`base`, `baseRoots`); one `<RetireBar …/>` line in the toolbar at 233. `ops/graph.ts`: **not touched**. I left `board.ts`, `state.ts`, `COLUMN_OF`, `explainers.ts`, `CommandBar.tsx` and `TaskGraph.tsx`'s `title=`/`help=` alone.

## 12. Follow-ups
- `theseus-zo72s` (rest out of the Done column): once it joins, nothing more is owed here; the `rest` class reads `row.bucket`.
- The Fleet graph relayouts when the choice changes (the live layout is over the drawn sessions).
- A counter's click and the retire filter compose by intersection; a hidden card that a counter selected stays hidden until its class is checked.

## 13. Data floor
Nothing new is sent to a model or routed anywhere; the change reads data the page already holds.

## Gate result
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` as given **failed in `door`** at once: origin/main (`2d1d8d6f`) has moved past my base, so main's keel guard, judging base..tree, reports 5 "erosions" (persona_cmd.rs tests, keel-guard.py files, and unverifiable acks) that are main's own later commits, none in my diff. My own `keel-guard.py --base 0fda4d57 --head HEAD --quiet` is clean (exit 0). I did not merge main.
I then ran the tree's own gate with `THESEUS_GATE_DOOR=judge THESEUS_KEEL_BASE=0fda4d57` (debug info off, the VM disk is small). It passed keel, keel tests, fmt, shape, recall tests, features, clippy, clippy-probe, **cockpit (lint, tests, build)**, test build, reader rule. The suite phase failed on:
- the known L1 cases: theseus-sandbox `bench spawn_100` and 19 contract tests, and 13 theseusd `sandbox` tests (root daemon, no job cgroup, theseus-pv6i);
- `theseus-core tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` and `theseus-discord tests_walk1b::a_dm_under_notify_starts_two_tasks_and_both_are_seen` (101 s): timing under load; each passed alone on rerun (1.3 s and 1.4 s).
`protocol_types` (generated TypeScript) passed inside the suite and `git status` is clean after it. `cargo deny --offline check`: advisories, bans, licenses and sources ok. Benches were skipped (`THESEUS_GATE_NO_BENCH=1`); no Rust file changed. The cockpit's own `npm test` count is the one in section 7: 799 of 799.
Note: an earlier full build filled this VM's disk; I deleted `target/` and rebuilt without debug info. No source was lost.
