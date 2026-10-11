# CLOUD REPORT: routing-owner (theseus-p8dy)

Branch `cloud/20261011-routing-owner`. Commits (each gated, then pushed):

| step | commit |
|---|---|
| 1. xhigh/max only above `high_effort_confidence` | d4839c7 |
| 3. the dear tier's context guard | 0dfb357 |
| 4. Jev's elevation steps back down | cda1a9b |
| 2. `review` mode, pack `route.v4` | 537b258 |
| live check as a daemon test | 58cbd2b |

## Step 1: high effort needs high confidence
**Found.** `effort::decide` applied any sure level. **Changed.** `[routing] high_effort_confidence` (0.70, validated above 0, at most 1, in
config/routing.rs and the example template). An `xhigh` or `max` answer applies only when its confidence is strictly above it; else the turn
runs `high` and `effort_reason` is the new `held_high` (the answered level stays in `effort`; `effort_applied` is true, `effort_ran` high).
The gate runs first, the bounds clamp after; where both lower an answer the reason is the gate's (`held_high`), where only the bounds do it is
`clamped`. A floor above `high` can lift a held answer; if it lifts it back to the answered level it reads `applied`.
**Proof.** Unit tests `routing::effort::tests` (0.70 vs 0.71 for xhigh and max, 0.65, bar moved to 0.9, below the confirm bar stays `unsure`, bounds+gate
combinations) and `tests_route_effort` (two whole-turn tests: max 0.65 -> high/held_high, 0.9 -> max, xhigh at 0.70 held; gate+bounds reasons).
Planted revert (gate switched off): 4 tests failed (2 unit, 2 turn); restored and touched.

## Step 3: the dear tier's context guard
**Changed.** `[routing] dear_tier_max_tokens` (200,000, at least 1). `routing::decide`: for `sophisticated` or `review` at an estimate strictly
over it, a usable profile in the mode's list whose catalog input price is above the next usable one's is skipped (`dear_guard`, pure; the last usable
profile is never skipped). The row's reason is the new `Reason::ContextGuard` (`context_guard`) where the guard moved the pick (a Capped reason still
wins). `routing::Profile` gained `input_per_mtok` (not stored). **Proof.** `routing::tests` (at the threshold nothing changes, over it the dearer first is skipped, cheaper
first untouched, the last usable kept, other modes unguarded, the config's threshold, reason only where the pick moved). Planted revert (guard never
applies): both guard tests failed.

