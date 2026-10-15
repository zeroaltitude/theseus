# CLOUD REPORT: prefix-library (T13, theseus-rr15)

Branch `cloud/20261013-prefix-library`, cut from `1c381243` (main at `3f1f5248` plus the task commit). Started
2026-10-14 23:51 UTC; deadline 2026-10-15 05:51 UTC.

## The decision rule and its numbers (written before the code)

**What can be shared.** The provider caches an exact byte prefix per model: tools, then system, then messages.
Layers 1 (header + standing) and 2 (every persona's files) are already one text per config and place class; layer 3
(the system-level files, then the session's categories' guidance, a thread's topics among them) is where sessions on
one project differ: a thread with topic A and one with topic B write two layer-3 entries, so the second pays the whole
layer at the write price. The library keeps layer 3 as headers by digest and lets the second reuse the first's bytes,
with what it lacks after the cached prefix.

**Sections and fit.** Layer 3 is sections: `files` (the system-level files, one section: every session on a config
and class carries the same ones) and each guidance section, named by its first line. A header **fits** a fresh layer
when every section it holds is in the fresh layer byte for byte; `fit = covered bytes / fresh bytes`. A header with a
section of the same name and other bytes (its source changed) or a section the fresh layer lacks (another project's
or topic's guidance) never fits. So the owner's "surplus tolerated" is not taken for guidance: the near miss (another
project's test command in the cached prefix beside this one's in the delta) is exactly the false fit the guard is for,
and the only surplus layer 3 can have is guidance. (A design choice to revisit with the evaluation.)

**The decision**, at a compile point only, on the best fitting header among the k nearest (k = `[prefix]
candidates`, 16) by sketch, on the same lower layers (tools, header, persona: what the cache reads first) and the same
place class:

1. `fit ≥ confirm` (0.60): **reuse** it verbatim; the missing sections go after the cached prefix, at the head of the
   session's block (layer 4's side of the breakpoints). Below `act` (0.90) it is watched: a turn that goes wrong after
   it is a labelled false fit.
2. `fit < confirm`: **mint** iff `p(reuse within TTL) × reads × (w − r) × D > (w − r) × C_warm`, else **coarser**
   (reuse the best, the project's header without the topic, with the bigger delta). `D` = tokens the new header adds
   over the coarser one; `C_warm` = the coarser header's tokens when it is warm on this model, 0 when cold; `w`, `r` =
   the model's cache write (at the header's TTL) and read prices. Why: a mint writes the coarser part again instead of
   reading it (the premium, `(w − r) × C_warm`); each later read of the new header reads `D` instead of writing it
   (the gain per read, `(w − r) × D`). `w − r` cancels: the rule is `p × reads × D > C_warm`. A cold coarser header
   costs nothing to pass by, so then a mint always pays. With no fitting header, mint.
3. `p(reuse)` and `reads` from the hit log of the headers on the same lower layers and class, Laplace's rule:
   `p = (reused + 1)/(minted + 2)`, `reads = (Σ reuses + 1)/(reused + 1)`: no history is an even chance and one read.

**Numbers** (Sonnet 5.5, 5-minute TTL: w $3.75, r $0.30 per Mtok): a warm 20,000-token project header, a
15,000-token topic (fit 0.57). No history: gain 0.5 × 1 × 15,000 × $3.45/M = $0.026, premium 20,000 × $3.45/M =
$0.069: **coarser** (the topic rides in the delta). A history of 8 headers, 7 reused 27 times: p 0.8, reads 3.5,
gain $0.145 > $0.069: **mint**. A 2,000-token topic on the same header has fit 0.91: **reuse**, quiet. The prefix
test `reuse_at_confirm_and_below_it_mint_only_when_minting_pays` holds these numbers.

## Commits

- `8f711a69` prefix: the library, the pick, the canonical layers (standing's local records), format 34, the report,
  `[prefix]`, and the keep-warm read's fix.
- `998359f6` tests: `tests_kept` reads a turn's results in their calls' order (a race the suite hit here; see "The gate").
- `3ef963b9` routing: a held switch counts what the library holds warm on the target's model.

## Step 1: canonical layers

**Found.** Layers 1 to 3 were already free of dates, ids and turn counts but for one thing: the standing block put a
**session's and a place's** standing records inside layer 1 (`standing::Block::apply` appended every record of the
turn's scope to the header text). Two sessions, or two places, with their own records sent different layer-1 bytes,
and so different everything after.

**Changed** (8f711a69). `Block` now has `text` (the records for everywhere, the header layer's) and `local` (this
session's and this place's, under their own head line, at the head of the session's block). The manifest's `standing`
lists the header's records first, then the local ones. `standing::missing` reads the whole system, so the check is
unchanged.

**Proved.** `rpc/tests_prefix.rs::sessions_threads_and_places_on_one_project_send_layers_one_to_three_byte_for_byte`:
a CLI session and a DM session (two places) with an everywhere order and a session-scoped order on one of them, then
two threads, on one project with a persona: the three layers are byte-identical across all four requests, nothing of
a session id, a thread, the session's own order, `Directory:` or a `2026-` date is inside them, and the session's order
is in its own block. Planted: a date in layer 3 (`spec_step.rs`: `As of <now_ms>` after the system files) failed it
(`two sessions, two places`); the local records back in the header failed it the same way.

## Step 2: the library store

**Changed** (8f711a69): `prefix.rs` (the library), `prefix/fit.rs`, `prefix/sketch.rs`, `prefix/turn.rs`,
`config/prefix.rs`, `fact/prefix.rs`; store format **34**.

- A header is `prefix::Header` (digest, layer, class, lower, sections, text, sketch, created): META `prefix:<digest>`,
  never edited. Its hit log is META `prefix.hits.<digest>` (reuses, last read), rewritten by each reuse; a retirement
  is META `prefix.retired.<digest>` (`source_changed` or `evicted`). All ride the frame of the compilation that minted
  or reused it (`prefix::records`, from `persist_compilation`): **no new frame**.
- "Stored apart and combined": the header is layer 3 alone, keyed by `lower` (the digest of the tools, layer 1 and
  layer 2, which the cache reads first), so one project header serves every persona set and thread whose lower layers
  match, and a persona set or core change is a different `lower`. Layers 1 and 2 are not stored: they have one text
  per config and class, so there is nothing to choose.
- The sketch: a 64-bucket signed hashed bag of words, `i8`s, computed in the daemon (not the index tender's model
  embedding: the pick is on the turn's path, and asking the tender would put a socket round trip there; the tender's
  embedding can be stored beside it later).
- Loaded once into memory by the first compile point after a start (one META scan of three prefixes); in memory under
  `[prefix] max_bytes` (10 MiB, counted as text + sketch + per-section overhead); past it the header read least
  recently retires (never the one just minted). A header whose source changed no longer fits (its section has the
  same name and other bytes), the next compile point mints the new layer under its own digest, and the stale header
  retires at that mint once it is cold on every model.
- Store format: `MANIFEST_FORMAT` 33 → 34 (the manifest's new `prefix: {header, fresh}` field, the META keys, the
  ledger kind `prefix.picked`). `tests_layouts.rs`: a format-33 compilation sample (`COMPILATION_BEFORE_PREFIX`, a
  thread's, with the layers' cache layout) in `SAMPLES`; the header's literal (`PREFIX_HEADER_AT_34`) read and written
  back byte for byte; a format-33 store that opens with no headers, takes one with its hit log, moves to 34, and
  reopens holding it. The four "moves to this build's format" assertions now read 34 (theseus-core's `store.rs`,
  `tests_layouts.rs`, theseusd's `versions.rs`, whose "newer store" case is now 35). The goldens did not move: no
  golden scenario has a third layer (`core_output.txt` byte-identical; `kernel_frames.txt` untouched).

**Proved** (`prefix/tests.rs`): `past_the_cap_the_header_read_least_recently_retires` (planted: no eviction → failed);
`a_changed_source_mints_a_new_header_and_the_stale_one_retires_once_cold` (warm: kept; cold: retired as
`source_changed`; planted: never retire → failed); `rpc/tests_prefix.rs::a_changed_context_file_is_a_new_header` (a
context file edited between two turns of a thread: two mints, two digests, the new text in layer 3); the restart case
in `a_second_thread_on_the_project_reuses_its_header_and_its_topic_follows_it` (a second core on the store reads the
headers back and sends the same bytes).

## Step 3: the pick, only at compile points

**Changed** (8f711a69). `prefix/turn.rs` (`TurnRunner::prefix_spec`, called from `compile_step` each loop) and
`prefix::applied` (one call in `compile_with`, applied to the append's and the recompile's spec). The rule is the
section above. A compile point is: no compilation (a thread's birth, `/new`), a forced recompile, a cold resume, or,
at the turn's first loop only, a system that would change under the session anyway (`changes`: the append's system
digest with the replayed plan against the manifest's). Otherwise the plan the current compilation used is replayed
from memory (`Library::replay`: the header's text and this run's delta), so appends repeat the bytes. The candidates
are the k nearest by sketch of the new message, the thread's card and the layer's section names, filtered to the
same lower layers and **place class** (a shared place reuses only a shared place's header).

**Departures.** The fit is code's, not Jev's: the `prefix` pack (`options_from = "headers"`) is not built. A fit
over section identity is exact and free; Jev on the compile point would add a model call to a turn's start. The
bands are the task's (act 0.90, confirm 0.60, `[prefix] act|confirm`), so a pack can replace `fit::fit` and keep
`decide`. Surplus guidance is never tolerated (see the rule).

**Proved.** `reuse_at_confirm_and_below_it_mint_only_when_minting_pays` (the numbers above, and the dollar sums) and
`a_mint_is_refused_when_reusing_the_coarser_header_pays` (planted: always mint below confirm → both failed, the
second with `Sums { gain_usd: 0.0003082, premium_usd: 0.0013179 }`); `a_topic_reuses_the_project_header_and_the_same_layer_reuses_itself`;
`a_near_miss_project_is_not_reused` and `rpc/tests_prefix.rs::a_near_miss_project_is_never_reused` (planted: a
foreign section tolerated → the fit test and the whole-core test failed; the unit near-miss test still passed, since
its two headers also conflict on a shared section name); `shared_and_private_headers_never_cross` (planted: the class
filter removed → failed: `a private header is no shared candidate`);
`compiler/tests_layers.rs::a_planned_third_layer_is_the_headers_bytes_and_an_append_repeats_them` (the three-block
request with a plan: layer 3 is the header's bytes, the delta heads the session block, breakpoints `[true, true,
true, false]`, the manifest names the header; an append repeats the system; an edited layer recompiles
`system_changed` with no header named); `rpc/tests_prefix.rs::a_second_thread_on_the_project_reuses_its_header_and_its_topic_follows_it`
(picks `mint, reuse`, fit ≥ 0.90, layer 3 verbatim, the topic in the session block, the append's system equal;
planted: no replay on an append → failed: `the append repeats the bytes`); `off_each_session_renders_its_own_layers`
(`[prefix] enabled = false`: no pick, the topic in the thread's layer 3, no header in the manifest).

## Step 4: warmth

**Changed.** Warmth is per header and model, in memory: each compile touches the header its compilation names
(`prefix_touched`, at the profile's TTL), so a header is warm until its TTL passes; a start forgets it (a header
reads cold until its next use: the estimate errs toward the write price). Routing (`route_step::decide_route`,
3ef963b9): when the rule holds a switch back as `cache_hold`, `warm_prefix_tokens` builds the switch target's own
spec (its lower layers), asks the library for the biggest warm header on the target's model that fits the session's
third layer at `confirm`, and decides again with those tokens (tools at the JSON rate, the system and header at the
text rate) taken off the estimate. Asked only on a cache hold, never on a warm turn.

**Defect fixed on the way** (8f711a69): the keep-warm read (`keep_warm_request`) built the session's next request
without the standing block, so any session with a standing record compiled `system_changed` there and was stopped as
"its next turn recompiles"; it would also have missed the library's header. It now applies `standing_block` (the
turn's own, factored out of `stand`) and the replayed plan. `rpc/tests_prefix.rs::a_thread_on_a_reused_header_is_kept_warm_with_its_next_turns_bytes`:
a thread on a reused header with a session-scoped order is read, and the read's system is the next turn's; planted:
no replay → `Stopped("its next turn recompiles…")`; no standing block → the same.

**Not built:** keep-warm refreshing the few most-read headers alone, with a header-only request, instead of one read
a session. The current read already refreshes the shared header for every session it reads.

**Proved.** `tests_route_warm.rs::a_warm_library_header_on_the_targets_model_counts_as_cached` (the first turn is
held, `cache_hold`, cold on opus; with the project's header touched warm on opus's model the next session's switch
goes at once, `("opus", "verdict")`; planted: the warm tokens ignored → `("sonnet", "cache_hold")`).

## Step 5: the false-fit guard and the report

**Changed** (8f711a69). Every pick is one `prefix.picked` row (choice, fit, below_act, header, fresh, class, model,
candidates, header and delta bytes, p_reuse, reads, gain and premium in dollars, the prices), in the compilation's
frame. `prefix.report` (`rpc/prefix.rs`, a read for a surface that reads private text, on the blocking pool, no
lock, no frame) and `theseus prefix report`: picks, reuse/coarser/mint counts and rates, the reuses whose turn's first
`provider.call` read the cache and the header tokens they read instead of writing (and the dollars), the reuses below
`act`, and the false fits: a reuse below `act` followed, in that turn or the session's next, by a `route.corrected` or
`fact.corrected` row, or by the owner's next message asking the same again (`report::re_ask`: at least 3 words of 3+
letters and a Jaccard of 0.6). The library's resident size against `max_bytes` heads it.

**Proved.** `a_reuse_below_act_that_went_wrong_is_a_labelled_false_fit` (five planted picks: counts, rates, one cache
hit saving 10,000 tokens at $3.45/M, false fits `[("s3", "correction", 0.70), ("s5", "re_ask", 0.50)]`, a reuse above
`act` and one that went well not listed; planted: `below_act` ignored → failed); the CLI's words
(`prefix_cmd::tests`, a golden of the lines).

**Not built:** the false fits do not yet feed Jev's learning report (there is no `prefix` pack to label); a re-ask is
the words' overlap, not a judgment.

## Step 6: the off switch

`[prefix] enabled` (true), `max_bytes` (10 MiB), `act` (0.9), `confirm` (0.6), `candidates` (16), each in the
template's new `[prefix]` section with `config::prefix::tests::the_templates_prefix_section_is_the_default`, and
validated (bands within 0..1 and not crossed, a floor on the cap). Off: no pick, no replay, no read, no row.

## FAST

- A warm turn: no frame, no timer, no store read for the library. The pick and its records ride the compilation's
  own frame. At a warm turn's first loop `changes` composes the guidance and takes one system digest (CPU only); later
  loops read the plan from memory. The library's load (one META scan) happens at the first compile point after a
  start, on the blocking pool: a store read on that one turn's path, once per daemon run (it could move to a warm-up
  after serving, as the threads' board has).
- `theseus-sim bench turn --check --runs 5 --burst 0`: frames_plain 4 (budget 4), frames_tool 8 (8), frames_batch 14
  (14), syncs_plain 2, syncs_first_byte 2, first_request_kb 38.6 (40): ok.
- `theseus-sim bench lifecycle --runs 10 --check`: every budgeted row ok **but** "health, after the seed, each call its
  own connection", p95 7.9 ms and then 7.3 ms against 5 ms. The base build (1c381243, built in a worktree on this VM)
  misses it the same way, p95 7.6 ms: this VM, not the change (nothing of the library is on health's path).
- The library's resident size: `theseus prefix report`'s first line (`1 headers, 0.0 of 10.0 MiB` in the live check).

## The live check on this VM

A scratch daemon of the branch's debug build on a fresh state dir, the stand-in model (`theseus-sim fake-model --dump`,
which keeps every request's body), `[discord] [web] [index] [judge]` off, one system-level context file (17 KB), no
persona. The script and its config maker are in the session's scratchpad (`run.sh`, `note.py`); the commands:

```
theseus ontology topic add tides; theseus ontology guide tides "Quote every level in metres."
theseus ontology topic add gauges-project; theseus ontology guide gauges-project "Run the tests with: cargo test -p gauges"
theseus ontology topic add tides-project;  theseus ontology guide tides-project "Run the tests with: cargo test -p tides"
theseus thread new harbour;                                  theseus ask --thread harbour "what does the log say?"
theseus thread new gauges --topic topic:tides;               theseus ask --thread gauges "and the gauges?"; … "and again"
theseus thread new gauge-work --topic topic:gauges-project;  theseus ask --thread gauge-work "run the gauge tests"
theseus thread new tide-work --topic topic:tides-project;    theseus ask --thread tide-work "run the tide tests"
theseus --json ledger --kind prefix.picked; theseus prefix report
```

What it showed (each request's system blocks by digest, `cc` = a breakpoint):

```
request-000 harbour     ['ade95a38b5', '89c091785f', '6c6ea27261'] [cc, cc, --]
request-001 gauges      ['ade95a38b5', '89c091785f', '9573e2ad57'] [cc, cc, --]   "metres" in block 2 (the session's)
request-002 gauges      ['ade95a38b5', '89c091785f', '9573e2ad57'] [cc, cc, --]   the append: the same bytes
request-003 gauge-work  ['ade95a38b5', '89c091785f', 'df84e12caf'] [cc, cc, --]   "-p gauges" in block 2 only
request-004 tide-work   ['ade95a38b5', '89c091785f', '44be5b43c1'] [cc, cc, --]   "-p tides" in block 2, no "-p gauges" anywhere
prefix.picked: mint (header 89c091785fb5) · reuse fit 0.990 · reuse 0.989 · reuse 0.989 (deltas 186, 207, 205 bytes)
prefix library: on, 1 headers, 0.0 of 10.0 MiB, 1 warm
4 picks: 3 reuse, 0 coarser, 1 mint (reuse 75%, mint 25%)
cache writes saved: 0 tokens ($0.0000) over 0 reuses that read the cache
```

The second thread's layers 1 to 3 are the first's bytes (with a persona the persona layer would sit between); its
topic follows them. The two near-miss projects share the project header and each carries only its own test command;
neither ever reads the other's. The stand-in reports no cache reads, so "cache writes saved" is 0 here: **the cache
read in the ledger is the maintainer's check**. (The "token mirror" stand-in the task names is not in this tree;
`fake-model` has no cache model.)

## The maintainer's live check (Sonnet 5.5, the owner's keys)

On a scratch daemon of the install build (`--config`, `--socket`, `--state-dir` of its own, `[discord]` and `[web]`
off), the live profile on `claude-sonnet-5-5`, `[context] files` naming one project file of at least 8 KB (layer 3 has
to reach Sonnet's caching minimum with the header), and the three topics above:

1. `theseus thread new harbour && theseus ask --thread harbour "what does the log say?"`
2. `theseus thread new gauges --topic topic:tides && theseus ask --thread gauges "and the gauges?"` within 5 minutes.
3. `theseus --json ledger --kind provider.call -n 2`: the second row's `usage.cache_read_input_tokens` covers layers
   1 to 3 (about the first call's `cache_creation_input_tokens` less the first thread's own messages), and its
   `cache_creation_input_tokens` is about the delta and the messages alone.
4. `theseus --json ledger --kind prefix.picked -n 2`: `mint`, then `reuse` with the first's `header`.
5. `theseus thread new gauge-work --topic topic:gauges-project && theseus ask --thread gauge-work "run the gauge tests"`,
   then the same for `tide-work`/`tides-project`: neither request's system holds the other's test command.
6. `theseus prefix report`: `cache writes saved` > 0 over the reuses that read the cache; `1 headers`.
7. With `[prefix] enabled = false` and a fresh state dir, step 2's second call writes its own layer 3 (its
   `cache_creation_input_tokens` includes the project file again): the A1n arm.

## What is left, uncertain, or for the owner

- **Threads and plain sessions never share a cache entry, library or not.** `thread.note` is offered only in a
  thread's session, and the tools come first in the cache's order, so a thread's whole prefix differs from the DM's
  from byte one. The library shares layer 3 between threads, and between plain sessions, but not across the two. To
  share the DM's warm prefix with its threads, offer `thread.note` in every private session (refusing it outside a
  thread), or put thread-only tools after a breakpoint: the owner's call.
- **Jev's `prefix` pack is not built**; the fit is code's (section identity), the bands are the pack's. **Keep-warm
  for headers alone** is not built (`[prefix] keep_warm` was dropped rather than shipped unread).
- **Surplus is never tolerated** in layer 3 (only guidance can be surplus there, and another project's guidance in the
  cached prefix is the false fit). Revisit at the evaluation if a broader "persona plus a bug" reuse is wanted.
- **Warmth lives in memory** and a start forgets it, as do this run's deltas: the first compile point after a start
  picks again (the same bytes, so an append, no recompile), and its pick bumps the header's hit count in memory
  without a record. A pick made for a compilation routing then discards bumps the count the same way.
- **A replayed header is not checked against the current lower layers**: a tools change mid-session recompiles with
  the same header (still the right bytes for layer 3) and touches its warmth under a lower it was not minted for. The
  routing estimate builds the target's own lower, so it is not misled, but a pick's warmth check could be, slightly.
- **The sketch is a hashed bag of words**, not the index's embedding; with `candidates` = 16 it only orders the
  candidates, and the fit decides.
- **The re-ask is words' overlap** (Jaccard 0.6 over words of 3+ letters); misroutes are the owner's `route.corrected`.
- **Docs to change** (the maintainer's): the spec's Part III item for T13 (what it built, the decision rule and its
  numbers, the departures above, store format 34); `docs/status.md` (the row, its "Updated" line); the spec's version
  line; `docs/design/` (the threads design, if it holds T13's plan: the pick is code's, surplus is not tolerated, the
  thread-only tool breaks sharing with the DM). `crates/theseus-core/AGENTS.md` is updated in the branch (the prefix
  library's entry, and standing's: its local records head the session block).

## Keel findings expected

None from this branch: `python3 scripts/keel-guard.py --base 1c381243` (the task commit) reports `0 findings` over
the branch's commits. The guard's default range here, `merge-base(HEAD, origin/main)..`, starts at origin/main
`9fa84735`, an older side of main, and reports one finding that is main's own: `crates/theseus-sim/src/lifecycle.rs:155:
budget-raised: "seed" => Some(250.0)`, acked by commit 1c2beb27 with a signature this VM cannot verify (no keyring:
`%G? is E`). The gate was run with `THESEUS_KEEL_BASE=1c381243` for that reason.

Rewritten assertions (no count lowered): `standing::tests::the_block_lists_by_kind_and_a_shared_place_withholds_the_private_ones`
(the place's preference is checked in `local` and absent from `text`; the shared place's leak check reads both; the
manifest order is the header's records, then the local ones); the four format assertions 33 → 34 (and theseusd's newer
store 34 → 35).

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=1c381243 scripts/gate.sh` on the final
tree (main's copy through the door, then the tree's): door, keel (0 findings), keel tests, fmt, shape, features, clippy,
cockpit, test build, reader rule: ok. **Suite: 4,633 run, 4,600 passed, 33 failed, 47 skipped**: the 33 are exactly
the known L1 failures of a root VM (theseus-pv6i): theseus-sandbox's 19 contract tests and `bench spawn_100`, and
theseusd's 13 `sandbox` tests (`the_jobs_bench_l1_row` among them). The phases after it, run by hand: protocol types
ok (the TypeScript staged with the change); `theseus-sim bench turn --check --runs 5 --burst 0` ok (4, 8, 14 frames;
2 syncs); size skipped (no stamped release build); `cargo deny --offline check`: bans, licences and sources ok,
**advisories fail on `rustls 0.23.45`** (RUSTSEC-2026-0346, -0347 and more), with `Cargo.lock` unchanged by this
branch: main's, for `deny-daily.sh`. `theseus-sim bench lifecycle --runs 10 --check` on the final build: every budget
ok but health after the seed (p95 7.6 ms against 5 ms), which the base build misses the same way here (p95 7.6 ms).
`incremental` was off because the VM's disk filled during the first gate (the incremental cache had reached 18 GB).

Earlier in the session the gate's suite (before the third commit) also failed `tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends`:
reads of one response run together (theseus-d1hi), so their results land in the order they finish, and the test took
them by position. Reproduced under load (nice 19 beside four busy loops: 1 of 10 failed, as in the suite), fixed in
`998359f6` (results sorted by call id), then 0 of 20 under the same load. Two of this branch's own new layout tests
failed there too (a raw string's delimiter), fixed before the first commit. No timing test of the preamble's list
failed in the final run. Python under bench/ was not touched, so its suites did not run.

Not run on this VM: the size check (no release build), and the benches' recorded rows (`THESEUS_GATE_NO_BENCH=1`).
