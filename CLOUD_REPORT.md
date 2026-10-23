# CLOUD_REPORT: tail7b-budgets (theseus-d9je7 rows 1, 3, 6)

Branch `cloud/20261021-tail7b-budgets`, base `ae420a49`. Commits: `c2d757e1` (row 1), `c86943b0` (row 3). This report is the last commit.
No file is over its ceiling. Store format stays 49; nothing here is stored. No budget, ceiling or test assertion was loosened (see "Existing tests" for the five that now run on a quick detour).

## Row 1: a trivial detour carries `tools_find` only (c2d757e1)

**Found.** `defer_tools` never read the route mode, so a trivial detour got the place's families.
**Changed.** `Deferral` gains `bare` (`tools_defer.rs`). `defer_step::deferral_in` takes a small `Said { ask, jev, bare }` and, when bare, starts `loaded` empty (no place defaults, board loads, `always_loaded`, plan family or conversation `wanted`); only `want::extra`'s families (named by the message, or judged relevant by Jev) are added. `resolve` falls back to "every tool" only when the set is empty and not bare, so it now returns `tools_find` alone. `detour_step.rs` names the row's tools part `trivial: tools_find only`.
**What still loads on a trivial turn.** `tools_find`; any family the message names; any family Jev judged relevant. I kept the Jev rule: it is a judgment about this very message and it is held to the same fit/ceiling, so a "hey, check my bill" Jev calls AWS still reaches AWS. A model with provider tool search (Mechanism::Search) is unchanged: the existing test says the API refuses a request whose tools are all deferred, and I could not check that here. Trivial detours go to cheap profiles without tool search, so this is the case that matters.
**Tools part, before and after** (tokens; the count the tests use): cli 6,526 before (census total 7,353 less 827 of system), 225 after; DM about 8,600 before (default shape; the owner shape about 8,800), 225 after.
**Proof.**
- `cargo nextest run -p theseus-core`, whole crate, `TZ=America/Phoenix`: 4,129 passed, 0 failed (before the last test was added); targeted runs after: tests_trivial_tools 3/3, threads/detour/defer/families green.
- Plant 1 (`let bare = false && ...` in `defer_step.rs`): `a_trivial_detour_carries_only_tools_find` failed (the place's families came back) and so did `..._naming_a_family_still_loads_it` (fs_read in the list). Restored, touched.
- Plant 2 (bare keeps no named family): `a_trivial_detour_naming_a_family_still_loads_it` failed (`["tools_find"]` only). Restored, touched.
- `a_quick_detour_keeps_its_tools` passes (files, commands, tools_find).
**Existing tests.** Five tests set Jev's mode to `trivial` and then read the budget-raise rule (a named AWS family admitted past the cli's 8,000 "by exactly its size"). A bare request is small, so the family now fits under the budget and nothing is raised. I changed only their mode input from `trivial` to `quick` (same detour, same assertions), with a comment: tests_families (2 places: `detour()` and `rig_on`), tests_detour `a_detour_naming_a_family_stays_within_its_place_budget`, tests_route_defer `..._an_aws_tool_within_the_budget`. The trivial-path versions (`a_detour_without_tool_search_reaches_a_tool_the_message_names`, tests_families line 297) still run on trivial and pass. The maintainer may want to look at this: keel-guard reported nothing.

## Row 3: threads' leeway (c86943b0)

