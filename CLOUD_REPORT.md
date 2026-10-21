# CLOUD_REPORT: detours-why (theseus-v4u66, theseus-r5ydi, theseus-3bs4e)

Branch `cloud/20261021-detours-why`. Code commit `7d92404`.

## 1. Files over a line ceiling

None. (`route_step.rs` 1,128 lines, `rpc/context.rs` 699; new code is in `turn/detour_step.rs`, `fact/detour.rs`, `tests_detour.rs`.)

## 2. What changed

One commit for all three pieces, not three: they share `route_step.rs` and `tests_detour.rs`, so a split would not build or pass alone. Rollback is the revert of `7d92404`.

- **A (v4u66).** `compile_detour` calls `project::slice::attach` last, as `tail_blocks` does. No project attached: nothing added.
- **B (r5ydi).** New ledger kind `context.detoured` (`fact/detour.rs`, type `ContextDetoured` in `protocol/context.rs`), recorded per detoured loop beside `loop.started` in the same frame. `context.explain` reads it when a turn has no `context.compiled` row: new optional `detour` field, `ContextTurn.detoured`, parts from the row, profile and model the turn ran on, recalls via `turn_recalls`. Words: `why::detour_words`, a "left out" list, `fill::context_short`. TypeScript regenerated. No store-format change.
- **C (3bs4e).** `want::extra(ask, catalog, loaded, &Fit)` keeps a named family while `Fit::total` (system + held + loaded + the family) is within the budget; best first, a family that does not fit is skipped. `detour_spec` does the clearing; `compile_detour` and `defer_tools` both call it, so the fit is counted on the request a detour sends. The owner's-word comment is gone.

Before, on a detoured turn: `no turn of this session has compiled a context yet`. After (golden `crates/theseus-protocol/tests/why/detour.txt`):

```
turn trn_k3m9x2 · session ses_reef42 (the harbour survey)
place: CLI (private)
detour: ran as a quick detour on haiku (claude-haiku-5-5); its request was built outside the conversation's compilation
ran on: haiku (claude-haiku-5-5), a window of 200,000 tokens
routing: task class quick, a detour, outside the session's compilation, on haiku instead of sonnet
context: 6 parts, 7,369 tokens
  header ... exchanges ... message ... project ...
left out:
  context files: a detour carries none
  the persona's body: a detour carries only its tail line, when one answers
  the conversation's summary and older turns: only the last exchanges ride a detour
  earlier recall: a detour carries none
```

(A trivial detour also lists "this turn's own recall".) The CLI's `why.rs` needed no change: the route row is found by the turn id, which `context.explain` now returns for a detour.

## IMPORTANT: a finding that contradicts the brief

The brief says `tests_route_defer::a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget` must stay green unchanged because a detour's request is small. It is not small at the cli: measured on the rig, system 922 + deferring 226 + files/commands/memory 5,975 = 7,325 tokens before any named family, and the AWS families cost 892 to 1,271. So 8,519 > 8,000 and no AWS family fits at the cli under the whole-request rule. Same for terminals (1,264) in `tests_defer`. Only small families fit (ask 253, projects 153, threads 184).

I applied the rule as the owner asked and changed the tests, not the rule:
- `a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget` now runs in the owner's DM (budget 15,000); assertions otherwise identical, budget constant `DM_TOKENS`.
- `tests_defer::a_model_that_never_calls_tools_find_still_reaches_the_family_the_message_names` names the projects family instead of terminals.
- `want.rs`: the two budget tests now pass a `Fit` (same names kept, so the keel guard sees no removed test).

The owner should decide whether a cli detour reaching AWS only by `tools_find` is acceptable. Alternative: let a named family displace a default family, or give a detour a smaller default set.

## 3. Tests and planted reverts

Plants were temporary env switches in one build, removed afterwards (files restored and touched).

| Test | Plant | Failed |
|---|---|---|
| `a_quick_detour_in_a_project_conversation_carries_the_slice` (key) | skip `attach` | yes ("no slice at the tail"); `why_explains_a_detoured_turn` failed too |
| `why_explains_a_detoured_turn` | do not record the fact | yes; `a_detour_naming_...` and the frames test also failed |
| `a_detoured_plain_turn_keeps_its_frames` | fact in its own frame | yes (5 frames vs 4) |
| `a_named_family_is_counted_on_the_whole_request` | ignore `fit.system` | yes (`{aws, terminals}` vs `{terminals}`) |
| `a_detour_naming_a_family_stays_within_its_place_budget` | fit on the full routed spec | yes (the family never loads) |
| `a_compiled_turn_explains_as_before` | none (the detour path is taken only without a compile) | n/a |

The budget test first passed under the plant because the owner-shape rig had no context file; I added a 2,000-token one, which the full spec carries and a detour does not. Protocol golden test: `a_detoured_turn_says_it_detoured_and_what_it_left_out`.

Frames: a detoured plain turn is 4 frames, with and without the fact (the plant with the fact skipped also read 4).

## 4. The fact's fields

`context.detoured`: `session_id`, `turn_id`, `loop`, `profile`, `model`, `kind` (trivial|quick), `class`, `est_tokens`, `parts[{block,name,tokens}]` (header, session, tools, persona, standing, exchanges, message, recall, project), `families`. Names and counts only. Known gap: a part's `bytes` is 0 in `context.explain` for a detour (only tokens are recorded).

## 5. Not run here

The disk allowance filled twice. The gate's whole-workspace test build could not finish, so the gate was not green end to end. What ran:
- gate phases keel, fmt, shape, features, clippy (workspace, `-D warnings`): passed.
- `cargo nextest run -p theseus-core -p theseus-protocol -p theseus -p theseus-discord -p theseus-tui`: 5,398 tests, one failure (the terminals test above), fixed and re-run green (`tests_defer::`: 21 passed). Not re-run: the full set after that one edit.
- Not run: theseusd, theseus-sim, other crates' tests; cockpit lint/test/build; deny; benches.

Maintainer: `TZ=America/Phoenix scripts/gate.sh`, then `theseus why --session <a session whose one turn detoured>` on a scratch daemon.

## 6. Keel guard

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty, exit 0.

## 7. Follow-ups

- Cli detours cannot reach an AWS or terminals family by naming it (see above).
- `bytes` for a detour's parts.
- Docs to update at review: spec Part III item for these three beads; `docs/status.md`. I updated `crates/theseus-core/AGENTS.md` and `crates/theseus-protocol/AGENTS.md`.
