# Cloud report: xlri5-families (theseus-xlri5)

Branch `cloud/20261021-xlri5-families`, from `integ/threads` at `6d4e0dd9` (the task commit `f0e2ec8` on top).
Store format unchanged at 48. No new dependency (Cargo.lock and the package-lock files are untouched).

## First: files at their ceilings, and one line of another row's

- `crates/theseus-protocol/src/lib.rs` is now 2,733 / 2,733: `TurnSubmitResult.raised` took the three lines of room as
  `#[serde(default, skip_serializing_if = "Option::is_none")]`, `#[cfg_attr(test, ts(optional))]` and the field. The
  doc line the brief asked for would have made four, and the crate's invariant needs `ts(optional)` for an absent
  key, so the field has no doc of its own: the type `route::ToolsRaised` carries it. If a sibling adds a line at the
  join, lib.rs is one over.
- `crates/theseus-core/src/turn.rs` grew 3 lines (3,414 to 3,417 of 3,525): the `mod families_step;` line, the
  `families_ready` call before `defer_tools`, and `raised: None` in the result's literal (the result takes
  `defer_ended`'s return on the line that already called it).
- `turn/route_step.rs`'s `compile_first` sits at clippy's 100-line limit: the routed call is now
  `self.routed_to(..).await?` (one line, in place of `route_to(..)?`), a helper in `families_step.rs` that takes the
  judgment for the routed model and then calls `route_to`. `route_to` stays sync.

## What changed

**Commit `16d99b2`: `judge: families.v1, Jev judges each hidden tool family against the prompt (theseus-xlri5)`.**
theseus-judge only, plus the one match arm core's `learning/rebuild.rs` needs for the new `Builder`.
- `packs/families.v1.toml`: `point = "inbound"`, `action = "none"`, `baseline = "none"`, two per-item Nouls,
  `relevant` (`per = "families"`, the first ten) and `relevant_more` (`per = "more_families"`, the next ten), as
  `citation.v1` splits its pairs. The brief asked for one question at the per-item limit (10); the owner's cli hides
  about 20 families (web, tasks, reminders, routines, threads, terminals, code, channels, six AWS families, ask,
  projects, extensions, and MCP servers), so one question would cut them by table order. Bars as `recall_cross.v1`'s:
  act 0.90, confirm 0.60. The confirm bar is the admission bar: a wrong yes costs tokens (bounded by the ceiling),
  never correctness, and the families admitted are only the ones the request hides.
