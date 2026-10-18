# CLOUD_REPORT: flow-runner (theseus-pw1q.21.4), branch cloud/20261017-flow-runner

Commit: 93555db2 (the step). No store format change (a META key `flows` and three ledger kinds), no new dependency (Cargo.lock unchanged).

## What I found
- A step's `ToolCall`/`ToolResult` nodes with ids not in the assistant message are ignored by the renderer (`results_of` looks results up by the assistant's own tool_use ids), so nested step calls do not break `compiler::situation::closed`.
- `toolrun::admit` + `start` are exactly what `run_batch` does per call; a flow step is those two for one `ToolUse`.
- The old `origin/main` ref in this container was older than my base (40aa83cd), so the gate's door and keel judged other work as erosions. I set `refs/remotes/origin/main` locally to 40aa83cd (no push) and the gate then judged my range: `keel: ok, 0 findings`.

## What I changed
- `flow/{mod,spec,class,load,run,tool}.rs`: the v1 spec (parse, validate, canonical SHA-256 digest, bindings), result classes, freeze/load/retire/board, the run engine, the per-flow tool. `toolrun/flow_step.rs`: `flow_step` (admit+start) and `run_flow`. `fact/flow.rs`: `flow.loaded`, `flow.retired`, `flow.ran`. `theseus-protocol/src/flow.rs` (+ one `pub mod`, + 3 `LedgerKind`s).
- Small edits in shared files: `toolrun.rs` (mod line, `flows` field, tool/tool_by_wire/definitions_for branches, one `execute` arm; `guarded.rs` untouched), `policy/modes.rs` (one arm: a flow's own call is open), `tools_defer/family.rs` (`flow__x` -> family `flow:x`, summary), `turn/defer_step.rs` (`flows_in_scope`), `rpc/mod.rs` (`seed_flows`, 1 call), `rsi/revert.rs` (one verb arm), `lib.rs` (mod line), AGENTS.md files.
- Design choices the owner should hear:
  1. **No separate `flow.run` tool.** Each loaded flow is its own tool `flow:<name>` (wire `flow__<name>`, deferred family `flow:<name>`); a generic `flow_run` would add a tool to every request with none loaded.
  2. **Approval is handed back, not waited for.** A step whose gate says "ask" is not asked: the flow stops with `needs-approval` and the exact call, and the model makes it itself (so it waits as any call does). A flow cannot hold state across a parked turn. A refused step stops the flow with the refusal as an error result.
  3. Scope: `scope.repos` (>=1 id `r:<hex>`, = first 12 hex of sha256 of the repo root path, `spec::repo_id`) and `scope.places` (PlaceKind names). Offered by `flows_in_scope` and checked again at call time.
  4. A spec's step ids and input names may coincide (the design's example does: `test`).
  5. `Core::flow_load/flow_retire/flow_list` exist, but **no wire method or CLI loads a flow** (shared files: protocol lib.rs at its ceiling, server.rs, main.rs). A load by the `"self"` principal asks `rsi::gate` first. R8 or the join adds `theseus self flow load`.
- Result classes: mine is `flow::class::classes` / `Seen` (+ `program_output`, `conflict_files`, `listed_paths`). R5's own is on its branch; the joiner should fold it into mine.
- Known limits: step calls have no turn spans/metrics (their ledger rows, ids `<call>.<n>` and `ms` in the result's meta exist); keep-warm, census and `context.explain` specs are built without a session directory, so they show all loaded flows' tools (keep-warm cache mismatch only while a flow is loaded). Step output comes from the capped result text. L1 steps are not exercised (root VM).
- Docs to change at review: spec Part III item and version line; `docs/status.md`; the self log (`SELF_ROWS`) could list `flow.loaded`/`flow.retired`.

## How I proved it
- `cargo nextest run -p theseus-core -E 'test(/^flow::/)'`: 30 passed (spec/class units; real temp git repos with an upstream: clean merge, rerere-known conflict, unknown conflict -> hand-back with files, red tests, refusal via the sealed dir, needs-approval, scope, catalog, digest freeze, secret, gate, revert).
- Planted reverts (each restored, `touch`ed; `git status` clean after):
  - add `channel.post` to ALLOWED_TOOLS -> `a_disallowed_tool_cannot_be_named` failed
  - skip embedded-digest check -> `the_digest_follows_the_content...`, `the_digest_freezes_the_spec` failed
  - ignore the "Not run:" refusal -> `a_step_refused_by_policy_stops_the_flow` failed
  - offer flows everywhere (`flows_in_scope` -> is_some) -> `a_flow_is_deferred_and_scoped`, `a_flow_names_the_kinds_of_place...` failed
  - swap rerere-left/resolved in class.rs -> class unit + both rerere tests failed
  - map `rerere-left` to `goto:commit` in the spec (the brief's revert) -> `a_conflict_rerere_does_not_know...` failed
  - steps not run through `start` -> `each_step_writes_its_own_rows`, `a_clean_merge...` failed
  - retire does not drop from the board -> `no_flow_loaded_changes_nothing`, `the_owners_revert...` failed
  - scrub check off -> `a_secret_shaped_constant...` failed; self gate off -> `a_self_minted_load_asks_the_gate_first` failed; scope backstop off -> `a_flow_called_out_of_scope_does_not_run` failed; revert arm off -> revert + no-change tests failed; asks treated as run -> `a_step_that_would_wait...` failed; gate arm (`flow::quiet`) off -> same test failed.
- No run under load (no race fixed).

## Live check for the maintainer
`cargo nextest run -p theseus-core -E 'test(/^flow::/)'` (all 30 pass; they use real git and in-process jobs). On a scratch daemon there is no load verb yet; once one exists: load a spec, start a turn in the repo, expect `flow__repo_land` as a deferred tool, call it, expect `flow.ran` and one `tool.job_started` per step; `theseus self revert flow:repo_land` expects `flow.retired {by: owner}`. Run the benches (lifecycle, turn: 5 frames) on the owner's machine: I skipped them.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule all passed. Suite: 6463 run, 6424 passed, 33 failed (all theseus-sandbox contract/bench and theseusd sandbox L1 tests: root VM, theseus-pv6i), 6 timed out (theseus-discord tests_default_forum/tests_task_threads/tests_war_room; two of them time out identically on the untouched base, so they are this VM's). The phases after the suite (protocol types: no change in `protocol.gen`) were checked by hand; the benches were skipped (NO_BENCH); `cargo deny` could not run offline (no advisory db fetched; Cargo.lock unchanged). The first gate died on a full disk, so I removed `target/debug/incremental` twice.
Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`, origin/main = 40aa83cd): empty output.
