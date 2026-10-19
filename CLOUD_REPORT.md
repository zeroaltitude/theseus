# CLOUD_REPORT: speed-send-path (theseus-nsp9l.2)

Branch `cloud/20261019-speed-send-path`, base `8b48a446` (main `bbc65530`). A cloud VM (4 cores, root, no neighbour IO):
its numbers prove a direction, not the target.

## Step 1 and 2: where the time goes

**Measured (T2, `bench/h2h/run.py --theseus-only --rows T2`, release-thin, medians in ms, 60 runs each, interleaved in
three rounds of 20):**

| Build | T2 interactive | T2 one-shot |
|---|---|---|
| `026dce3f` (Oct 12) | 8.6 | 10.4 |
| base `8b48a446` | 11.4 | 13.6 |
| head `56cbf47e` | 10.6 | 13.8 |

An earlier 10-run pass gave old 7.6 / 9.8 and base 10.5 / 12.7. This VM shows the regression at about +3 ms, not the
owner's +25 ms, and the wall-clock gain from the fix is inside the noise (about 1 ms interactive, none one-shot). I
did **not** reproduce the owner's disk-bound doubling under neighbour IO: fdatasync costs 0.2 ms here.

**Syncs and reads before the request leaves** (`strace -f -ttt -y`, the daemon on a scratch state dir, the stand-in
model, a warm `theseus ask`): both builds do exactly **2 `fdatasync`** before the request (the input's frame with its
admission, then the call's plan frame) and **no** other file I/O (no `openat`/`statx`; the WAL `pread64`s are the
transcript's kept reads, 24 at base, 59 at old). So the regression is not a sync, a store scan or a file read in this
configuration. What differs is memory: in the window from the ask to the request, `mprotect` calls (glibc growing
a thread arena a page at a time) were **102 at `026dce3f`, 343 at base, 75 at head**, and under strace the request left at
+28.9 ms (old), +46.6 ms (base), +26.9 ms (head). Hypothesis for the owner's number (unproven here): a page fault or
`mprotect` costs several times more on WSL2 than on this VM, and under neighbour IO the allocator contends for the
same pages, which would fit "grew by 25 ms, doubled under IO". The maintainer's live check below is what settles it.

**Cause (temporary marks, not committed, release-thin, under strace so absolute ms are inflated about 4x; the shares are
the point).** Pre-request work on a plain turn at base, in order: input frame sync ~2, `request_spec` 5.8
(`ToolRuntime::definitions_for` rebuilt every tool's JSON, 100 KB: **13 %**), `compile()` 24.4 in all, of which
`tools_defer::forks` **14.0** (two spec clones and two `resolve`s, each a deep copy of the whole catalog),
`manifest_for` **4.4** per call, three calls a compile (an append's, the recompile's, the clearing pass's `base()`),
and the report phase 5.5 (cache layout, request digest, JSON sizes), then the plan frame sync ~2.5. Every one of these
cloned or serialized `spec.tools` again: `Value::Array(spec.tools.clone())` plus `digest_json` in `manifest_for`, the same
in `cache_layout`, `serde_json::to_string(&spec.tools)` twice in the prefix library's pick (`lower_of`, every compile),
`tooled`'s `into_owned()` then `resolve`'s per-tool clone. `026dce3f` had none of it (no deferred tools: the tools were
sent whole, and `manifest_for` ran once). Cause, in one line: **the deferred-tools machinery (theseus-7m7w.2), the
prefix library (rr15) and a larger catalog made one 100 KB `Vec<Value>` get copied and re-hashed about a dozen times
per compile.** Shares: catalog build ~13 %, `forks` ~31 %, `manifest_for` ×3 ~30 %, report/digests ~12 %.

After the fix (same marks, no strace, head): `definitions_for` 5 µs, `forks` 15 µs, `manifest_for` 41 µs, report 0.72 ms;
the whole path from `turn.submit` to the provider call is ~4.5 ms, of which the two syncs are ~2.2 ms.

**Candidates from the brief, checked and found small in this configuration (marks, head build, no strace):**
inbound point (persona engage, thread router, surface tag) 0.04 ms; `defer_tools` 0.19 ms; the recall step 0.3 ms
(index disabled, as in the bench config); tail blocks (task view, standing-here, persona line, project slice) 0.1 ms;
sticky effort, steer, checkpoint: not on a plain first loop. **Not exercised:** the bench config has the judge off, no
index, no personas, no threads, no MCP. The owner's own config (Jev's route wait beside the compile, a live index, many
personas/threads) was not measured: work on that path may still exist.

## Step 3: the fix (commit 56cbf47e)

Move nothing, drop nothing: share what was copied. `RequestSpec.tools` is now `tool_set::Tools`, an `Arc`'d immutable
catalog that keeps its JSON length, its digest, and the hash state after its JSON (so the prefix library's digest is
byte for byte the old one), plus the sets a deferral loads (`resolved`, at most 4) and a rule's reduced sets (`reduced`,
for the thread tools). `ToolRuntime::definitions_for` (moved to `toolrun/catalogs.rs`; `toolrun.rs` shrank by 39 lines) keeps
each place's catalog keyed by the place class, the ceiling (leaked once, so its address names it), the MCP and flow lists
it was made from (shared `Arc`s a change swaps whole; kept alive, compared by address), the public roots, the wait cap
and the registry's size. A new set is a new `Tools`, so nothing kept goes stale. Digests and lengths equal the old
values: `compiler::tests::the_tools_digest_matches_the_one_stored_manifests_carry` (a literal) still passes, and
`tool_set::tests::what_is_kept_equals_what_a_fresh_walk_gives` checks all three against a fresh walk.
No behaviour, row or frame changed; no store format change; no new dependency; `Cargo.lock` untouched.

### Tests, and the planted reverts

- `tests_send_path::a_plain_turn_after_the_first_makes_no_tool_catalog_and_walks_no_tool_json` (no timing: the catalogs
  made, and tool JSON walked, across a second plain turn are both 0; the two requests' tools are byte for byte equal).
  Planted revert (the catalog cache never hits): this test failed ("the second turn made a catalog again") and the next.
- `tests_send_path::a_catalog_is_kept_per_place_and_made_again_when_what_it_is_made_from_changes` (invalidation: a
  class, the wait cap and the public roots each make a new catalog, and the old one is still there). Same plant: failed.
- `tool_set::tests::a_deferral_is_resolved_once_and_the_oldest_is_dropped_past_the_limit`,
  `a_clone_shares_its_definitions_and_a_new_set_starts_fresh`. Planted revert (`resolved` never returns the kept set):
  this test failed, and so did the turn test above (tool JSON walked again on the second turn).
  Each plant was restored, the file `touch`ed, and `git status` checked.
- The existing frame tests pass: `tests_m3::a_plain_turn_stays_within_its_frame_budget` (5 frames; ≤ 4 after the
  first), and `theseus-sim bench turn --check` (frames budget and first-request budgets ok; the harness overhead of a
  plain turn p50 27.0 ms in a debug build here).
- `tests_first_request` passes (first-request tokens cli 7,332, dm 9,411, task 6,585 at the p95).
- A known counter note: `tool_set::walks()` is a process-wide counter, which is exact under nextest (a process per
  test, the gate's runner) and could count another test's walks under plain `cargo test`.

## The live check for the maintainer (the owner's quiet machine, then under IO)

```
scripts/build.sh --profile release-thin
git worktree add /tmp/h2h-old 026dce3f   # and the base 8b48a446 the same way; build each with its own CARGO_TARGET_DIR
python3 bench/h2h/run.py --theseus-only --rows T2,T8 --runs 20 --bin-dir target/release-thin --out /tmp/h2h-head
```
Expected: T2 interactive and one-shot fall by the same amount (the cause was on the daemon's side). If the owner's drop
is far smaller than 25 ms, the remaining time is in his config's extras (Jev's route wait, the index, personas): take
`THESEUS_LOG=info,theseus_core::compiler::phases=debug` for the compile's phases, and
`strace -f -c -e trace=mprotect,madvise,fdatasync -p <theseusd pid>` around one `theseus ask` (head should show about 75
`mprotect`s in the window, base about 340).

## What is left or uncertain

- The wall-clock gain on this VM is not significant (T2 interactive about -0.8 ms median, one-shot flat); the evidence is
  the syscall count and the per-step marks, plus the unit tests. The owner's disk-bound component I could not reproduce.
- `ProviderRequest.tools` is still a `Vec<Value>` (`spec.tools.to_vec()` once a compile, the loaded subset). Making it
  shared would remove the last copy but touches the provider layer; I left it.
- A flow-scoped catalog (`defer_step::flows_in_scope`) rebuilds a `Tools` per turn only while a flow is loaded and one is out
  of scope; rare. The thread tools' reduced set is kept.
- Docs: `docs/spec` Part III wants an item for this step (a shared `Tools`, a place's catalog kept); `docs/status.md`
  "recently landed". I added one bullet to `crates/theseus-core/AGENTS.md` (a spec's tools are shared).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (three runs; the first two died on this VM's disk
allowance, "No space left on device", in clippy and then the test build, which I fixed by deleting build artifacts).
Third run: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build all passed; the
suite ran 6,851 tests: 6,810 passed, 34 failed, 7 timed out.
- 33 L1 failures (`theseus-sandbox::contract`, `::bench spawn_100`, `theseusd::sandbox`): the known root-daemon
  refusal (theseus-pv6i).
- `theseus-core tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`: failed
  under the suite's load, **passes alone** (re-run: 1/1).
- 7 timeouts in `theseus-discord` (`tests_war_room`, `tests_task_threads`, `tests_default_forum`): not mine. On the
  base tree (my changes stashed) `a_shared_place_refuses_a_share_and_keeps_its_guests_rule` and
  `a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end` time out the same way
  (the Discord test hang named as in flight on the owner's machine).
After the suite, run by hand as `machine_checks` names them: protocol types (no `cockpit/src/protocol.gen` change: ok),
turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`: ok), `Cargo.lock` unchanged. `cargo deny` was **not
run** (I did not `cargo deny fetch` before the disk filled; no dependency changed). Lifecycle and jobs benches are
skipped by `THESEUS_GATE_NO_BENCH` and are the owner's machine's.

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` output: (empty, exit 0).
