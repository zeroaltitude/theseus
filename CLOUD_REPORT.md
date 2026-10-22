# CLOUD REPORT: tail5-effort (theseus-7pm8t)

Branch `cloud/20261021-tail5-effort`, base `origin/main` at `f1fc9b03`. One step, one commit: `5622f55`
(`routing: Jev's effort answer applies on its own bar, not the model switch's (theseus-7pm8t)`).

**Line ceilings:** no file grew past its ceiling (the files touched are all unlisted and under 2,500 lines:
`routing/effort.rs` 629, `config/routing.rs` ~745, `turn/route_step.rs` 1,327, `tests_route_effort.rs` 475).
**Store format:** unchanged (49). No stored field, no wire shape: the new reason is a string in the existing
`route.decided` row's `effort_reason` and `TurnRoute.effort_reason`.
**Dependencies:** none added. **Signing:** `git commit -S` succeeded with the VM's configured SSH signing (`gpg.format = ssh`); this VM
cannot verify it (no `allowedSignersFile`), so check the signature on your side. Trailer copied from main's recent
commits (`Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>`), as the row's rules say.

## What I found

`routing::effort::decide` returned `EffortReason::Unsure` as soon as `!answer.sure` (the effort question's band was
`escalate`, under route.v7's `reply_effort` confirm bar of 0.60), before it reached the `xhigh`/`max` gate. So the
Terminal-Bench rerun's eight `max` answers at 0.35 to 0.42 all ran at the profile's own effort, and lowering
`high_effort_confidence` alone could not change that. That is the bar a model switch needs, applied to an effort level.

## The rule as built

`decide(bounds, bars, answer, runs, acts, carried)`, where `bars` is a new `effort::Bars { effort, high }` (from
`[routing] effort_confidence` and `high_effort_confidence`). The order of checks:

1. `!acts` → `recorded`; `carried` → `carried`; `unclear` → `unclear` (unchanged).
2. **`!answer.sure && confidence < effort_confidence` → `unsure`**, the profile's own stands (was: any `!sure`).
3. no effort on the model → `no_effort`; `effort_fixed` → `fixed` (unchanged).
4. The gate: `xhigh`/`max` at or under `high_effort_confidence` run `high` (`held_high`). Unchanged, and it
   applies to an unsure answer exactly as to a sure one.
5. The floor, then the bounds (unchanged).
6. The reason: `floored`, `held_high`, `clamped` win as before; where the answer's own level ran, it is `applied`
   for a sure answer and **`applied_unsure`** for one that passed only on `effort_confidence`. `applied()` counts it.

The rule is in the module doc's bullets (the new "The effort has its own bar" bullet, and the `xhigh`/`max` bullet
saying the gate holds whatever the band).

## Defaults and validation

- `[routing] effort_confidence`, new, default **0.35**, placed directly under `high_effort_confidence`.
- `[routing] high_effort_confidence` default **0.70 → 0.40**, the owner's live value since 2026-10-22 (his decision,
  not a budget).
- Validation: `effort_confidence` must be above 0 and **at most `high_effort_confidence`**, error
  "routing.effort_confidence must be above 0 and at most routing.high_effort_confidence". Why that rule: a bar to
  apply an unsure answer above the bar for `xhigh`/`max` would make the high gate the lesser bar, which reads as a
  mistake in a config; equal is allowed.
- Template: the `[routing]` comment paragraph says the new rule (`applied_unsure`), `high_effort_confidence = 0.40`
  and one new key line `effort_confidence = 0.35`.
- Bench: `bench/theseus-bench-routed.toml` `[routing]` sets `high_effort_confidence = 0.40` and
  `effort_confidence = 0.35` under "# the owner's live bars, 2026-10-22", and its header's "where Jev is sure" now
  says "where its confidence reaches `[routing] effort_confidence`, xhigh and max only above
  `high_effort_confidence`".

## Reason strings

Added `applied_unsure` (`EffortReason::AppliedUnsure`). I looked for lists of effort reason strings
(`git grep -n held_high`, `"floored"`, `"clamped"`): there is none outside effort.rs and tests. The cockpit's
`lib/route.ts` `effortWords` reads an applied effort generically (`effort max (Jev)`), the CLI's and Discord's
`why.rs` prints the reason string as written (`effort: Jev said max (applied_unsure), the request carried max`),
and `why.rs`'s `reason_words` is the route's reason, not the effort's. So nothing else needed a line.

