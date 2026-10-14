# CLOUD_REPORT: work-digest (theseus-qkoi3)

Branch `cloud/20261013-work-digest`, work commit `dc42b60`.

## What I found
- `WorkBoard::notices_since` and the 4,096 ring were there. A fresh daemon seeds the board at position 0, so the ring is complete from the start; the ledger is read after a restart or an eviction.
- The policy gives a cancelled task a "reported: cancelled" notice, a wake two notices (its conversation's and its own), and every job's end one. The digest folds these: a cancel is `cancelled` (from the board's views), a wake is told once, a job is told only when it failed.
- The `/digest` command, the welcome-back and the TUI/CLI/cockpit readers are all built. `runtime.rs` was at 3,497/3,500: Discord's hooks add 2 lines net (typing goes through `Place::typed`, controls through `read_only`).

## What I changed (all in `dc42b60`)
- **Protocol** `work_digest.rs`: `work.digest` params/result, `DigestLine`, `digest_words`, `fold` (3+ of a kind under one root within 10 min), `summary`. TS generated.
- **Core** `rpc/digest.rs`, `push/work/digest.rs`: ring when complete, else ledger by kind (newest 2,000, blocking pool); `from_ledger` says which; `roots` scopes (a task's own timeline); place rule (guarded callers name roots, shared places only, scrubbed, no answers; the in-process Discord binding may also name a private place such as its DM). No frame, no store change.
- **CLI** `digest.rs`, `seen.rs` (`Looked`, `record_looked`), `status.rs` (one line). **TUI**: look moves while focused; digest at connect and on refocus after 60 s. **Cockpit**: `sincedigest.ts` + Watch.tsx; the time machine/replay still fold the ledger (`plateOf`). **Discord**: `runtime/digest.rs`.
- AGENTS.md of core, CLI, TUI, Discord, cockpit. `voice.rs` command-count test rewritten for 17 commands.

## How proved
- Core `tests_work_digest.rs` (4): ring and ledger agree on the board's scenario (kind/work/count equal); window before the ring starts reads the ledger and says so; `roots` = one task; shared place sees only its roots, scrubbed. Protocol (5), CLI unit (3) + `tests/digest.rs` (4), seen (2), TUI (4), Discord (4), cockpit `sincedigest.test.ts` (5, one over a stream recorded from a real daemon).
- **Planted reverts**: ledger fallback skipped (`from_ledger = ledger_only`) failed `a_window_past_the_rings_start...` and `a_shared_place...`; place filter skipped (`_ => true`) failed `a_shared_place_sees_only_its_roots_scrubbed`. Both restored and touched.
- **Live check**: `theseusd/tests/digest.rs` runs a real daemon with the stand-in model: 3 tasks report, 1 cancelled, 1 answered, 1 timer; digest from the earlier look tells each once; after a SIGKILL restart the ledger path gives the same counts; the real `theseus digest --since 10m` prints them. **One task failing is not in it** (the stand-in cannot fail a task); failures are covered by the ledger/ring mapping only.
- Full suite: 4,505 passed, 35 failed: 33 the known L1/sandbox tests, `the_substrate_scenario...` (passes alone; load) and the voice count test (fixed after). clippy, fmt, shape, cockpit lint/test/build clean.

## Not done / uncertain
- **Gate**: `scripts/gate.sh` stops in `keel` on a finding from the base, not mine: `crates/theseus-sim/src/lifecycle.rs:155` budget-raised, whose ack signature cannot verify here. I ran the later phases by hand. `cargo deny` could not run (offline, crates missing); `features` and `size` not run; benches not run.
- `theseus digest` under 10 ms: not measured.
- No unprompted post exists, so no pacer was needed (the welcome answers the owner's typing). Its "last sight" is in memory: no welcome after a restart until a second typing.
- Cockpit plate keeps sessions/spend cells from the fold (the digest has no such counts).
- Keel findings expected: none of mine.
- Docs to change: spec Part III item, status.md, and protocol method list for `work.digest`.
