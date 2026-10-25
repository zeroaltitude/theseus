# CLOUD_REPORT: p3b-context-chart (theseus-11cvd)

## 1. For the maintainer
Store format 51, untouched. No Rust touched. No OWNER CALL, no STOP.

## 2. The chart as built
Commit 9083c24d. Files: `cockpit/src/lib/explorer.ts`, `cockpit/src/views/Context.tsx` (`Timeline`),
`cockpit/src/components/ui.tsx`, `cockpit/test/explorer.test.ts`.
- Floor: `DATED_FLOOR = '2000-01'`, with a comment saying why. Months before it are never drawn as bars.
- Window: `?range=` is `2y` (default, the 24 calendar months ending at the newest month), `5y` (60) or `all`
  (from the first dated month, so never 1970). The key is dropped from the URL at its default. `2y`, `5y` and `all` are
  `Segmented` buttons.
- Scale: `?scale=` is `sqrt` (default) or `linear`. Heights come from `barHeight`, with the 3 % floor kept for any
  non-zero month.
- Gaps: a run of more than `GAP_MONTHS = 6` empty months is one dashed gap column, labelled and with a tip
  `2009-02 to 2012-07: no episodes`. Shorter gaps stay as empty months.
- Labels: the top three bars (fewer in a narrow window) carry their counts. The footer keeps `most: N in a month` and
  adds `N earlier than <first>` for dated months that fall before the window.
- `Segmented` now sets `aria-pressed` and `type="button"` (a small edit to the shared `ui.tsx`; it also helps its other users).
- The `on`/opacity rule for a picked month or range is kept, as are the per-bar tip and click-to-keep.
- `monthBins` is left in place, unused by the view, with its test unchanged.

## 3. The undated episodes
- They show as a chip beside the chart, `undated · 10 episodes`. Its tip lists the months and counts.
- A click on the chip keeps `first..1999-12` through the existing `month` range filter, so the query can express it.
- Table mode lists every month, and months before the floor are marked `undated`.
- Every episode is counted somewhere: the window's bars, the `earlier than` footer count, or the chip. A test sums them.
- Where the bad dates come from: not determined. The importer
  (`crates/theseus-core/src/import/episode.rs`, `fn time` at line 188) parses `as_of.start` and `as_of.end` with
  `wake::parse_at` and refuses a string that is not RFC 3339, so the epoch-0 and 1973 stamps are valid timestamps in
  the source export. I did not trace the source side. Follow-up row: check which source or sources carry the
  1970-01 and 1973-11 episodes (their `source` facet) and whether the exporter writes 0 for a missing date. Nothing
  was changed.

## 4. Tests
- `cd cockpit && npm test`: 732 pass, 0 fail. Three new tests in `test/explorer.test.ts`:
  1. The owner's shape: undated group of 10, window `2024-11` to `2026-10`, no bar before the floor, `all` starts at
     `2006-07`, at most 60 columns, and episodes are conserved.
  2. A 40-month gap is one gap mark, and a 3-month gap is three empty months.
  3. sqrt gives more than 10 % for 323 against 19,778, and linear gives 3 % or less. The default is sqrt.
- P1 plant: `DATED_FLOOR` set to `1000-01`. Test 1 failed. Restored and touched.
- P2 plant: the default scale set to linear. Test 3 failed. Restored and touched.
- After each restore, `git status` showed only my intended files.

## 5. Gate
Cockpit part only (no Rust changed): `npm ci` ok, `npm run lint` 0 errors (existing warnings), `npm test` 732/0,
`npm run build` ok. I did not run the full `scripts/gate.sh` or the Rust build. The Rust suite and `cargo deny` were not
run here. I did not load the Context page from a scratch daemon, so the rendered chart is unseen. Please look at it
(see the live check below).

## 6. Keel
`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` exited 0 with no output.

## 7. DM golden
`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` exited 0 (no difference).

## 8. Join notes
Touched: `cockpit/src/lib/explorer.ts` (appended only), `cockpit/src/views/Context.tsx` (`Timeline` and its import
lines), `cockpit/src/components/ui.tsx` (`Segmented`: `aria-pressed`), `cockpit/test/explorer.test.ts`. I did not touch
`figures.ts`, `ShipsWheel`, `Heartbeat`, `People` or `OntologyParts`.

Live check: on a daemon with an import fixture, open `/context`. Expect the `as of` panel to show 24 months of
sqrt-scaled bars, an `undated · N episodes` chip, and `2y/5y/all` and `linear/sqrt` buttons. Reload with `?range=all&scale=linear`
and confirm the view persists. Click the chip and expect `month=1970-01..1999-12` in the URL and the list filtered.

## 9. Follow-ups
- Trace the source of the epoch-0 and 1973 stamps (section 3).
- Remove `monthBins` if nothing else comes to use it.
- Should `5y` and `all` share one window rule when the newest data is old? Today the window ends at the newest month, not today.
