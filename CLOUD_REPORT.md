# CLOUD_REPORT.md: system-board (theseus-jn17m)

Branch `cloud/20261013-system-board`, from main at f8aec2d2 (store format 27, unchanged). Commits: 3362c346 (daemon side),
1d24eece (clients), and this report. Started 19:01 UTC, report at about 21:05 UTC.

## What I found

- `WorkKind::System` and the `system` root were declared in the work board's types and in the policy (`root_ref`) with no
  producer. No system job wrote a start or a progress row the board could read: `import.batch` had no batch number or total
  (the batches are one `import.episodes` call each, from a client that knows its run), an erase's receipt came only in its
  last frame, `import.topics`/`import.people` rows said what a frame held and not how far the run was, consolidation's
  `synthesis.proposed` rows said nothing of the run, and `index.rebuild` existed only on the tender's own socket, so the core
  wrote nothing about it.
- The brief's example `✗ the nightly import failed: …` needs a noun phrase; the board has one title per job, so the line
  reads from the title (see "Design choices").
- The TUI has no work tree at this base (`tui-rows` is not on main); `work-digest` is not on main either.

## What I changed

**3362c346, the daemon side** (protocol, core, generated TypeScript)
- `theseus-protocol`: `work_system.rs` (the root's id, `KEEP_MS` 1 h, `QUIET_MS` 5 min, `line`, `count_words`,
  `status_words`, `collapsed`); `notices.rs` rule 8's system arms (a failed job is an Inform with its reason, an ended one
  after `long_run_ms` an Inform, progress nothing; rule 3 no longer fires for a system job); `work.rs` (`attention_of`: a
  failed system job is `Idle`, never needs-you; `done · <what it did>`, `stopped · …`); `ImportEpisodesParams.batch/batches`
  (optional, absent from the bytes when unset); `method::INDEX_REBUILD`; ledger kinds `index.rebuild` and `import.erasing`;
  `WorkWatchParams.roots` documents `system`.
