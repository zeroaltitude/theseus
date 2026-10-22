# CLOUD_REPORT: tail6-cn (theseus-4h85l, theseus-atbgl, theseus-uaqox)

Branch `cloud/20261021-tail6-cn`, base `49352e8a`. Commits: `249f246e` (uaqox), `90a53a3a` (atbgl), `72da27ad` (4h85l).
Commits are not signed (no key on this VM). No new dependency, no stored field: **store format stays 49**.

## theseus-4h85l

1. **Footer word only when the gate decided.** The test lives in `turn/data_step.rs` `cn_would_win`, called from `data_gate_why`. The route decision knows its filtered candidates only as unusable profiles, so the check re-walks the mode's list as for a cleared ask (`classes.effective(mode, true)` and `routing::pick` over profiles built with `cleared: true`) and gates only if the winner is restricted. The floor's own words (`gated`, e.g. `private_view`, `sensitive`) are unchanged. `route.decided`'s other data fields are unchanged.
2. **Open-only turn.** `note_open` now builds its route through `open_route` (pure, in `data_step.rs`) with an empty `reason`. The CLI's `status_line` already skips an empty reason, so it shows `open` alone. Discord still shows `routed: open` (its footer prefixes the data word when there is no mode or source); changing that is in `theseus-discord` `runtime/route.rs`, which I did not touch (ceiling tight). Left for the owner to decide.
3. **`sensitive` end to end.** `a_sensitive_name_gate_is_on_the_row_and_counted_once`: a name in the message with a pinned EU-origin profile (only the names' floor excludes it), so the default runs, row `data_gate: sensitive`, footer `data gate: sensitive`, counter once. Note: with a China-origin profile the word is the admission's (`verdict_private`), since a name makes the verdict private; `sensitive` arises only for a non-restricted, non-US profile.
4. **Chokepoint counter.** `Gated` holds a `data_gate::Counter` (`Arc<OnceLock<Telemetry>>`), set in `Core::build` and `build_telemetry` (`Core.gate_counter`); no global. `guard` takes it (two test callers updated). Pairs: `chokepoint` / `restricted_origin` and `chokepoint` / `sensitive`.
5. **Background counters.** `record_data_gate`: `compaction`/`<DataWhy word>` (the brief wrote `data_gate:<word>`; I used the bare word, as the route does), `consolidation`/`restricted_origin`, `keep_warm`/`restricted_origin`, `replay`/`restricted_origin`. `unrestricted`, `refuse_restricted`, `keep_warm_prepare` became `pub(crate)` so one test can drive them.

## theseus-atbgl
`tests_tool_screen::a_turns_fetch_that_redirected_into_the_owners_network_is_not_screened_open`: a stand-in `http.fetch` toollet answers `meta.final_url = http://10.0.0.10/tides`; the open task's result fails the screen (`outside` row, taint). The data-level test already used `10.0.0.10`.

## theseus-uaqox
Row, not "writes nothing": a refused reservation spends nothing, so the `data.reply` row (no choice, no cost, run unchanged) is written through `land` after the same between-turns wait; an unprepared ask (state/client unbuilt) still writes nothing. The module doc says so. Test: `a_refused_reservation_writes_the_reply_row_without_an_answer`.

## Proof
Targeted: `cargo nextest run -p theseus-core -p theseus-protocol -p theseus-discord`: **4925 passed, 0 failed**. `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean.

Planted reverts (each: break, test fails, restore, `touch`, `git status` checked):
- drop `cn_would_win` (old `cn_profiles` non-empty rule) -> `a_private_turn_a_us_model_wins_anyway_shows_no_data_gate` fails (tests_data_footer.rs:89).
- restore `reason: "no_verdict"` in `open_route` -> `an_open_only_turn_shows_no_route_reason` fails.
- `sensitive` word changed -> `a_sensitive_name_gate_is_on_the_row_and_counted_once` fails.
- remove the chokepoint `counted` call -> `the_chokepoint_counts_its_refusal` fails.
- remove the replay `record_data_gate` -> `the_background_refusals_are_counted` fails.
- `final_url` replaced by `None` in `screen_step.rs` -> the atbgl test fails.
- return before `land` on a refused reservation -> the uaqox test fails.

The open-only test is a unit test of `open_route`, not a whole turn: a whole turn that carries a pass but asks no route needs a continuation of an execution that is not waiting, which the rigs can't set up cheaply.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, keel tests, fmt, shape, features, clippy, cockpit **passed**; the **test build failed with "No space left on device"** (this VM's disk allowance filled by the debug build; the full-workspace test phase did not run). Not a code failure. The suite for core, protocol and discord was run separately (above). The maintainer should rerun the gate and the daemon/sim test crates.

Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0.

## Live check for the maintainer
Scratch daemon with a config listing a China-origin profile in `trivial`'s `cn_profiles` after Haiku (`cn_after = "haiku"`): send a private message; the footer should show no `data gate:`. With `cn_after` unset the same message shows `data gate: verdict_private`. `theseus.route.data_gate` should count `chokepoint`, `compaction`, `consolidation`, `keep_warm` and `replay` pairs when those paths refuse.

## `docs/status.md` sentence
"The route footer's `data gate: <why>` now shows (and counts) only when the data rule kept a turn off a China-origin model it would otherwise have taken; the provider chokepoint and the compaction, consolidation, keep-warm and replay refusals count on `theseus.route.data_gate`."

## Follow-ups
Discord's `routed: open` prefix; `keep_warm.telemetry` is not set by `build_telemetry` (existing gap, not touched).
