# Cloud report: tiering, step 33 (theseus-6fn.13)

Branch `cloud/20261005-tiering`, from `main` at d5a4b80 (plus the task commit 6ee435d). Started 01:09 UTC, report
written 02:55 UTC (every time from `date`). Three code commits, then this one:

| Step | Commit | What |
|---|---|---|
| 1. The bench row first | 34c8dfd | `bench turn --session-nodes N`, `bench idle --active N`, the tender's memory |
| 2. The heat cache | 5492bb4 | `node_cache.rs`, `[memory] node_cache_mb`, health line, metrics |
| 3. Stubs | 719d6f7 | `stub.rs`, `Transcript = Vec<(u64, Stub)>`, the readers, `context.compiled`'s `decoded`/`stubs`, failed rehydration logged and counted |
| 4. The rows after | (no code) | the numbers are below; the failed-rehydration log and counter landed with the stubs in 719d6f7 |

All numbers below are this VM's (4 cores, debug binaries, the same build profile for main and for the branch).
Main's binaries were copied to `/tmp/main-bin` from the step-1 tree, which has no core change.

## What changed since the design (the code won)

- **The transcript read is not from the index alone.** The design says a read takes positions from
  `positions_in_scope`. The compiler's whole-transcript walks (the floor, the visible set, the ring, the render's
  filters, recall's view) need each node's body kind, a summary's range's end, its origin, and its turn, which the
  index does not keep. So a read still scans the session's records (`scan_scope`, payload bytes included) and a stub
  takes those fields from a **peek**: a serde struct of the five fields, so the payload is skipped by the parser and
  never built. A full `Node` decode happens only at a reader's first touch. Putting those fields in the index (a
  projection term) would let a read skip the payload bytes too; that is a store change I left out (below).
- **The cache is keyed by WAL position, not node id** (the task says position; the design said id). A node never
  changes, so its position names its bytes for good, and a recall's source is read by position already.
- **A turn reads its transcript once** (theseus-qa0) and the debug check holds: it now compares positions and
  record keys from a scan, decoding nothing (before, it decoded the whole session at every read in a debug build).
- **Compaction is on main**: a summary's floor is read from the stubs (`Stub::summary_last`), so everything before a
  floor, and everything before a ring's cut, stays a stub after the turn that rang.
- `node_cache_mb` sits in `[memory]` and the template line says it serves with `mode = "off"` too.

## Step 1: the bench row (34c8dfd)

Found: `bench turn` measured one warm, short session only; no long-session row, no decode count, and `bench idle` read
the daemon's memory without the index tender's.

Changed: `theseus-sim bench turn --session-nodes N [--result-bytes B]` (in `src/perf/long.rs`, beside `perf.rs`)
writes one session of N nodes before the daemon starts (`synth::long_session`: five-node exchanges, the model reading
a file of B bytes, 8192 by default, and answering; parked waiting for input), then measures `--runs` turns in it:
wall time by both clocks, frames, nodes decoded per turn (health's `store.node_cache.decodes` before and after; a
build before tiering has no count, and the row says so), and memory with the tender's. `bench idle --active N` opens N
sessions after the window and runs a turn in each, then reads both memories again. History columns: `turn_long`,
`decodes_long`, `rss_long`, `rss_tender`, `rss_active`.

Main's numbers (`/tmp/main-bin`, debug):
- `bench turn --session-nodes 2000 --runs 10`: 4.5 MB of WAL; wall p50 326.5 ms (each: 3241 317 298 363 327 328 346
  362 320 299; the first compacts the session); daemon p50 284 ms; frames 8, then 5; decodes not counted (every turn
  decodes the session whole, and the memory pass reads it again); memory 77.0 MB after the start, 200.1 MB after the
  turns; tender 41.9 then 47.8 MB.
- `bench idle --sessions 10000 --active 50 --seconds 10`: daemon 140.7 MB and tender 71.9 MB parked (212.6 MB
  together); after 50 active, 158.1 + 88.7 = 246.8 MB. Under §9's 1 GB.

Proof: `perf::tests::every_column_a_bench_records_has_a_history_column` covers the new columns (both reports),
`perf::long::tests::a_long_session_reads_back_whole` reads the synthetic session back. The gate (below).

## Step 2: the heat cache (5492bb4)