- `theseus-core`: `push/work/system.rs` (the jobs, from rows; the sweep; the view builders), wired into `push/work.rs`
  (`ROWS` gains `import.`, `synthesis.proposed`, `index.rebuild`; `Dirty.system`; `take_rows`; `apply` sweeps), `settle.rs`
  (builds system views; a system job's `progress` is its own; `Done` counts for a system job in the root's rollup),
  `place.rs` (`Scope::sees("system")` is false for a guarded scope whatever it names; a private scope has it on the whole
  board or by naming `system`), `rpc/work_board.rs` (`roots: ["system"]` for a private surface), `seed.rs` + `push.rs`
  (`read_system`, `warm_system`: after serving, the last hour's rows make the same jobs again, no notice).
  Rows, all in frames the jobs already wrote: `fact/import.rs` (`ImportBatch` carries `batch`, `batches`, `source`;
  `ImportErasing` on an erase's earlier frames; `done`/`of` on `ImportTopics`/`ImportPeople`), `import/write.rs`,
  `import/topics.rs`, `fact/synthesis.rs` + `consolidate/run.rs` (`run`, `n`, `of` on `synthesis.proposed`, through a
  `Mark` that replaces the `trigger` argument, so no `#[allow]` was added), `fact/index.rs` (`Rebuild`) and
  `rpc/index_rebuild.rs` (`index.rebuild` through the core, owner's act via `judge_act(Act::JudgeRun)`, `requested`,
  `progress` at each fifth of the tender's backfill, `finished`/`failed`; a task on tokio's timer reads the tender's status
  every 3 s while one is outstanding, at most 22 frames a rebuild).
- `cockpit/src/protocol.gen/` regenerated (`ImportEpisodesParams.ts`, `WorkWatchParams.ts`).
- AGENTS.md: core and protocol.

**1d24eece, the clients**
- `theseus work`: the root as `⚙ Theseus  (1 working, 2 done, 0 failed)` with its jobs under it, `◐ Importing Claude Code
  history · batch 41 of 120 · 3:12`, a failed one `✗`.
- `theseus status` (long form): `⚙ importing 41/120` after the time on its first line, from `work.watch` naming `system`
  alone. `--short`, `--json` and `--watch` are untouched.
- TUI: asks `work.watch {roots: ["system"]}` at each connect and after `events.lost`, keeps the views by the position rule,
  the header says the root closed (`⚙ importing 41/120`, else `⚙ Theseus · 2 done, 1 failed`), a system Inform is a footer
  line, never a ring. Change kept to the header and the connect effects (there is no tree to add a kind to).
- `theseus index rebuild` (in `client::OPERATORS`); the three importers number their batches (`import::Numbered`; the Claude
  Code and Codex importer's total is an estimate that grows with the bytes the episodes come to, and only the last batch
  says it is the last; the sync sends the batch number alone and `import.synced` ends it).
- AGENTS.md: CLI and TUI.

No new dependency (Cargo.lock untouched), no config key (so nothing in the template), **no store format bump**: the new
fields are keys of ledger rows' JSON data, not a stored record's layout, and `tests_layouts` is unaffected.

## How I proved it

- `cargo nextest run -p theseus-protocol`: 91 passed. New: `work_system::tests` (clock, count words, the status line and the
  closed root).
- New core tests, all passed: `push/work/tests_system.rs` (12: an import batch by batch and its end; a failed sync is one
  Inform with the reason `✗ Syncing OpenClaw history failed: the exporter's cursor is ahead of the file`, and its batches made
  none; a sync that failed at once still informs; a 6-minute import ends with an Inform and a viewer with sound still gets a
  badge; a rebuild's share and end; consolidation's clusters; an erase's progress and receipt; topics and people; a silent job
  cancelled at the quiet time and an old one leaving with the root; a shared place never sees the root; a private scope may
  ask for it alone; the last hour's rows after a restart, no notice), `import/tests_system.rs` (12 episodes in four batches,
  numbered or not: 4 frames each, the WAL's frame count equal, the rows carry the numbers), `tests_push/work.rs`
  (`an_import_shows_its_progress_on_work_changed_and_ends`: a fixture history over a real connection, `work.changed` says
  `batch 1 of 4` … `batch 3 of 4`, `done · 12 sessions`; a fresh owner's watch has the root, a binding's has none).
- CLI: `work::tests::the_theseus_root_lists_its_jobs_with_progress_and_time`, `status::tests::the_first_line_says_the_daemons_own_jobs_running`,
  `harness::tests` (the numbering and the estimate). TUI: `tests_system.rs` (2; 72 in all pass).
- **Planted reverts** (each restored and `touch`ed, `git status` clean, no `PLANTED` left):
  1. an extra `store.append` after the batch's frame (a frame per batch added): `import::tests_system::numbering_a_batch_writes_no_frame…`
     failed, `assertion failed: one frame a batch  left: (4, 8)  right: (4, 4)`.
  2. `Scope::sees` returning true for `system` (the root shown to a shared place): `push::work::tests_system::a_shared_place_never_sees_the_theseus_root`
     and `tests_push::work::an_import_shows_its_progress_on_work_changed_and_ends` (its binding assertion) failed.
- **Frames per import**: a 12-episode fixture in four batches writes 4 frames numbered and 4 unnumbered (the test above, and
  the live check: 200 sessions, 3 batches, 3 frames). Erase rows ride frames it already wrote, and the three existing erase
  frame tests passed unchanged (the first design put progress on `import.erased`, which those tests count; I moved it to a
  kind of its own, `import.erasing`, instead of editing them).
- **Board per-frame cost** (`theseus-sim bench lifecycle --phases push`, a debug build here, 10,000 frames, three runs each,
  base f8aec2d2 built in a worktree against the same target dir): 0 watchers p50 131.7 to 139.6 µs, p95 270 to 289 µs at the
  base; p50 131.4 to 136.6 µs, p95 278 to 304 µs here. 1 watcher p50 253 to 260 µs base, 239 to 247 µs here; 10 watchers p50
  249 to 258 µs base, 250 to 282 µs here. Within noise, as expected: a frame with no system row costs the three added
  prefix checks in `wanted` and an early return in `sweep`. Idle is unchanged: the only timer is the rebuild wait, and only
  while a rebuild is outstanding.

### Live check on this VM (a scratch daemon, a stand-in key, `[discord]` and `[web]` off)

200 generated Claude Code sessions (12 MB), then `theseus index rebuild`; `theseus work --watch` and a poll of `theseus status`:

```
$ theseus work --watch
nothing is going on
watching the work board: 0 of 0 roots at position 0 (Ctrl-C to stop)
@1788 system system ready  ⚙ Theseus  (1 working, 0 done, 0 failed)
@1788 system 026729 running  ◐ Importing Claude Code history · batch 1 of 3 · 0:00
@3540 system 026729 running  ◐ Importing Claude Code history · batch 2 of 3 · 0:02
@5017 system system ready  ⚙ Theseus  (0 working, 1 done, 0 failed)
@5017 system 026729 running → done  ✓ Importing Claude Code history · done · 200 sessions · 0:03
@5018 system system ready  ⚙ Theseus  (1 working, 1 done, 0 failed)
@5018 system 032778 running  ◐ Rebuilding the index · 0:00
@5019 system 032778 running  ◐ Rebuilding the index · 35% · 0:03
@5020 system system ready  ⚙ Theseus  (0 working, 2 done, 0 failed)
@5020 system 032778 running → done  ✓ Rebuilding the index · done · 0:06
$ theseus status          (polled twice a second during the import)
theseus 20:50 · ⚙ importing 1/3 · nothing needs you or works
theseus 20:50 · ⚙ importing 2/3 · nothing needs you or works
$ theseus status          (polled during the rebuild, then after it)
theseus 20:50 · ⚙ rebuilding · nothing needs you or works
theseus 20:50 · ⚙ rebuilding 35% · nothing needs you or works
theseus 20:50 · nothing needs you or works
$ theseus work            (afterwards)
⚙ Theseus  (0 working, 2 done, 0 failed)
  ✓ Importing Claude Code history · done · 200 sessions · 0:03
  ✓ Rebuilding the index · done · 0:06
```
The import wrote `4800 nodes in 3 frames`. The rebuild ended within the 3 s status read of finishing.

### The live check the maintainer should run (the owner's keys are not needed)

```
D=$(mktemp -d); printf 'k' > $D/key; chmod 600 $D/key
printf '[secrets]\nanthropic_api_key = "file:'$D'/key"\n[discord]\nenabled = false\n[web]\nenabled = false\n' > $D/c.toml
theseusd --config $D/c.toml --socket $D/sock --state-dir $D/state &
theseus --socket $D/sock work --watch &            # the Theseus root, then each job as below
theseus --socket $D/sock import claude-code <a dir of a few hundred sessions> --tag t
theseus --socket $D/sock status                     # first line: ⚙ importing N/M while it runs
theseus --socket $D/sock index rebuild              # ⚙ rebuilding, with a percent, then done
theseus --socket $D/sock import sync --dry-run      # (OpenClaw's stores) its job ends at its record; break the exporter to see ✗ … failed: <reason>
theseus --socket $D/sock shutdown
```
On the operator's own store the nightly sync, a consolidation (`theseus memory consolidate`) and an erase
(`theseus import erase --tag t`) show the same way; then restart the scratch daemon and `theseus work` again: the hour's jobs
come back (`warm_system`).

## The gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, `CARGO_INCREMENTAL=0`, `THESEUS_KEEL_BASE=f8aec2d2`)

Passed: door (via the base override), keel (`keel: ok (f8aec2d2..the working tree, 61 files changed, 0 findings acked)`),
keel tests, fmt, shape, features, clippy (`-D warnings`, after two fixes: a function over 100 lines, `checked_div`,
`unused_async`, a needless lifetime), cockpit, test build, reader rule, protocol types (`git diff` of
`cockpit/src/protocol.gen` empty). The suite ran 4541 tests: 4505 passed, 36 failed (none new):
- 33 are the L1 tests the brief names (theseus-sandbox `bench::spawn_100` and 19 `contract::*`; theseusd `sandbox::*`, 13).
- `theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends`
  (failed in two full runs, passes alone): its `numbers(mid)` read 948 lines where it expected `mid.rs`'s 600, i.e. the
  first result was `big.rs`'s: the three calls are reads and run together (parallel-calls), so under load the results are
  written in the other order. A race in the test's ordering, not in this work (nothing here touches `fs.read` or `results`).
- `theseus-core tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory` and
  `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`: not in the brief's list; both pass alone
  (re-run together with the first: 3 passed in 1.2 s). Load-dependent; I did not find their cause (neither touches the board,
  the import, the CLI or the TUI).

Not run or not green: `cargo deny` bans, licenses and sources are ok; **advisories fail** on RUSTSEC-2026-0346, -0347 and
-0348 (rustls-family findings in the advisory database fetched today); Cargo.lock is unchanged by this branch, so they
are not from it. The benches (`lifecycle`, `jobs`, `turn`, `size`) did not run (`THESEUS_GATE_NO_BENCH=1`).

`scripts/gate.sh` run as the plain command stops in the keel with findings that are not this branch's, because the local
`main` here is older than the branch's base: `merge-base(HEAD, main)` takes in main's later commits, among them a
rewritten test, a raised seed budget, and an ack the VM cannot verify (no signer key). `THESEUS_KEEL_BASE=f8aec2d2` gives
the true range and 0 findings.

**Keel findings expected:** none. I raised no cap, ceiling or budget, deleted and ignored no test, added no `#[allow]`.

## Design choices the owner should hear about

1. **The failure line says the job's title**, not "the nightly import": `✗ Syncing OpenClaw history failed: <reason>`. The
   sync's job is titled by its source (`openclaw-sync`), whoever runs it. A different noun for the line needs a second
   string on the view.
2. **A job whose rows stop is "cancelled" after 5 minutes (a rebuild 30), shown `stopped · at batch 41 of 120`**, at the next
   frame: the board has no timer. This is how a `theseus import openclaw FILE` ends (its streaming reader cannot know a full
   batch is the last before it reads the end of the file), how a consolidation that stopped at the day's limit ends, and how a
   client that quit ends. A job that was running when the daemon stopped reads the same after a restart.
3. **A consolidation that errors, or a dry run, writes no row**, so it shows nothing; the failure rows the brief asks for exist
   for the sync (`import.synced`'s error) and the rebuild (`failed`), not for an importer that dies on the client (its error is
   the CLI's).
4. **`import.erased` stays the receipt** (what counts erases counts it); progress is `import.erasing`. Two new ledger kinds
   in all: `import.erasing` and `index.rebuild`.
5. **`roots: ["system"]`** is how a private surface asks for the daemon's work alone (the TUI, `status`): it is not a
   session id, and `work_scope` special-cases it for an unguarded connection only.
6. **`theseus index rebuild` through the core is the only rebuild that shows**: `theseus-index rebuild` on the tender's own
   socket still works and writes no row. A tender restart's automatic rebuild (its cursor missing, the WAL rewound) is not
   shown either.

## Left, and joins

- `work-digest`: not on main at this base. System Informs are in the board's notice ring (`notices_since`) like any
  other; the nightly import that failed while nobody looked will be in the digest with no change, but `warm_system` makes no
  notice after a restart, so an Inform from before a restart is not in the ring.
- `tui-rows`: the TUI here has no work tree, so the "root collapsed with its count" is the header text
  (`work_system::collapsed`). When the tree lands, the kind match for `WorkKind::System` and that function are the join.
- `horizon-board`: additive. It adds kinds to the same enum; `bump` in `settle.rs` counts a `Done` system job in the
  root's `done` beside `Task`, and `is_active`/`open_below` special-case `System` (an ended child stays visible there for
  its hour).
- `theseus status --short` and `--watch` do not say the daemon's jobs; only the long form's first line does.
- The estimate of a harness import's total can be low or high; the board shows `max(estimate, batch)` and the job ends at
  the last batch whatever it was.
- A full disk stopped the first gate run mid test-build (`No space left on device`, 19 GB of `target/debug/incremental`);
  deleting the incremental directory freed it, and the rest ran with `CARGO_INCREMENTAL=0`. Say so if a later session meets it.
