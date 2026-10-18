# CLOUD_REPORT: review-fixes (theseus-oejh3)

Branch `cloud/20261017-review-fixes`. Commits: `4b06f0f1` (D: task channels, with the fake's fix), `715d186e` (A, B, C). Base `f3a1cb3f`, store format unchanged, no new dependency (Cargo.lock untouched; `sha2`/`hex` stay, since `pack.rs` and others still use them).

## D. Task channels close to the operator alone

**Before (read from the code, then shown by the new test on the unchanged close).** `task_channel_close` already PATCHed the channel with `task_overwrites(guild, true)`, and the fake's PATCH replaces the overwrite set (`channels.rs`: `c.overwrites = overwrites(..)`), as Discord does. So a guest's member overwrite and a hand-added role were already gone after close: `a_closed_task_channel_is_the_operators_alone` passes on the unchanged close. What was missing: the fake could not put an overwrite on a channel the bot made (`PUT/DELETE /channels/{id}/permissions/{id}` looked only in the guild's own channels, 404 otherwise), so no test could share a task channel; the guest list `discord.task_guests.<task>` stayed after close; `share:` of an ended task would put a guest back; a task thread's guests stayed members.

**Changed.**
- Fake (`theseus-sim` `fake_discord/overwrites.rs`): PUT/DELETE permissions and `overwrites_of` cover bot-made channels; new `put_overwrite` (a person adding a role in Discord's client).
- `war_room.rs`: `Shared::end_guests` (list cleared; a thread's guests removed with `remove_thread_member` while the thread is open; one Discord refuses stays listed, so `unshare:` can retry; no write when there are no guests); `share:` of an ended task answers "That task has ended; its place is yours alone." (an `unshare:` still works). Called from `finish_task_channel` (after a successful close only) and `finish_task_thread`.
- Docs: `task_channels.rs` module comment and `theseus-discord/AGENTS.md` (every overwrite but `@everyone`'s deny, the operator's read and the bot's goes; a member with Administrator, and the guild's owner, see every channel whatever the overwrites say).

**Operator set.** `owner_ids()` = `place_rule.owners(&cfg)` filtered to `discord:` ids: `[places] owner`, else the person of each DM the bindings file binds. On a default config with one DM that is one person; with several DMs bound (the test rig binds ana and ben as owners) all of them are kept. A configured second owner is kept too. Config question for the owner, not changed.

**Tests (theseus-discord).** `a_closed_task_channel_is_the_operators_alone` (guest via the real `/task share:` path, role via `put_overwrite`, end; overwrite set is exactly `@everyone`, owners, bot; `viewers::can_view` gives only ana and ben); `an_ended_task_lists_no_guests_and_refuses_a_share`; `ending_a_task_thread_removes_the_guests_the_war_room_added`. 24 task-channel/war-room tests pass; every earlier one still passes.
**Plants.** Close without `.permission_overwrites(&over)`: `a_closed_task_channel_is_the_operators_alone` FAILED. `end_guests` neutered (list not cleared, thread member not removed, share refusal off, all together): the guest-list test and the thread test both hung to nextest's 120 s kill (failed as the bug does). The share refusal alone was not isolated by its own plant.

## A. `[privacy] sensitive_names`
- `config/privacy.rs` (`PrivacyConfig`, deny_unknown_fields, default empty), `Config.privacy`; `RoutingData` lost `private_topics`; `names(&PrivacyConfig)` builds `DataNames`. `routing_data_private_topics_is_refused` pins the unknown-field error. Template: commented `[privacy]` block, placeholders only (`Examplecorp`, `Project Placeholder`), its comment says what a hit does, how matching works, and that the list lives in the owner's own config; `[routing.data]` keeps `open_topics`. `example_template_uncommented_still_parses` passes.
- Matcher: `theseus_judge::names` (`hits`, `Names`, `packs_naming`). Test `sensitive_name_matching` (table: case, possessive, hyphen, underscore, longer word, newline and punctuation, word between, non-ASCII, empty list, punctuation-only name). Plant (no lower-casing): it FAILED.

**For the maintainer's install** (`/etc/theseus/theseus.toml`):
```toml
[privacy]
sensitive_names = [<the owner's names>]   # each spelling separately: "BigCo", "Big Co"
```
**Row for trim-names' renamed-key table:** `[routing.data] private_topics` -> `[privacy] sensitive_names` ("`[routing.data] private_topics` was renamed to `[privacy] sensitive_names`").
**Names trim-names renames that I used:** `crates/theseus-core/src/tests_data_gate.rs:52` (`c.routing.modes.routine_coding.profiles`). Nothing else.

## B. route.v6 and the pack-text check
- `data_names.private` comes from the privacy list (`turn/inbound_step.rs`, `rpc/thread_flow.rs` call `names(&cfg.privacy)`). route.v6 goldens/fixtures (`route.v6.request.json`, `inbound.names.json`, `inbound.state.json`) untouched, and `the_operators_names_ride_the_state_only_when_configured` (moved to the new key) passes.
- `verdict(j, turn, sensitive)`: a hit makes `data` = `private`, 1.0, band `Act` (`DataAnswer::private_by_name`). Jev's own answer is not kept beside it (the type has no field for it; it stays in the judgment's record). Test `a_sensitive_name_makes_the_data_answer_private`; plant (`if false && sensitive`) FAILED it.
- T28 `the_pack_text_names_no_organisation`: `DENIED_DIGESTS`, `digest`, `denied_in` deleted; the test runs the matcher over every embedded pack against the template's placeholder names (must not hit) and keeps the planted-text mechanism check ("Examplecorp private plans"). **This changes what the assertion reads** (hashes -> the config's list). The keel guard printed no `pack.rs` finding in my range, so I have no `Keel:` trailer to give; if the joiner's run reports one, the DM to quote is: "I think you should make a sensitive names config, and instead of basing it on a hashed name, just use the config" (2026-10-17 21:46:39 MST).
- Start check: `Core::check_pack_text_after_serving` (called from `warm_after_serving` in theseusd), blocking pool, after serving. A hit is a `tracing::warn!` and a background startup phase `privacy.packs` in health (`{"packs":[{"pack":"route.v6","hits":1}]}`; the pack name carries its version; never a name). Design choice: health has no generic problems list and `protocol/lib.rs` is at its ceiling, so the "health line" is the startup phase. Test `a_pack_naming_a_sensitive_name_is_reported_at_start`; plant (early return) FAILED it. **Cost:** 17.5 ms in a debug build, 2.9 ms in release for two names over all 24 packs, off the serving path, so "serving at" does not move.

## C. The floor
- Route (`turn/data_step.rs`, `route_profiles(sensitive)`, `decide_floored`): with a hit, restricted profiles are `unusable: "data_gate"`, so routing takes the next usable one; if the decision (or the base, or a pin) still lands on a restricted profile, the `[model]` default runs (`Reason::DataGate`, row reason `data_gate`); if that is restricted too, the turn fails before any model call, class `data_gate`, "this message names something in `[privacy] sensitive_names`, and no profile of unrestricted origin is configured for it". Tests `a_sensitive_name_skips_restricted_profiles`, `a_pin_of_a_restricted_profile_gives_way_to_the_default`, `a_sensitive_name_with_only_restricted_profiles_sends_nothing`; planted `data_floor` no-op FAILED the last two (the first is carried by `route_profiles`).
- Limit: the route skip runs only on a turn that asked route.v1. Other turns (judge off, a wake or continuation, which carry no message text) are covered by the chokepoint alone, which fails them with `data_gate`.
- Chokepoint (`data_gate.rs`): `guard()` wraps every provider in `Core::build`, so every caller is covered; with an empty list nothing is wrapped (zero work on a plain turn); an unrestricted model is passed on without a look at its text; the walk is over each JSON string value of system, messages and tools (also `count_tokens`). New `ProviderError::DataGate` (class `data_gate`, not transient). Tests `a_restricted_model_is_never_sent_a_sensitive_name` (unit, records nothing), `every_string_of_the_request_is_walked`, `every_provider_of_the_core_is_behind_the_floor` (through the built core); plant (`guard` returns the providers unwrapped) FAILED the last.
- Docs: `crates/theseus-core/AGENTS.md` bullet "Sensitive names".

## Proof nothing was lost
- route.v6 goldens/fixtures byte-identical (not edited); `tests_route_data`, `tests_inbound` pass.
- Plain turn on an unrestricted model: the whole workspace suite passes except the cases below, which includes `a_plain_turn_stays_within_its_frame_budget`, the first-request budget tests and `tests_output`; the core adds no frame. The turn bench itself (`theseus-sim bench turn`) was not run here.
- Task-channel and war-room tests: all 24 pass.
- T28 fails on a planted name (mechanism check inside the test).

## Gate
- `scripts/gate.sh` (with `THESEUS_GATE_NO_BENCH=1`) stops at the keel phase and in the door: the keel guard, run over this clone's history, reports three erosions (`lifecycle.rs` seed budget, `keel-guard.py`, `test_keel_guard.py`) and refuses three `Keel:` acks on commits it cannot verify the signature of (`%G? is E`; no signing keys in this VM). None is in my diff (I touched neither file). Same output from `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`. So the later phases did not run under the gate; I ran them by hand: `cargo fmt --all -- --check` ok; `scripts/shape.sh` ok; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo nextest run --workspace --no-fail-fast`: 6415 run, 6381 passed, 34 failed.
- The 34: 20 `theseus-sandbox` (contract clauses and `spawn_100`), 13 `theseusd::sandbox`, all the known root/L1 failures (theseus-pv6i); and `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`, which failed under the full run's load and passes alone (6.1 s). Not run: benches, `deny` (not fetched), the cockpit phase, live checks.
- VM note: the disk allowance filled twice during the builds; I deleted `target/debug/incremental` and stale artifacts and used `CARGO_INCREMENTAL=0`.

## Live check for the maintainer
1. On a scratch daemon with `[privacy] sensitive_names = ["Examplecorp"]` and a profile on a China-origin endpoint: `theseus ask "plan Examplecorp's budget"` should run on a US profile; `theseus health` shows no `privacy.packs` phase. Add a word that occurs in `route.v6` to the list: health's startup shows `privacy.packs` with `route.v6` and a count, and no name.
2. With Discord: share a task channel with a guest, give a role View by hand, finish the task: the channel's members list shows only the operator(s) and the bot; `/task name:<it> share:@guest` answers that the task has ended.