## Step 4: only Jev elevates, and it steps back down
**Changed.** `Ask.from` = the session's base (`Routed.from`) while `Routed.profile` is set, passed only by `decide_route` (Jev's verdict path); the
owner's corrections (`decide_steer`, `steer_to`) pass none. `routing::step_down`: the session is elevated when its current profile's short cost is above the base's; a
non-detour verdict at its bar whose mode picks the session's own (chat, other) or a profile no dearer than the base is eligible; the first such turn
returns `CacheHold` with `Hold { mode: "step_down", profile: base }` and stays; the second (finding that hold) switches to the base with the new
`Reason::StepDown` (`step_down`), at any estimate (the hold governs, no third turn). A dearer verdict or one under its bar clears the hold (existing
rule); a detour keeps it. A step-down goes to the base, never below it (routine_coding's `haikuhi` is cheaper than the base `sonnet`: it counts as eligible, and
goes to the base). **Existing tests that changed meaning** (tests_route_base.rs, tests_route.rs): they used a `chat` verdict to leave a raised session raised, which is
the defect; they now use `deep_coding` (first profile opus) for that, a second switch is to `fable` (a dearer profile; a cheaper one is now a step-down),
and `a_switch_back_to_the_base_ends_the_move` takes two routine_coding turns and expects `step_down`. A helper `moved_to_opus_with` takes a config tweak.
New: `tests_route_down.rs` (whole turns: chat, chat steps down; a hard verdict between breaks the row; with `cold_switch_tokens = 1` the hold governs; an owner's `-P` pick is never routed or stepped down) and
`routing::tests` (two agreeing turns, one is not enough, below bar, detour, deep_coding, not-below-base, unusable base, `steer_to` never steps down).
Planted revert (step-down off): 4 tests failed.
**No store format change.** `Routed` has no owner/Jev field, so the owner-pick exemption is partial: an `-P` pick is exempt (it resets the base), and the
correction turn itself never steps down, but a session raised by an owner's correction (`steer_to ... at_once`) is stored identically to one Jev raised, so later chat verdicts
step it down. A `Routed.by: Option<String>` (or a bool `by_owner`) set at the switch and cleared at a switch back would close it: one `MANIFEST_FORMAT` bump, which belongs to
the `standing` row's bump this round. **Design choice for the owner:** detour verdicts (`trivial`, `quick`) neither advance nor break the row (they keep any hold, as today); I did not count them as agreeing turns because a detour
cannot switch the session. If you want `quick` to count, it is a small change in `step_down`'s caller.

## Step 2: `review` mode, pack `route.v4`
**Rule followed.** A live pack's options never change in place: route.v2 added `quick` and route.v3 added a question, each as a new version. So `route.v4.toml` = v3 with a
`review` option (before `other`); a test holds that every other option, the thresholds and `reply_effort` are v3's word for word. `ROUTE_PACK` = route.v4, adopted live (`ADOPTED`, reason "decision of 2026-10-11"),
in `LEFT_ALONE`, embedded list, rollback rules the same as v1's (test), pack count 18; route.v1-v3 stay embedded. `MODES` has 7, `RoutingModes.review` defaults to `["opushi","fable"]`, the learning report's acting
modes include `review`, the template has `[profiles.opushi]` (Opus 5.5 at high effort) and the `[routing.modes.review]` lines. **Goldens:** only the new
`route.v4.request.json`; no existing golden moved (route.v1..v3 request goldens and the core output golden are unchanged). `tests_route_keep` still passes (v4's request is one option longer); the
tests that named the acting pack (`tests_inbound`, `tests_ladder*`, `tests_route*`, `tests_judge`, `tests_continue`, theseusd's `tests/judge.rs` and `bench_routed_live.rs`, sim's `perf/judge.rs`) now say `route.v4` and the status line `decision of 2026-10-11`.
Tests: `routing::tests::a_review_verdict_routes_to_opushi_then_fable`, `tests_route_down::a_review_verdict_runs_on_opushi_and_then_fable`, `pack::tests::route_v4_adds_a_review_mode_to_v3s_and_changes_nothing_else`. Planted revert (`review` removed from `RoutingModes::table`): 3 tests failed.
**Not edited (for the maintainer):** bench/theseus-bench-routed.toml's header comment still says route.v3, and its tables name no `review` (it falls to the default `["opushi","fable"]`; the bench has no `opushi` profile, so a review there runs on `fable`). Docs to change at review: the spec's Part III item (route.v4, the three new keys, `held_high`/`context_guard`/`step_down`), the model guide, status.md.

## FAST
No new frame, wait or call: the decisions are pure over data the turn holds. `theseus-sim bench lifecycle --runs 10 --check`: LIFECYCLE OK (cold start p95 28.7 ms vs 50; shutdown 26.7 vs 100; SIGKILL restart 26.7 vs 150; swap 30.2 vs 200; cancel 12.6 vs 100), on this VM's debug build.

## Live check
On this VM, real daemons (`crates/theseusd/tests/route_owner.rs`, committed): the routed bench profile, the stand-in model, a scripted `FakeJev`, one session, five `theseus --spawn ask` (each its own daemon). `route.decided` rows:

```
"chat" 0.65 -> "bench" "verdict" switch=false | effort "max" 0.65 "held_high" ran "high"
"chat" 0.90 -> "bench" "verdict" switch=false | effort "max" 0.90 "applied" ran "max"
"sophisticated" 0.95 -> "fable" "verdict" switch=true | effort "medium" 0.95 "applied" ran "medium"
"chat" 0.95 -> "fable" "cache_hold" switch=false | effort "medium" 0.95 "applied" ran "medium"
"chat" 0.95 -> "bench" "step_down" switch=true | effort "medium" 0.95 "applied" ran "medium"
```
**For the maintainer, against the owner's real Jev** (a scratch state dir, never the operator's daemon; `$CFG` a copy of the owner's config with the live profile Sonnet 5.5, `[routing]` defaults):
`S=$(theseus --spawn theseusd --json ask "weigh two designs for a crash-safe log, in depth" | tail -1 | jq -r .session_id)` (state dir, config and keys via `THESEUS_CONFIG`, `THESEUS_STATE_DIR`);
then `theseus --spawn theseusd --json ask -s $S "ok"` twice; then `theseus ledger -k route.decided -n 10 --json`. Expect: the first row's `effort_reason` `held_high` where Jev's `effort_confidence` is 0.70 or below on an xhigh/max answer, `applied` above; a `sophisticated` row switching to opus (`switch: true`); the next chat-like message `cache_hold`, the one after `step_down` back to the live profile. A message such as "review this diff before I merge" should show `mode: review` and `profile: opushi` (or `fable` if `opushi` is not configured).

## Gate
Every commit ran `CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=fdd1492 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`; fmt, shape, features, clippy, cockpit, test build, reader rule pass; the suite's only failures are the 33 known L1 tests (theseus-sandbox contract tests and `spawn_100`, theseusd's sandbox tests; root VM, theseus-pv6i), 0 others. The gate stops at the suite on those, so I ran the remaining phase myself: `cargo deny --offline check bans licenses sources` ok, and protocol_types (TurnRoute.ts regenerated and committed in 537b258). Benches skipped by `THESEUS_GATE_NO_BENCH`; lifecycle ran once by hand (above).
Notes: (1) this clone's `origin/main` is stale (a9ad950), so the keel guard's default range pulled in unrelated history and failed; I set `THESEUS_KEEL_BASE=fdd1492` (the main I cloned): `keel: ok, 0 findings` for every commit. **Keel findings expected:** none. (2) The disk allowance filled once (incremental builds, 16 GB), which made the first suite run fail 129 tests with "disk under the state dir has 370 MB free"; I removed `target/debug/incremental` and used `CARGO_INCREMENTAL=0` afterwards. No timing test failed.

## Left / uncertain
- The owner-pick exemption (above): a field would add exactness; the stored shape is otherwise unchanged. The owner's real config may set `[judge.packs."route.v3"]` keys, which now name an embedded but non-acting pack.
- `Reason::StepDown`/`ContextGuard` and `EffortReason::HeldHigh` are new row values; the cockpit shows `reason` as text, so nothing there changed.
