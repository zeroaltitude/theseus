# CLOUD_REPORT: tail-effort-400 (theseus-blzt6)

Branch `cloud/20261021-tail-effort-400`. Every test here ran on the scripted provider: this VM has no provider key,
so nothing was sent to the live API.

No file went over its line ceiling. `turn.rs` is untouched. Store format unchanged (48): nothing new is stored.

## The root, shown red

Key test `a_tool_loop_request_with_an_effort_message_carries_the_beta` (new `tests_effort_beta.rs`, one `mod` line in
`lib.rs`), on the unchanged code, with Sonnet 5.5 as the live model (`c.model.live = "sonnet"`, the rig's own sonnet
profile): a first turn at the profile's level, then a turn Jev puts at `low` that calls `fs_read` once and ends.

    assertion `left == right` failed: request 2: a message and the beta go together: [... user "Where do they nest?",
    assistant tool_use fs_read, user tool_result] ... []   left: false  right: true

Request 2 (the one after the tool call) held the turn's `{"role":"system","output_config":{"effort":"low"}}` message
and had `betas == []`. The hypothesis in the brief is confirmed: `retarget` returned early on a request whose last
message is tool results, before `sync_beta`, so the beta was missing from every request after a tool call.

## What changed (721af5e)

- `compiler/effort.rs`: `retarget` is now `place_own` (the old body, no sync) plus one `sync_beta` after it, for any
  model that takes per-message effort, whether or not a message was placed. `place` is unchanged in shape (it
  returns early for a model that does not take messages, as before). `route_step.rs` also calls `retarget` directly,
  so the sync had to stay inside `retarget`, not move to the end of `place` as the brief suggested. A request with no
  message still carries no beta; a first request's bytes are unchanged.
- `turn/effort_step.rs`: `EffortRefusals` keeps the time each model was marked; `mark(model, now_ms)` and
  `all(now_ms)` leave out marks older than `REFUSAL_HOLDS_MS = 3_600_000`. Callers pass a local `wall_ms()`.
  The ledger row and the once-per-turn retry are as before.
- Comments: module comments in `effort.rs` and `effort_step.rs`, the catalog comment beside Sonnet 5.5 and Fable 5.1,
  and the effort paragraph in `crates/theseus-core/AGENTS.md`. The doc comment of
  `a_400_on_the_messages_asks_again_without_them_and_remembers` says "the next hour".

## Tests and planted reverts

- `tests_effort_beta::a_tool_loop_request_with_an_effort_message_carries_the_beta`: every request carries the beta
  exactly when it carries an effort-only message; the last request must carry a message. Plant: beta synced only when
  the last message opens a turn (the old behaviour). It fails on the loop-1 request.
- `tests_effort_beta::a_model_without_per_message_effort_gets_no_message_from_another_models_record`: a Sonnet turn
  at `low`, then a Haiku 5.5 turn: no message and no beta. Plant: `place` skips its `takes` check. It fails.
- `turn::effort_step::tests::a_refusal_mark_lapses_after_an_hour` (explicit times, no sleeping): held to
  `t + REFUSAL_HOLDS_MS - 1`, gone at `t + REFUSAL_HOLDS_MS`, a second model's mark its own. Plant: a mark that lasts
  forever. It fails.
- Each plant was restored and `touch`ed; `git status` showed only my changes.
- `cargo nextest run -p theseus-core effort first_request`: 49 of 49 pass, with the existing effort tests unchanged
  and the first-request golden untouched.
- A model that takes messages gets the records of other models' answers (the existing
  `the_historys_messages_follow_the_records` covers Fable's; the new key test covers Sonnet's) and now carries the
  beta with them.

## Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`)

Door, keel, fmt, shape, features, clippy, cockpit and test build passed. The suite failed only on:
- theseus-sandbox `contract` (38) and `bench` (2), theseusd `sandbox` (28): the known L1 root-VM failures.
- theseusd `acceptance_journey::the_owner_starts_asks_answers_joins_pauses_ends_and_finds_two_tasks`: a load timeout
  (16 s in the suite); it passes alone (1 of 1, 13 s).

Two earlier gate runs failed because the VM's disk filled (the allowance is about 39 GB, and the
target dir is 27 GB; one failed in the test build, one with about 450 suite failures); I freed the incremental caches and test binaries and reran. The reported run is the third.

Phases after the suite, run by hand: `protocol_types` clean (no protocol type changed); the turn bench
`theseus-sim bench turn --check --runs 5 --burst 0` passes (first-request budgets dm 9,411 / headless 7,846 /
task 6,585 tokens at the p95, all under their budgets); size is skipped by the gate itself. `cargo deny` was not run:
cargo-deny is not installed here (the brief's install step was skipped for disk space), so the deny phase was not run.

Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0).

## For the maintainer: two live checks

1. On a daemon with the fix, after a Sonnet 5.5 turn at a Jev-chosen level that makes tool calls:
   `theseus ledger -k provider.error -n 50` should show no new `output_config` 400, and
   `theseus ledger -k provider.effort_refused -n 50` no new row.
2. One live request pair on Sonnet 5.5: a first request that places `low`, then the tool-loop request that repeats
   the message and now carries `mid-conversation-output-config-2026-07-01`. It should answer 200 and read the cache.

## Left and uncertain

- The root is proved by the scripted provider only: that the live API accepts the message once the beta rides along
  is the live check above, which I could not run.
- Docs to change at review: the spec's Part III item for theseus-posww/theseus-rwpbg.4 could say the 2026-10-11
  refusal was a request sent without the beta; `docs/status.md` the same.
- The one-hour hold is a constant, not a config key, as asked.
