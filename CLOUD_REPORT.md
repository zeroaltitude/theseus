# Cloud report: the review's small changes to 39a, 38a and 28a

Branch `cloud/20261004-smalls-tasks`, from `1bb9b9b` (main at `e27405a` plus the task commit). Three parts, each
its own green commit, in the order asked:

| Part | Issue | Commit |
| --- | --- | --- |
| Layer 1 guards only the owner's tasks; an expired change leaves no proposal | theseus-ext.10 | `a333d4b` |
| A bad place fails alone; health says what each place's start found | theseus-ext.11 | `a07ca47` |
| A check sees the checked task by title and state | theseus-w8ys | `7eef796` |

No new dependencies (Cargo.lock and the package-lock files are untouched). No spec, status, README or design edits.

## 1. Layer 1 guards only the owner's tasks (theseus-ext.10), `a333d4b`

**Found.** As the brief says: `authority_of` decides from the input alone, so every `task.update` of objective or
acceptance and every abandoning `task.close` asked, plan items included; `expire_question` declined the call but
left the proposal on the record. Nothing else differed from the brief.

**Changed.**
- `TaskOrigin.by_model` (theseus-protocol `tasks.rs`; `#[serde(default, skip_serializing_if = Not::not)]`), set
  `true` for a plan item (`create_item`) and for each `task.split` child; `false` for a task session opened with a
  brief (`task.rs::graph_record`, with a comment). Absent reads as the owner's. `TaskRecord::is_owners()`.
- The harness decides it before the gate: `task_graph::tools::authority_for(store, name, input, plan)` drops
  `Plan::authority` when the named task is the model's; toolrun.rs's `gate` calls it (3 lines). A task that cannot
  be read keeps the authority (the safe side; its run refuses it anyway). `lock_for_call` and `proposed` use the
  same word (`owners_target`), so no proposal is ever written on a plan item. `update` and `close` apply a plan
  item's layer 1 without approval, as `task.updated` / `task.closed` (no `change_accepted`), version +1,
  `task.changed`, the narrative line.
- The view marks the owner's tasks (`task_graph::OWNERS_MARK`, ", the operator's objective" after the owner) and
  its head says only a marked task's change waits; task.update's and task.close's descriptions (and two schema
  descriptions) say the same.
- `expire_question` (confirms.rs, ~15 lines): takes the task's lock (`lock_for_answer`) before the frame, stages
  `tools::expired(...)` in the expiry's frame (the decline's code, now `cleared(.., verb)`), and announces
  `change_expired` after. New ledger kind `task.change_expired`, its fact `TaskChangeExpired` in `FACTS`.
- `MANIFEST_FORMAT` 14 → 15, and `tests_layouts` gains `TASK_BEFORE_MODEL_MARK`, a format-14 plan item (written by
  hand in the old layout; the round trip test holds it byte for byte). The version tests move to 15/16
  (`store::tests::a_write_moves…`, theseusd `tests/versions.rs`). Cockpit types regenerated
  (`TaskOrigin.ts`, `TaskChanged.ts`). The core golden's one task line gains the mark.

**Proved.**
- `tests_task_layers::a_plan_items_layer_one_applies_at_once_and_so_does_a_split_childs`: a plan item's
  objective+acceptance change and a split child's acceptance change apply with no question at the template's
  posture (notify), v1→v2, `task.updated` rows, no `change_proposed`/`change_accepted`; abandoning the other child
  applies at once (`abandoned`, v2, `task.closed`); the view's head names the mark and no plan item carries it.
- `tests_task_layers::an_old_records_change_waits_and_its_expiry_clears_the_proposal`: the layout sample, appended
  as raw bytes, reads as the owner's; its acceptance change waits (`layer 1` in the reason, proposal on the record
  with the card); `expire_questions(expires_at_ms - 1)` expires nothing, `expire_questions(expires_at_ms)` one;
  the proposal is gone, version 1, `task.change_expired` names the card, the call is answered `Declined`. Frames:
  the expiry writes 2 on its thread, the same as before (the decline's frame and the card's settle): the clear
  rides in the decline's frame.
- `tests_task_graph::a_layer_one_change_waits_and_accept_applies_it_and_decline_leaves_it` now runs on a task
  session opened with a brief and an arrangement, held open by the stand-in model (`hold_tasks`), since a plan item
  no longer waits: accept, decline and abandon behave as before.
- Planted reverts: brief tasks given `by_model: true` → the task session's waiting test FAILS; the clear in
  `expire_question` switched off (`&& false`) → the expiry test FAILS at `left.proposal.is_none()` (the record
  still holds the proposal). Each file restored, `touch`ed, `git status` clean of it.

