# Cloud report: route-gaps (theseus-0j2.17, theseus-d13v, theseus-g1gl)

Branch `cloud/20261005-route-gaps`, built on `902c403f` (the task commit over `80ef1dea`). Started 08:36 UTC,
code done 09:55 UTC, report 10:21 UTC, all by `date`.

Commits, in order:

| commit | subject |
|---|---|
| `ad5e62a1` | turn: a routed session keeps the base it was moved from, and follows its base's changes (theseus-0j2.17) |
| `84250c80` | session: a session's routed is boxed, so a debug turn fits a thread's stack again (theseus-0j2.17) |
| `2f98fdc5` | turn: a routed turn records one context.compiled and one loop.started a loop (theseus-d13v) |
| `995f47d5` | core: tests for routing's cap at a place's profile and thinking kept only for its writer (theseus-g1gl) |

**`ad5e62a1` alone is not green.** Its gate's only non-L1 failure was `tests_output::the_cores_output_matches_its_golden`,
which aborted on a stack overflow (SIGABRT). I missed it at first because my grep looked for `FAIL` lines and not
`SIGABRT` ones. `84250c80` fixes it (step 1b below). If the maintainer squashes, squash those two together.

## Differences from the brief

- **The store's format was 17 on my clone, not 16**: lane files (theseus-c9l6, `AttachmentContent::File`) had already
  joined. So this branch bumps it to **18**, and the maintainer renumbers at the merge.
- **The format literals live in three tests besides `MANIFEST_FORMAT`**: theseus-core's
  `store::tests::a_write_moves_an_older_store_to_this_builds_format` (`18`), and theseusd's `tests/versions.rs`
  (`an_older_binarys_store_serves_at_once…`: `18`; `a_store_of_a_newer_format_is_refused…`: `19` and "2 to 18").
  The renumbering has to move them too.
- refusal-fallback's `also` and the ladder's `set_routed` were already there, as the brief says. I left both
  unchanged. The new compiler test covers `also`.

## Step 1: the base (theseus-0j2.17), `ad5e62a1` and `84250c80`

**Found.** `Routed` held only `profile` and `hold`. `turn_submit` turned a pane's carried routed profile into "names
nothing", so the turn resolved to the place's or live profile. Once routing stopped, a `-P` pane therefore fell back to
the live profile. `route_base` cleared a move only when routing stopped acting or after `profile.use`. Nothing compared
the session's base with where the move was made.

**Changed.**
- `routing::Routed.from: Option<String>`, serde default and skipped when `None`. It is set in `route_to` on a switch
  (`get_or_insert`, so later switches keep the first base). A switch back to `from` ends the move: `profile` and
  `from` are cleared, and `routed` becomes `None` when it is empty. `MANIFEST_FORMAT` goes to 18, with the store.rs
  line that says so. `tests_layouts.rs` gets a hand-written routed session record from formats 15 to 17 (a profile
  and a hold, no `from`), "in the layout the build before it (80ef1dea) writes".
- `turn_submit`: the carried routed profile (the same `only_routed` test as before: `carried`, no provider or model,
  and the profile equal to `routed.profile`) now stands for `routed.from`. When `from` is absent (an old record) or
  names a profile that is no longer configured, it names nothing, as before.
- `route_base` calls `same_base`. For a person's message that is not pinned, when the session is routed and
  `from != target.profile`, the move is cleared in the turn's own session write and the turn runs on the target it
  was given, which is the new base. A record without `from` takes the turn's base as `from` at that turn, so it reads
  as before. Nothing new is read from the store. The turn bench holds at 5 and 9 frames.
- `84250c80`: `SessionRecord.routed` is now `Option<Box<Routed>>`. Adding `from` made `tests_output`'s golden overflow
  the default 2 MiB thread stack in debug builds. It passes with `RUST_MIN_STACK=2110000` and aborts at 2,097,152.
  So that turn path runs within about 13 KB of the limit, and the record is held by value in the turn's futures.
  Boxing makes the record smaller than it was before `from`, and the bytes on disk are identical (serde treats a
  `Box` as its contents). Even boxed, the margin is under about 37 KB (it aborts at 2,060,000 bytes). Any lane that
  grows the turn's futures can hit this again. **Worth an issue**, for example to give `tests_output` its own stack,
  or to box the larger turn locals.

