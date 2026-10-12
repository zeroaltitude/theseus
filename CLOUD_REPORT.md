# CLOUD_REPORT: reminders (theseus-8ux6x)

Branch `cloud/20261011-reminders`, from the task commit `932b7567` (store format 29 on main as cloned, so this
branch is **format 30**; the maintainer renumbers at the join). Four code commits, then this report.

| Commit | What |
|---|---|
| `3b6d193d` | kernel, store, protocol, core: the record change, notice firing, Home, the owner's verbs, limits, the model's `wake.at` |
| `d3582684` | cli: `theseus remind`, `snooze`, `pause`, `resume`, `done` |
| `70393a1a` | discord: the reminder post with four buttons, `/remind`; the real-daemon restart test |
| `47d86177` | tui: a fired reminder rings; the commitments block and wake-line tests |

## What I found

- Today's wake is a turn of its conversation (`wake.rs`, `kernel/wakes.rs`), posting where the session posts (nowhere for
  a placeless session), with the 30-day constant and 5 per conversation. The due scan (`due_now`) only looks at a
  *waiting* execution, so a notice that must fire while a turn runs needed a term of its own.
- `lib.rs` of theseus-protocol and `kernel.rs` were within a few lines of their ceilings; the kernel's config got one
  field (`wakes: WakeLimits`) and the new kernel code is in `wake_kinds.rs`, `wake_owner.rs` and `when.rs`.
- `dispatch` in `rpc/server.rs` is at clippy's length and complexity limits (100 lines, 25): the owner's five verbs go
  through `rpc_prefixed` (the existing seam), not new arms.
- Main's store format is **29**, not the 27 the brief says. I bumped to 30.

## What I changed, by the brief's items

1. **The record change** (store format 30). `PendingWake` gains `purpose` (`Check` default | `Remind` | `Routine` |
   `Watch`), `run` (`Turn` default | `Notice` | `Task(TaskRun)` | `Probe(WakeProbe)`; named `WakeProbe` because the kernel
   already exports a `Probe` for reconcile) and `paused_at_ms`, each left out of the bytes at its default. `Task` and
   `Probe` are refused by `insert_wake` with words naming the row that will build them. Old layout sample: `tests_layouts`
   (an execution with a check-in and a weekly series as format 29 wrote them); the format asserts in `tests_layouts.rs`,
   `store.rs` and theseusd's `tests/versions.rs` moved to 30/31.
   The kernel's terms projection is renamed `terms.kernel.3` for the new `nw` term (an open execution holding an
   unpaused notice wake): **the first start of an existing store builds the kernel's terms once** (as every earlier
   rename did); measure it on the owner's store.