**Live check (maintainer).** A fresh scratch daemon, Discord and the web off, `[secrets] anthropic_api_key`, and
`[kernel] confirm_ttl_secs = 60` (`S=<dir>/sock`):
1. `theseus --socket $S ask "Record a plan item titled 'Chart the pier' with task.create and no brief, acceptance 'a chart exists'."`
   then `theseus --socket $S ask "Change that task's acceptance to 'every buoy has a depth' with task.update."`
   → no card; `theseus --socket $S tasks` (or `task.get`) shows it at v2 with the new acceptance; `theseus --socket
   $S ledger -k task.updated` has the row.
2. `theseus --socket $S policy tighten proc.run`, then ask for a task with a brief that runs `uname -a` with
   proc.run (it waits on proc.run's card, so it stays open); then ask to change that task's acceptance → the
   task.update waits on a card (`theseus confirm list` shows `layer 1`), and the view's line says "the operator's
   objective".
3. Answer nothing for a minute: `theseus tasks`/`task.get` shows no proposal; `theseus --socket $S ledger -k
   task.change_expired` has the row; the narrative says "the change to task … expired unanswered".

**Left / for the owner.** A plan item made with an arrangement (pieces quoted from the owner's words) is still the
model's: the brief said so, and I kept it. Docs: the core AGENTS.md "task graph" entry and Part III should say
layer 1 is the owner's tasks only, name `by_model`, `authority_for`, `task.change_expired`, and format 15.

## 2. A bad place fails alone (theseus-ext.11), `a07ca47`

**Found.** As the brief says (`check_profiles` failed the binding; `bind_places` only logged a family). `ceiling.rs`
and `places.rs` are named as in flight elsewhere: I did not touch `ceiling.rs`, and `places.rs` gained only a
field, a setter and one line in `health` (~10 lines).

**Changed.**
- theseus-discord `runtime/guilds.rs`: `unbind_unknown_profiles` takes each place whose ceiling names an unknown
  profile out of the bindings it serves (so no route, no session, no answer: like a channel the file does not
  name), and `tell_core` does the start's telling (each guild's word, the places, the warnings). `runtime.rs`:
  `let mut bindings` and the call replace the old check (net −5 lines; 3,427 lines now).
- theseus-core `place_warnings.rs` (new): `Core::place_warnings(bound, unbound)` makes, per place, the unknown
  families (`unknown_families`, moved out of `bind_places`) and the limit check: the ceiling's profile else the live
  one, `reserve_micros(effective_max_tokens, 0)` against `usd_to_micros(spend_limit_usd)`, said as "#pier's $1.00
  limit is below one call's $1.28 on sonnet (before its input)". An unpriced model: no warning, an info log saying
  why. Each warning: `tracing::warn!`, a `place.warned` row with its narrative line ("Places: …."), recorded once a
  start, and health.
- theseus-protocol `places.rs`: `PlaceWarning` and `PlacesHealth.warnings` (lib.rs untouched). New ledger kind
  `place.warned`. CLI: `theseus health` prints each as a warning line under `places:`, and `theseus places` lists
  them. Cockpit types regenerated (`PlaceWarning.ts`, `PlacesHealth.ts`); the cockpit does not show them yet.

**Proved.**
- `runtime::guilds::tests::one_places_unknown_profile_leaves_only_it_unbound` (against the REST stand-in):
  `#lab` (`profile = "nosuch"`) is not among the binding's places, has no session, reads shared, and a `/status`
  there gets no answer; `#pier` and the DM bind and answer `/status`; health's warnings are `#lab unbound` (its
  detail names `profile "nosuch"`), `#pier unknown_family`, `#pier limit_below_call` with the exact sentence above;
  three `place.warned` rows. The 38a test beside it still passes.
- `place_warnings::tests::a_limit_below_one_call_and_an_unknown_family_warn`: $1 warns with both figures, $2 does
  not, and health holds this start's warnings alone; `an_unpriced_model_has_no_figure`.
- `render::places::tests::a_places_warnings_are_lines_of_their_own` (CLI).
- Planted revert: `tell_core` clears every place when one is unbound (the whole binding failing) → the
  other-places test FAILS. Restored, `touch`ed.

**Live check (maintainer).** `theseus-sim discord rig <dir>` and its printed `fake-discord` / `fake-model` /
`theseusd` commands (no keys). Edit `<dir>`'s bindings to three places: `#lab` with `[channel.ceiling] profile =
"nosuch"`; `#pier` with `[channel.ceiling] tools = ["web", "nosuch"]` and `spend_limit_usd = 1`; and the `[[dm]]`.
Start the daemon, then:
- `theseus --socket <sock> health` → the binding `ready`; under `places:` three ⚠ lines: "#lab is not bound: its
  ceiling names profile "nosuch", which the config does not have (configured: …)", "#pier's ceiling names tool
  family "nosuch", …", "#pier's $1.00 limit is below one call's $1.28 on sonnet (before its input)" (the figure
  is the rig's live profile's; $1.28 on the template's sonnet). `theseus places` lists the same.