## Tests and planted reverts

New tests (each failed under its plant; every file restored with `cp` from a copy and `touch`ed, `git status`
checked after):

| Test | Plant | What failed |
|---|---|---|
| `routing::effort::tests::a_max_answer_at_0_42_with_an_unsure_band_runs_at_max` (the key test) | restore `if !answer.sure { return Unsure }` | `left: (Unsure, Some(Medium)) right: (AppliedUnsure, Some(Max))` |
| `tests_route_effort::an_unsure_max_at_0_42_runs_at_max_on_the_effort_bar` (turn-level, through `route_step`; Jev `max` at 0.42, band escalate; asserts the request carries `max`, the `route.decided` row and `TurnRoute` say `applied_unsure`) | same plant; and separately `effort: 1.0` at the `route_step` call site | `left: None right: Some("max")` (both plants) |
| `routing::effort::tests::an_unsure_answer_under_effort_confidence_keeps_the_profiles_effort` | compare `<= 0.0` instead of `< bars.effort` | `left: (HeldHigh, Some(High)) right: (Unsure, Some(Low))` |
| `routing::effort::tests::an_unsure_xhigh_between_the_bars_runs_high` (bars 0.35/0.50, `xhigh` at 0.42 → `held_high`; also floor and bounds win over `applied_unsure`) | gate only sure answers (`held = answer.sure && …`) | `left: (AppliedUnsure, Some(Xhigh)) right: (HeldHigh, Some(High))` |
| `config::routing::tests::an_unsure_effort_applies_at_its_own_bar_under_the_high_one` (defaults 0.35/0.40, equal bars allowed, above refused with the message, both raised together allowed) | — (config) | — |

Existing tests changed, none deleted or weakened:

- `config::routing`: `the_templates_routing_section` asserts 0.40 and 0.35; `the_checks_refuse_what_cannot_hold`
  gains `effort_confidence = 0.0` and `= 0.5` (above the default 0.40); the default test asserts 0.40 and is renamed
  `xhigh_and_max_need_confidence_above_a_bar_that_defaults_to_four_tenths` (was `…seven_tenths`).
  `each_mode_routes_at_its_own_bar_and_trivial_at_four_tenths` checked that the written-back section holds no
  `0.4` (trivial's unset bar); the new default `high_effort_confidence = 0.4` is in that text, so the check now
  looks for `switch_confidence = 0.4`, which is what it meant.
- `tests_route::a_quick_question_detours_to_haiku_and_the_next_prefix_is_byte_identical` failed in the gate: an
  unscripted fake Jev answers `reply_effort` with its first option at 0.5, which now applies (`high`), and the test
  asserts the detour runs at `haiku`'s own `low`. The test is about the detour's prefix, so it now scripts Jev's
  effort as `unclear` (0.95) for the detour message; its effort assertion stands. No other test relied on the fake's
  unscripted effort (the whole suite ran).
- Tests that draw the gate's line at 0.70 (they mean that bar, not the default) now set it in their own rig:
  `tests_route_effort::max_runs_only_on_high_confidence_and_is_held_at_high_otherwise` and
  `the_gate_runs_before_the_bounds_and_its_reason_stands`, `tests_checkpoint_effort`'s `routed` rig (0.69/0.71),
  and theseusd's `tests/route_owner.rs` `trial` (0.65/0.9, on the bench profile, which now says 0.40).
- Tests whose "unsure" answers sat between 0.35 and 0.60 moved under 0.35, keeping their meaning (the profile's
  own stands under both bars): `effort.rs`'s `a()` helper's unsure confidence 0.5 → 0.3, the
  `xhigh_and_max_apply_only_above_the_confidence_bar` "not sure at all" line 0.5 → 0.3, and
  `tests_route_effort::below_the_bar_unclear_or_fixed_the_profiles_own_effort_stands` 0.55 → 0.30.
- `effort.rs`'s test `BAR` is `Bars { effort: 0.35, high: 0.70 }` (its 0.70 is the tests' own line, not the default).

