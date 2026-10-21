# CLOUD_REPORT: cn-routing-compose (theseus-a1wvq.4)

Branch `cloud/20261021-cn-routing-compose`, from main at `87696a7`. Row commit: `7128181`.

## 1. Files over a line ceiling

None. `config.rs` gained two lines (a `mod` line and one call), `routing.rs` grew by about 50 lines; the shape phase passed ("no file over 2500 lines but the 8 listed").

## 2. What changed (rollback: revert `7128181`)

- `routing.rs`: `Profile.restricted`, `Ask.cleared`. `Profile::runs(images, cleared)` is usable && priced && vision-ok && (`!restricted || cleared`). `pick` takes a trailing `cleared`; `dear_guard`, `step_down` and `steer_to` read it (`a.cleared`). `cheapest` calls `runs(.., false)`, so it never returns a restricted profile, cleared or not. `decide` reads `cfg.classes.effective(mode, a.cleared)`.
- `config/routing.rs`: `ClassProfiles.cn_profiles` and `cn_after` (both `skip_serializing_if`, so the sparse writer and a default config serialize as before), `RoutingClasses::effective` (borrowed when uncleared or no `cn_profiles`), `trivial()` is now `["haiku", "cheapest"]` with its doc fixed.
- `config/routing_cn.rs` (new; one `mod` line and one call in `Config::validate`, after the catalog is built): the three load refusals. An `unknown`-origin profile gets the `[providers.<name>] origin` way out in its message.
- Every `routing::Ask` literal gets `cleared: false` (`route_step.rs`, `route_step/correct.rs`, `brief_step.rs`, `routing/checkpoint.rs`); test `Profile` literals get `restricted: false`; `correction/mod.rs`'s `pick` call passes `false`.
- Template (`[routing.classes.trivial]` comment block), `fixtures/owner_shape.toml` and `owner_shape_old.toml` as the brief says. `crates/theseus-core/AGENTS.md`: one passage in the routing section.
- No store format change (stays 47), no new dependency.

## 3. Tests and plants

New: `routing/tests_cn.rs` (5 tests) and `config/routing_cn.rs` (3 tests). All 9 pass, as do the existing routing, correction, template, owner-shape, first-request and frame-budget tests; clippy `-D warnings` is clean.

Each plant was shown failing, restored with `git checkout` and `touch`, then `git status` clean:

| plant | test that failed |
|---|---|
| drop `!restricted \|\| cleared` from `runs` (key test) | `routing::tests_cn::a_restricted_profile_runs_only_when_cleared` |
| `cheapest` filters with `runs(.., true)` | `cheapest_never_picks_a_restricted_profile` |
| `effective` ignores `cleared` | `cn_profiles_come_first_only_when_cleared_and_obey_the_cap` |
| `effective` ignores `cn_after` | `cn_after_places_cn_profiles_after_the_named_profile` |
| drop the `m != "trivial"` class check | `config::routing_cn::tests::cn_profiles_are_refused_where_china_models_never_answer` |
| `"glm"` back in `fn trivial()` | `config::routing_cn::tests::a_mode_list_never_names_a_restricted_profile` |

`a_detour_to_a_cn_profile_needs_clearance` (T16) and `cn_profiles_parse_per_mode` pass; no separate plant (the detour test goes through `decide`, which the first three plants already break; the parse test is pure serde).

## 4. Existing tests changed

- `config/routing.rs`: `the_defaults_are_the_steps_table` and `trivial_and_quick_are_the_detours` now assert `["haiku", "cheapest"]` for trivial (the expected keel findings; each still pins the default).
- `config/tests.rs::a_config_table_cannot_loosen_a_builtin_cn_origin`: its "tightening a Claude row is allowed" lines tightened `claude-sonnet-5-5`, which the default classes list, so the new load check refuses it. The catalog-level meaning holds: it now tightens `claude-opus-5` (no class names it) and expects a clean load, and asserts that tightening `claude-sonnet-5-5` is refused only by routing's check ("is of restricted origin"). Every other assertion is unchanged.
- Mechanical: the `pick(..)` calls in `routing.rs`'s tests gained a trailing `false`; `Ask`/`Profile` literals gained the new field. No assertion changed.

