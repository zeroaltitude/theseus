# CLOUD_REPORT: p3b-busy-wheel (theseus-6o8q8)

Branch `cloud/20261024-p3b-busy-wheel`, from `ac298666`. Commits: `b28031c7` (cockpit), `4b700dd6` (daemon).

## 1. For the maintainer
- Store format untouched (51). No ceiling raised: `rpc/people.rs` 456 -> 427 lines, `rpc/proposals.rs` 838 -> 739 (accept code moved into the new `rpc/proposals/accept.rs`, 336 lines). No new dependency. No OWNER CALL, no STOP.
- Not done: no live check on a scratch daemon, and I did not load the Ontology view in a browser (see 7).

## 2. Why accepts are slow
Measured case: `accept_all` of 12 topic proposals (frames counted with `store.stats().frames_appended`; the test uses 12 sessions, not 50, because each needs a real judged exchange; the cost is linear).
- Base: 12 frames, so 12 fsyncs, 12 whole-ontology clones, 12 session reads, plus ~2-3 announcements each. Shown by planting P2 (per-proposal write restored): `left: 12, right: 1`.
- One ledger scan (`listed_now`) per `accept_all` call at the base too. The "scan per row" cost was the cockpit sending one `accept_all` per picked person row (K rows = K scans, K judge_act checks) plus one for topics, awaited in series.
- After: 1 frame, 1 clone, 1 scan for the whole call. Cockpit: picked rows that need no `as_person` go in the topics call; rows with an `as_person` need their own call (`as_person` is one value per call); all rejects are one `reject_all`. So 10 plain person rows + 50 topics = 1 RPC (was 11).
- Remaining: `reject_all` still writes one frame per proposal (`reject_open`), unchanged. Follow-up (see 11).

## 3. The fix
- Daemon (done, existing frame kinds only, no layout change): `finish_bulk` plans each accept against a working clone of the ontology (so a person made by an earlier proposal is seen by later ones, and a session's member list accumulates), then writes records + rows in one frame with `Board::stage`, then announces per proposal as before. Per-proposal label rows, membership rows, announcements and error text unchanged. One that cannot be planned is left with why ("`<judgment>: <error>`") and the rest go on. A "needs a name" proposal is left first (its `left` line now comes before the batch's `left` lines; `accepted` order unchanged). `accept_open` (single accept) uses the same plan and still writes its own frame (frame bytes unchanged).
- Cockpit: `acceptCalls` / `rejectCalls` in `lib/people.ts`.

## 4. The wheel
`components/ShipsWheel.tsx`: props `size?: 'sm'|'md'` (12 / 16 px), `label?` (default "working"), `className?`. `role="status" aria-busy="true" aria-label`; rim, hub, 8 spokes with handles past the rim; `currentColor`. CSS `.ships-wheel` (2.4 s linear) in `index.css`; stopped, still drawn, under `html.calm` and `prefers-reduced-motion` (same pattern as the other animations). It deliberately keeps turning when the page is not "stirred" (it means work in flight).
Moved onto it: `Act` (OntologyParts), `Btn` (ui.tsx), `ActBtn` (ops), Revoke (Extensions, StandingRecords), Halt and Revert (SelfChanges), make live (Systems), Heartbeat's refresh icon (the only `animate-spin`; the icon shows when idle).
Kept as is: `Shell.tsx` :197/:221 and `TimeMachine.tsx` :261 (live / "approvals waiting" pulses), `Transcript.tsx` :470 (streaming cursor). The `…` strings in titles and placeholders mean "not loaded yet" (value unknown), not in-flight work; left.

## 5. The lock
`lib/filing.ts` (pure): `begin/advance/end/pickOff/filingWords`. While filing: both checkbox kinds and "select all" are disabled (`pickOff(progress, own)`), kind filters stay live, person ✓/✗ were already disabled by `busy`. A line after `<Refused>` shows the md wheel and `filing N of M`. M = proposals; N advances per call by that call's proposals (the daemon gives no progress inside a call, so with batching N jumps by a whole call). Refusal: lock clears in `finally`, the refusal words still show; picks clear only on success as before.

## 6. Tests and plants
- `cockpit/test/filing.test.ts` (5 tests): busy disables every pick control; progress 0..M and capped; end clears; the panel source wires both checkboxes and select-all to the lock; call batching. `npm test`: 734 pass, 0 fail.
- Daemon: `tests_accept_batch::an_accept_all_of_many_topics_is_one_frame_and_each_still_lands`: 12 accepts are 1 frame, each session holds the topic, nothing left to propose, a repeat accepts none.
- P1 (`pickOff` ignores filing): `not ok ... every pick control is off` red; restored, touched, green.
- P2 (finish_bulk loops `accept_open`): `one frame for 12 accepts, left: 12, right: 1`; restored, touched.
- Existing proposals/people tests all pass (80 selected earlier; whole theseus-core suite below).

## 7. Gate
- Cockpit: `npm ci`, `npm run lint` (pre-existing warnings only), `npm test` 734/0, `npm run build` ok.
- Rust: `cargo fmt --check` ok; `cargo clippy -p theseus-core --all-targets -D warnings` clean; `scripts/shape.sh` ok; `TZ=America/Phoenix cargo nextest run -p theseus-core --no-fail-fast`: 4494 passed, 0 failed, 25 skipped.
- `scripts/gate.sh` itself fails in the door/keel phases on this tree (it judges against current main's gate: erosions in `persona_cmd.rs` tests and keel-guard edits, from commits on main this branch does not contain; unsigned acks). Not my changes; I ran the individual phases instead. Bench not run. The rest of the workspace (other crates) was not re-run; the only other crate touched is none.
- Not done: no scratch daemon, no browser check of the Ontology view or the spinner sites (a build succeeded, nothing was loaded). I hit a full disk once (deleted `target/debug/incremental`) and reran the suite.

## 8. Keel
`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`: no output, exit 0. No test removed or weakened.

## 9. DM golden
`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt`: no output, exit 0.

## 10. Join notes
Files: `cockpit/src/components/{ShipsWheel(new),OntologyParts,People,ui,Extensions,StandingRecords,SelfChanges,Heartbeat}.tsx`, `cockpit/src/ops/ActBtn.tsx`, `cockpit/src/views/Systems.tsx`, `cockpit/src/lib/{filing(new),people}.ts`, `cockpit/src/index.css`, `cockpit/test/filing.test.ts`; `crates/theseus-core/src/{lib.rs (+mod line), rpc/people.rs, rpc/proposals.rs, rpc/proposals/accept.rs (new), tests_accept_batch.rs (new)}`.
`People.tsx` (onto lane edits it too): imports (line 11 names and two new import lines after `OntologyParts`); `ProposalsPanel`: hook destructure adds `progress` (:127); `answer()` body :142-150 now builds `calls` from `acceptCalls`/`rejectCalls` and passes weights to `writeEach` (only the `calls` construction and the `writeEach` call changed); "select all" button gets `disabled={pickOff(progress)}` (:161); one new progress `<div>` right after `<Refused words={refused} />` (:165, after the second `<Refused>`); both checkboxes' `disabled=` (:172, :185). Person-row buttons unchanged.
`OntologyParts.tsx`: hook adds `progress` state, a third optional `weights` parameter to `writeEach`, `progress` in the return (existing names kept); `Act` shows `<ShipsWheel />` beside its label while busy (:59-65). Note: `Act` no longer replaces the label with `…`.

## 11. Follow-ups
- `reject_all` batching (one frame for many labels): same shape as accept, labels only.
- A progress notification from the daemon would let N advance inside one call.
- `scripts/gate.sh` needs main merged in to run its door phase here.