Runs: `cargo nextest run -p theseus-core -E 'test(effort) | test(routing)'`: 108 passed; after the detour fix,
`-E 'test(tests_route) | test(effort) | test(routing)'`: 175 passed. No load runs: the task asked for none and
nothing here is timing-dependent. Whole suite: in the gate, below.

## Keel guard

`python3 scripts/keel-guard.py --base origin/main --quiet` (working tree, before the commit): empty output, exit 0.
`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` after the commit: empty output, exit 0.
The gate's own keel phase: `keel: ok (f1fc9b03..the working tree; 10 files changed, 0 findings acked)`.

## Live check for the maintainer

On a scratch daemon of this build with the routed bench profile (or the owner's config, which already has 0.40):

1. `theseusd example-config | grep -n effort_confidence` shows `high_effort_confidence = 0.40` and
   `effort_confidence = 0.35`.
2. A message Jev answers `max` under 0.60 but at 0.35 or more on a model that takes effort (Opus 5.5 under
   `sophisticated`): `theseus why` shows `effort: Jev said max (applied_unsure), the request carried max`, and
   the ledger's `route.decided` row has `"effort_reason":"applied_unsure","effort_applied":true,"effort_ran":"max"`.
3. The same at 0.30: `effort_reason` `unsure`, `effort_applied` false.
4. A config with `effort_confidence = 0.5` and the default high bar: `theseusd check` refuses it with the message above.

## Left, uncertain, owner-visible

- **Owner-visible:** an effort answer under its confirm band now applies at 0.35 or more; `xhigh`/`max` still need
  more than 0.40. Rows say `applied_unsure` for those turns. The bench's routed arm now runs at max on answers like
  the rerun's eight (0.40 < c).
- A `docs/status.md` sentence: "Jev's effort answer now applies on its own bar, `[routing] effort_confidence`
  (0.35), even when its band is under the question's confirm bar (row `effort_reason` `applied_unsure`); `xhigh`
  and `max` still need `high_effort_confidence`, now 0.40 by default (theseus-7pm8t)."
- The checkpoint path (`turn/checkpoint_step.rs`) reaches the same `decide` through `route_step`, so a checkpoint's
  unsure effort answer also applies at 0.35 now; it still never lowers effort. Its module doc (lines 29-30) still
  says "the confirm bar, the 0.70 gate": a doc line for the integrator to update ("the effort bar, the
  `high_effort_confidence` gate"); I left the file alone, as it is outside this row's files.
- Follow-up: none required. theseus-1g7uj (the request digest) left alone as asked.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0`, for disk): door, keel,
keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule passed. The suite:
**7851 run, 7816 passed, 35 failed**:

- 34 are the known L1 failures of a root VM with no job cgroup (theseus-pv6i): theseus-sandbox's `bench spawn_100`
  and its 19 contract tests, and 14 of theseusd's `sandbox` tests.
- 1 was this change's: `tests_route::a_quick_question_detours_to_haiku_and_the_next_prefix_is_byte_identical`
  (above), fixed in the same commit and rerun green with `tests_route`, `effort` and `routing` (175 passed).
- No flaky retries were needed; `tests_output`'s golden passed under `TZ=America/Phoenix`.

The phases after the suite, run by hand on the committed tree: protocol types (no change to
`cockpit/src/protocol.gen`): ok; `fmt --check`: ok; `cargo clippy -p theseus-core --all-targets -- -D warnings`
(after the test fix): ok; lifecycle and jobs benches: skipped (`THESEUS_GATE_NO_BENCH`); turn bench
(`theseus-sim bench turn --check --runs 5 --burst 0`): exit 0, every budget ok; size: skipped (no release build);
deny (`cargo deny --offline check`): advisories, bans, licenses, sources ok.

**Environment notes.** The VM's disk allowance filled twice (a linker bus error at the setup build and in the first
gate's test build); I removed `target/debug/incremental` and stale test binaries and libraries in `target/` only,
and built with `CARGO_INCREMENTAL=0`. `cargo deny fetch` ran late (the setup chain stopped at the full disk), then
deny passed offline.
