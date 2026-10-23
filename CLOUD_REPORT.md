# CLOUD_REPORT: ops-verbs (theseus-jid7t.2)

Branch `cloud/20261023-ops-verbs`, from `main` at `2d1d8d6f`. Commits: `b5cb7d3` (the row), `f186e98` (fmt: sort the `ops` module
line), `223c693` (clippy: redundant clones in a kernel test), `4ec69ab` (a revert's restore, tested). The commits are
unsigned (no key on the VM) and carry the trailer `CLOUD_TASK.md` names.

**First section: the numbers.**
- Store format stays **49**: the hold is a META key, the new ledger kinds and fields are serde-default. No `tests_layouts` sample owed.
- `crates/theseus-protocol/src/lib.rs`: **+10 lines**, two over the brief's 8: the module line, three method lines, and two fields
  of three lines each (`#[serde(default, skip_serializing_if…)]`, `#[cfg_attr(test, ts(optional))]` and the field; rustfmt will not
  put them on one line). 2,573 lines, under the 2,733 ceiling. Everything else is in `ops.rs`.
- `turn.rs`: **no `mod` line and no call added**; one existing call site changed (the report's target read, see item 2): 2 lines
  changed, 3,469 lines still.
- `kernel.rs`: untouched. The kernel's new code is `task_edit.rs` (one `mod` line in `lib.rs`).
- No new dependency. Other files touched outside the brief's list, each by a field line or a moved call: `data_step.rs`
  (`uncleared_profile` now calls `data_mark::uncleared`), `persona_step.rs` (`target_as_ran` reads the hold), `methods.rs`
  (`sticky_named`, the two new fields), `outbox.rs` (a `task_unbound` and an empty-place record), `push.rs` and `rpc/work_board.rs`
  (stamping), `task_start.rs` (`start_of` split so `start_time` is shared), and `data: None` in `WorkView`/`SessionInfo` initializers
  of the TUI, `theseus` and `theseus-discord` renderers and tests (`render/board_v2.rs`, `render/snapshot.rs`, `render/task_view.rs`,
  `runtime/tasks.rs`, `courier/tests_v2.rs`: one field line each, no behaviour; `golden/` untouched).

## What I built, by item

1. **`session.profile { session_id, profile | null } → { session_id, profile, previous, refused?, note? }`**
   (`rpc/session_profile.rs`, `Core::session_set_profile`; `fact/ops.rs`'s `SessionProfileSet`, ledger `session.profile_set`
   with `{session_id, from, to, by}`). The hold is META `session.profile.<session>` = `{profile, was}` (`null` once let go), one
   frame with its row. Authority: `judge_act(Act::Session)`, as `session.retire`. A refusal is an **ok answer with `refused` set**
   and nothing written (no META, no row); `note` is set for a restricted-origin profile on an open session ("the floor decides
   each turn"). Same profile twice, or letting go of nothing, writes nothing.
2. **`task.rehome { task, home } → { task, from, to, note }`** (`rpc/task_rehome.rs`). `home` is a conversation's session id or
   `thread:<slug>`. It moves where the report is posted: `TaskOf.target` on the task's record, the execution's `reports_to`,
   and META `task.place.<session>` (empty = none), in one frame with the `task.rehomed` row, through the new kernel
   `Kernel::edit_task` (`task_edit.rs`: one frame, the execution's record and the caller's records; the parent never changes).
   The budget stays carved from the original parent and the parent's next turn still reads the report (`note` says so).
   **Found and fixed:** a turn in flight held the task's record as read at its start, so its end-of-turn report went to the *old*
   place. The frame that ends the task now reads the target again (`TaskOf::target_for_now`, `task_home.rs`; the one
   changed call in `turn.rs`). Without this a rehome of a running task did not take effect. Refused: an ended task; a missing,
   retired, task-kind or shared-place home; a missing or done thread; a home whose mark is weaker than the task's. A home with
   no place of its own (a terminal conversation, a thread no place has shown) sets the target to none: the report stays on the
   board and in the parent's next turn. A `thread:<slug>`'s place is where that thread's session last answered
   (`Outbox::show_at`), so a thread with no shown place reports nowhere. Authority `Act::Task`. No Discord code runs.
3. **`task.reschedule { task, start_at_ms | start_after_ms | start_when } → { task, starts_at_ms, previous }`**
   (`rpc/task_reschedule.rs`, ledger `task.rescheduled`). Only `task_start::starts_at(e)` (Waiting, `DueAt`, no turn, a parent);
   a held (paused) task is refused ("resume it first"). The time is parsed by `Core::start_time`, split out of `task.start`'s
   `start_of` (shared, not copied). It moves the execution's due wake and `task.deferred.<session>`'s `due_at_ms` (`set_at_ms`
   stays) in one frame; the check is repeated under the kernel lock.
4. **The data mark** (`DataMark` in `ops.rs`; `data_mark.rs`'s `session_mark(store, cfg, session_id)`): `private` when the Google
   private mark is on the session or any task above it (`google::private::lineage`), else `open`. **The data judgment keeps no
   verdict per session** (`DataAnswer` is per turn, `data.taint` is bookkeeping that "never gates"), so none is read; if the
   owner means something else by "the stored verdict", say so. `SessionInfo.data` is filled in `session_info`; `WorkView.data`
   is stamped as a view leaves the board (`Core::stamp_data` on each frame in `Push::apply`, and `work_snapshot_marked` for
   `work.watch`'s snapshot) because the board reads no store. Only conversation and task views carry one. A mark taken
   between two frames shows on that view's next frame. `sensitive` is defined and documented, never produced here.
5. **`ProfileInfo.origin`** (`Option<String>`, from `Config::profile_origin` through `data_gate::word`: `us`, `eu`, `cn`,
   `unknown`), filled in `profile_list`.
6. `cockpit/src/protocol.gen/` regenerated by the protocol test; three wire fixtures in `tests/wire/ops/` (a subdirectory, because
   `every_fixture_decodes_as_an_event…` requires every top-level fixture to be an event notification).
7. Docs: core `AGENTS.md` (one paragraph) and protocol `AGENTS.md` (`ops.rs`). `docs/` lists no protocol methods.

## The sticky profile's precedence, where it is read, how a task picks it up

For a turn: **(1)** its own `profile` (not a pane's `carried` one) > **(2)** the hold > **(3)** the last turn's (`carried`, or routing's
`from`), the place's, the live one. Read in two places: `turn.submit` (`TurnRunner::sticky_named` in `methods.rs`'s resolution) and
`target_as_ran` (`persona_step.rs`: what a continuation, a wake, a task's next turn, keep-warm, the census and `context.explain`
run on, so they agree with the turn). Routing still moves a turn that was not pinned; `chosen` (a pin) is only the turn's own
profile/provider/model. **A running task takes the hold at its next turn, not "its next loop"**: a turn fixes its model once
(`route_to` routes once: `"a turn routes once"`), so loops inside a turn stay on the model the turn began on. The test shows the
in-flight call and the second loop of the same turn on the old model, and `target_for_session` (what `continue_execution` builds)
on the held one. `null` restores the session's `last_target` from before the hold (`was`), so a continuation goes back to where it
ran and not to a model the owner let go of.

## The refusal function and its table

`crate::data_mark::profile_refusal(cfg: &Config, catalog: &Catalog, mark: DataMark, profile: &str) -> Option<String>` (`pub(crate)`),
pure over `refusal_of(mark, profile, known, cleared) -> Option<String>` and built on `data_mark::uncleared`, which
`TurnRunner::uncleared_profile` (the route floor's) now calls, so they share one function. `per_turn_note(cfg, catalog, mark,
profile)` is the open-session caveat. Table (refused?): unknown profile: always. Known profile, mark `open`: never. Mark
`private` or `sensitive`: refused iff the profile's origin is not `us` (`cn`, `eu`, `unknown` refused; `us` set).
`turn/tests_sticky.rs` pins it against `floor_refuses` for every mark × origin (`cn`, `eu`, `unknown`, `us`) × (`glm`, `sonnet`);
`rpc/tests_session_profile.rs::the_refusal_over_every_mark_and_origin` tests `refusal_of` directly, `DataMark::Sensitive` included.

## Where `// ops-cal:` waits, and the join line

`crates/theseus-core/src/data_mark.rs`, in `session_mark`: replace the `match google::private::lineage(...)` with the level read, i.e.
`match crate::google::private::level(store, cfg, session_id) { Some(Level::Sensitive) => DataMark::Sensitive, Some(_) => DataMark::Private, None => DataMark::Open }`
(adjust to ops-cal's actual return type). No test changes: the sensitive cases test `refusal_of` and `DataMark` directly.

## The Discord check

No verb calls Discord code. Each test of a verb asserts `kernel.outbox_actions().len()` is unchanged by it
(`no_verb_posts_anything_to_a_place`, `moving_a_report_posts_nothing`, `the_ledger_row_names_the_task…_nothing_is_posted`). There is no
fake Discord server in these rigs; the outbox is where a binding would read a post from. `cargo nextest run -p theseus-discord`
(with protocol and kernel): 1086 passed, 4 skipped. `git diff --exit-code origin/main -- crates/theseus-discord/src/golden/`:
exit 0 (printed `golden-unchanged`). A rehomed task whose new home has a place has its report posted there by the ordinary
report path (the report staging reads the record), not by any Discord edit.

## Tests, with their plants

Red-first on the base is vacuous for the three methods (they do not exist there), so each behaviour has a planted revert:
(files `rpc/tests_session_profile.rs` = SP, `tests_task_rehome.rs` = TR, `tests_task_reschedule.rs` = TS, `tests_data_mark.rs`
= DM, `turn/tests_sticky.rs` = ST, kernel `tests_task_edit.rs` = KT.) Each plant: edit, run the named tests, `git checkout` the file,
`touch`, `git status` clean (the script did it; `/tmp/plants.py`).

| Plant | Test that failed | Failing line |
|---|---|---|
| `sticky_named` ignores the hold | SP `the_hold_stands_across_turns_and_a_turns_own_profile_beats_it_once` | SP:168 |
| `target_as_ran` ignores the hold | SP `a_running_task_takes_the_hold_at_its_next_turn_not_mid_turn` | SP:448 |
| refusal treats private as open | SP `a_cn_profile_on_a_private_session_is_refused…`, `an_eu_profile_on_a_private_session…`, `the_refusal_over_every_mark_and_origin` | SP:97, 108, 365 |
| rehome skips the mark check | TR `a_private_task_never_reports_into_an_open_home` | TR:320 |
| report reads the stale target | TR `the_next_report_lands_in_the_new_home…`, `a_thread_is_a_home_and_a_conversation_with_no_place…` | TR:174, 237 |
| reschedule leaves the wake | TS `the_due_scan_wakes_it_at_the_new_time…`, `previous_and_a_second_call_restore_it` | TS:82, 96 |
| `uncleared` always clears | ST `the_menus_refusal_and_the_routes_floor_agree…`, `an_unconfigured_profile_is_refused…` | ST:50, 76 |
| views not stamped | DM `work_views_carry_the_mark_and_a_task_follows_its_parent` | DM:112 |
| revert does not restore `was` | SP `a_revert_puts_back_the_target_it_ran_on_before_the_hold` (my first test of this passed with the plant; I wrote this one after) | SP:489 |

Other tests (all pass): SP: `a_cn_profile_on_an_open_session_is_set…` (note says the floor decides), `an_unknown_profile_is_refused`,
`only_the_owner_from_a_private_place_holds_a_session_to_a_model` (authority), `null_reverts_and_previous_is_the_undo`,
`the_ledger_row_names_the_session_the_change_and_who`, `a_mark_taken_after_the_hold_still_cannot_reach_the_model` (no judge: the
provider's floor refuses, nothing sent), `a_mark_taken_after_the_hold_moves_a_routed_turn_off_it` (route on: moves to the next
usable profile, `route.from` names the held one), `the_method_has_its_arm_and_refuses_an_unnamed_connection`. TR: ended task /
missing home / shared place / task-as-home refusals, authority, `thread:<slug>`, the carve (parent's reserved and spent, the new
home's untouched), the ledger row, no post. TS: each of the three forms, previous/undo, refusals (started, no deferred start, held,
past, two forms, none), the META times, the row, authority. DM: `session_list` open/private, Google off reads open, a task private by
its parent's mark taken after it started, frames carry the mark, `profile_list` origin for `us`, `cn`, `unknown`, `eu`;
`google/tests_private.rs::a_google_read_shows_as_the_sessions_data_mark` (a real read through the fake Google). KT: the due scan
on a virtual clock wakes at the new time and not the old, moved back, the carve and parent unchanged, the caller's records ride the
frame. Protocol: `ops_verbs` (wire), `the_ops_fields_are_optional_on_the_wire` (old readers). The brief asked for the due scan "on a paused
clock": the kernel clock is a `VirtualClock`, which is what drives the due scan (a tokio paused clock does not).

Runs: `cargo nextest run -p theseus-protocol -p theseus-kernel -p theseus-discord`: 1086 passed, 4 skipped.
`-p theseus-core task session data`: 563 passed. `-p theseus-core a_revert_puts_back` and clippy for core after the last commit: pass.
No run under load was asked for; none was made.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `223c693` (with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`:
this VM's disk filled twice at full debuginfo; flag it if the owner's machine builds differently). Door, keel, keel tests, fmt, shape,
features, clippy, clippy-probe, cockpit, test build, reader rule: ok. **Suite: 8080 run, 8046 passed, 34 failed**, all L1:
14 in `theseusd::sandbox` (`health_reports_the_last_real_l1_launch`, `l1_argv_routes_a_call_to_l1`, `the_jobs_bench_l1_row`,
`a_probe_script_in_l1_shows_the_contract_and_l0_the_contrast`, `a_job_with_no_list_has_no_proxy`, and nine more),
19 in `theseus-sandbox::contract` (`clause_01`…`clause_12`, `egress_18b_*`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`,
`sigterm_is_forwarded_to_the_command`, `a_job_that_cannot_start_says_why`) and `theseus-sandbox::bench spawn_100`: the root-daemon L1
refusal (theseus-pv6i) the brief names. None of the other known cases failed. Phases after the suite, by hand: protocol types
(`git diff --quiet cockpit/src/protocol.gen`: clean), `cargo deny --offline check bans licenses sources` (ok; no advisory database
fetched), size step (skipped: no release build). The lifecycle, jobs and turn benches did not run (`THESEUS_GATE_NO_BENCH`; the budgets
are the owner's machine's). After that run: `4ec69ab` (a test and its fmt) was checked with `cargo fmt --check`,
`cargo clippy -p theseus-core --all-targets -- -D warnings` and its own test, not the whole gate again.

**Keel guard** `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: no output (exit 0).
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean.

## The live check for the maintainer

A scratch daemon on a fresh state dir (never the owner's), with `[google]` on and a fake Google, or just `session.profile` on an open session:
- `theseus --socket <s> ask …` to make a session; then a raw JSON-RPC `session.profile {"session_id": "<id>", "profile": "opus"}` should answer
  `{profile: "opus", previous: null}`; the next `turn.submit` with no `profile` should report `profile: "opus"`; `profile: null` should answer
  `previous: "opus"` and the next turn runs on the live profile.
- After the session reads Google (or `META session.profile`-less: put the private mark through a Google tool call), `session.profile` with
  a `cn` profile should answer `refused: "this conversation holds private content, and `glm` is not a model of US origin"` and
  `theseus ledger` shows no `session.profile_set` for it.
- `task.start` a deferred task (`start_after_ms: 120000`), then `task.reschedule` with `start_after_ms: 600000`: `previous` is the first time;
  `theseus task list` shows `starts_at_ms` moved; calling again with `start_at_ms: <previous>` restores it.
- `task.rehome` a running task to a conversation bound to another Discord place: its report posts there. (This needs the owner's Discord.)
  Note: the CLI has no command for the three verbs and `client::OPERATORS` does not list them: a job's shell could call them raw (the
  speed bump the guide describes does not cover them). A CLI row, with the `OPERATORS` entries, is a follow-up.

## What is left or uncertain; design choices for the owner

- A running task takes a held model at its next turn, not mid-turn (above). Moving a turn's model mid-turn would need `route_to` to route twice.
- `task.rehome` moves only where the report is *posted*. The report still lands in the **original parent's** next turn (the carve and
  `Execution.parent` stay; changing the parent needs a stored-record change and a format bump). The brief's "`wake_parent` target
  moves" is therefore not done: `wake_parent` is a flag on the original parent's wake. If the owner wants the new home's next
  turn to read it, that is a follow-up with a format bump to 50.
- A refused `session.profile` is an ok answer (`refused`), not an RPC error, so the page can show the words; the `task.*`
  refusals are RPC errors (`invalid params`, or `refused` for the place rule, with the words), as `task.pause`'s are.
- `data` on a work view is stamped as it goes out, so a mark taken between two frames shows at that view's next frame; a snapshot is exact.
- `WorkView.data` costs one META read per conversation or task view per frame when `[google]` is on (none otherwise).
- Follow-ups: CLI verbs and `OPERATORS` entries; the cockpit's menu calling `data_mark::profile_refusal` (through a method, since it is
  `pub(crate)`: the page can only grey by `ProfileInfo.origin` and `SessionInfo.data` today, which is the same table); `docs/status.md`
  sentence below.

For the integrator's `docs/status.md`: "ops-verbs (theseus-jid7t.2): `session.profile` holds a session to a model from its next turn
(the floor's own refusal; `null` reverts and `previous` is the undo), `task.rehome` moves where a task's report is posted (the carve
stays with its parent), `task.reschedule` moves a deferred task's start; `SessionInfo.data`, `WorkView.data` and `ProfileInfo.origin`
tell the page what each may reach. Store format unchanged."

## Data-routing review

Every way a turn's model is now chosen, and its guard:

1. **The turn's own profile** (`turn.submit { profile }`, not `carried`): `resolve_target`; pinned (`chosen`), so routing never moves it.
   Guards: the route floor `floor_unrouted`/`data_floor` at the first loop (a judge-less core's refused base fails at the provider chokepoint
   `data_gate.rs`, as before), then the chokepoint for every request. Tests: `tests_data_floor`, `google/tests_private` (existing),
   and SP `a_mark_taken_after_the_hold_still_cannot_reach_the_model` for the chokepoint on a held base.
2. **The hold** (new): replaces the base of a turn that names no profile of its own. It cannot bypass anything: the base still goes
   through the same floor (item 1's guards), because the hold is only a base. `session.profile` additionally refuses what the floor would refuse
   *now* (`profile_refusal`, shared code with the floor's `uncleared`), pinned by ST for every mark × origin. A mark taken *after* the hold
   is caught at turn time: SP `a_mark_taken_after_the_hold_moves_a_routed_turn_off_it` (route on: moved, `route.from` the held profile,
   nothing sent to it) and `…still_cannot_reach_the_model` (no judge: the chokepoint refuses, nothing sent). Open-session restricted
   holds are allowed and caveated (`note`): a turn admitted open may run there, any other turn is moved by the floor (`restricted`
   branch of `floor_refuses`; existing `tests_data_floor`).
3. **The last turn's profile** (`carried`, `session.last_target`, `routed.from`): unchanged; the hold beats it. Same floor.
4. **The parent's** (a task's creation copies the parent's target to `last_target`): unchanged, behind the hold for a held task. Same floor,
   and a task's floor includes its lineage (`private_session` reads `lineage`).
5. **Routing's pick** (route.v1, `route_profiles(floor)`): unchanged; it skips every profile the floor refuses, from the hold's base too.
   SP `…moves_a_routed_turn…` covers a held base it must leave.
6. **The floor's move** (`floor_unrouted` → `[model]` default with `Reason::DataGate`; or the turn ends): unchanged. The hold only changes the
   base it starts from.
7. **Continuations** (`target_as_ran`): now read the hold first. They run through the same `run` and the same floor at their first loop.
   SP `a_running_task_takes_the_hold_at_its_next_turn_not_mid_turn`.

`task.rehome` keeps a private task's report out of an open or shared home by the mark check
`home.mark >= task.mark` (both from `session_mark`; the task's includes its lineage, so a parent marked after the task started counts:
TR `a_private_task_never_reports_into_an_open_home`, plant P4) and by refusing a shared-place home (`class_of(home) == Shared`:
TR `an_ended_task_a_missing_home_and_a_shared_place_are_refused`). **What it does not guard:** a thread home is taken as private
without a place check (threads are private places by `PlaceRule::class`), and its mark is its session's if it has one, else `open`: a
private task cannot report to a thread whose session has no mark, since `open < private` refuses it; but a thread *with no session
yet* has mark `open`, so a private task is refused there too (safe direction). The posted report goes to the home's place target; a
home's place that was open when chosen can become shared later: rehome checks at the call, as `task.start`'s `home` does.
The report text itself still reaches the original parent's next turn (above), which is where it already went.

Unsure of: (a) the mark's source (item 4: no per-session data verdict exists, only the Google mark, so `session_mark` ignores a
session an open run judged private message by message); (b) `WorkView.data` staleness between frames; (c) `target_for_now` falls back
to the turn's copy when the record cannot be read, which is the earlier behaviour; (d) a task paused before its first turn and a
deferred start both wait on `DueAt`, which is why `task.reschedule` checks the pause (`PAUSED_AT_MS` tasks would otherwise be
"deferred"): the check is `TurnRunner::task_paused`, tested by `a_started_task_a_task_with_no_deferred_start_and_a_held_one_are_refused`.
