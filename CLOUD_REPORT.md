# CLOUD REPORT: thread-context (T2), theseus-h4a7

Branch `cloud/20261013-thread-context`; code commit `f22ef78`. Base: main with T1 (store format 33) joined.

## What was built (commit f22ef78)

1. **The layered request** (`compiler/layers.rs`, moved `system_blocks`/`cache_layout`/`CacheLayout` out of compiler.rs, now 2,449 lines).
   Layers, each with a breakpoint when its prefix reaches the model's minimum: `header` (header text + the standing block, moved from
   `context`), `persona`, `project` (system-level files, then the guidance of the session's categories), then the messages' automatic
   breakpoint: the provider's four. The session's directory (`Directory: …`) is a fourth, **unmarked** system block after them, so it
   cannot break layers 1 to 3 across sessions. `context.compiled`'s `cache.layers` holds each block's digest.
2. **The thread card** (`compiler/card.rs`, `thread_context.rs`): first block of the first message in a thread's session. A snapshot, kept as
   META `thread.card.<compilation id>` in the compilation's own frame (`persist_compilation`): appends repeat its bytes, a recompile
   (system change, ring, compaction, cold resume) takes the thread as it stands.
3. **`thread.note {line}`**: harness tool, offered only to a thread's own session (left out of every other spec, `thread_spec`). The call's
   completion frame carries the thread's next record (card + one line, newest 12 kept) and its `thread.edited` row (`via: thread.note`).
4. **The cold-resume recompile** (`compiler/cold.rs`): at a threaded session's first loop, when `cold_now` says the cache window passed, the
   compile is a recompile with trigger `cold_resume` (fresh card). If the request is over `[threads] working_tokens` (60,000) it keeps the
   newest `resume_keep_turns` (6) turns, stubs results over 2,000 chars before the newest turn (the clearing stub, `cleared` drops), and
   drops leading turns only if still over: a `ring` that `compact()` summarizes like any ring. Unthreaded sessions and warm threads: untouched.
5. Config: `[threads] resume_keep_turns`, `working_tokens` (defaults, template, validation, test).
6. A thread's `topics` become operator memberships of its session in the compile walk (`Walk::with_topics`), so their guidance is in layer 3.

## Deviations and choices the owner should hear about

- **No store change.** The brief wants "the manifest records each layer's digest". A field on the stored `Manifest`/`BlockBreakpoint` is a stored
  record field (format bump), so per the brief I stopped there: the digests ride `context.compiled` (a ledger row) instead. A manifest field
  needs a format bump and a `tests_layouts` sample if the owner wants it.
- **Persona layer is the persona in play's files, not every configured persona's.** I built "every persona" first; the existing test
  `context_files_come_in_two_levels…` ("a persona that names no files adds none") pins the rule, and loading other personas' files into a
  session leaks them. The persona is a global default today, so the layer is still byte-identical across sessions. Say if you want all.
- **Layer order changed**: persona now precedes the system-level files (brief's order). `tests_m3::system_then_persona_files…` was rewritten
  for the new order (assertion rewritten, not deleted).
- **`thread_note` is in a thread's tools only**, so a thread and an unthreaded session share layers 1 to 3 byte for byte, but the *tools block
  differs* and the provider's prefix is tools-then-system: they do **not** share a cache entry; two threads do. If thread/DM sharing matters,
  offer the tool everywhere (the note refuses in an unthreaded session).
- `Threads::noted` swaps the board before the call's frame is written (one frame required). A `thread.edit` racing the same instant could be overwritten.
- The standing block now ends the header layer (was first in `context`): one `system_changed` recompile for every session at upgrade, as any system change.
- Existing tests rewritten to the new layout (assertions rewritten, none deleted or weakened): `cache_header.rs` (4 blocks), `tests_session_dir`
  (session block carries no breakpoint), compiler `…two_blocks…` / `…under_the_models_minimum…` (block name `project`), `tests_output` golden
  (`cache.layers`, no session breakpoint; the normalizer masks layer digests), `tests_m3` order. The "no breakpoint on the session block" narrative
  line is no longer said for the (never marked) session block.
- The tests' `RequestSpec` literals gained `persona_text`, `card`, `cold`.

## Proof

- `cargo nextest run -p theseus-core -p theseus-protocol`: 2217 run, 2216 passed; the one failure (golden) was fixed (digest masking) and passes.
- New tests: `compiler/tests_layers.rs` (4), `tests_cold.rs` (5: a 200-turn thread under 20,000 with stubs and six turns kept; warm untouched; small cold thread takes
  fresh card; big results stubbed before a turn is dropped; card changes only at a recompile), `rpc/tests_thread_context.rs` (6, through the core):
  two threads and a DM render layers 1, 2 and 3 byte for byte and all marked `[true,true,true,false]`; shared topic gives identical layer 3 with the guidance;
  `thread.note` leaves every earlier message the same bytes, the next recompile shows the line; card keeps newest 12; cold thread recompiles
  (`cold_resume`, ≤ working tokens) while a warm one appends; an unthreaded session is not resumed small. `tests_standing` passes unmoved.
- **Planted reverts**: (a) a per-request `Now: <ms>` in layer 1 (`spec_of`): 5 of the 6 core thread tests failed (byte-identity first: "layer 1 differs at …");
  (b) the card refreshed on every turn (recorded snapshot ignored): `a_note_leaves_the_prefix_alone…` failed. Both restored and touched; `git status` clean.
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, CARGO_INCREMENTAL=0): door, keel (0 findings), fmt, shape, features, clippy, cockpit,
  test build, reader rule all pass. **Suite**: 4436 run, 36 failed: 33 are the known root-L1 `theseusd::sandbox` / sandbox-contract tests;
  `theseusd::cache_header` ×2 were mine (test rewritten above, both pass); `theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window…` failed
  under the suite and **passes alone** (not on the known list; I did not find a cause: not touched by this change). The later phase `protocol types` is clean
  (`git status` empty after the tests; `cockpit/src/protocol.gen/CacheSummary.ts` is committed).
- Benches (debug, this VM): `bench turn --check`: frames plain 4 / tool 8 / batch 14, syncs 2: all within budget (no frame added). `bench lifecycle --runs 10 --check`:
  startup 26/33 ms, shutdown 12.6/30, SIGKILL restart 28.9/40.6, swap 30/33.7 all ok; two **MISSED** that touch nothing here: health after the seed p95 7.3 ms vs 5, and
  `execution.cancel` p95 333.6 ms (one outlier of 10) vs 100: a busy 4-core debug build, rerun on the owner's machine.
- `scripts/keel-guard.py`: 0 findings. **Keel findings expected**: none.
- Disk: the VM ran out of space during the gate (debug test binaries); I deleted `target/` outputs (`cargo clean -p <workspace crates>`, `target/debug/incremental`).

## Live check for the maintainer (not run: no real keys here, and no stand-in-model daemon run)

I did not run the scratch-daemon live check. Do:
1. `theseusd --config <scratch.toml> --socket S --state-dir <fresh>` on the stand-in model with a context file and `[context] default_persona`; `theseus --socket S thread new tides`, `thread new moons`;
   `theseus --socket S ask --thread tides hi`, then `… --thread moons hi`.
2. `theseus --socket S ledger …` / `context explain`: both turns' `context.compiled` show `cache.breakpoints` = header, persona, project, conversation and equal `cache.layers.header|persona|project` digests.
3. Real model (two threads on Sonnet 5.5): the second thread's first turn must show `cache_read_input_tokens` covering layers 1 and 2 (and the tools block): the tools are identical between the two threads.
4. Cold resume: set `[cache]`/profile `cache_ttl = "5m"`, wait past it, message the 200-turn thread: `context.compiled` trigger `cold_resume`, `est_tokens` ≤ 60,000.

## Docs to change at review
`docs/spec` Part III item and `docs/status.md` (thread-context row done); core `AGENTS.md` updated here (new bullet "The layered request…"). `docs/design`: T13's prefix library builds on `compiler/layers.rs`.
