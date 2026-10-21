# CLOUD REPORT: effort-on-evidence (theseus-53ujg.3)

Branch `cloud/20261021-effort-on-evidence`, from `main` at `1d092ded` (+ the task commit `8b698a8`). Started
2026-10-21 06:22 UTC.

The base has the routing lane's join: `tests_turn_checkpoint.rs`, `placed_by_route`, and `TURN_SOURCE` are all
present. I didn't rename, re-gate or re-time any of the conversation-turn checkpoint machinery, and `SOURCE` and
`TURN_SOURCE` are unchanged.

## The step: one commit, `add7062`

`routing: a checkpoint raises effort without a move, and a routed profile never runs below its own effort (theseus-53ujg.3)`

### What I found
- `checkpoint_verdict` computed effort only on a move: `target.as_ref().and_then(|_| self.route_effort(…))`. A
  verdict that kept the profile and asked for more effort was recorded and then dropped.
- `route_effort` always went through `stick`. A move's one-step raise past `keep_warm_min_tokens` was held at the
  previous level (`sticky`), so a checkpoint's evidence was overruled by the cache rule.
- `routing::effort::decide` let Jev's sure answer replace the profile's own effort outright. A routed `haikuhi`
  (Haiku at high) therefore ran at `medium` whenever Jev said `medium`.
- **Item 3 applies.** `checkpoint_apply` used `let State::On(mut a) = std::mem::take(..) else { return }`. That
  put `Off` back as `Unread` (the `Default`), so an ineligible turn ran `checkpoints_for` again at every loop's end.
  The routing lane had not fixed this.

### What I changed
- `routing/effort.rs`:
  - `Runs.floor: Option<Effort>`.
  - In `decide`: `ran = max(gated, floor)`, then the bounds clamp, so the bounds win. The floor is not gated.
  - New `EffortReason::Floored` (`"floored"`), recorded only when the floor raised the final level, i.e. when
    `ran > gated.clamp(bounds)`. It counts as `applied()`.
  - The module doc says all of this.
- `turn/route_step.rs`:
  - `route_effort` takes an `EffortAt` (the session, `acts`, and whether it is a checkpoint) instead of `acts: bool`.
  - It sets `floor` to the profile's own effort when the profile is not the session's base. The base is
    `effort_base`: `session.routed.from`, else the turn's own profile.
  - `compile_first`'s call grew by one line; it is at clippy's 100-line limit again.
- `turn/effort_step.rs`: `stick` takes `checkpoint` and never holds a checkpoint's raise (`asked > held`). A
  checkpoint asking one step lower is still held, as before.
- `turn/checkpoint_step.rs`:
  - Effort is computed when the verdict keeps the profile, as well as on a move.
  - On a keep, `raised` applies the level only when all of these hold:
    - the decision applied;
    - the level is above the turn's current `spec.effort` (none set counts as `high`, the API's default);
    - the model does not take per-message effort.
  - The level goes on `spec.effort`, with no slot used. `t.route.result` is set (`checkpoint_routed`, shared with
    the move path), and the row's `effort_applied` comes from the level actually placed (`Verdicted.applied`).
  - A per-message model or a lower level records `effort_applied = false` and changes nothing.
  - The move path's application is unchanged, apart from the floor and the sticky exception.
  - `Off` (and `Unread`) stay as read in `checkpoint_apply`.
  - The module doc has a paragraph on all of this.
- Tests:
  - New file `tests_checkpoint_effort.rs` (one `mod` line in `lib.rs`).
  - `routing::effort::tests::a_routed_profiles_own_effort_is_its_floor`.
  - `tests_turn_checkpoint::an_ineligible_turn_pays_one_match`. Its counter is a `#[cfg(test)]` map keyed by
    session in `tests_turn_checkpoint.rs` (`READS`), not a bare `AtomicUsize`, so parallel tests in one process
    don't interfere. `checkpoints_for` bumps it under `#[cfg(test)]`.
  - `loops`, `ASK` and `checkpoints` in that file are now `pub(crate)` for reuse.

No store format change, no protocol change, no config key, no new model-facing text. Nothing new runs on a plain
turn: a turn with no checkpoint pays the same `match`, now once instead of a read per loop.

### How I proved it
`TZ=America/Phoenix CARGO_INCREMENTAL=0 cargo nextest run -p theseus-core --no-fail-fast -E 'test(checkpoint) | test(effort) | test(route)'`:
**188 passed, 0 failed.** That covers every existing checkpoint, effort and route test: `tests_route_effort`,
`tests_effort_sticky`, `tests_turn_effort`, `tests_task_checkpoint`, `tests_turn_checkpoint`, `tests_route*`, and
`routing::*`. None of them was edited.