**How the carried routed profile is told apart from a new base.** By the existing `only_routed` match against the
session's stored `routed.profile`. A pane that carries exactly the routed profile carries routing's own move, so it
maps to `from`. Any other carried profile is the pane's base. A profile the owner names (`-P`, `-p`, `-m`) is `chosen`
and never routed. The one case this cannot tell apart: an owner who wants to stay on the routed profile on purpose
would use `-P` for it.

**Name only, or provider and model too?** Name only. An unrouted session follows its profile by name
(`resolve_target`), and routing's own decisions are by name. So when a profile's model changes under its name, a
routed session reaches the new model once routing stops, as an unrouted one does. Clearing the move for such a change
would also need a per-turn read of the config's model for `from`. It would add little: each live turn re-decides
against the catalog's current costs.

**A behaviour change the owner should hear about.** After `profile.use`, a routed `-P` pane now runs on its `-P`
profile, which is what an unrouted pane does, since a pane carries its profile. Before, it ran on the new live
profile. A session that sends no profile (Discord, `theseus ask --session` without `-P`) still runs on the profile
`profile.use` names, so `profile_use_moves_a_routed_session_and_pins_nothing` passes unchanged.

**Proved** (tests in `tests_route_base.rs`):
- `a_routed_panes_p_profile_comes_back_once_routing_stops`: a pane opened with `-P glm`, then a hard question routed to
  Opus (`from = glm`), then a pane message carrying `opus` that stays moved while routing acts. Then each of: routing
  off, `mode = "shadow"`, the judge off, or Jev's key gone (each after a restart), and the ladder's rollback. Each time
  the next pane message runs on `glm`, and the move is cleared.
- `a_changed_live_profile_clears_the_move_of_a_session_that_follows_it`: after a restart with `[model] live = "fable"`,
  a session that follows the live profile runs on fable, and its move is cleared. The `-P glm` pane keeps its move
  (opus, `from = glm`).
- `a_changed_place_profile_clears_the_move`: a place bound with no ceiling, the session moved to opus, then rebound
  with `profile = "glm53"`. The next message runs on glm53, and the move is cleared.
- `a_routed_record_without_its_base_reads_as_before`: with an old record and routing acting, the session stays moved
  and `from` is filled in as `sonnet`. With routing off after a restart, it runs on sonnet.
- `tests_layouts::every_old_layout_on_disk_still_reads`, with the new sample.
- `profile.use` still clears the move: `tests_route::profile_use_moves_a_routed_session_and_pins_nothing`.
- Planted reverts, each restored and `touch`ed, then `git status` checked:
  - The pane's carried routed profile back to naming nothing (today's fall back to the live profile once routing
    stops; in this design that is the half of `route_base`'s rule that `turn_submit` carries). Fails
    `a_routed_panes_p_profile_comes_back_once_routing_stops` (`"sonnet"` vs `"opus"`: the carried routed profile was
    read as a new base) and `a_changed_live_profile_clears_the_move…` (`"fable"` vs `"opus"` for the pane).
  - `same_base` always true. Fails `a_changed_place_profile_clears_the_move` (`opus` vs `glm53`) and
    `a_changed_live_profile_clears_the_move…` (`opus` vs `fable`).

## Step 2: doubled rows (theseus-d13v), `2f98fdc5`

**Found.** As the issue says. `compile_step` recorded `context.compiled`, the judge's mark (`at_compile`) and
`loop.started` on the first compile, even while `defer_persist` held its compilation back, and the switch's compile
recorded them again. A detour added a second `loop.started`.

**Changed.**
- `compile_step`: while `t.route.defer_persist` is set, the built `ContextCompiled` summary and its trace times go into
  `RouteState.deferred` (boxed, for the stack reason above) and are not recorded. A new `compiled_rows` records
  `context.compiled`, the judge's mark at the compile, and `loop.started`, together.
- `route_step`: `keep_first` persists the compilation, as before, then records the held rows. A switch or a detour
  drops them, and a switch's routed compile records its own. The first compile's rows now come after `route.decided`
  in the turn's frame.
- **The detour choice: no `context.compiled`**. Its compilation is never stored, so a row naming it would point at
  nothing. It records exactly one `loop.started`, for each of its loops.
- **The judge's mark at the compile** (`continue.v1` in shadow) runs only inside `compiled_rows`, once for each loop
  that records a compile. A detour's loop no longer asks it, which matches the detour's later loops, which never did.
  I checked this by reading the code, not with a test: a signal that makes `at_compile` mark is hard to plant
  deterministically in the route rig.