2. **`Notice` firing** (`Kernel::fire_notices`, `Core::fire_notices`, run from the harness's 500 ms tick: no new timer).
   One frame: the wake leaves (a series re-arms with its numbering), a `wake.fired { notice: true, purpose, … }` row, the
   record a snooze reads (META `wake.fired.<id>`), and the outbox post. No turn, no model call, no admission slot, in any
   state of the conversation, a running turn included.
3. **Home** (`wake_home.rs`): the session the owner's DM runs on, else a "Reminders" conversation made on first use
   (META `home.session`). A reminder with no session (`wake.create`) lives there; a `Remind` wake in a placeless session
   stays where it was set and posts to the DM when it fires.
4. **When it fires**: a `reminder` post where its conversation posts, else where the wake was set, else Home's DM; the
   `wake.noticed` notification (`fact/wake.rs`, sent to every connection that watches anything: `bus::wide`) carries the
   four answers (`reminder_answers()`: Done, Snooze 10 min, Snooze 1 h, Tomorrow 9:00 as direct calls, a note in the
   `rpc/wake_owner.rs` doc says a later `work.answer` will route them). `notices.rs` rule 4 is now by purpose
   (`wake_urgency`, `reminder_notice`); a view that says no purpose keeps the old root guess. `Woke` gained `purpose`.
   With no place bound at all, the post has nowhere to go: the surfaces still hear the notification (the CLI's `watch`,
   the TUI), and the row is in the ledger.
5. **The model's side**: `wake.at { for: "owner" | "self", say_only }`, one sentence in the description and two schema
   properties. The tools block grew about 300 tokens, so the windows of `tests_route_keep.rs` (41,900 to 42,200, billed
   36,400 to 36,700) and `tests_tiering.rs` (40,800 to 41,100) widened by 300, as their comments record for earlier steps.
6. **Owner verbs** (`kernel/wake_owner.rs`, `rpc/wake_owner.rs`): `wake.create`, `.snooze`, `.pause {until?}`, `.resume`,
   `.done`, each one frame with a row (`wake.set`, `wake.snoozed`, `wake.paused`, `wake.resumed`, `wake.done`; three new
   ledger kinds, all written). Surfaces: `theseus remind WHEN NOTE [--every --days --until --session]`, `snooze`, `pause`,
   `resume`, `done`; Discord `/remind when: what:` with autocomplete in `runtime/remind.rs`, and the reminder's four
   buttons (a press is `wake.done` / `wake.snooze { fired }` as the presser; the message edits itself).
7. **Limits**: `[kernel] max_wake_ahead_days` (365; `MAX_AHEAD_MS` is gone; `task_start` and `wake.move` read it),
   `max_wakes` (5, check-ins per conversation), `max_reminders` (100, counted over `pending_wakes()`), `max_routines` 50 and
   `max_watches` 20 declared; all in the template. The commitments block says `⏰ … remind: …` and `⏸ … (paused) …`.

Semantics worth the owner's eye:
- **Pause.** `wake.pause` holds a wake (`paused_at_ms`); the due scan passes it over; `resume` puts a series at its next
  occurrence after now, numbered by its place in the series (days 1, 2, 3 passed while held: the next is `#4`). With
  `until`, nothing is held: a series' next occurrence at or after it, the ones before counted as passed over; a one-shot's time
  moves to it if later.
- **Snooze of a fired reminder** sets it again from its record. For a series' fired occurrence it makes a one-shot with
  an id of its own (the series goes on). The CLI names a fired reminder by its whole id (its notice carries it).
- **The owner's verbs are anyone's**, like `wake.cancel` (not `judge_act`): a reminder asks nothing of a model or a tool,
  and `max_reminders` bounds a flood. If the owner wants them judged, add an `Act`.
- **A reminder's post is no node in Home's transcript** (a node written while Home's turn runs would trip the debug
  transcript check), so Home's model does not see it when the owner replies in the DM. A follow-up could write a note at
  Home's next turn.
- META `wake.fired.<id>` is one record per wake id (a series overwrites its own); never deleted.

## How I proved it

Setup note: the VM's disk allowance is about 40 GB and filled three times; I ran with `CARGO_INCREMENTAL=0` and pruned
`target/debug/incremental` and stale test binaries. `cargo deny fetch` ran (the chain had stopped before it).

New tests, all passing:
- kernel `tests_wake_owner.rs` (7): a reminder fires in **one frame** (`frames_written_here`) with no turn while the
  conversation's turn runs; a turn takes a check-in and leaves a notice; pause/resume keeps the numbering (`#4`);
  `pause --until`; snooze is one frame (pending and fired); `Task`/`Probe` refused with words and nothing written; the
  horizon (365 days in, 366 out) and the caps (`max_reminders` over two conversations, `max_wakes`).
- kernel `when::tests` (4), core `wake_when::tests` (3): the time grammar.
- core `tests_reminders.rs` (6, a whole core, the driver, the real clock, a stand-in model that counts requests):
  a reminder with no session lives in the DM and is posted there with **no model call**; a CLI-session reminder posts to
  the DM (**0 silent**); with no DM it lives in one "Reminders" conversation and every surface gets `wake.noticed` with
  the four answers; a snooze by button re-arms and fires again, `wake.done` writes its row; the configured horizon;
  the model's `for: owner, say_only` makes a notice that takes no turn.
- protocol `tests/wake_notice.rs` (3, byte goldens: the notification, the notice with its four answers, a late one and a
  check-in) and `tests_notices.rs` (rule 4 by purpose); core `tests_commitments` (the block's golden for `remind:` and
  `(paused)`); CLI `render::wakes` (wake line, a fired reminder's lines); TUI `tests_order` (rings once; check-in and
  `--notify off` ring nothing); discord `runtime::remind` (3) and `tests_gateway/remind.rs` (a reminder reaches the DM with
  four buttons, never silent; Snooze 10 min re-arms ten minutes ahead; Done writes its row).
- theseusd `tests/wakes.rs::a_reminder_survives_a_stop_fires_once_late_with_no_model_call_and_a_snooze_fires_it_again`
  (the horizon rig's restart): `wake.create` with no session, `kill -9`, down 9 s past the due time, restart: it posts
  **once** in the DM, "late, the daemon was down", four buttons, `wake.fired {notice, while_down: true}`, no model request;
  `wake.snooze { fired }` fires it again. 12 s.
- Rewritten expectations (the meaning moved on purpose, none deleted): the kernel frames golden (`PendingWake`'s Debug has
  three more fields; `THESEUS_GOLDEN=write`), the format asserts, the 30-to-365-day strings in `wake.rs`,
  `task_start.rs` and `tests_tasks_deferred.rs`, the voice command count (`/remind` registers after `/orders`).

**Planted reverts** (each restored and `touch`ed, `git status` checked):
- A notice taking a turn (`PendingWake::turn_due` without its `run` test): `tests_wake_owner::a_turn_that_takes_its_wakes_leaves_a_notice_alone` fails (the turn took two wakes, not one).
- Home ignored (the post's target without the DM fallback): `tests_reminders::a_reminder_in_a_placeless_session_posts_to_the_owners_dm_when_it_fires` fails ("no the reminder's post in 10 s").
- Snooze writing two frames (the record and its row committed apart): `tests_wake_owner::a_snooze_is_one_frame_and_sets_a_fired_reminder_again` fails (2 frames, not 1).

**FAST.** One frame per fire; no admission slot; no new timer (the harness's 500 ms tick calls one term query). A plain
turn is unchanged (`a_plain_turn_stays_within_its_frame_budget` passes). `theseus-sim bench lifecycle --runs 10 --check`
on this VM (debug, 4 cores, not the owner's machine): every phase within its budget except **`health, after the seed`
(p95 6.5 to 7.4 ms against 5 ms)**, which misses identically **at the base commit** (three runs there, p95 6.7 to 8.0 ms;
three here, 6.5, 7.4, 6.9): not this branch's. Starts: cold 21.8 ms p50 (budget 50), SIGKILL restart 27.7, swap 27.4,
clean shutdown 11.8 (budget 100). One run also missed `execution.cancel` once (p95 339 ms from a single sample; the
other two runs 16 ms). `theseus-sim kernel-sim --seeds 30 --p-race 0`: all invariants held (it does not create
reminders: not extended, see below).

## The live check on this VM (done)

A scratch daemon (`/tmp/live`, fresh state dir, fake `op`, no Discord, no model: its api base is a closed port):

```
10:11:28  theseus --socket S remind "in 1m" "stretch"      # no session: Home
reminder f70ac5 set for 2026-10-12 10:12:28 +00:00 (in your Home conversation) …
theseus --socket S sessions   →  … Reminders   (the made-on-first-use conversation)
10:12:28  theseus watch <that session> printed:
  ⏰ stretch
      Done: theseus done wak_01a1…0ac5 · Snooze 10 min: theseus snooze wak_01a1…0ac5 "10m" · Snooze 1 h: … · Tomorrow 9:00: …
theseus ledger -k wake.fired   → one row, late_ms 414, notice true, purpose remind, while_down false
theseus ledger -k provider.call → 0 rows (no model call)
10:12:42  theseus snooze wak_… 1m      →  fires 10:13:42
10:12:42  theseus shutdown; daemon down until 10:13:48 (past the due time); started again
10:13:52  wake.fired again, late_ms 10059, while_down true; no pending wakes; still 0 provider.call rows
```

## Live checks for the maintainer (the owner's keys)

Discord rig (a scratch daemon with its own bindings and a test bot, never the operator's):
1. In the DM: `/remind when: in 1m what: stretch` (the `when` box suggests "in 10m … fri 9:00"). Expect, about a minute
   later, one DM message `⏰ stretch` that pings, with **Done · Snooze 10 min · Snooze 1 h · Tomorrow 9:00**, and no model
   call in `theseus ledger -k provider.call` since the command.
2. Press **Snooze 10 min**: the message edits to `⏰ stretch` plus `-# 😴 Snooze 10 min: again at <time>`, its buttons go,
   `theseus wakes` shows `remind: stretch` due in ten minutes, and at that time the DM speaks again with four buttons.
   Press **Done** on the second: a `wake.done` row (`theseus ledger -k wake.done`), nothing pending.
3. From a placeless CLI session: `theseus remind "in 1m" "stretch"` reaches the DM too (Home is the DM's session);
   `theseus snooze wak_… 1m` (the id the CLI's `watch` prints) fires it again.
4. `theseus pause <id>` on a daily series, `theseus wakes` shows `once (paused)` / `every 1d … (paused)`; `theseus resume <id>`.

## Gate

`scripts/gate.sh` stops in its **keel** phase here, on six findings that are **not from this branch** (they are in the
range `merge-base(HEAD, origin/main)..HEAD`, which in this clone reaches back through main's own history): the TUI's
`a_finished_task_is_done_counted_and_rung_once` (7 to 6 assertions), `crates/theseus-index/src/vectors.rs` and
`crates/theseus-protocol/src/tests_work_join.rs` (`allow-added`), the template's `daily_spend_ceiling_mode` line, and two
`long-files.txt` ceilings; the guard also refuses six old acks whose signatures do not verify here. **Keel findings
expected from this task: none** (no test deleted, no assertion lowered, no `#[allow]` or `#[expect]` added, no ceiling or
cap raised; the long-file ceilings were not touched). `python3 scripts/keel-guard.py` output is the six lines above.

I then ran the later phases by hand on the final tree (`/tmp/gate4.sh`): `cargo fmt --check` ok, `scripts/shape.sh` ok,
`cargo clippy --workspace --all-targets -- -D warnings` clean (two functions had to be split: `wake::set`, and the owner's
verbs go through `rpc_prefixed` so `dispatch` stays at its limits), the cockpit's `npm run lint` / `test` / `build` ok,
protocol types committed (`cockpit/src/protocol.gen` clean), `cargo deny --offline check` ok, and the suite: **4253 run, 4220
passed, 33 failed, all 33 the known L1 ones** (`theseus-sandbox::contract` ×19, `::bench spawn_100`, and theseusd's
`sandbox` tests: the root daemon without a job cgroup, theseus-pv6i). Not run: the gate's lifecycle/jobs/turn benches as a
phase (run by hand above) and the keel tests (`scripts/test_keel_guard.py`, unchanged).

Failures seen on earlier runs of the suite and explained:
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: failed twice in the
  suite, and **fails the same way at the base commit** with my changes stashed (21 s timeout on `outbox.status().pending`);
  it passed in the last full run: a load-dependent test of the base, not this branch's.
- `theseus-core tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory`: once in a loaded
  suite run, passes alone and in the last full run.
- `theseusd::versions` (two) and `tests_route_keep` / `tests_tiering` (three): my change (formats; the tools block's
  300 tokens): fixed in `3b6d193d`.

## What is left, and what is uncertain

- **kernel-sim** does not create reminders or run `pause`/`resume`/`snooze`/`fire_notices` (AGENTS.md: "a new transition
  belongs in its random operations"). Its invariants hold with the new records; the transitions are held by their own
  tests. A cancel in the sim draws from `pending_wakes()`, which would now include notices; none is created.
- **Home's transcript** does not show the reminder (above). The cockpit shows nothing new but the generated types
  (`WakeNoticed`, `WakeAnswer`, the verbs' params): its rendering of `wake.noticed` is a cockpit row.
- **`work.answer`** will route the four answers; until then they are the calls named above. `Woke.purpose` is filled by
  nobody yet (the work board's row); `reminder_notice` is what the surfaces that hear `wake.noticed` use (the TUI does).
- **Where docs should change** (I did not touch them): the spec's Part III item for this step (a new item: the record
  change, store format 30, the firing, Home, the verbs, the limits); `docs/status.md` (the recently-landed step, the roadmap
  row `reminders`); `docs/design/` (horizon HZ2's "reminders" row: done; routines and watches now bump no format);
  the spec's chapter on wakes (§3.3, §3.15: the 365-day horizon, the purposes and runs).
- **The format number**: 30 on this branch; standing, fork-a, tamper-ledger may also bump: renumber at the join, and the
  `tests_layouts` / `store.rs` / `versions.rs` asserts (30, 31, "2 to 30") move with it.
