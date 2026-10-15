# CLOUD_REPORT: initiative-voice (theseus-slvl.9)

Branch `cloud/20261013-initiative-voice`. Store format: **unchanged** (4 new ledger kinds, 1 META key, no stored field).

## What I found
- A wake's result and a task's report are already turns of a session, so they already speak as its persona (`persona_of`; a task takes its parent's). The persona contract only needed building for messages with no turn: nothing wrote a consolidation finding or a morning note.
- The built-in persona already carries the one line "A promise to act later is scheduled in the same turn." (persona-data). I documented it for owner persona files in the example template.
- Base `9fa84735` fails the keel phase on its own history (see Keel findings).

## What I changed (commit dc6b519a, one commit)
1. **Persona contract**: `initiative/speak.rs` (persona file or built-in default + standing block as system, one factual task sentence, no tools, `[initiative] profile`, reserved on the day ceiling, held to `[initiative] day_usd` read back from `initiative.spoke` rows; `initiative.spoke` is now a `day_ceiling::SPEND_KINDS` kind), `initiative/lines.rs` (one template table reading only persona `name`/`emoji`; used past the cap, with day_usd 0, or on a model failure). Producers: morning note (`initiative/morning.rs`, driver tick via `due_without_turns`, one lock+compare per tick, once a day, only if a task ended/wake fired/promise unkept/note proposed in the last day) and the nightly consolidation finding (`initiative/consolidation.rs`, chatty only). Posts go to the owner's DM (courier kind `initiative`, key `ini:<id>`). Wake and task turns write `initiative.spoke` (kind wake/report).
2. **Promises**: `initiative/promise.rs` (phrase table, fenced code and quotes ignored), `turn/promise_step.rs` (`promise.kept` if the turn set a wake/watch/task, counted by `Pending::scheduled`, else `promise.unkept`; no row for replies without a promise). `theseus persona` shows the week.
3. **Level**: `theseus_protocol::Initiative` (moved from core config, re-exported), `Initiative::lets`, `Rules::initiative` + `Rules::shaped` in `notices::policy`. quiet: finished roots and non-reminder wakes inform instead of interrupting; reminders and met watches never; normal = table as written (default unchanged); chatty: promised check-ins interrupt, observations allowed.
4. **Verdict**: `initiative.rate` / `initiative.week`, `theseus notices rate up|down [ID]` (in OPERATORS), Discord 👍/👎 on a reply or note (`runtime/route.rs`), `initiative.rated` row; last verdict per message stands.
Config `[initiative] profile, day_usd (0.50), morning_hour (8)` with template lines. Golden `core_output.txt` regenerated (two rows and one narrative line; frame count unchanged, only new rows).

**Header cost**: 0 tokens added to any request header (the persona default already had the line). The voiced call has its own small request (persona + standing + ~40-token task sentence).

## How proved
- `cargo nextest run -E 'test(tests_initiative)'`: 8 passed (promise kept/unkept, wake turn carries persona and is said first, task spec carries parent's persona, consolidation request carries persona and standing block, template in persona's name past/at zero cap, day cap from rows, morning obeys level/once/only-when-something-happened, verdict recorded/last stands/refusals). Protocol `the_initiative_level_shapes_a_planted_days_pings` (7 events x 3 levels), promise and lines unit tests.
- **Planted reverts**: `scheduled().max(1)` made `a_promise_with_a_wake_...` fail; `lets(Morning) || true` made `the_morning_note_obeys_the_level...` fail. Restored and touched.
- Full suite (`cargo nextest run --workspace`): 5500 run, 40 failed: 33 known L1/sandbox; `rendering_costs_little_per_hundred_lines`, `tests_kept::fs_read_of_a_long_file...`, `stop_checkpoint::a_clean_stop...`, `work::the_work_board_and_the_cli...`, `herdr::a_change_reaches_herdr...`, `wakes::a_reminder_survives_a_stop...` passed when rerun alone (load); `tests_output` golden regenerated as above. `cargo clippy -D warnings` and `cargo fmt --check` clean. `theseus-sim bench lifecycle --runs 10 --check` in a debug build: starts/shutdown/kill/swap budgets ok; it reports "LIFECYCLE BUDGET MISSED" for rows I did not touch (budgets are for the owner's release build; not investigated).
- **Not run**: the full `scripts/gate.sh` (stops at the keel phase on the base's own history), the cockpit phase, deny, and the live scratch-daemon check.

## Live check for the maintainer
Scratch state dir, config with `[personas.ondine] name="Ondine" emoji="🌊" files=[...]`, `default_persona="ondine"`, `api_base` at `theseus-sim fake-model --rules <file>` with rules: `{when:"PROMISE", text:"I'll let you know when it lands."}` and a `wake_at` rule (after 5s) then a promise.
1. `theseus ask "PROMISE"`; then `theseus ledger -n 20 --kind promise.unkept` shows one row; with the wake rule, `promise.kept` shows one.
2. After the wake fires: `theseus ledger --kind initiative.spoke` shows `kind: wake, persona: Ondine`; the model dump (`fake-model --dump`) shows the persona file in the wake turn's system.
3. `theseus notices rate up` prints `👍 recorded on turn_…`; `theseus persona` ends with `initiative this week (normal): promises kept 50% (1 kept, 1 unkept)` and the verdict line.
4. Morning note and consolidation finding need a bound DM (fake-discord) and `morning_hour = 0` / `initiative = "chatty"`.
**Week-long measure**: promises kept >= 95% and the 👍 share, both from `theseus persona` (`initiative.week`).

## Left open / design notes
- The owner-decision pacer (one unprompted DM post per 30 minutes) had not joined this base; morning and consolidation posts go straight to `initiative_post` and should call the pacer when it merges.
- `theseus persona --json` is unchanged (persona list only); the week is in text mode and `initiative.week`.
- The matcher is a substring table (`initiative/promise.rs`); revisit after a week of rows for false positives.
- Failure messages are not voiced: failed tasks/turns already notify through `notices::policy` templates; I left them templated.
- Docs to update at review: spec Part III item, `docs/status.md`.

## Keel findings expected
None added by this branch. `scripts/keel-guard.py` fails on the base: `crates/theseus-sim/src/lifecycle.rs:155` budget-raised, because the ack on `1c2beb2747` has an unverifiable signature here (`%G? is E`: no signing keys on this VM). Not mine.