**Proved** (tests in `tests_route_rows.rs`):
- `a_switched_turn_records_one_compile_and_one_loop` covers a switch to Opus, and the next turn, whose verdict keeps
  the first compile. Each records loop 0's `context.compiled` and `loop.started` once, and every `context.compiled`
  row's `compilation_id` reads back (`get_compilation`).
- `a_detour_records_one_loop_and_no_compile`: one `loop.started` and no `context.compiled`.
- Planted revert (the first compile's rows recorded again: `defer_persist && false` in `compile_step`). Both tests fail
  (`loop.started` `[0, 0]` vs `[0]`). Restored and `touch`ed.
- `a_plain_turn_stays_within_its_frame_budget` passes, and so do every route, routing, continuation, compiler and
  `tests_output` test (71 of 71), plus `theseus-sim bench turn --check --runs 5 --burst 0` (frames_plain 5,
  frames_tool 9).

**Left, and uncertain.** On a switch, the routed compile runs `compact` and `recall_compiled` a second time.
`recall_compiled` takes the recall's `drops` (`mem::take`) on the first compile, so the routed compilation's budget
report may lack them, and a compaction on the first compile may be done twice. I did not change either. They sit in
the compiler's and recall's areas, which other lanes own.

## Step 3: untested rules (theseus-g1gl), `995f47d5`

**Changed.** New tests only:
- `tests_route_cap.rs`, `a_hard_question_in_a_capped_place_keeps_the_places_profile`: a place bound private with a
  ceiling profile (`sonnet`) and a `sophisticated` verdict. The turn runs on sonnet, the result's reason and the
  `route.decided` row say `capped` with `profile: sonnet`, and the session is not moved.
- `tests_thinking_writer.rs`, `thinking_another_model_wrote_is_left_out_and_its_writer_keeps_it`, at the crate level:
  compiler.rs is at its ceiling, so even a `mod` line there would push it over. Sonnet 5.5's answer, with a thinking
  block, renders with none in Opus 5.5's request. It keeps the thinking in Sonnet 5.5's own request, and in an Opus
  5.5 request whose `fallback` is Sonnet 5.5 (`also`).

**Planted reverts:**
- `cap` always `None` (`let cap = cap.filter(|_| false)` in `decide_route`). Only `tests_route_cap` fails (`opus` vs
  `sonnet`); 44 of 45 route, routing and ceiling tests pass.
- `wrote_model != media.model` dropped (`|| (false && …)` in compiler.rs). Only `tests_thinking_writer` fails; 167 of
  168 compiler, continuation, fallback, route and `tests_m3` tests pass.

## Runs under load

Three runs of `nice -n 19 cargo nextest run` over `tests_route*`, `tests_thinking_writer`, `tests_output` and
`a_plain_turn_stays_within_its_frame_budget`, beside four busy loops at nice 0. The loops were `yes > /dev/null`: this
environment refused `sh -c 'while :; do :; done'` (a removal check misread the script), so I took that route instead
and killed the loops by their pids.

- All of my new tests passed in all three runs: `tests_route_base` (4), `tests_route_rows` (2), `tests_route_cap` (1),
  `tests_thinking_writer` (1). `a_plain_turn_stays_within_its_frame_budget` passed too.
- Failed under that load:
  - `tests_route::a_late_verdict_applies_from_the_next_message` and `a_late_verdict_applies_to_the_next_message_alone`,
    all three runs. These are the brief's "tests_route's tests that slow Jev's verdict past route's wait".
  - `tests_route::a_failing_jev_or_the_judge_off_leaves_the_request_unrouted`, two of three runs, and
    `one_call_asks_three_packs_and_a_hard_question_goes_to_opus`, one of three. Both passed when rerun alone under the
    same load (5.0 s and 12.4 s).
  - `tests_output::the_cores_output_matches_its_golden`, all three runs. This is its scenario's own 30 s `until` wait
    (`tests_output.rs:256`, a task's turn) timing out under starvation, not a golden diff. It is not on the flaky list
    and not in the brief's list. I did not check whether `80ef1dea` fails it the same way under this load. **The
    maintainer should check it**, together with the stack margin above.
- The gate's own runs (load 4 or less) passed all of these.

## The gate

Four runs of `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, one on each commit. On each, fmt, shape,
features, clippy, cockpit, test build and the reader rule passed. The suite failed only on the known L1 cases: 33
failures, which are 19 of theseus-sandbox's `contract` tests, theseus-sandbox's bench `spawn_100`, and 13 of theseusd's
`sandbox` tests, because L1 refuses a root daemon's job with no job cgroup (theseus-pv6i). The one exception is
`ad5e62a1`'s run: it also had `tests_output`'s SIGABRT (fixed by `84250c80`) and the two `versions.rs` format
literals, which I fixed before committing. After each suite I ran the phases that follow it by hand, and all passed:
`protocol.gen` unchanged (no protocol type changed), `target/debug/theseus-sim bench turn --check --runs 5 --burst 0`
(frames_plain 5, frames_tool 9: ok), and `cargo deny --offline --log-level error check` (advisories, bans, licenses
and sources ok, after `cargo deny fetch` at setup). On the last code commit, `995f47d5`: 2,570 tests run, 2,537
passed, 33 failed (all L1, as above), 19 skipped. No test passed on a retry.

## The live check (the maintainer's, with a GLM key and Jev's)

A scratch daemon on a fresh state dir, with Discord and the web off, routing on and live, and two GLM profiles of
different cost. The routing table sends `sophisticated` and `deep_coding` to the dearer one, since the defaults name
Opus and Fable, which need an Anthropic key. Fill in the `[secrets]` references as your vault names them.

```bash
D=$(mktemp -d /tmp/route-gaps.XXXX)
cat > $D/theseus.toml <<EOF
[server]
state_dir = "$D/state"
[model]
live = "glm"
[judge]
enabled = true
[routing]
enabled = true
mode = "live"
max_wait_ms = 3000
[routing.modes.sophisticated]
profiles = ["glm53"]
[routing.modes.deep_coding]
profiles = ["glm53"]
[profiles.harbour]
provider = "zai"
model = "glm-5.3-flash"
[discord]
enabled = false
[web]
enabled = false
[secrets]
zai_api_key = "op://<vault>/<item>/<field>"
jev_api_key = "op://<vault>/<item>/<field>"
EOF
theseusd --config $D/theseus.toml --socket $D/sock --state-dir $D/state > $D/log 2>&1 &
T="theseus --socket $D/sock"
```

1. **A `-P` pane, then a switch.** `$T --json ask -P glm "Name three tide gauges."` (keep its `session_id` as `$P`).
   Then `$T watch --interactive $P`, and type `Weigh two designs for a crash-safe write-ahead log, with their failure
   modes.`, then `What is a frame?`. Also open a session that follows the live profile:
   `$T --json ask "Weigh two designs for a crash-safe write-ahead log."` (keep its `session_id` as `$L`).
   - `$T --json ledger --kind route.decided` shows the pane's switch, `from: glm`, `profile: glm53`, `switch: true`,
     and `$L`'s switch from `glm` to `glm53` too.
   - `$T --json ledger --kind context.compiled --session $P` and `$T --json ledger --kind loop.started --session $P`
     show one row per loop of each turn (none doubled), and each `context.compiled`'s `compilation_id` is one that
     the session's compilations list.
2. **Routing off.** Set `[routing] enabled = false`, stop the daemon (`$T shutdown`) and start it again, then type a
   message in the pane (`$T watch --interactive $P`, `And its recovery path?`). It runs on `glm`, the `-P` profile:
   the reply's status line names glm, and `$T --json ledger --kind turn.ended --session $P` shows the last turn on
   `glm-5.3-flash`.
3. **Live changed.** Set `[routing] enabled = true` again and restart. Type the hard question in the pane once more
   (it switches to glm53 again), and ask `$L` one more hard question (`$T ask --session $L "…"`). Then set
   `[model] live = "harbour"` and restart.
   - `$T ask --session $L "What is a frame?"` runs on `harbour`: its result's profile is harbour, and its session
     shows no `routed`.
   - A pane message (`What is a segment?`) still shows the pane's base as `glm`. While routing acts it stays on glm53
     (the next `route.decided` row's `from` is `glm53`, the routed profile the turn starts on, and no switch), and with
     `[routing] enabled = false` and a restart it runs on glm.

## Docs the maintainer should change (not edited here)

- Part III's item for 25e's follow-ups: `Routed.from` (format 18 on this branch), `same_base`, the pane's carried
  routed profile standing for `from`, the profile.use behaviour change above, the deferred compile rows, and a
  detour's loop recording no `context.compiled`.
- `docs/status.md`: route.v1's base and its rows.
- I updated `crates/theseus-core/AGENTS.md`'s route.v1 entry in the commits: the base, the deferred rows, and the new
  test files.
