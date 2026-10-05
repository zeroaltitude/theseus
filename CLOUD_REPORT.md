# Cloud report: claim leases, the board, `/tasks`, the layer-1 card, and the cockpit's task graph (39b, theseus-ext.14)

Branch `cloud/20261005-task-board`, built on `main` at 3085f71 (format 16, after the tasks smalls). Started 03:01 UTC,
report written 04:56 UTC (just under two hours in). Every commit's subject carries `theseus-ext.14`; none is signed (the maintainer's merge is).

| Commit | Step |
|---|---|
| 95bbf69 | 1. claim leases, `task.claim`, the due pass; store format 17 |
| 3369904 | 2 and 3. the board (one message a place, edited in place, pinned) and `/tasks` |
| 75253bf | 4. the layer-1 card names the change, before and after |
| ae51f2e | the fake model's tool_use ids are new each turn (found by the live check) |
| 1155d26 | 5. the cockpit's task graph |
| e241d76 | the board hears every session's `task.changed`; the cockpit's push carries it |

Steps 2 and 3 are one commit: `/tasks` and the board share their renderer and runtime module (`render/board.rs`,
`runtime/board.rs`), and splitting them cost a gate for no reviewable difference. The sim fix and the cockpit commit
were proved by one gate run over both (the cockpit's files and the fake model are independent).

## Step 1: claim leases (95bbf69)

**Found.** 39a left `claim` out of `TaskRecord`; the due pass (the driver's tick, `harness::drive`) already expires
questions from an atomic due time, which is the shape a lease wants. The kernel's clock is injectable but the core always
builds it with `RealClock`, so the "virtual clock" here is the due pass taking its `now` as an argument
(`Core::free_expired_leases(now_ms)`), as `expire_questions(now)` does; the driver passes `kernel.now_ms()`.

**Changed.**
- `TaskRecord.claim: Option<TaskClaim { by: exe_…, session, until_ms }>` (protocol `tasks.rs`; I added `session` beside
  the design's `by` so the refusal can name the session without a read). `TaskRecord::claim_at(now)` reads a claim past
  its `until_ms` as free; `task_graph::shown` clears it for every surface.
- `task.claim { id, version }` (`task_graph/lease.rs`), registered with the harness tools. A claim is a layer-2 edit: it
  names the version it read and moves it by one. The holder claiming again renews (no version move, `renewed: true` on
  its row); the holder's `task.update` and `task.split` renew in the frame they write; `task.close` ends it, and so does
  a task session's report closing its record. A held task refuses another execution's claim **before** the version is
  compared, whatever version it names: `blocked: claimed by session <short> until <HH:MM>. Task … is another
  session's for now: …` (the call's error result; no `task.stale_refused`, no row).
- `[kernel] task_lease_minutes` (default 30, at least 1: `KernelSection::task_lease_ms`), in the template with its
  `[policy.tools]` line for `task.claim`.
- The due pass: `ToolRuntime::leases` (`Leases`) keeps the claims that hold in memory, task id → until. It is built
  once after serving (`Core::warm_leases` in theseusd's after-serving list, or by the driver's first tick), with its
  lock held across the read so a claim that lands meanwhile is noted after, never lost; each edit's changes are noted
  after its frame (`harness_done`). `Core::free_expired_leases_if_due` runs on the driver's tick beside the questions'
  expiry; a lapsed claim is freed under the task's lock: the record without its claim and a `task.lease_expired` row
  in one frame, the version kept (a lease's end is not an edit), then `task.changed` (`lease_expired`).
- Facts: `task.claimed` and `task.lease_expired` (`LedgerKind`, `fact/task_graph.rs`, `FACTS`), their narrative lines.
- Surfaces: the view's line (`, claimed by session d4e5f6 until 14:05`), `theseus tasks` (`· claimed by session
  d4e5f6, in 29m` while it holds), `task.changed`. Tests' tables: `tests_explain`'s input for `task.claim`, and
  `tests_places`' shared catalog (`task_claim`, a public tool as every `task.*` is).
- Store format 16 → 17, with `TASK_BEFORE_CLAIM` (a format-16 plan item, `by_model: true`) in `tests_layouts`; the
  format tests in `store.rs` and theseusd's `tests/versions.rs` moved with it.

**Proved.**
- `tests_task_claims.rs` (4 tests): two executions claim one task (one `task.claimed`, the other `blocked: claimed by
  session … until HH:MM` for both v1 and v2, record unchanged, no stale row); a renewal keeps the version, the
  holder's edit renews, another session's edit applies and leaves the claim, a close ends it and empties the kept map;
  a lapsed lease is freed by the due pass at `until` and not at `until - 1`, in exactly one frame, version kept, a
  second pass frees nothing, and another session can then claim it; `claim_at` and the config's minimum.
- Under load (AGENTS.md's recipe: the tests at nice 19 beside four busy loops at nice 0, killed by pid): 5 runs of
  `tests_task_claims`, 4/4 passed each time (about 18 s a run under the load).
- Planted reverts: a due pass that frees nothing (`due` emptied in `free_expired_leases`) fails
  `a_lapsed_lease_is_freed_by_the_due_pass_at_its_until_not_before` (line 171, `left: 0, right: 1`); a second claimer
  let through (the holder check made always false) fails `two_executions_claim_one_task_and_the_second_is_blocked`
  (line 63, the result was `Ok`). Each file restored and touched; `git status` clean of them.
- The gate (below), the turn bench (`frames_plain: 5 … ok`, `frames_tool: 9 … ok`), and the format tests.

**Should a claim hold back other sessions' edits?** I left it as the brief says: it does not, CAS guards them, and the
test shows another session's rename applying while the claim stays the holder's. My recommendation is to keep it so
for the plan (title, deps, split) and consider refusing only `task.close` by a non-holder while a claim holds: closing
someone else's claimed task is the one edit that silently ends their work. That is a one-line check in `close` beside
the holder check, if the owner wants it.

## Steps 2 and 3: the board and `/tasks` (3369904, and e241d76)

**Found.** `task.changed` reached only the watchers of the session whose call made it, and the binding watches only its
places' sessions; the router sends each notification to the place of the session it names. So a change to a DM's task
made by a CLI session, a task session's own plan item, or a lease that lapsed under another session's claim never
reached the DM. The cockpit (which hears every session through `executions.watch`) never got `task.changed` at all.

**Changed.**
- `task_graph::home` (core): a task's home is its root's origin session, or, when that is a task session (which no
  place routes), the task session's own record's home, so a task session's plan items show on its parent's board.
- `runtime/board.rs`: the router takes every `TaskChanged`, finds its home, and sends `PlaceMsg::Board` to that place
  alone; the place reads its tasks as they read now (`all_shown`) and sends the tree to its lane as one live upsert
  under `render::BOARD_KEY` (`render/board.rs`: id tail, title, state, owner, claim while it holds; 25 lines at most).
  It is made at the place's first change, coalesced, never replayed.
- `courier/board.rs`: the lane writes it like any upsert (no reply anchor), pins it once made (a refusal logged once a
  lane, at info), and after a restart, at the first board write, reads the channel's pins for the bot's message that
  begins with `📋 **Task board**` and edits it ("the task board: found the pinned one, edited in place"), or makes a new
  one ("the task board: made a new one"). Never on the start path.
- `/tasks`: the place's records as a tree (states, owners, claims, versions), then the task sessions of today
  (`render::tasks` over those created on today's local date), or "No task sessions here today."
- e241d76: `task.changed` joins the wide notifications (`bus::wide`), and the binding calls `executions.watch` once as it
  starts (in a task, after its places are bound) so its router hears every change. Its snapshot is not read; each
  connection still gets a notification once; the router still hands every other notification only to the place whose
  session it names. This is a change to what the binding subscribes to: say if the owner would rather the core
  publish a task change to its home's session as well, which avoids the binding hearing every execution.changed.
- Live, on a second rig with e241d76's build: a CLI session's claim of the DM's task showed on the DM's board
  (`🔒 session … until …`), and its lapse a minute later cleared it (`task.lease_expired`, a third version of the
  board), both from a session the binding does not watch.
- The fake Discord: `PUT /channels/{c}/messages/pins/{m}` (or 403 with `refuse_pins(true)`) and `GET
  /channels/{c}/messages/pins`; `Msg.pinned`.
- Discord's runtime.rs, render.rs, and courier.rs gain only `mod` lines, a variant, fields, and calls.

**Proved.** `tests_outbox`: the board is one message in the DM, made at the first plan item and edited at the second,
its last state the whole tree, and pinned; a refused pin leaves it made, edited, and unpinned (a `PUT … refused` seen);
a lane after a restart (nonce window closed, so only the pins can find it) edits the pinned board and creates
nothing; a change published for a session the binding does not watch reaches the board of its task's home.
`render::board::tests` (the board's and `/tasks`' text, a claim shown while it holds and gone at `until`, another
place's task left out); `task_graph::tests::a_tasks_home_is_its_roots_origin_or_a_task_sessions_parents`.
Planted reverts: a new board key at each change fails `the_board_is_one_message_edited_in_place_and_pinned` (timed out:
the board's second task never reached the first message); a lane that never looks at the pins fails
`a_lane_after_a_restart_finds_the_pinned_board` (timed out: the board edited); `task.changed` out of the wide
notifications fails `a_change_from_an_unwatched_session_reaches_its_homes_board` (timed out).

## Step 4: the layer-1 card (75253bf)

**Changed.** `ConfirmRequest.change: Option<TaskChange { task, title, field, before, after }>` (the type in
`tasks.rs`; the protocol's lib.rs gains only the field, its ceiling 2705 → 2709). `task_graph::tools::change_of`
builds it from the record (before) and the call's input (after) wherever a question is built (the gate's `ask`,
`confirm.list`), so `confirm.requested`, `confirm.list`, and the card carry it. `TaskChange::question` words it once:
"Change the acceptance of tsk_… (title)? Before: … After: …" (acceptance lines joined by "; ", an empty side
"(none)"), and "Abandon tsk_… (title)? Before: accepted After: abandoned". Discord's card (`render::board::change_card`)
opens with 📝 and that question, says what Accept and Decline do and when it expires, and carries Accept and Decline
(`Buttons::Accept`, `runtime::accept_buttons`) on `confirm:approve:<id>` and `confirm:decline:<id>`, so a press
parses and is judged as any card's. Discord's render.rs gains the variant and the call (ceiling 2930 → 2935). `theseus
confirm` and the CLI's live line print the question, then `accept: theseus confirm <id>` and the decline.

**Proved.** `tasks::tests` (the question's words), `tests_task_layers` (a waiting owner's change carries it, exact
text), `render::board::tests::the_layer_one_card_says_the_change_before_and_after`,
`runtime::board::tests::the_layer_one_cards_ids_parse` (Accept parses as approve, Decline as decline, of the card's
question), `render::task_graph::tests::a_layer_one_question_says_the_change` (CLI). Planted revert: an Accept button
with its own verb fails the ids test (`parse_confirm_id` returned None).

## The fake model's ids (ae51f2e)

The live check found that an accepted layer-1 change never applied on the rig: `theseus-sim fake-model` gave every
turn's calls the ids `toolu_fake_0…`, so a session's later call shared its id with an earlier answered one, and the
continuation after the approval read it as answered (`nothing_new`). Each id now carries the time and a count. With it,
the accept applied (`task.change_accepted`, v1 → v2). This is a fake's defect, not the core's; the core's own test of
the accept (`tests_task_graph`) always passed.

## Step 5: the cockpit's task graph (1155d26)

**Changed.** `components/TaskGraph.tsx` and its pure `lib/taskgraph.ts`: in Actions, a "Task graph" panel with
`task.list`'s records as a tree (state, title, claim while it holds, version, id tail, owner, deps, evidence count, the
owner's tasks marked, a waiting change in the card's words and the ConfirmCard of its question), and "open as a graph":
the same records in React Flow (each task a node at its depth, parent → child edges, dashed edges to what it waits on),
its state in the address (`?taskgraph=1`, `&task=<id>` for the one picked, whose objective and acceptance show below).
It shows the present: under the time machine it says so and the card's acts are off. `bindPush` reads the pushed reads
again on `task.changed`. A layer-1 ConfirmCard shows `changeQuestion` (the protocol's words) and calls its approve
Accept. I put the graph under Actions rather than a route of its own, so `main.tsx`'s routes are untouched (42b adds to
them).

**Proved.** `npm run lint` (0 errors; the warnings are main's), `npm test` (63 pass, 4 of them new in
`test/taskgraph.test.ts`), `npm run build`. A scratch daemon of this build (the rig below, web on 7533) served
`/actions` and `/actions?taskgraph=1` to headless Chromium: the panel read "Task graph · 2 open of 2", two graph nodes
drawn, no console or page error.

## The live check I ran, and the maintainer's

I ran the brief's four steps on a scratch rig here (fresh state dir, nothing else running) with this build; each
showed what it should, after the fake model's fix. The maintainer's commands, on a build of this branch
(`target/debug` or an install):

```bash
R=/tmp/rig39b; rm -rf $R
theseus-sim discord rig --dir $R
# in $R/config.toml: [kernel] task_lease_minutes = 1, heartbeat_secs = 10
cat > $R/rules.json <<'EOF'
[{"when": "plan the reef", "calls": [{"name": "task_create", "input": {"title": "Chart the reef"}}], "text": "Recorded."},
 {"when": "", "text": "Noted."}]
EOF
theseus-sim fake-discord --addr 127.0.0.1:9447 --gateway 127.0.0.1:9449 --guild $R/guild.json --log $R/fake.log &
theseus-sim fake-model --addr 127.0.0.1:9448 --rules $R/rules.json &        # --rules takes a file's path
PATH=$R/bin:$PATH OP_SERVICE_ACCOUNT_TOKEN=proof-not-a-token theseusd --config $R/config.toml --socket $R/sock --state-dir $R/state &
say() { theseus-sim discord say --user 900000000000000101 --name ana "$1"; }   # the rig's DM
```

1. `say "plan the reef"`, then `theseus --socket $R/sock --json tasks` gives the id (`ID`, v1). Add a rule
   `{"when": "claim the reef", "calls": [{"name": "task_claim", "input": {"id": "<ID>", "version": 1}}], "text":
   "Claimed."}` before the catch-all and restart the fake model. `say "claim the reef"`, then `theseus --socket
   $R/sock ask "claim the reef"`: its line reads `← task.claim error … blocked: claimed by session <short> until
   <HH:MM>. Task … is another session's for now…`. `theseus --socket $R/sock tasks` shows `[accepted] v2 · claimed by
   session <short>, in 54s`; `theseus --socket $R/sock ledger -k task.claimed` has one row (`from: 1`, `renewed:
   false`).
2. `theseus-sim discord read --fake 127.0.0.1:9447`: one message beginning `📋 **Task board**` in the DM, `pinned:
   true`, its `versions` the board before and with `🔒 session <short> until <HH:MM>`; `fake.log` has its POST, its
   PATCHes, and the `PUT …/messages/pins/<id>`. `say "/tasks"`: `**Task graph here** · 1 open, 0 closed`, the line with
   the claim and `v2`, then `-# No task sessions here today.`
3. A minute on: `theseus --socket $R/sock ledger -k task.lease_expired` has one row; the board's last version has no
   🔒; `theseus tasks` shows no claim, still v2. (Restart the daemon and change a task: its log says "the task board:
   found the pinned one, edited in place", and the fake has no second board.)
4. A brief task: `say "the reef needs a full chart of every marker today"`, then a rule `{"when": "delegate the
   reef", "calls": [{"name": "task_create", "input": {"title": "Survey the reef", "brief": "Survey the reef: chart
   every marker on its north edge and report the depths.", "arrangement": {"pieces": [{"quote": "the reef needs a full
   chart of every marker today", "role": "objective"}]}, "fidelity_ack": true}}], "text": "Started."}` and, so the
   task stays open, `{"when": "[Task", "calls": [{"name": "wake_at", "input": {"after": "1h", "note": "look again"}}],
   "text": "Waiting."}`; `say "delegate the reef"`, read the task's id (`T`), add `{"when": "change what the survey
   takes", "calls": [{"name": "task_update", "input": {"id": "<T>", "version": 1, "patch": {"acceptance": ["every
   marker on the north edge has a depth", "the chart is signed"]}}}], "text": "Asked."}`, restart the model, `say
   "change what the survey takes"`. The card at the fake reads `📝 Change the acceptance of <T> (Survey the reef)?
   Before: (none) After: every marker on the north edge has a depth; the chart is signed`, buttons Accept and
   Decline; `theseus --socket $R/sock confirm` prints the same question with `accept: theseus confirm <id>`; `theseus
   confirm <id>` applies it (`task.change_accepted`, the record v2 with the new acceptance). Rules reload only with a
   restart of `fake-model`; the fake model before ae51f2e reused tool ids and the accept never ran.
5. The cockpit: `[web] enabled = true` on a free port; `/actions` shows the Task graph panel; `?taskgraph=1` the graph.

## What is left, or uncertain

- `until HH:MM` is the minute the lease lapses in, truncated: a claim until 04:25:59 reads "until 04:25".
- After `/new` in a place, the board shows the new session's tasks; the old session's tasks have no routed home and
  their later changes draw nothing. The lane keeps one board key per place.
- Buttons on the board stay filed (§2.4).
- The board and `/tasks` read every task record per change (O(N)), as the view does each loop (39a's choice).
- Docs the maintainer should write: Part III's 39b item; `docs/status.md` (the step, store format 17); M7 §2.4's
  claim (it now carries `session` beside `by`; a claim moves the version, a renewal and a lease's end do not; the
  refusal comes before the version compare) and its "In chat" (the home rule; the restart's pin lookup; Accept and
  Decline on Approve's and Decline's ids); `docs/technical-overview.md` if it lists the ledger's task rows.
- Store format: 17 here; renumber at the merge after the other format bumps.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0`, see below) on each commit's
tree: fmt, shape, features, clippy, the cockpit (lint, test, build), the test build, and the reader rule pass; the suite
fails only on the 33 known L1 tests of a root daemon with no job cgroup (theseus-pv6i): theseus-sandbox's contract
clauses and `a_job_that_cannot_start_says_why`, its bench's `spawn_100`, and theseusd's `sandbox` tests (2,496 tests;
the last run: 2,501 run, 2,468 passed, 33 failed, 17 skipped, and `theseus-sim`'s `the_kernel_holds_its_invariants_under_seeded_faults`, on the flaky list, passed on its third try). Then by hand: the protocol types check (the generated files committed), `theseus-sim bench turn --check
--runs 5 --burst 0` (`frames_plain: 5`, `frames_tool: 9`, both ok), and `cargo deny --offline check` (advisories,
bans, licenses, sources ok). Neither `tests_pages` nor the python REPL test failed in any run.

The VM's disk filled during the first gate (`target/debug/incremental` reached 17 GB of a 39 GB allowance): I removed
that directory (inside the repository) and ran every build after with `CARGO_INCREMENTAL=0`.