**36 tests in the cn-floor row's sort list fail on this branch, and I did not edit them** (the brief says to stop and say so). They put `glm`/`glm53` in a class's `profiles` (through `tests_route.rs`'s `glm_placements` and the files that use it), which the new load refusal rejects (`routing.classes.<class>.profiles: glm is of restricted origin (cn): list it in routing.classes.trivial.cn_profiles ...`). They are: `google::tests_private` (2), `own_bench::tests_sampler::an_attachment_is_never_sent_to_another_provider`, `tests_data_gate` (6), `tests_offer` (2), `tests_onto::the_turns_wait_is_unchanged_with_onto_in_a_slow_batch`, `tests_route` (14), `tests_route_base` (7), `tests_route_effort::a_model_without_effort_ignores_it`, `tests_route_model::a_routed_turns_metrics_name_the_model_it_ran_on`, `tests_standing_ring::a_detour_carries_the_standing_block`, `tests_task_brief::a_brief_that_names_a_sensitive_name_skips_restricted_profiles`. They pass again once the floor row's sort rewrites them to unrestricted second models, which is the join order the brief gives (floor first). The other 3,567 core tests passed in the run that found them.

## 5. Join notes for the cn-floor join

- Replace, in `turn/route_step.rs` `route_profiles`, the line `restricted: false, // set from the origin at the cn-floor join (theseus-a1wvq.4)` with `restricted: self.cfg.profile_origin(name, &self.catalog).restricted(),`. It waits because filling it from `profile_origin` turns red exactly the ~20 test files the floor row is sorting; once that sort is on the line it changes no test, since the floor's `data_gate` mark already makes a restricted profile unusable for an uncleared turn.
- Shared files and what I added: `turn/route_step.rs` (the `restricted` line, last field of the `Profile` literal; `cleared: false`, last field of `decide_route`'s `Ask`), `turn/route_step/correct.rs` and `turn/brief_step.rs` (`cleared: false`, one line each), `config.rs` (`mod routing_cn;` and `routing_cn::validate(self, &catalog)?;` right after the `refusal_fallback_model` loop), `crates/theseus-core/AGENTS.md` (one passage in the routing section).
- Later, `Ask.cleared` should come from the turn's `Clearance` (`is_cleared()`), so the floor's `data_gate` mark and `runs` lift together.

## 6. Before install

A config whose class list names `glm` (or any DeepSeek profile, or any profile of unknown origin) stops loading. For each such name: remove it from every class's `profiles`, then add

```toml
[routing.classes.trivial]
profiles = ["haiku"]          # keep the entries that were there, minus the restricted one
cn_profiles = ["glm"]
cn_after = "haiku"
```

`cn_profiles` is accepted on `trivial` only. A profile whose endpoint is trusted but unknown can instead be vouched with `[providers.<name>] origin = "us"`. The owner's note that has `profiles = ["haiku", "glm"]` under `[routing.classes.trivial]` is the known case.

## 7. Not run here

- Nothing of this row needs a live daemon or key: it is pure routing plus load checks, and every caller passes `cleared: false`. The maintainer's live check: `theseusd check --config <the operator's config>` (or `theseusd example-config > /tmp/x.toml` then `theseusd check --config /tmp/x.toml`) should load; with `glm` in `[routing.classes.chat] profiles` it should refuse with the message above.
- The own bench's rung table (`own_bench/rung.rs`) reads `[routing.classes.*]` profiles only and so never sees `cn_profiles`, which is right for a measure of what the owner's routing may use today. It should read them when the open-request row clears asks.
- The disk allowance on this VM filled twice (a full workspace test build is about 28 GB with debug info), so the gate was run with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.

## 8. Keel guard

`python3 scripts/keel-guard.py --base 6e02e02 --head HEAD --quiet` printed nothing (no findings). Against `origin/main`, which has moved on since my base, it lists 8 test removals in `tests_day_total.rs`, `tests_task_threads.rs` and `lifecycle/copies.rs`; those are other changes' files that main has and my branch does not, not my edits.

## 9. Store format

Unchanged at 47.

## 10. Follow-ups

- Run the gate after the cn-floor join, when the 36 sorted tests pass again.
- `cn_after` names an entry of `profiles`, so a class edited later to drop that entry fails the load by name; fine, but worth knowing.
- `effective` is read only in `decide`; if a later row walks a class list elsewhere (the brief's or checkpoint's own picks go through `decide`/`brief_base` today), it must read `effective` too.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, for disk) on `7128181` **failed in the suite phase**: 7,282 tests run, 7,213 passed, 69 failed. Every earlier phase passed (door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build, reader rule). The 69:

- 33 are the known L1 failures on this root VM (`theseus-sandbox` contract tests and `spawn_100`, theseusd's `sandbox` tests): theseus-pv6i.
- 36 are the cn-floor row's sort list, failing on my load refusal as described in section 4 (a class's `profiles` naming `glm`/`glm53`). Their names are listed there. I did not edit them. They are not flaky and not mine to fix on this branch; they should pass once the floor row's sort has joined.

I then ran the phases after the suite by hand: the generated TypeScript is unchanged (no protocol type changed; `git status` clean for `cockpit/src/protocol.gen`); `theseus-sim bench turn --check --runs 5 --burst 0` passed (first-request budgets: cli 7,332 of 8,000, dm 9,411 of 15,000, task 6,585 of 8,000, all ok); `cargo deny --offline check bans licenses sources` ok (advisories not checked: no database fetched here); the size step skipped (no release build). The lifecycle and jobs benches are skipped by `THESEUS_GATE_NO_BENCH`. `tests_m3::a_plain_turn_stays_within_its_frame_budget` passed.