- `builders/families.rs`: `Input::Families(FamiliesInput { message, families: Vec<FamilyItem { key, holds, tools } > })`,
  at most 20 families, three tools each (wire name and the description's first sentence), every string scrubbed and
  clipped, the state under `state_cap_tokens` (3,000). Its dynamic items are the families the state kept, so Jev is
  never asked about one it cannot see.
- Fixtures: `inputs/families.json`, `golden/families.state.json`, `golden/families.v1.request.json`, and the
  labelled set `eval/families.v1.json` (7 cases: AWS by "which cloud account", a REPL needing terminals, background
  work needing tasks, an MCP server's readings, AWS only mentioned, a thank-you, a code question needing none).
  `EMBEDDED` (24 packs), `replay.rs`'s builder identity, `eval_labelled::PACKS`.

**Commit `b87e310`: `defer: admitted families raise the first request past the budget, under a ceiling (theseus-xlri5)`.**
- A. `[tools.defer]` (`config/defer.rs`, template, `check`): `admit_ceiling = 2.0` (refused outside 1.0 to 3.0),
  `judge_families = true`, `judge_wait_ms = 200` (at most 2,000). The doc comment carries the reasoning for 2.0.
- B. `Fit` gains `ceiling` (`floor(budget × admit_ceiling)`, the multiple passed to `Fit::of`) and `raised`.
  `want::extra(ask, jev, catalog, loaded, fit) -> Vec<Admitted { family, tokens, by }>`: named families first (by
  `named`'s score), then Jev's (by Noul, highest first), each hidden and hideable; nothing when the base is over the
  budget; each kept while the whole stays at or under the ceiling, a family past it skipped and a smaller one after it
  tried. `Fit::demote` holds learned families to `budget + raised`. `deferral_in` sets `fit.raised` from what `extra`
  kept, before the second `offer`. `Mechanism::Search` is untouched: no Jev set read, no raise.
- C. Jev in the loop: `judge/families.rs` (`JudgeService::families_start -> Option<oneshot::Receiver<Relevant>>`,
  `relevant(&Judgment)`: actionable, not shadow, Noul true at the confirm bar, with the Noul as its confidence), wired
  **live** in `WIRED`. `turn/families_step.rs`: `families_start` at the inbound point (after the judge-enabled check,
  not for a steered message) starts it when deferral is on, `judge_families` is on, the message is not empty, and the
  session's model has no tool search or the route pack is live (a detour may go to one that has none); it parks the
  receiver and `deadline = inbound + judge_wait_ms`. `families_ready(t, model)` runs before each `defer_tools` (the
  session's spec in `turn.rs`, the routed spec in `compile_first`): nothing for a tool-search model; otherwise it
  waits until the deadline at most, once, and keeps the set for the turn. The defer board keeps the conversation's
  last set by the prompt's digest (memory only): the same prompt again asks nothing, a turn with no new message
  reuses it, a new message not asked about gets none. Candidates (`hidden_families`): the place's catalog's hideable
  families less `always_loaded`, the place kind's own defaults and `plan` (its mode loads it), in the families
  table's order, at most 20. I read "not loaded by default everywhere" as the place kind's defaults: the families
  loaded by default in every kind are none (the task kind has none), which would have asked about `memory` at the cli.
- D. `ToolsRaised { budget, ceiling, tokens, families: [RaisedFamily { family, tokens, by }] }` and its `line()` in
  `theseus-protocol/src/route.rs`; `harness_lines()` chains it, so Discord, the CLI and the TUI print it with no
  renderer edit. It rides `Deferral.raised` (not stored; an append over a recorded set takes `raised: None`), then the
  board's note for the turn (`note_raised`, set in `defer_noted` only when the compile loaded every raised family),
  `TurnUse.raised`, `tools.used`'s additive `raised` key (only when something passed the budget; families admitted
  under it are not listed), and `TurnSubmitResult.raised` from `defer_ended`. `cockpit/src/protocol.gen/` regenerated.
- E. `tests_route_defer::a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget` back at the cli with its
  `88ddae57` body, plus `raised` naming `aws`, `by: named`. `tests_defer::a_model_that_never_calls_tools_find_still_reaches_the_family_the_message_names`
  back to its terminals body; the projects variant is the new
  `a_model_that_never_calls_tools_find_reaches_a_small_family_the_message_names`.
  `tests_detour::a_detour_naming_a_family_stays_within_its_place_budget`: name and assertions unchanged, its rig sets
  `admit_ceiling = 1.0`, its doc says it pins the rollback. `want.rs`'s three tests from before keep their assertions:
  their fixture's ceiling is the budget (`admit_ceiling = 1.0`) and a test-module wrapper keeps `extra`'s old
  four-argument, set-valued form for them.
- AGENTS.md: one paragraph in `crates/theseus-core/AGENTS.md` (after the deferred tools' tests line), one line in
  `crates/theseus-protocol/AGENTS.md`.

## Measured: before and after

Census (`cargo nextest run -p theseus-core every_kind_of_place_is_within_its_budget --no-capture`, `TZ=America/Phoenix`),
on the base (my changes stashed) and on the head: every figure unchanged (an empty `ask` admits nothing, and Jev is
not asked).

| first request | base | head |
|---|---|---|
| cli | 7,353 | 7,353 |
| dm | 9,432 | 9,432 |
| channel | 7,391 | 7,391 |
| task | 6,606 | 6,606 |
| headless | 7,867 | 7,867 |
| owner's shape, cli | 7,548 | 7,548 |
| owner's shape, dm | 13,027 | 13,027 |
| owner's shape, channel | 7,838 | 7,838 |
| owner's shape, task | 6,801 | 6,801 |
| owner's shape, headless | 8,062 | 8,062 |

The cli detour in the owner's shape (Haiku 5.5, `context.detoured`'s `est_tokens`; base from a scratch test on the
stashed tree, not committed; head from the key test's `eprintln!`):

| message | base | head |
|---|---|---|
| "Call aws_whoami and tell me in one line which account you are on." | 7,139, no AWS tool | 8,331, `aws_whoami` offered (the AWS family is 1,194 tokens by `Fit`; the request grew 1,192) |
| "Open a terminal and run true." | 7,128, no terminal tool | 8,389, `term_open` offered |

Both under the ceiling (16,000). The detour's base is 7,130 to 7,140 here, a little under the brief's 7,300.

## Tests, and each planted revert

All in `crates/theseus-core/src/tests_families.rs` unless named. Each plant was made, the test run and seen failing,
the file restored and touched, and `git status` checked (`/tmp` plant script; every restore left only my own edits).

| # | test | plant | result |
|---|---|---|---|
| 1 (key) | `a_cli_detour_naming_aws_loads_it_past_the_budget_by_exactly_its_size`: the first request offers `aws_whoami`; it grew by the family's `Fit` cost within a tolerance of one token per AWS tool plus one (the estimate rounds the tools' bytes once, `Fit` once per tool; measured 2); over the budget and under the ceiling; `tools.used` `raised.families` = `aws`, `named`; the "Tools raised" line in `harness_lines()` | `extra` holds to `fit.budget` | FAILED (and with it `admission_stops_at_the_ceiling` and both restored tests) |
| 2 | `a_family_jev_judges_relevant_loads_without_being_named` ("Which cloud account is this session on right now?"; FakeJev scripts the AWS family's own question, read from a first call's numbering): loads, `by: jev`; with `judge_families = false` no AWS tool, no raise, no `families.v1` call | `deferral_in` ignores Jev's set (`&[]`) | FAILED |
| 3 | `jev_late_or_unreachable_falls_back_to_the_named_rule`: FakeJev holds the families state (released after 3 s); the named rule loads `aws_whoami`; the `judge` wait span says `answered: false` and lasted at most `judge_wait_ms` + 250 ms; Jev unreachable (dead `api_base`, session model without tool search): no `families.v1` span at all | wait without the deadline | FAILED |
| 4 | `one_families_judgment_per_prompt`: a three-loop detour turn, compiled again on the routed profile, makes one `families.v1` call; a second prompt a second | start the call in the step before each `defer_tools` (removed from inbound) | FAILED |
| 5 | `tools_defer::fit::tests::a_raise_never_evicts_or_admits_a_learned_family` | `demote` back to `budget` | FAILED |
| 6 | `tools_defer::want::tests::admission_stops_at_the_ceiling`: named then Jev, a family past the ceiling skipped and a smaller one after it kept, a named family not admitted twice, a hidden family only, a base over the budget admits nothing | `extra` holds to `fit.budget` | FAILED (plant 1) |
| 7 | the restored AWS and terminals tests | default `admit_ceiling` 1.0 (code and template: the rigs load the template) | both FAILED |
| 8 | `a_detour_naming_a_family_stays_within_its_place_budget` at `admit_ceiling = 1.0` | (the rollback itself) | passes |
| 9 | `a_model_with_tool_search_asks_no_families_judgment_and_raises_nothing` (cli defaults with an AWS account, Sonnet with tool search, route pack off) | (a) drop the `Find` check in `deferral_in`; (b) drop the tool-search check at the start | both FAILED |
| 10 | `config::defer::tests::a_ceiling_or_a_wait_out_of_bounds_is_refused` (0.5, 3.5, 5,000 ms), the template test extended | `check` refuses no ceiling | FAILED |
| | theseus-judge: `builders::families::tests::each_family_is_asked_once_with_three_of_its_tools`, both golden tests, the eval set's shape test, the pack count (24) | (pure additions with goldens) | pass |

Unchanged and passing: all of `tests_first_request`, `tests_m3::a_plain_turn_stays_within_its_frame_budget`, the
existing `want.rs` and `fit.rs` tests. The wired-pack lists in `tests_judge` (2), `tests_inbound`, `tests_continue` and
theseusd's `tests/judge.rs` gained `families.v1: live`.

Latency on a test turn (FakeJev answering at once): the judgment is marked at the inbound point (8.2 ms into the
turn) and taken at the routed recompile (29.3 ms) with a 4 µs wait: it is done long before the route verdict. A real
Jev answers one pack's request in a few hundred ms; the wait is bounded at 200 ms from the inbound point, so a turn
whose session model has tool search never waits, and a Haiku detour waits only for what is left after the route
wait (which is up to `[routing] max_wait_ms`, 200, from the same start).

Jev's cost per prompt (`price.rs`: jev-1.13.0 at $0.042 per million tokens in and out): a state of about 2,000 tokens
(20 families) and 20 questions. If the state is billed once: about 2,000 + 20 × 40 + 320 ≈ 3,100 tokens, $0.00013. If
billed per question, as the fake models it: 20 × 2,040 ≈ 41,000 tokens plus 640 out, about $0.0017. Each call's real
cost is in its `judge.call` row (`purpose: "families"`).

## The ceiling: reasoning and worst case

2.0 × the place's own budget: the smallest round multiple that admits the largest single family, the tasks' (about
6,000 tokens), beside a cli detour's base with room for an AWS family and the terminals (7,300 + 6,000 + 1,271 + 1,264
= 15,835 < 16,000); at 1.5 (12,000) the tasks could never load at the cli. A multiple keeps one set of numbers (the
keel's constants). Worst-case extra per request at the cli: 8,000 tokens past the budget. At Haiku 5.5's catalog
price ($0.10 per million input, $0.125 per million cache write, $0.01 cache read) that is $0.0008 uncached, $0.001
as a cache write, $0.00008 a cached read thereafter. At a DM (budget 15,000, ceiling 30,000) twice that.

## What could not run here, and the maintainer's live check

Not run on this VM: no API keys, so no live Jev, no real model, no bench. The 33 or so L1 tests fail here as known
(root daemon, theseus-pv6i).

Live check, on the owner's cli with routing live and a Haiku-class detour, on the install build of this branch:

1. `theseus ask "call aws_whoami and tell me the account"`: the reply calls `aws_whoami` with no `tools_find`, and
   its footer ends `Tools raised the request to …: aws +… (named).`
2. `theseus ask "which cloud account am I on?"`, then `theseus ledger -k tools.used -n 1 --json`: the turn's row has
   `raised.families` holding `{"family": "aws", "by": "jev"}` when Jev judged the family relevant (the reply's footer
   says `(Jev)`).
3. `theseus judge log --pack families.v1`: one `families.v1` judgment per prompt, with its answers, cost and Jev's
   time; the same prompt again in the same session (`theseus ask -s <session> …`) adds none.
4. Rollbacks: `[tools.defer] admit_ceiling = 1.0` (no raise, no footer line), `judge_families = false` (no
   `families.v1` judgments).

## Keel guard

`python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet`:

```
(no output; exit 0, run on b87e310)
```

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the tree that became b87e310 (`CARGO_INCREMENTAL=0`,
to stay inside this VM's disk allowance; two earlier runs died on `No space left on device` and one on a rustfmt
line, fixed). Door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule passed.
The suite: 7,353 tests, 7,319 passed, 34 failed, 53 skipped. The 34:

- 33 known L1 failures of this VM (root daemon, no job cgroup, theseus-pv6i): theseus-sandbox's `bench spawn_100` and
  its 20 `contract` tests, and theseusd's 13 `sandbox` tests.
- `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` ("the live wrapper's job
  runs on: dispatched 2"): a timing failure under the suite's load, in code this row does not touch; rerun alone it
  passed (1/1). It is not on the flaky list.

The phases after the suite, run by hand (`machine_checks`): protocol types ok (`cockpit/src/protocol.gen/` staged and
committed); lifecycle and jobs benches skipped by `THESEUS_GATE_NO_BENCH`; `theseus-sim bench turn --check --runs 5
--burst 0` passed (plain turn 4 frames of 4, tool-call 8 of 8, batch 14 of 14; syncs before the first byte 2 of 2);
size skipped (no release build). The deny phase's database was fetched at setup (`cargo deny fetch`, exit 0).

The first commit, 16d99b2 (the pack alone), was not gated on its own: its crate's tests (theseus-judge, 399 with
theseus-protocol's) passed, and core compiles with it (its one core line is the `rebuild.rs` arm the new builder
needs). The whole gate ran on the head.

## Left, uncertain, and for the owner

- **Tool-search models**: nothing changes for them (no Jev set read, no raise). Whether loading Jev's families up
  front for them is worth it is a bench question: the provider's search finds them already, and up-front loading
  costs the tokens on every request that Jev says yes to; I would measure it as a `+families` arm of the routing
  bench (search calls avoided against tokens added) before building it.
- The Jev call runs for every prompt on a cli whose route pack is live, even when the turn stays on a tool-search
  model and the answer is never read. That is the brief's "one Jev call per prompt"; a narrower rule (only when the
  route pack's answer comes back as a detour) would need the wait to begin after the verdict.
- The eval set has 7 cases; it wants real prompts from the owner's ledger (`families.v1` judgments with the
  families the turn then used, as labels) before the bars are tuned.
- The two-question split (20 families) differs from the brief's one question of 10.
- Docs for the maintainer: `docs/status.md` (the recently landed step), the spec's Part III item for theseus-xlri5,
  and `docs/design/` where the deferred tools' budget rule is described (the soft cap and its ceiling).