Changed: `crates/theseus-core/src/node_cache.rs`. `Arc<Node>` by position, one per store, shared by every handle.
Every node read goes through it: a turn's transcript, the nodes a turn's frames write, `session_nodes`, `get_node`,
`first_node`, `recent_nodes`, and a recall's source (`Store::node_at`, which `recall/render.rs`'s `by_position` now
calls). Bounded by `[memory] node_cache_mb` (64; 0 off; at most 16,384), counted in record bytes plus 128 a node.
Past the bound it evicts to 7/8 of it: `decay_sweep`'s hints first (the science, set from `Memory::science_owned` at
build), then by (last touch ms, touch count, touch order). A node larger than the bound is never kept. Health:
`store.node_cache` (`NodeCacheHealth`, in `theseus-protocol/src/health.rs`: bound, bytes, entries, hits, misses,
decodes, evictions, failed), and one CLI line: `node cache: 3.0 of 64 MB, 812 nodes; 95% hits of 1000 reads, 50
decoded, 0 evicted`. Metrics: `theseus.node_cache.bytes` (gauge) and `theseus.node_cache.reads` by outcome (hit,
miss, decode, eviction, failed), recorded as each turn ends. Nothing on the start path fills it.

Proof:
- `tests_tiering::decodes_fall_on_a_long_session`: a 60-message session's second turn decodes exactly its new nodes
  with the cache, and the whole session again with `node_cache_mb = 0`.
- `node_cache::tests::eviction_by_heat_stays_under_the_bound` (proptest, keeps/touches/clock): never over the bound;
  the bytes add up; nothing evicted unhinted while a hinted node is kept, or while a colder one is kept.
  `decay_sweeps_hints_go_first`, `a_node_decoded_once_is_served_after_and_zero_keeps_none`.