- `theseus-sim discord say` in the DM → it answers; in `#lab` → nothing.
- The daemon's log has three WARN lines at the start; `theseus ledger -k place.warned` three rows, and the
  narrative "Places: …" lines.

**Left / for the owner.** An unknown profile is now a warning, not a failed binding: a typo leaves one place silent
rather than all of them, which the brief chose. The cockpit's Boundaries view could show `places.warnings` (left
to the cockpit's owner). Docs: theseus-discord AGENTS.md "Guilds and ceilings" and the core's "Ceilings" entry
should name `tell_core`, `place_warnings.rs`, `place.warned` and `places.warnings`.

## 3. A check sees the checked task by title and state (theseus-w8ys), `7eef796`

**Found.** One difference from the brief worth hearing: a task session's scope is its own subtree plus its
parent's line (`scope`), so the checked task appears in a check's view only when the check was created under it
(`parent`), and tasks under the checked task appear only if the check sits under one of them. With no `parent`
the check's view never showed the checked task at all. I built the rule over the whole graph regardless, so it
holds wherever such a line shows.

**Changed.** `task_graph/view.rs`: `render_for(tasks, session, check)` (`render` calls it with `None`, so every
other view is byte-identical), `restricted_ids` (the checked task's subtree and records of excluded sessions,
never the check's own task or anything under it), the bare line `id "title" [state]` (a closed, all-closed one
folds as before but stays bare). `attach` now takes the session record and reads `TaskOf.check` itself, so
`compile_step.rs` changes one argument. `TaskViewSummary.restricted` (absent when 0, so other rows are unchanged);
cockpit types regenerated.

**Proved.**
- `task_graph::tests::a_check_sees_the_checked_task_by_title_and_state` (pure): a checked task with deps and
  acceptance shows `- tsk_maker "…" [done]` in a check under it, the check's own line whole, restricted 1; a task
  under the checked one shows bare in a check under that task; `restricted_ids` never holds the check's own; the
  conversation's view equals `render`'s.
- `tests_check_view::a_checks_view_shows_the_checked_task_by_title_and_state` (whole core, 28a's rig): the maker
  reports, its record gains deps and a subtask, a check opens with `check_of` and `parent` = the maker's record;
  every one of the check's requests has the maker's line bare, no `deps`, nothing of the plan item it waits on;
  its `context.compiled` rows carry `tasks.restricted ≥ 1`; the parent's next view holds the maker's and the
  subtask's full `line()`s and its row has no `restricted`.
- Planted revert: restricted lines rendered with `line()` → both tests FAIL (the pure one prints the full line
  with owner, deps, acceptance and version). Restored, `touch`ed.

**Live check (maintainer).** With the model key: ask for a task (brief + arrangement) that reads a file and closes
its record with evidence quoting it; once it reports, ask for a check of it with `check_of` (and `parent` the
same task, so the checked task is in the check's scope). `theseus ledger -k context.compiled -s <check session>
--json` shows `tasks.restricted` ≥ 1, and the check's request view (trace or `--json` turn) shows the checked task
as `id "title" [done]` alone.

**Left / for the owner.** Whether a check should be put under its checked task by default (so the bare line is
what it always sees) is a design question for 28a/39a; today it is the model's `parent`. Docs: the core AGENTS.md
"Check tasks" and "task graph" entries should mention the restricted view and `tasks.restricted`.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on each commit: fmt, shape, features, clippy, cockpit, test build, the
reader rule pass. The suite (2,328 tests on the last commit) fails only on:
- the 33 L1 contract/sandbox tests of `theseus-sandbox::contract`, `theseus-sandbox::bench spawn_100` and
  `theseusd::sandbox` (the VM runs as root; theseus-pv6i);
- `theseus-core tests_output::the_cores_output_matches_its_golden`: this VM's clock is UTC, and the golden holds
  the owner's negative offset in two wake lines (`-#:#` vs `+#:#`); it passes with `TZ=America/Los_Angeles`, and I
  committed only the one task line the change moves;
- once, on the second commit's run, `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`
  (15.8 s, a timing test, not on the flaky list, not touched here); it passed alone right after and on the third
  commit's run.

The phases after the suite, run by hand on each commit: protocol types clean (after `git add` of the regenerated
TypeScript); the turn bench's shape `frames_plain 5/5, frames_tool 9/9` (the frame budget unchanged; the
maintainer measures the times); `cargo deny --offline check`: advisories, bans, licenses, sources ok (the database
fetched at setup). The lifecycle and jobs benches were skipped (`THESEUS_GATE_NO_BENCH`).

Runs under load (AGENTS.md's recipe: the tests at `nice -n 19`, four busy loops at nice 0, stopped by their pids):
five runs of the 8 new and changed tests (`tests_task_layers` ×2, the layer-one test, the Discord other-places
test, `place_warnings` ×2, both check-view tests): 8/8 passed each run, 17 to 19 s a run.
