# CLOUD_REPORT: p3b-human-time (theseus-hnm37)

Branch `cloud/20261024-p3b-human-time`, base `ac298666`. Commits: cockpit `0c040bc7`, Rust `2b25c116`.

## 1. For the maintainer
- Store format untouched (51). No ceiling raised. No OWNER CALL, no STOP.
- Disk: this VM ran out of space mid-run; I deleted `target/debug/incremental` and built with `CARGO_PROFILE_DEV_DEBUG=0`.
- **Not run:** the full `scripts/gate.sh`, `cargo deny`, the workspace-wide `cargo build --all-targets` (only the three crates below were built and tested), and the live check. Mentioned plainly: the gate is NOT proven green.

## 2. The helper
Cockpit `cockpit/src/lib/figures.ts` (re-exported by `format.ts`):
- `span(ms): string`, `ago(at, now?)`, `until(at, now?)` (`in …`; overdue `due 3m ago`, `due now` under 5 s), `when(at, now?)` (`20:00`, `tomorrow 08:00`, `yesterday 17:10`, `Mon 09:00` within 6 days, `Oct 30 09:00`; fixed English day/month names).
- `ms()` keeps latency style under 1 h, then uses `span`. `uptime()` uses `span` from 1 d.
- Rule as built: <1 m `40s`; <1 h `12m`; <1 d `2h 46m` (`2h`); <7 d `1d 21h` (`1d`); else `9d`. `ago` is `now` under 5 s.

Rust `crates/theseus-protocol/src/human_time.rs` (`pub mod human_time;` in lib.rs): `span(ms: u64)`, `ago(now_ms, then_ms)`, `until(due_ms, now_ms)`; same rule, 3 tests.

## 3. Sites
Moved to the helper: Queue.tsx Coming up (via new `comingTimes` in ops/facts.ts), facts.ts due line, RoutinesTable, Actions deadline, agenda `wakeFacts` (Wakes.tsx passes `span`), EventDetail reminder text, threadboard `span`, ship/watch.ts `span` (re-export), ship/words.ts `span` (≥1 min), taskgraph claim, books episode span, durability minutes, agendawords (seconds dropped via `.slice(0,5)`).
Kept: `clock()` and `stamp()` (log-like and "as of" callers, mixed), Transcript/ModelInspector/Ledger/SessionDeck times (log-like), Economics `bucketWords` (chart bucket axis), `progressLine.left` (mirrors Rust progress_line.rs text), `bench.ts` secs and `inspect.ts waitWords` (latency/wait figures), `Heartbeat`/others not touched.
Rust moved: CLI `until_due` (render.rs), `render/aws.rs` durability lines, herdr `span`, TUI banner `ago`.
Left (may reach Discord or other surfaces): `wake.rs span`, `notices.rs`, `work/horizon.rs`, `narrative.rs`, `render/wakes.rs span_of`, `render/postmortem.rs span`, `routine.rs`, TUI `live.rs`, `card.rs`, `thread_ask.rs`, `render/memory.rs`, aws.rs account-age lines. Follow-up.

## 4. Tests
- New `cockpit/test/figures.test.ts` (boundaries, each future and past, `when`, Coming up 166 min = `in 2h 46m`, source check that Queue.tsx has no inline minutes). `npm test`: 736 pass, 0 fail. `npm run lint`: warnings only (pre-existing kinds); `npm run build`: ok.
- P1 (old `ago()` planted): 18 fail incl. figures test. P2 (inline `…m` planted in Coming up): 1 fail. Both restored and touched.
- Old expectations updated: books (`40m`, `3h`, `3d`), durability (`3m ago`, `2m ago`), ops-board (`due HH:MM, in 2h`), since (`3h`, `2h`, `2h 30m ago`), watch (`1h 5m`, `6h`), words (`3m 12s`, `2h 5m`).

## 5. Part 2
Done as in §3. Tests updated: herdr_slots (`41s`, `4m`, `1h 5m`), tests/herdr.rs 1257 (`job · 4m`), TUI banner (`now`, `30s ago`, `5m ago`, `3h ago`, `1d 6h ago`, `5d ago`), aws.rs durability goldens (`3m ago`, `2m ago`). No golden files moved.

## 6. Gate
`cargo nextest run -p theseus-protocol -p theseus -p theseus-tui` (TZ=America/Phoenix): 1336 passed, 1 skipped. `cargo fmt --check` clean. `cargo clippy ... --all-targets -D warnings` clean on the three crates. `scripts/shape.sh`: ok. cockpit lint/test/build as above. Not run: gate.sh, deny, theseus-core tests (not touched).

## 7. Keel
`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`: no output, exit 0.

## 8. DM golden
`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0 (identical). Discord and core crates do not reference the moved helpers.

## 9. Join notes
Files: cockpit lib/figures.ts, agendawords.ts, books.ts, durability.ts, taskgraph.ts, threadboard.ts; ops/Queue.tsx, facts.ts, EventDetail.tsx; components/RoutinesTable.tsx, Wakes.tsx; views/Actions.tsx; ship/watch.ts, words.ts; six existing tests and the new figures test; Rust files in §3 plus protocol lib.rs (one `mod` line). None of the other rows' files (People, OntologyParts, Heartbeat spinner, Context Timeline, explorer monthBins) touched.

## 10. Follow-ups
Remaining Rust helpers in §3 "Left"; cockpit `stamp()` split into schedule-like and log-like; thread a `now` into `markLine` to use `when()`; run the full gate and a live look at Ops "Coming up".
