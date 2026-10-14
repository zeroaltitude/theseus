# CLOUD_REPORT: self-backlog (theseus-pw1q.7)

Branch `cloud/20261013-self-backlog`. Commits: 50180de (the feature), eed573d (tests made independent of the host's disk).

## What I found
- Rows are dated in the ledger index by the frame's write time, not `at_unix_ms`. A test that seeds rows must run the night at a clock read after the seeding.
- Health warns on a nearly full disk, so a seeded-day test saw one more candidate on this VM when its disk was 98% full. Fixed in eed573d (`disk_warn_mb = 0` in the tests' config).
- Started items are not closed by `task_graph::closed_by_report` (it keys on task sessions). The night closes them itself (`start.rs::backlog_sweep`, from the execution's end).

## What I changed (50180de, eed573d)
- `rsi/backlog.rs` and `rsi/backlog/{read,rank,keel,state,start,word,tender}.rs`; facts `fact/rsi_backlog.rs`; ledger kinds `self.backlog.filed|ranked|started|dropped` (the dropped row is new in SELF_ROWS); `[self] backlog_starts = 1`, `backlog_max_open = 20` (template and test); `Act::SelfBacklog` (owner_anywhere, refusal ledgered); methods `self.backlog`, `self.drop`, `self.move`; CLI `theseus self backlog [--all] | drop | move` (drop and move in `OPERATORS`); the tender is started from theseusd's main after consolidation's, only with mode act.
- Items are tasks under the parent "Self-improvement backlog", `principal = "self"`, `by_model`, state `proposed` with no session (never run by anything), or `waiting_human` for a keel class. Per-item cause, counts, score, rank, pin and the dropped causes are one META record `self.backlog`: **no store format bump**.
- Score is documented in `rank.rs`; no model call. The optional nightly phrasing call is not made (it would be the first spend outside a turn; `rsi::budget::reserve` is self-budget's).
- Starting: `open_session_as` under `Authority{principal:"self"}` (made `pub(crate)`), one frame per start, a DM line through the digest's path, then the tender runs the first turn. The objective names `self.propose_change { item, issue, why }`. Tests stop at the session; the turn is not run in tests.
- Doing more: none beyond the above.

## How I proved it
- `cargo test -p theseus-core --lib -- tests_self_backlog`: 8 pass (seeded day files and dedups with evidence; same cause tomorrow adds evidence, files nothing; rank deterministic over reordered input; keel item never starts and starts are capped, principal is `self`; dropped never returns; non-owner/job drop and move refused with `approval.refused` rows; move pins and survives a night, size cap; mode off or halted writes not one frame). Plus unit tests in `rsi::backlog::*`, `config::rsi`, CLI `self_cmd::backlog`.
- Planted reverts (each restored and touched): the open-cause lookup returning none: `the_same_cause_tomorrow_adds_evidence_and_files_nothing_new` fails; WaitingHuman allowed to start: `a_keel_marked_item_never_starts_and_the_starts_are_capped` fails; drops forgotten: `a_dropped_item_never_comes_back` fails.
- Under load: not run.
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run 4, on the code of 50180de): door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule pass. Suite: 4350 run, 4316 pass, 34 fail: the 33 known L1 failures (theseus-sandbox contract and `bench spawn_100`, theseusd `sandbox`) and `theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends`, which passes alone (rerun 1/1 pass; not in the known list; the disk was near full at the time, so I take it as load/disk, unexplained otherwise). Protocol types (`cargo test -p theseus-protocol`, 7 binaries pass) and `cargo deny check` pass. The later benches were skipped (NO_BENCH). After eed573d (tests only) I ran fmt, clippy on theseus-core and the eight tests, not the whole gate again.
- Earlier gate runs failed in clippy (fixed) and once on "no space left on device" (I deleted `target/debug/incremental` in the repo).

## Live check for the maintainer
1. Scratch daemon with `[self] mode = "act"`, `theseus self resume`, a few failing turns or invalid tool inputs in the ledger; call `backlog_night` is not an RPC: wait for the tender (consolidate_hour + 1) or restart past that hour: `theseus self backlog` should list the items with evidence; `theseus self log` shows `self.backlog.filed|ranked|started`.
2. `theseus self move <id> 1`, then `theseus self drop <id>`: the list shows the pin, then the item gone and its cause under "Dropped".
3. From a job's shell or a non-owner Discord account: `drop`/`move` refused, `approval.refused` rows.
4. With mode off the daemon must show no `self.backlog.*` row and no `self.backlog` META.

## Left open / for the owner
- The keel "owner card" is a status (`waiting_human`) plus a DM line; no approval card or verb to release a keel item exists yet (it is not in the task's verbs). The join with self-pr should decide how an owner's yes starts one.
- No RPC to run a night on demand; `backlog_night` is tested directly.
- Budgets assumed for "slow turn": 120 s, first token 10 s (`read.rs`); the owner may want the spec's numbers.
- A started item whose first turn fails stays `in_progress` until its execution is seen terminal at a later night.
- Health warnings used: disk, failed MCP servers, import sync warn only.
- Docs to update at review: spec Part III item, status.md; `crates/theseus-core/AGENTS.md` already has a paragraph.

## Keel findings expected
None. `python3 scripts/keel-guard.py`: `keel: ok (... 37 files changed, 0 findings acked)`.
