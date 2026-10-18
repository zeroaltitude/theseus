# Cloud report: trim-config-inherit (theseus-rwpbg.7)

Branch `cloud/20261017-trim-config-inherit`, base `432bbbeb`.

## Step 1: pin the resolved profiles first — `5e5c99b8`
- **Found:** nothing pinned what a config resolves to.
- **Changed:** `config/tests_inherit.rs::the_owner_shaped_config_resolves_to_the_same_profiles` and `config/snapshots/resolved_profiles.txt`: every resolved profile, every field (`{:#?}`), of `fixtures/owner_shape.toml`, the example template, `bench/theseus-bench.toml` and `bench/theseus-bench-routed.toml` (theseus-sim writes no profile fields). Written on main's code (`THESEUS_GOLDEN=write`).
- **Proved:** passes on main's code, and passes **with the same snapshot** after step 2 (run again after the change: 123 config tests green).

## Step 2: one rule, `--sparse` on written values — `493c06a8`
- **Found:** `inherit` passed down only `cache_ttl`/`keep_warm_hours`, so `[model] max_loops_mode` never reached a named profile. `--sparse`'s `acting()` serialised `Config`, whose resolved profiles are `#[serde(skip)]`, so it compared the profiles *as written*: a restated `cache_ttl = "1h"` could never be dropped.
- **Changed:** smaller diff than making fields `Option`: `config/inherit.rs` writes `[model]`'s keys (provider, max_output_tokens, system, effort, thinking_display, max_loops, max_loops_mode, refusal_fallbacks, cache_ttl, keep_warm_hours) into each profile table the profile leaves them out of, before the profiles are read. `ProfileConfig` and `ModelConfig` are unchanged, so nothing else in the crate moves; `cache::inherit` still fills the defaults and `ttl_of`/health still see the raw `None`. `model` stays required (a profile always names its model). `sparse.rs::acting` compares the resolved profiles. The template's `[model]` comments now say "the defaults every profile inherits" and the stale "each profile has its own" line is fixed. No default value, cap or ceiling changed.
- **Proved:** new tests in `tests_inherit.rs`: `a_model_field_reaches_every_profile_that_leaves_it_unset`, `max_loops_mode_in_model_is_inherited`, `a_profile_value_wins_over_model`, `sparse_drops_inherited_profile_keys`, `sparse_output_loads_to_the_same_config` (all four documents plus the owner-shaped case: resolved profiles of the sparse note equal the whole note's). `cargo nextest run -p theseus-core -E 'test(tests_inherit)'`: 6 passed.
  - Planted revert 1 (inherit only the cache fields): `a_model_field_reaches_every_profile_that_leaves_it_unset` and `max_loops_mode_in_model_is_inherited` fail; restored and touched.
  - Planted revert 2 (`acting` compares raw profiles): `sparse_drops_inherited_profile_keys` fails; restored and touched.
- **Snapshot/bench/sim:** all four documents resolve identically before and after; no `[model]` field newly reaches a profile in any of them. Every bench profile sets its own `max_loops_mode = "end"`, so each still ends.

## Proof nothing was lost
- Resolved profiles of the owner shape, template and both bench configs: byte-identical snapshot before and after.
- `--sparse` output line counts (whole / before / after): owner_shape 172 / 149 / 144; template 1581 / 82 / 82; bench 104 / 52 / 50; bench_routed 167 / 101 / 87. Sparse output loads to the same resolved profiles (test above).
- Keel guard: `python3 scripts/keel-guard.py --base 432bbbeb --head HEAD --quiet`: empty, rc 0.

## Item 3: `[aws] hands_network` in the template
Already there at `config/theseus.example.toml` (the `[aws.accounts.<id>.hands_network]` table with `vpc`, `subnets`, `security_group`, commented, with its default and why), asserted by `config/tests.rs`' template test. Nothing to add.

## Gate
Run as `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=432bbbeb scripts/gate.sh`. Without `THESEUS_KEEL_BASE` the keel fails on commits already in this branch (this clone's `origin/main` is older than the base, and their signatures do not verify here). Phases through suite: door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule pass. Suite: 6339 run, 6305 passed, 33 failed, 1 timed out:
- 33 failures: the known L1 ones (theseus-sandbox contract tests and `spawn_100`; theseusd sandbox tests; root VM, theseus-pv6i).
- 1 timeout: a theseus-discord `tests_task_threads` test (120 s under load). An earlier run timed out two others in the same file; all 19 tests of the file passed alone (7.5 s). Not on the flaky list and not touched by this change.
- The phases after the suite (protocol types, benches, turn, size) were not run: the change adds no protocol type, no startup work and nothing on the turn path.
- The cloud VM's disk ran out twice; I built with `CARGO_PROFILE_{DEV,TEST}_DEBUG=0` and `CARGO_INCREMENTAL=0`.

## Not done / for the maintainer
- Rewrite the owner's live config with `theseusd config --sparse` after install (your step).
- A `[model]` value now reaches profiles that left it out: the owner's real file should be checked once with `theseusd config --sparse` before and after (or `theseus profile list`) for any profile relying on the serde default under a non-default `[model]` value.
- Docs to change at review: the config sentence in `docs/status.md`/spec about `[model]` and `inherit` (cache fields only).
- Left alone: Discord's copies of `clip`/date math.