- Planted revert "a read that decodes every node again" (`scan_kept` decoding each record without the cache):
  `tests_tiering::decodes_fall_on_a_long_session` failed at its first assertion ("with the cache, only the turn's new
  nodes"). Restored, touched, `git status` clean.
- Planted revert "eviction that ignores the bound" (`keep_at` never evicting): the proptest failed ("258 bytes over a
  bound of 215", shrunk), and `decay_sweeps_hints_go_first` failed. Restored and touched. That run left a
  `proptest-regressions/node_cache.txt` that 5492bb4 committed by mistake; 719d6f7 removes it.

## Step 3: stubs (719d6f7)

Changed: `crates/theseus-core/src/stub.rs`. `Stub` has `id`, `kind`, `origin`, `turn_id`, `summary_last` as its own
fields (so `n.id`, `n.origin`, `n.turn_id` on a stub never decode), keeps the record's bytes until it hydrates, and
derefs to its `Node`, decoded at the first touch through the cache (or taken from it; or read again by position when
the cache let it go). A trait `Shaped` lets `compiler::renderable` and `compaction::is_summary` take a stub or a node.
A failed rehydration logs a warning naming the node and position, counts in `store.node_cache.failed`, and reads as a
harness user message `[node … at @… could not be read: …]` (where today's read failed whole: the stub's bytes peeked,
so the record was readable; a full decode failing after that is a corrupt store).
`context.compiled` carries `decoded` (nodes this compile read past their stub: from the cache or decoded) and `stubs`
(left undecoded), both skipped when zero, so old rows and wire fixtures round-trip. `decoded` counts the turn's own
stubs, not the store's decode counter, so the output golden's shape does not depend on what another reader did; the
store's real decodes are health's.

Readers changed (each reads stub fields before a body, so a whole walk touches only what it needs): the compiler's
`floor`, `visible`, the ring's sequence and starts, `ring_cut`, the fresh strategy's start, and `render_request`'s
filters (its `section` closure was typed `&Node`, which derefs every node: now `&Stub`); `recall_view`, `scene`, and
`recall_tokens_in_tail`; `has_news`; `detour_start`; `unanswered` (results after the last reply only, which is the
same set); `resume`'s calls; a late result's placeholder and `call_input` (from the end, by kind; `already` after the
placeholder only); `holder_of`; the memory pass's eligible set (labeled first, then kind), `external_in`, `waiting`,
and `resolve`'s turn filter (the pass's logic is unchanged). Readers that needed nothing: those that walk from the end
until they find what they need (`signals::read`, `counted_part`, `unread_relays`, `task::last_message`,
`recall::query_of`, whose turn test is a stub field), and those that need every text (`arrangement::resolve`,
`theseus-exam`'s replay): they rehydrate what they touch. `session_nodes` still returns owned nodes (cloned from the
cache) for the readers outside a turn that keep them.

Proof (`tests_tiering.rs`, `stub::tests`):
- `every_request_is_the_same_with_the_cache_off`: one script through whole turns on two cores, `node_cache_mb` 64
  and 0, recall live: a plain turn recalling another session's note, a `text_diff` tool call, 6,000-token turns until
  the session compacts, and two turns after. Every request (system, messages, tools), with ids and clock times
  normalized, is the same. The last row has `stubs > 0` and `decoded <= prefix + tail`. Then the session's current
  compilation rendered from stubs equals the render from the session decoded whole, and left stubs.
- `the_transcript_check_holds_through_a_ring_and_a_recompile`: appends, `Recompile::Transcript`, turns until the ring
  (compaction off), one after; every transcript read in a test build asserts the debug check. The scenario above runs
  it through a compaction. The row after the ring has stubs.
- `a_recall_source_from_before_a_floor_renders_the_same_bytes`: session A's note is before A's compaction floor and a
  stub there; `node_at` by position returns the stored node; B recalls it and the model's request quotes its bytes;
  cache on and off.
- `a_stub_decodes_at_its_first_touch_and_only_then`, `a_summary_knows_its_range_and_a_lost_node_says_so`.
- `tests_m3::a_plain_turn_stays_within_its_frame_budget` passes; `theseus-sim bench turn --check --runs 5 --burst 0`:
  plain 5 frames, tool-call 9.
- Planted revert "a stub rendered without rehydration" (every hydrate of a stub read from the store returns the
  harness placeholder): `every_request_is_the_same_with_the_cache_off`, `a_recall_source_from_before_a_floor_…`,
  `the_transcript_check_holds_…`, `decodes_fall_on_a_long_session`, and both `stub::tests` failed. Restored, touched.
- Under load (four `while :; do :; done` loops at nice 0, the tests at nice 19; the twelve tests of tiering, the cache,
  the stubs, the frame budget, and the kept transcript): 5 of 5 runs passed, 12/12 each (38 to 51 s a run).

## Step 4: the rows after

`bench turn --session-nodes 2000 --runs 10`, debug, side by side:

| | main | this branch |
|---|---|---|
| wall p50 / p95 (first turn compacts) | 326.5 / 3240.8 ms | 217.1 / 3114.9 ms |
| turns after the first | 297 to 363 ms | 201 to 281 ms |
| daemon p50 | 284 ms | 197 ms |
| nodes decoded per turn | the session whole (not counted) | 2003, then 2 each |
| frames | 8, then 5 | 8, then 5 |
| memory after the turns (daemon + tender) | 200.1 + 47.8 MB | 158.0 + 48.0 MB |

The cache after the run held 4.6 MB of 64, 2,021 nodes, 10,778 hits, 2,021 misses, no eviction, no failure.

`bench idle --sessions 10000 --active 50 --seconds 10`: main 140.7 + 71.9 MB parked, 158.1 + 88.7 = 246.8 MB active;
branch 138.2 + 71.7 MB parked, 155.0 + 89.4 = 244.4 MB active. The idle CPU in the 10 s window swung between 0 and
43 ms on both builds over four runs (noise, not a change). Lifecycle `--runs 10 --check`: both OK; main cold start p50
16.3 / p95 20.9 ms, branch 17.7 / 22.2; restart after SIGKILL main 16.6 / 20.7, branch 15.8 / 18.5; clean shutdown
main 6.2 / 26.2, branch 5.6 / 10.0; swap main 17.6 / 28.6, branch 18.9 / 21.2.

## The live check (the maintainer's)

1. Side by side, on the stand-in model:
   ```
   <main's install>/theseus-sim bench turn --session-nodes 2000 --runs 10 --theseusd <main's install>/theseusd
   target/release-thin/theseus-sim bench turn --session-nodes 2000 --runs 10 --theseusd target/release-thin/theseusd
   ```
   Main's row says "nodes decoded: not counted"; the branch's shows about 2,003 for the first turn and 2 for each
   after; the branch's wall p50 and memory after the turns are lower.
2. A scratch daemon: a fresh state dir, `[discord] enabled = false`, `[web] enabled = false`, a GLM profile with
   `[catalog."<model>"] context_window = 32000`, `[memory] mode = "shadow"` (or off), `summary_profile = "session"`.
   Drive a session past the window with long messages until a `context.compiled` row says `strategy: compaction`
   (`theseus --socket <its socket> --json ledger -k context.compiled -n 5`), then two more turns. Each later row shows
   `decoded` (a handful: the prefix and tail nodes the turn had not read) and `stubs` (the summarized range, which
   never decodes again). `theseus --socket <its socket> health` shows `node cache: … MB of 64 MB, N nodes; X% hits of
   … reads, D decoded, 0 evicted`, and D grows by about the new nodes per turn.
3. `theseus-sim bench idle --sessions 10000 --active 50` with release binaries: daemon plus tender under 1 GB in both
   rows; `theseus-sim bench lifecycle --check`, as main's.

## Left, uncertain, and for the owner

- **A ring decodes the session once.** The ring tries cuts from the earliest and renders each candidate, so the turn
  that rings (or compacts) decodes every node it keeps or tries; the cache keeps them. After it, nothing before the
  cut is touched. Estimating candidates from record sizes would avoid it but changes which cut the ring picks.
- **A read still reads the payload bytes** from the WAL each turn (and peeks them). The index could carry each node's
  kind, origin, turn, and summary range as terms (a projection change, so a rename and a rebuild after serving), and
  then a read would be positions and keys alone, as the design says. Not done: the step expected no store change.
- **The bound counts record bytes**, not the decoded heap, which is somewhat larger (JSON values for blocks). 64 MB of
  records may be 100 MB or more resident in the worst case. A heap estimate would be more honest; say if you want it.
- **`decay_sweep`'s baseline hints only nodes idle 30 days**, so in a running daemon the hints rarely fire and heat
  decides. The order is the science's hook for 32a's retention, as the design says.
- **A deref trap**: a closure typed `|n: &Node|` applied to a stub decodes it silently (`render_request`'s `section`
  did, and decoded every node). theseus-core's AGENTS.md says so; `tests_tiering` catches it in the compiler's paths.
- `decoded` in `context.compiled` counts nodes read past their stub (from the cache or decoded), not store decodes;
  health has the true decode count. Named in the field's doc.
- 719d6f7's message says compiler.rs's count "holds at 2,547"; after `cargo fmt` it is 2,541 (lower). turn.rs is
  unchanged at 3,445. `turn/compile_step.rs`'s function passed clippy's 100 lines by two, and takes the repo's
  `#[expect(clippy::too_many_lines, reason = "shape budget: split it")]`.
- Other changes in flight touch the same readers: route (25e), the tasks smalls, and 31b/32a/32b's recall arms. A
  new reader of a transcript should read stub fields for its walk; a merge that adds `match &n.body` over every node
  will compile and work, but decode the session.

Docs the maintainer may want (not edited here): the spec's Part III item for step 33; `docs/status.md`; design
`m6-memory.md` §2.10 (the read is a scan plus a peek, not `positions_in_scope` alone; the cache is by position). The
template, theseus-core's and theseus-sim's AGENTS.md are updated in the commits.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on 719d6f7's tree: fmt, shape, features, clippy, cockpit,
test build, and the reader rule pass; the suite ran 2,454 tests, 2,421 passed, 17 skipped, 33 failed, every one an
L1 test this VM fails as root without a job cgroup (theseus-pv6i): `theseus-sandbox::bench spawn_100`, 19
`theseus-sandbox::contract` tests (`a_job_that_cannot_start_says_why`, `clause_01` to `clause_12`, the three
`egress_18b_*`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`, `sigterm_is_forwarded_to_the_command`),
and 13 `theseusd::sandbox` tests. The phases after it, run by hand: protocol types clean; `bench turn --check` plain 5
and tool-call 9 frames; `deny` bans, licences and sources ok, advisories not checked (no advisory database here). The
lifecycle and jobs benches are skipped by `THESEUS_GATE_NO_BENCH`; the lifecycle bench was run by hand above. The
same held for 34c8dfd and 5492bb4. `tests_output::the_cores_output_matches_its_golden` passed under the Phoenix clock;
neither theseus-hohs's nor theseus-1n2y's test failed.