New tests:
- **`a_checkpoint_raises_effort_without_a_move`** (the key test). A `sonnethi` profile (Sonnet 5.5 at high) is
  added to the template, Sonnet is marked as taking no per-message effort, and `keep_warm_min_tokens = 1` stands in
  for `tests_effort_sticky`'s `long` config. The route places routine coding on `sonnethi` at high; after three
  failed loops the checkpoint keeps `routine_coding` and asks `xhigh` at 0.9. The test checks that:
  - every request goes to Sonnet;
  - the first loops carry `high`, and every request from loop 3 to 6 on carries `xhigh`;
  - the row says profile `sonnethi`, `switch: false`, `effort_reason: applied`, `effort_applied: true`,
    `effort_ran: xhigh`;
  - the result's `effort_applied` is `xhigh`.
- `xhigh_and_max_still_need_high_confidence_at_a_checkpoint`: `max` at 0.69 gives `held_high`, all nine requests
  stay `high`, and `effort_applied` is false. `max` at 0.71 raises the turn to `max`.
- `a_checkpoint_never_lowers_effort`: on the session's base (live profile `sonnethi`, so no floor applies), a
  verdict asking `low` leaves all nine requests at `high`, with `effort_applied: false`.
  - Note: the brief's `medium` is one step down, which the sticky rule already holds on a long prompt (the row then
    says `sticky`; I saw this in the first run). `medium` would therefore not exercise the never-lowers guard, so
    the test asks `low`, two steps down, and its doc says why.
- `a_model_with_per_message_effort_is_not_raised_mid_turn`: with the built-in Sonnet row (per-message effort), the
  raise is recorded with `effort_ran: xhigh` and `effort_applied: false`, and every request runs at `high`.
- `a_routed_profile_never_runs_below_its_own_effort`, on whole turns:
  - `haikuhi` with a sure `medium` sends `high`, reason `floored`;
  - the base profile (`sonnet`, via `chat`) with a sure `low` sends `low`, `applied`;
  - bounds `[low, medium]` with `haikuhi` sends `medium`, `applied` (the bounds win).
- `an_ineligible_turn_pays_one_match`: a pinned turn of 9 loops reads its checkpoint state once.

Planted reverts. Each was planted with a script, the named test was run, the file was restored from a copy and
`touch`ed, and `git status` showed only the intended changes after each restore:

| # | Plant | Test that failed |
|---|---|---|
| 1 | effort computed only on a move again (`(keeps \|\| target.is_some())` → `target.is_some()`) | **`a_checkpoint_raises_effort_without_a_move`**: "never xhigh" |
| 2 | `stick` holds the checkpoint's raise (the exception's `.filter` removed) | **`a_checkpoint_raises_effort_without_a_move`**: "never xhigh" |
| 3 | the gate skipped on checkpoints (`high_effort_confidence` 0.0 when `at.checkpoint`) | `xhigh_and_max_still_need_high_confidence_at_a_checkpoint`: the 0.69 run went `high ×4, max ×5` |
| 4 | the floor dropped (`floor: None`) | `a_routed_profile_never_runs_below_its_own_effort`: sent `medium`, expected `high` |
| 5 | lowering allowed (`level > now` → `level != now`) | `a_checkpoint_never_lowers_effort`: requests went `high ×4, low ×4, high` |
| 6 | `Off` not put back in `checkpoint_apply` | `an_ineligible_turn_pays_one_match`: 9 reads, expected 1 |

