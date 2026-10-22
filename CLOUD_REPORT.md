# CLOUD_REPORT: cn-bg-rest (theseus-a1wvq.10)

Branch `cloud/20261021-cn-bg-rest`. One code commit, `5987455`, on the task commit `860134c`.

## 1. The names as built

- `Held.summaries: Vec<String>` (`turn/open_step.rs`): the `Summary` nodes this turn's compaction wrote over a cleared range.
- `open_step::writer(n, turn, summaries: &[String])` (now `pub(super)`): one new arm, `Kind::Summary if summaries.contains(&n.id) => Writer::Loop`, and the parameter. **`writer` is shared with cn-tool-screen (.9): the join takes both arms.** Its three callers (`carried`, `data_move`, `data_end`) pass `&t.data.summaries`.
- `turn/compaction/gate.rs`: `Route { profile, own, cleared, note }` (+ `Route::is_cleared`), `TurnRunner::summary_route(t, ring, nodes, (profile, own)) -> Result<Route, String>` (the `Err` is the ring's `why`), `open_run(t)`, `range_clearance(..)`, and the pure `judge(run, nodes, ids, (turn, summaries))`. Declared with one `mod gate;` line in `compaction.rs`. `compaction.rs` also re-exports `DEFAULT_PROFILE` (`pub(crate)` in `data_step.rs`) as `crate::turn::compaction::DEFAULT_PROFILE`, so consolidation reaches it without a line in `turn.rs`.
- Refusal's error type for replay: the existing `own_bench::replay::Crossing` (so the RPC and the sampler map it as they do), built by `Core::refuse_restricted`.
- Consolidation: `Core::unrestricted(profile, &mut report) -> Option<String>`; skip word `restricted_origin`.
- Test-only: `Config::serve_glm_as(model)` in `config.rs`.
- `.8`'s names were all as the brief said (`clearance`, `Carried`, `Carry`, `Writer`, `admits_again`); none renamed.

### Design points the owner should hear about

1. **The first loop's compaction runs before its turn's admission is read** (`compile_first` compiles beside Jev's verdict; `data_admit` runs after). The brief's `t.route.admitted.0` is still `Unchecked` there, so the gate judged "not admitted" and a restricted session's ring stood (the key test failed on exactly this). `open_run` therefore uses the turn's run when admission is known, and otherwise the session's **stored run, unless tainted**. The range is what came before the turn's input, so the stored run is the one that covers it; the pass is minted only for a range the stored run clears. If the verdict later says the input is private, the turn is not admitted and stores no run.
2. **Keep-warm: the check is before `keep_warm_request`, not after.** `keep_warm_request` itself stops a session whose target changed (`its next turn recompiles`) before the brief's check could run. It now reads `target_for_session` and forgets a restricted one first. No request, no blocked row. `tools_count_keep_warm` takes no open word, so it is left alone.
3. **The laundering test cannot discriminate at turn level.** In an admitted turn, a range that is not cleared always holds a non-loop node past `run.through` (a tool result, a steer), and that node alone already stops `data_end` storing a run. So "treat every Summary as the loop's" cannot make a turn-level test fail on this base. I did **not** write `a_summary_of_an_uncleared_range_never_joins_the_run` at turn level. Instead `gate.rs` has a unit test of that name on `writer` itself, which fails under the planted revert (below). The guard against laundering stands by `Held.summaries` only being pushed when the range was cleared (`compact`: `joins`). After cn-tool-screen lands (a screened result is `Loop`) this stays true.
4. The refusal text says `…is a model of cn origin (a restricted origin)…`: the brief gave both "`<origin>` is `cn` or `unknown`" and "`restricted origin` in its text".
5. Existing test fixtures changed (not weakened): `own_bench/tests_sampler.rs::two_providers` now serves `glm` as a US-origin model priced between Haiku and Sonnet (`serve_glm_as("claude-sonnet-5")` and a catalog price), because a replay to a restricted profile is now refused before the attachment check the test exercises, and the sampler's rung must still be `glm`. `config.rs` gained the 9-line test-only `serve_glm_as`.

## 2. What changed (lines after)

- `turn/compaction.rs` (1,024): `mod gate;`, the re-export, `compact` calls `summary_route`; `summarize` takes `gate::Route`, sets the pass on a clone of the request just before `stream_message`, the `Call` carries the row's note; `settle_summary` writes it as the row's `why`; `joins` pushes the summary id.
- `turn/compaction/gate.rs` (267, new): the above, with tests `range_clearance_matrix`, `a_range_outside_the_open_view_is_private`, `a_summary_of_an_uncleared_range_never_joins_the_run`.
- `turn/open_step.rs` (411): `Held.summaries`, `writer`'s parameter and arm, three callers' arguments.
- `turn/data_step.rs` (181): `DEFAULT_PROFILE` is `pub(crate)`.
- `consolidate/run.rs` (995): `unrestricted`, called after `synth_profile` in `consolidate_plan`; the dry run reports `default`; the report's `why` carries "synth_profile `<p>` is of restricted origin; written with `default`" and is joined with the verdict's why instead of overwritten.
- `keep_warm.rs` (908), `own_bench/replay.rs` (467): as above.
- `tests_bg_gate.rs` (263, new; one `mod` line in `lib.rs`), appended tests in `tests_consolidate.rs`, `tests_keep_warm.rs`, `own_bench/tests.rs`; `crates/theseus-core/AGENTS.md` (the "Background calls" passage under "The open request, per request"). No long-file ceiling touched; no line over 2,500.

## 3. Tests and their plants

All run with `TZ=America/Phoenix`. Each plant was applied, the test run, then the file restored and `touch`ed (`git status` checked).

| test | plant | failure seen |
|---|---|---|
| `tests_bg_gate::an_open_tasks_compaction_carries_its_own_pass` (T23a) | A: the summary's `data_pass` line removed | chokepoint refused: "the summary call failed … (data_gate) … has no pass"; ring stands |
| same | B: the `Kind::Summary` arm of `writer` removed | fails (the compaction's request is `unvetted`) |
| `tests_bg_gate::a_restricted_summary_profile_in_a_private_session_falls_back` (T23b) | route always `Ok` (no fallback) | fails: glm gets the call |
| `turn::compaction::gate::tests::range_clearance_matrix` | `judge` returns a cleared clearance | fails |
| `gate::tests::a_summary_of_an_uncleared_range_never_joins_the_run` | B above | fails (`Other` where `Loop`) |
| `tests_consolidate::a_restricted_synthesis_falls_back` (T23c: "session", named "glm", and a restricted `[model]` skipping as `restricted_origin`) | `unrestricted` always returns the profile | fails |
| `tests_keep_warm::a_restricted_target_is_never_kept_warm` | check disabled | fails |
| `own_bench::tests::a_replay_to_a_restricted_profile_is_refused_in_words` (both scopes) | check disabled | fails |

Not done: a planted run **under load** (none was asked for beyond the key test's plants; the new tests are event-ordered, no timing). The turn-level laundering test: see 1.3.

Results: `cargo nextest run -p theseus-core`: **3,683 passed, 0 failed, 25 skipped** (final run, after all edits). An earlier run had `own_bench::tests_sampler::an_attachment_is_never_sent_to_another_provider` failing: that is the fixture change in 1.5.

## 4. What could not run on the VM

- The gate's own run (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`) **fails in its keel phase**, naming changes that are not this row's: the gate's range base is `main`, and `integ/cn` carries the earlier rows (a keel-file finding for `rsi/sealed/relay.rs` whose ack's signature does not verify here, and moved `lifecycle` tests). The guard against `origin/integ/cn` is empty (below). So I ran the remaining phases myself: `cargo fmt --all -- --check` (ok), `scripts/shape.sh` (ok), `cargo clippy --workspace --all-targets -- -D warnings` (clean; it caught a 112-line `replay_turn_with` and a `clone`-to-slice, both fixed), and the whole theseus-core suite (above).
- **Not run**: the rest of the workspace's suite (other crates; I touched only theseus-core), the cockpit's lint/test/build (no protocol or cockpit change; `protocol.gen` untouched), the benches (`THESEUS_GATE_NO_BENCH`), and **`cargo deny --offline check`**: it failed here ("attempting to make an HTTP request, but --offline was specified"; the advisory db was not cached by `cargo deny fetch`). The maintainer's gate covers all of these. The ~33 known L1 failures are in crates I did not run.
- Maintainer commands for the rest: `TZ=America/Phoenix scripts/gate.sh` at the join; `cargo deny check` with network.

## 5. The keel guard

`git fetch origin integ/cn` then `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` printed nothing (exit 0).

## 6. Store format

store format: unchanged (48). No stored record gained a field; the new words are in an existing row's `why` and in returned errors.

## 7. The maintainer's live check, on a scratch daemon, after the join

Scratch state dir, `trivial.cn_profiles = ["glm"]`, `[memory] synth_profile = "session"`. Start an open task on glm (`theseus task start --open`, profile glm) with a brief long enough to compact (use a small `[context] working_window_tokens`).

1. Read `context.compacted` rows (ledger kind): the task's row has `outcome: compaction`, `profile: glm`, and no `data_gate.blocked` row exists. The summary call carried a pass.
2. `session.data` in `theseus inspect`/the record: `run.through` is the task's last node after the turn.
3. `theseus memory consolidate` (after sources are last on glm): `synthesis.proposed` rows say `profile: default`, and the cluster's `why` says "written with `default`". A dry run reports `default` too.
4. `theseus model replay <session> <turn> glm` is refused with "not replayed on `glm`: … is a model of cn origin (a restricted origin) …", with no `turn.replayed` row.
5. A DM session last on glm with keep-warm due: no `keep_warm` row and no `data_gate.blocked` row.

## 8. Follow-ups

- **cn-observability join** (`record_data_gate`, what / why): compaction `compaction` / `data_gate: <word>` (both the ring and the default-fallback arms, `gate.rs` `summary_route`); consolidation `consolidation` / `restricted_origin` (skip) and `synth_profile restricted` (fallback), `consolidate/run.rs::unrestricted`; keep-warm `keep_warm` / `restricted_origin`, `keep_warm.rs` `keep_warm_prepare`; replay `replay` / `restricted_origin`, `own_bench/replay.rs::refuse_restricted`.
- The other background callers (initiative, learning audit and writer, people, cards, naming, census, thread import's card) still fail at the chokepoint with a `data_gate.blocked` row if configured restricted.
- A replay of an open turn to a restricted profile with a pass of its own.
- Docs to change at review: nothing in the spec or status beyond Part III's item; `docs/design/roadmap-v2.md` row for a1wvq.10.
