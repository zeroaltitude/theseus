# CLOUD_REPORT: health-imported (theseus-revl)

Branch `cloud/20261006-health-imported`. Commits: `ec4ed68d` (protocol + core), `bc42796f` (CLI + cockpit).
The brief's three steps landed as two commits: the core cannot build against the old wire type, so steps 1 and 2 are
one commit (gated together); step 3 is the second.

## Found
- `Core::session_totals` took `sessions` from the projection's key count (or the fallback's record count): imported and
  erased sessions included. An imported session's record adds 0 turns, 0 tokens, 0 cost to the projection (tested), so
  `turns`, usage and cost are unchanged and keep their meaning.
- Atomicity: `import_batch` pushes the tag's META counts into the same `store.append` as the episodes it counts, and an
  episode is never split across frames, so the counts never lag the keys during an import. **Erase** writes the
  `erased` count only in its *last* frame (the tombstoned sessions go in earlier ones). Mid-erase a reader sees
  `imported` too high (tombstoned sessions still counted held) and `erased` low until the last frame; an erase that
  dies mid-way leaves it so. Not changed (daemon-stops owns those loops). Keys never change on an erase.
- Readers of `HealthResult.sessions`: the CLI's first health line (now the owner's own, plus the new words), the
  cockpit's Systems "sessions · turns" (owner's own; a new "imported sessions" field beside it), and theseus-sim's
  lifecycle bench (`lifecycle.rs` 1253, 1618), which compares health's count across a restart on stores with no
  import, so unchanged. The TUI, herdr adapter and bench/ read none. `sessions` keeps its name and now means the
  owner's own; no reason found to keep the old meaning.

## Changed
- protocol: `import::HealthImported { sessions, erased }`, `HealthResult.imported` (`#[serde(default)]`), ts.rs's
  existing line; `cockpit/src/protocol.gen` regenerated; lib.rs ceiling 2715 -> 2719 in scripts/long-files.txt.
- core rpc/methods.rs: `session_totals` (own = keys less tags' `sessions`; imported = tags' sessions less erased;
  erased = tags' erased; the fallback counts the three from the records) and `imported_counts` (via `import::write::list`).
- CLI: `render/imported.rs` (`sessions_words`), one `mod` line in render.rs (stays at 3,100). Own count unformatted as
  before; imported/erased grouped (`21,151`). Cockpit: `lib/sessionwords.ts`, a field in Systems.tsx.
- `health_json` golden is unchanged: `--json` prints the daemon's value as sent, not a re-serialization.

## Proof
- `rpc/tests_health_imported.rs` (new): 4 live, imports of 300 and 200, one tag erased; at each stage own ==
  `live_sessions().len()`, imported/erased == tags', three sum to `session_count`; then the same through the fallback
  (a store whose terms an older writer left not whole: `terms_whole()` false, `totals` None). **Plant**
  (`sessions: n(0)`): fails "one import: the owner's sessions are the live ones, left 303 right 3". Restored, touched.
- Reads at health (`records_read_here`), at 21,151 imported sessions in two tags: **0 with no import, 1 at 1,000
  (one tag), 2 at 21,151 (two tags)**: one record a tag, none per session. Before: 0 reads and the wrong count.
- `render::imported::tests` (import + erase, import only, none, and "1234" unchanged) and golden
  `health_imported` (+ an assertion for no erase). **Plant** (import words dropped): the render test fails; restored.
- `npm test` in cockpit: 92 pass (incl. new `sessionwords.test.ts`); cockpit typecheck/build inside the gate.
- theseus-protocol: 31 pass; theseus: 169 pass.

## Live check (maintainer)
Scratch daemon with the stand-in model, fresh state dir: open two sessions; `theseus import openclaw <small synthetic
file>` (shape of import/tests.rs); `theseus import erase <tag>` on one tag of two. Then `theseus health` line 1 should
read `sessions 2 · imported N · erased M · turns …` and `theseus --json health | jq '.sessions,.imported'` the three;
the cockpit's Systems view shows "sessions · turns 2 · 0" and "imported sessions N · erased M". On the owner's
daemon after install: line 1 names the few hundred own sessions and the import's count apart.

## Left / uncertain
- Erase's lagging `erased` count (above). A fix is in write.rs's frame loop: not done.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, test build,
reader rule pass; suite fails only on the 33 known L1 tests (theseusd::sandbox, theseus-sandbox; the root VM, theseus-pv6i),
nothing else. `protocol types` phase run by hand: protocol.gen is committed and clean. Benches skipped (NO_BENCH).
`cargo deny fetch` was run in setup; the deny phase is part of the gate and did not fail before the suite.