Under load (AGENTS.md's recipe: the tests at `nice -n 19`, four busy loops at nice 0, killed by pid), I ran
`tests_checkpoint_effort`, `tests_turn_checkpoint` and `tests_task_checkpoint` together (24 tests) three times:
- **Every new test passed in all three runs**, and so did every `tests_turn_checkpoint` test.
- Three existing `tests_task_checkpoint` tests failed in 2 or 3 of the runs, each taking about 30 s:
  `a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes`, `a_task_steps_back_to_its_base_after_two_agreeing_verdicts`
  and `the_cap_stops_moves_up_and_not_the_way_down`.
- **These failures predate this change.**
  - The three tests pass when run alone under the same load.
  - On the base commit `8b698a8` (a worktree, same target, same load, two runs of the 18 checkpoint tests), the same
    file failed 4 and then 3 tests: those three plus `a_checkpoints_text_is_bounded_for_a_task_of_long_loops`.
  - They pass unloaded, both in the gate and in the targeted runs.

  They are not on `.config/nextest.toml`'s flaky list. They are the task rig's 30 s waits under CPU starvation,
  and may deserve an issue.

`cargo clippy -p theseus-core --all-targets -- -D warnings` and `cargo fmt --all -- --check` are clean, and the gate
ran both over the workspace.

### The live check for the maintainer
On a scratch daemon (its own `--config`, `--socket`, `--state-dir`) with Jev on, routing live, and a profile like
the bench's `sonnethi` (Sonnet 5.5, `effort = "high"`) leading `routine_coding`:
1. `theseus --socket <sock> ask "Set up the build for the parser crate, and make its failing tests pass"` on a
   scratch repository whose build fails for several loops.
2. `theseus --socket <sock> judge list --kind route.decided` (or the turn's `theseus why` / the cockpit inspector).
   This should show a `turn checkpoint: failing` row with `profile` equal to `from` (no switch). When Jev asked
   `xhigh` or `max` above 0.70, it should also show `effort_applied: true` and `effort_ran` at that level.
3. The turn's later provider calls (`turn.inspect`) should carry `output_config.effort` at that level from the
   loop after the checkpoint on. The cache write at that loop is expected: one rewrite of the messages.
4. A routine-coding turn on `haikuhi` where Jev answers `medium` should show `effort_reason: floored` and requests
   at `high`.

### What is left, and choices the owner should hear about
- **Per-message models.** The brief says only `claude-fable-5-1` takes per-message effort in the built-in catalog.
  On this base Sonnet 5.5's built-in row also says `per_message_effort: true` (the 2026-10-17 probe). With the
  built-in catalog a Sonnet turn checkpoint's raise is therefore recorded, not applied. If the owner wants the
  raise on Sonnet, it needs a per-message placement mid-turn (a message before the next user message). That is a
  compiler question I left alone. Only a model whose row says no per-message effort (Haiku, Opus, or Sonnet with
  the row turned off) gets the raise.
- **A request with no effort.** `raised` treats a spec with no effort as `high`, the Anthropic API's default, so a
  checkpoint's `medium` never "raises" a profile with no effort set.
- **The session's base.** The floor's base is `Routed.from`, else the turn's own profile. A task's brief pick is
  its base, so a brief-chosen profile gets no floor. Only a profile routing moved the session to has one.
- **The floor applies on the move path too.** A checkpoint that moves up to `opushi`, say, runs at no less than
  `opushi`'s own effort. "Never lowers" applies only to the keep path, since a move changes model.
- **Note for the maintainer (from the brief):** `fact/route.rs` (near :122) says "the task moves" for a turn's
  checkpoint too. I left it alone; it is not this row's file.
- **Docs to update at review** (I didn't edit them):
  - `crates/theseus-core/AGENTS.md` "The effort" paragraph: the floor and `floored`.
  - The "A running task's checkpoints" paragraph: effort without a move, the sticky exception, and `Off` kept.
  - The spec's Part III item and `docs/status.md`.
- **Commit trailer.** The brief asks for `Co-Authored-By: Claude (cloud session, Sonnet 5.5)`. The session's own
  rules forbid naming a model in commits, and that name may not be the model that served this run, so the trailer
  is `Co-Authored-By: Claude (cloud session) <noreply@anthropic.com>`. Commits are unsigned, as the brief says.

## The gate
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=false TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on
the step's tree, i.e. `add7062` before commit.

Two environment overrides were needed for the disk. This session's disk allowance is about 40 GB, and the first
workspace build filled it twice: once in incremental caches, once with the duplicated test binaries. So incremental
builds are off, dependency and workspace debug info are off, and the stale artifacts were deleted. Behaviour is
unaffected; only backtraces lose line numbers.

Results:
- **Through the suite**, every phase passed: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe,
  cockpit, test build, and reader rule.
- **Suite:** 7,256 tests run, 7,223 passed, **33 failed**. All 33 are the known L1 failures of a root VM
  (theseus-pv6i), none of them mine:
  - theseus-sandbox's `contract` tests;
  - its bench's `spawn_100`;
  - theseusd's `sandbox` tests.

  Nothing else failed and nothing needed a retry. `tests_output`'s golden passed under `TZ=America/Phoenix`.
- **The phases after the suite**, run by hand as the brief says:
  - protocol types: ok, nothing changed;
  - lifecycle and jobs: skipped, as `NO_BENCH` does;
  - turn: `theseus-sim bench turn --check --runs 5 --burst 0` passes, every first-request budget ok (cli 7,332 of
    8,000; dm 9,411 of 15,000; task 6,585 of 8,000);
  - size: skipped, no release build.
- **Deny:**
  - The setup's `cargo deny fetch` never ran (the first build failed on the full disk), so I ran it afterwards.
  - `cargo deny --offline check`: bans, licences and sources ok.
  - **Advisories fail on two new advisories against main's own Cargo.lock**, which this branch does not touch:
    - RUSTSEC-2026-0382 (`rustls-webpki` 0.103.15, fix ≥ 0.103.16);
    - RUSTSEC-2026-0381 (`webpki-roots` 1.0.9, fix ≥ 1.0.10).

  They need a `cargo update -p rustls-webpki -p webpki-roots` on main. That is not this row's change.
- **Keel guard:** `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0).