`[threads] working_percent` (default 40, 1-100), `working_tokens` is now `Option<u64>` (unset by default; when set it wins; the 2,000 floor applies to a set value), `resume_keep_turns` default 20. `ThreadsConfig::working_size(window)` is the one answer; it is read in `thread_context::cold_spec`, which builds `Cold`, the value `compiler.rs` and `compiler/cold.rs` compare against (so those two files are unchanged). The window is the catalog's, lowered by `[context] working_window_tokens`, 1,000,000 when the model is unknown (as `home_size_step`). A 1M window gives 400,000 and a 200k window 80,000; a set `working_tokens` wins.
**Tests.** `the_working_size_is_a_share_of_the_window` (config), `a_cold_resume_keeps_twenty_turns`, `a_cold_resume_works_to_a_share_of_the_window` (rpc/tests_thread_context), `the_templates_threads_section_is_the_default` (updated, green; template lines for `working_percent`, `working_tokens` and `resume_keep_turns = 20`).
**Plants.** Working size fixed at 60,000: `the_working_size_is_a_share_of_the_window` failed (left 60000, right 400000). Default keep 6: `the_templates_threads_section_is_the_default` (left 6, right 20) and `a_cold_resume_keeps_twenty_turns` failed. `cold_spec` reading `working_tokens.unwrap_or(60_000)`: `a_cold_resume_works_to_a_share_of_the_window` failed. All restored and touched.
**Note.** `keep_turns` is the most turns a resume keeps (it keeps the newest N, then fewer only if they do not fit), so the 20 has bite only on a thread over the working size. A 1M thread of 20 turns is ~80k, so most resumes now stay whole.
**For the owner's config:** the maintainer should check whether it sets `working_tokens` or `resume_keep_turns`; if so that value keeps winning.

## Row 6: the first request (no change)

Measured by the census tests (`--no-capture`, this build). Smallest = measured x 1.05, rounded up; "owner" is `owner_shape.toml`, which `the_proposed_live_block_loads_on_the_owner_shape` asserts equal to.

| place | default shape | owner shape | budget | smallest with 5% margin (worse shape) |
|---|---|---|---|---|
| cli | 7,353 | 7,548 | 8,000 | 7,926 |
| task | 6,606 | 6,801 | 8,000 | 7,142 |
| dm | 9,432 | 13,027 | 15,000 | 13,679 |
| channel | 7,441 | 7,889 | 7,900 | 8,284 (above today's budget) |
| headless | 7,867 | 8,062 | 8,600 | 8,466 |

The cli, task and DM budgets could be kept, or lowered (task to about 7,150). The channel budget has 0.1% of room on the owner shape, so it is the one that needs raising (about 8,300) if the owner wants a 5% margin; I changed nothing.
**Why the channel reads 8,090 on the installed daemon.** On the owner shape the channel is 655 + 241 tokens of system and 6,993 of tools (task_create alone 2,652, wake_at 1,032). The tools are the same on any shape, so the 201-token excess over the 7,889 is in the system part. A census of a fresh session has no bound place: no membership guidance for the channel's and guild's categories, no place standing records, no real public-readers context files. Those are what a live channel adds. I can show the gap lies in the system part, not which of those it is; I could not read the installed daemon. Check: `theseus census --place channel` on the installed daemon and compare its system sections with the shape's.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_PROFILE_*_DEBUG=0` to fit this VM's disk): door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule passed. Suite: 7,931 passed, 34 failed, all the known L1 cases (19 theseus-sandbox contract, 1 theseus-sandbox bench `spawn_100`, 14 theseusd sandbox). I then ran the later phases by hand: `cargo deny --offline check` ok (after `cargo deny fetch`), generated-protocol check: `git status` shows no change in `cockpit/src/protocol.gen`. Benches skipped (`NO_BENCH`). `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean. The disk allowance filled twice during the gate; I deleted stale build outputs only.
Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0). Commits are unsigned (no key), as the brief says.

## For the integrator
- `docs/status.md`: "A trivial detour carries `tools_find` plus any family its message names or Jev judged relevant (225 tokens of tools, was about 6,500 at the cli); a thread's cold resume works to 40% of the serving model's window (`[threads] working_percent`; `working_tokens`, when set, wins) and keeps 20 turns (theseus-d9je7)."
- Core AGENTS.md updated (threads paragraph, tool deferral paragraph).
- Follow-ups: a Search-mechanism (tool-search model) trivial detour is unchanged; the channel budget decision; `home_size_step.rs`'s module comment still names the old 60,000.
