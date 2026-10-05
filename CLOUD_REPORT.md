# Cloud report: memory-checks (branch cloud/20261005-memory-checks)

Four steps, one commit each, in crates/theseus-core. No store-format change, no config key, no protocol type, no new dependency.

## 1. theseus-3edq: the build's walk paced (1a0df0a8)

**Found.** `Projection::build` is `refresh` from 0, and both walk through `for_each`. A pace in `for_each` would also hit a turn's refresh, so the pace is passed in. `warm_labels`'s walk (recall/labels.rs) is a separate, unpaced one; I left it. It reads one kind's rows, and I'd pace it too if it shows on a big store: the live check's `dd` run will show it, since a pressured start finishes the labels walk first (opinion, unmeasured).

**Changed.** `Projection::refresh_paced(store, &mut dyn FnMut())` and `build_paced` call `pace` before every page after the first (nodes, edges, ledger, both ledger paths). `refresh` and `build` pass a no-op. `Adjacent::build(store, paced)`: the warm build (`warm`) passes true and waits `quiet_blocking(BOUND)` between pages, and the log line has `waited_ms` beside `took_ms`. A search's own build (`build: true`) passes false: it holds the mutex inside the turn's/search's deadline, and a person waits on it, so a wait of up to 10 s a page would only make it time out while the thread keeps working. `adjacency::pace()` is the one waiting function and counts calls in a test-only thread-local.

**Proved.** `tests_activation_pace::the_warm_build_paces_between_its_pages_and_a_refresh_never_does` (4097 nodes = PAGE+1): the warm build paces once, a search's build 0, and a refresh over PAGE+1 more writes 0. Planted: `pace()` called unconditionally in `for_each`: fails ("a search's build waits for no one", left 1, right 0). Restored and touched. Cost with no pressure, 9,000 nodes, debug build (3 runs): unpaced 68/48/47 ms, paced 88/49/47 ms; three pages' PSI reads are well under 1 ms.

## 2. theseus-cn0b: shadow / live-baseline turns leave retention unasked (7234b0d3)

**Found.** The shadow path calls `scene(.., MemoryArm::Baseline)`, so shadow never reads the arm. Nothing in the test rig warms retention (`warm_retention` is theseusd's startup).
**Changed.** `tests_retention::a_shadow_turn_and_a_live_baseline_turn_leave_retention_unasked` (shadow + `+retention`, live + `baseline`; one turn each; `phase() == Unasked`).
**Proved.** Planted `if true {` in `scene()`: fails ("a turn in Shadow mode under Retention asked for retention", left Ready, right Unasked).
**Opinion asked for.** Should a shadow daemon warm retention (`warm_retention` does, whenever memory is on and the arm reads it, shadow included)? Its shadow turns never read it, so the warm only costs a walk, but the `+retention` arm's first live turn then finds it ready. I'd leave it: shadow is where an arm's data gets gathered.

## 3. theseus-syxg: additions skip what the turn holds (1aa3876b)

**Found, a difference from the brief.** The index's hits are seeds, and a spread does not reach its seeds (`boosted: 0` in the run), so under this shape only the context nodes make the plant fail; `exclude` holds the hits back only as a second guard, which no test of mine exercises.
**Changed.** `tests_activation_arm::activations_additions_skip_what_the_turn_already_holds`: 2 context nodes, 2 index hits, 25 others, all sharing one commit; recall_max_items 40 and a large budget. Asserts `added == admitted_added == 20` (`adds`; no cap past it), every addition among the others, none held, none twice, no `in_context` drop.
**Proved.** Planted: `Ask::run`'s `exclude` skip deleted: fails (`admitted_added` 18, not 20: the context took two slots).

## 4. theseus-q0qe: a stub's kind agrees with its body (39835348)

**Changed.** `tests_stub_kinds::a_stubs_kind_agrees_with_its_body`: one sample per `Body` variant written through a store, which is then reopened (a fresh cache); each stub is read from bytes, so the kind comes from the peek. Asserts not hydrated, `node_cache().decodes() == 0`, `stub.kind == Kind::of(body) == the hand-written expected kind`, and `is_summary` for the summary alone, plus `summary_last`. `number()` matches on `Body` with no wildcard, so a new variant fails to compile there. The cache path is avoided by the reopen and the `decodes()` check.
**Proved.** Planted `Body::Synthesis => Kind::Summary` in `Kind::of`: fails (left Summary, right Synthesis).

## Proof overall

- Each new test 5 times under load (`nice -n 19`, four busy loops at nice 0; the cargo build done first, since a niced cargo starved): 5 of 5 runs, 4 of 4 tests each, all green.
- The gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`) on the whole tree: fmt, shape, features, clippy (`-D warnings`), cockpit, test build, reader rule all green. Suite: 2793 run, 2760 passed, 33 failed, all L1 on this VM as the brief names (theseus-sandbox's contract tests and `spawn_100`, theseusd's `sandbox::*`); theseus-core's whole suite, including every recall, retention, activation, tiering and consolidation test, passed. protocol_types is clean (no change under cockpit/) and `cargo deny --offline check` is ok. Lifecycle, jobs and turn benches not run (`THESEUS_GATE_NO_BENCH=1`). The gate ran on the four steps together; the commits were then cut, each compiling alone (the first without the later tests' mod lines).

## Live check for the maintainer

1. `theseus-sim synth-store` a scratch state dir (some thousands of nodes); config `[memory] mode = "canary"`, `arm = "+activation"`, Discord and web off, model `theseus-sim fake-model --rules`. Start while `dd if=/dev/zero of=<scratch> bs=1M count=4000 oflag=direct` runs: the log line "memory: the adjacency projection is built" shows `waited_ms` above 0 (IO pressure over 10 %), and a turn during the build answers `building` at once. Without the `dd`, `waited_ms=0`.
2. `mode = "live"`, `arm = "baseline"`: after a turn `theseus health`'s memory line names no retention.

## Left / uncertain

- A pace waits at most 10 s a page (BOUND) and 4096 records per page, so a store with a busy machine finishes late, never never. The spread and refresh still wait on the mutex the warm build holds only in a search (`build: true` skips the pace, so it never waits longer than before).
- Docs: AGENTS.md of theseus-core's "adjacency projection" bullet could say "the warm build waits between its pages while the machine is busy; a refresh and a search's build never do". I did not edit docs.
