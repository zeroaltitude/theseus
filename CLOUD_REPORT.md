# Cloud report: the task record, step 39a (theseus-ext.6)

Branch `cloud/20261004-task-record`, from `main` at af6790d. Started 11:20 UTC; commits f79dd15 (the step) and a docs
commit (theseus-core's AGENTS.md entry), then this report.

## What I found

- A task was only a session (DD7): `task.create { brief }` opened a child execution, `task.list` read executions, and
  nothing held a title past the brief's first line, a plan, acceptance, or evidence. 27's arrangement (`arrangement.rs`)
  sits in `task.create`; the harness runs `task.create` and `wake.at` in `toolrun::run_harness`, whose completion frame
  can carry extra records.
- Main's store format was 9. `kinds::SCHEMAS` is gone, as the brief said; the next free kind number was 12.
- The gate's floor (`Decision` at `Approve` before the posture) is the cleanest way for a call to "always wait", and
  `confirm_action` is the one place an answer becomes a decision (`judge_act`: the owner, from a private place).
- `turn.rs` was at 3,478 of its 3,500 ceiling; it is 3,489 now (the close-by-report and the view: four call sites).
- Test stand-in models in several suites match keywords in the request's last user text. The view, appended there,
  carries task titles, and broke four existing tests that way (a task title matched a script's keyword). I made each
  stand-in skip the view's block (`task_graph::view::is_view` / `HEAD`); see "Uncertain" for whether the view should
  live there.

## What I changed (f79dd15)

1. **The record.** `theseus_protocol::tasks` (new module): `TaskRecord { id, version, title, objective, acceptance,
   state, parent, deps, owner, session, origin { session, principal }, evidence [{ node, identity, note, by, at_ms }],
   proposal { objective?, acceptance?, abandon, by, card, base_version, at_ms } , created/updated_at_ms }`, `TaskState`
   (§3.5's nine), `TaskGetParams/Result`, `TaskChanged`, `TaskViewSummary`. It is stored as shown (one definition).
   Store kind `TASK = 12`; `MANIFEST_FORMAT` 9 → 10 (`theseusd/tests/versions.rs` and the store's own test follow).
   `tests_layouts` reads the kind; there is no older TASK layout on disk anywhere, and no existing record gained a
   field, so there is no new old-layout sample: the format-9 store test below stands for "a store written before".
   Ids: `tsk_<tail>` from the creating call's correlation id; a task session's record shares its session's tail
   (`task_graph::of_session`), so no session record field was needed.
2. **Lock and CAS.** `Store::lock_task` (the session-lock map's type, a second map). Order: session, task, execution.
   Every edit compares the `version` it names; stale → refused with the record as it is now
   (`"task tsk_x changed since you read it: v1 → v2. It is now: …"`), and a `task.stale_refused` row.
3. **Tools** (`task_graph/tools.rs`): `task.update { id, version, patch }`, `task.split { id, version, into }`,
   `task.close { id, version, outcome: done|abandoned, evidence }`, and `task.create` without `brief` (a plan item:
   `title`, `objective?`, `acceptance?`, `parent?`, `deps?`). Edits run in the harness; their records and rows ride in the
   frame that settles the call (so they are `SafeToRepeat`). `task.create` with `brief` keeps 27's rules and writes
   the record in `open_task`'s frame (state `in_progress`, objective/acceptance from the arrangement's standing
   `objective`/`acceptance` pieces, else the call's fields, else the title). `task.create`'s Input, schema and
   description changes are additive except that `required: ["brief", "arrangement"]` is gone (a plan item has
   neither); a brief without an arrangement is still refused in code, as before.
4. **Layer 1** — choice: **the gate's path.** `Plan` gained `authority: Option<String>` (`serde(skip)`, as
   `AwsPlan::guardrail`); `policy::decide_with` asks at every posture when it is set, right after the floor. A patch with
   `objective`/`acceptance`, and `close` with `abandoned`, set it. When the call asks, its proposal is written on the
   record in the asking frame (`plan_call`, under the task's lock; the version does not move), with a
   `task.change_proposed` row. The owner's yes (`theseus confirm`, the cockpit's card; refused from a shared place, and
   by the CLI inside a job) runs the call, which applies it in one frame (`task.change_accepted`, version +1, proposal
   cleared); a re-run after a restart takes `approve` only when its confirm is bound. A no clears the proposal in the
   answer's frame (`confirms.rs`, `task.change_declined`). Abandoning writes `change_accepted` then `closed`.
   Plan items need no arrangement (default taken); one given is resolved for objective and acceptance (no fidelity
   check, since nothing is delegated).
5. **A session's task** — the brief's default: running states are derived when read (`state_now`: queued/running →
   `in_progress`, waiting on a confirm → `waiting_human`, budget question / cancel / budget exhausted → `suspended`);
   the record is written at its own changes; the report closes it `done` with `report:<session>` evidence pointing at
   the report's node, a failure `failed` with the reason, in the frame that ends the task (`tools::Closing` in
   `turn.rs`, under the task lock). A cancel writes nothing (reads `suspended`).
6. **The view** (`task_graph/view.rs`): scope per §2.4 (a conversation: tasks it started and their subtrees; a task
   session: its parent's line and its subtree). A line per open task (id, title, state, owner, deps, first acceptance
   line, version), a closed subtree folded to one line with its count, about 1,500 tokens (3 bytes/token estimate);
   past it, open tasks only, then as many as fit, with "(N more left out …)". It is appended as the last block of the
   request's last message after `compile()` (which stays pure), and the conversation's cache marker is copied onto the
   block before it so the next request still shares a cached prefix. Its tokens join the estimate; `context.compiled`
   gets `tasks: { digest (sha256), open, closed, lines, left_out, tokens }`. No task in scope → nothing changes.
7. **Surfaces.** `task.list` keeps every field and adds `records` (all, or the session's scope); `task.get` (new
   method); `task.changed` (new notification, with the record and verb); `theseus tasks` prints the tree after the task
   sessions (`render/task_graph.rs`); ledger rows `task.created/updated/split/closed/change_proposed/change_accepted/
   change_declined/stale_refused`; a narrative line per change ("task tsk_x split into 2 (v1 → v2)."); metrics
   `theseus.tasks.changes{theseus.task.verb}` (task tool calls that ended ok, from the turn's trace) and a
   `theseus.tasks.open` gauge (exported as a non-monotonic sum; the last compile's open count in a turn). Template:
   commented `task.update/split/close` lines under `[policy.tools]`. Cockpit types regenerated.

## How I proved it

- New tests, all passing: `task_graph::tests` (6: CAS compare, scope, state from execution, view folding, view bound
  with its count, view placement/breakpoint) and `tests_task_graph` (6):
  `a_plan_splits_closes_with_evidence_and_refuses_a_stale_edit` (three plan items, a split, a stale edit refused with
  the record as it is now, a close with `commit:…` evidence, a second close refused, every stored version's evidence a
  prefix of the next, the view with versions in the next request, `context.compiled.tasks`, `task.list`/`task.get`);
  `a_plain_turn_carries_no_view`; `a_layer_one_change_waits_and_accept_applies_it_and_decline_leaves_it` (posture set
  `open` and it still asks; a guild channel's answer refused, proposal kept; the CLI's yes applies it, v2, and the next
  view shows it; a no leaves it; abandoning asks); `a_task_sessions_record_is_closed_by_its_report` (27's refusal for a
  brief without arrangement still holds and writes no record; the record closes `done` with the report node);
  `a_failure_closes_a_task_failed_and_a_cancel_suspends_it`; `records_survive_a_reopen_and_older_task_sessions_still_list`
  (the WAL replayed into a fresh store gives the same graph; the same records minus TASK under a format-9 manifest
  open, the task session lists from its execution, `records` is empty, and its turn sees no view).
- 27's refusal cases: `tests_arrangement` (all pass) and the new brief-without-arrangement check above. "A job's
  process can't accept": the core refusal is tested (shared place); the CLI's `THESEUS_SESSION` refusal is the existing
  `client.rs` test, which covers `theseus confirm` for any call.
- The frame budget: `tests_m3::a_plain_turn_stays_within_its_frame_budget` passes unchanged; `theseus-sim bench turn
  --check --runs 5 --burst 0`: plain 5 frames (budget 5), tool-call 9 (budget 9).
- Planted reverts (each restored, `touch`ed, `git status` clean):
  1. `check` accepting any version: `a_stale_version_is_refused_with_the_task_as_it_is_now` and
     `a_plan_splits_closes_with_evidence_and_refuses_a_stale_edit` FAIL.
  2. `task.update` applying an objective/acceptance patch at once (no authority, no approval check):
     `a_layer_one_change_waits_and_accept_applies_it_and_decline_leaves_it` FAILS (nothing asked: `left: 0`).
- Under load (4 busy loops at nice 0, the tests at nice 19), `package(theseus-core) and test(task_graph)` (12 tests:
  the CAS, layer and record tests) 5 runs: 12/12 passed each time.
- Existing tests I changed, each for a change I meant: the shared/task catalogs in `tests_places` (three new public
  `task.*` tools), the template's tool count (35 → 38), the store's format test (9 → 10), the output golden (the
  `task.created` row, `task.changed`, the result's record line, `context.compiled.tasks`; digests masked as sha256), and
  four stand-in readers that now skip the view block (`tests_external`, `tests_places`, `tests_arrangement`, theseusd's
  `tests/common/model.rs`, and `theseus-sim`'s `fake_model::last_user_text`).

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, test build, reader rule all pass;
the suite fails only on the root-only sandbox tests (theseus-pv6i): 19 `theseus-sandbox::contract`, 1
`theseus-sandbox::bench`, 13 `theseusd::sandbox`. I ran the phases after it myself: protocol types clean (after
`git add`), the turn bench (5 / 9 frames, ok), `cargo deny --offline check` (advisories, bans, licences, sources ok).

## The live check (the maintainer's)

A scratch daemon, fresh state, a GLM profile, a scratch git repo as the workspace:

```bash
S=$(mktemp -d); mkdir -p $S/work && git -C $S/work init -q && git -C $S/work commit -q --allow-empty -m init
theseusd example-config > $S/theseus.toml   # set state_dir=$S/state, [tools] projects_dir=$S/work, the GLM profile live
theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state > $S/log 2>&1 &
T="theseus --socket $S/sock"
$T profile use glm
SID=$($T ask --json "Three small changes: fix the README typo, add a CHANGELOG, and tidy .gitignore (split the
CHANGELOG work into a draft and a review)." | jq -r .session_id)
$T ask -s $SID "plan these as three tasks, and split the second into two"
$T tasks -s $SID          # the tree: 3 roots, the second at v2 with 2 children, all [accepted], versions shown
```
Expect `task.created` ×5 and `task.split` ×1 in `$T ledger -k task.split`.

```bash
echo typo-fixed >> $S/work/README && git -C $S/work add -A && git -C $S/work commit -qm "fix the typo"
$T ask -s $SID "close the first, with the commit $(git -C $S/work rev-parse --short HEAD) as its evidence"
$T ledger -n 5 -k task.closed    # data.evidence[0].identity carries the commit; the first task is [done] v2
$T ask -s $SID "change the second task's acceptance to: the CHANGELOG lists every change since init"
$T confirm                       # one question: task.update … approve (layer 1: changing task tsk_…'s acceptance …)
$T tasks -s $SID                 # the second task: "a change waits for the operator", still v2
$T confirm <id>                  # accept
$T ask -s $SID "what does the second task need now?"   # its view line: accept: the CHANGELOG lists …, v3
```

The stale edit, on `theseus-sim fake-model --rules` (point the profile's `api_base` at it): after the plan exists,
read the second task's id and version (`$T tasks --json -s $SID | jq '.records'`), then serve rules where one turn moves
it and the next edits it at the old version:

```json
[{"when": "rename it", "calls": [{"name": "task_update", "input": {"id": "tsk_…", "version": 2, "patch": {"title": "Changelog work"}}}]},
 {"when": "rename it again", "calls": [{"name": "task_update", "input": {"id": "tsk_…", "version": 2, "patch": {"title": "Changelog"}}}]}]
```
(`rename it again` holds `rename it`: put it first.) `ask -s $SID "rename it"`, then `"rename it again"`: the second
result is `Refused: task tsk_… changed since you read it: v2 → v3 …`, and `$T ledger -k task.stale_refused` has one row.

## Left, uncertain, and for the owner

- **Where the view sits.** It is appended to the last message (user input or tool results), with the conversation's
  cache marker copied to the block before it. That adds one explicit breakpoint per request in sessions with tasks
  (system blocks ≤ 2, this 1, the automatic 1: within Anthropic's 4). I did not measure GLM's handling of a
  block-level `cache_control`. Keyword stand-ins had to learn to skip it; a model reads it as part of the user's
  turn, introduced by its bracketed header. A per-turn fixed view (computed once a turn) would be quieter; I recompute
  it per loop so a loop after a split sees the split.
- **Layer 1 asks the owner**, not "the requester, else the owner": `judge_act` already restricts answers to the owner
  in a private place, and a non-owner requester has no path to answer. Filed for 39b's card.
- **A whole patch waits** when any field is layer 1 (title + acceptance in one call waits as one).
- **An expired proposal** (the question's TTL) stays on the record until the next accepted change or close; the view
  then says "a change waits" though nothing does. `expire_question` should clear it as decline does.
- **Metrics** count only edits made inside turns (a decline's change in `confirm_action` is not counted); the gauge is
  the last view's open count in scope, not a global count.
- **Reads** of the graph (`all()`) scan every TASK record per loop for the view; fine at today's sizes, an index by
  origin session would be the fix.
- **Docs** for the maintainer: spec Part III item for 39a; §3.5's "as built" note (the default taken for session
  states, the gate path for layer 1, plan items without arrangement); `docs/status.md`; M7 §2.4 (`Observatory` → the
  surfaces as built); the CLI's `theseus tasks` help line in `main.rs` (a shared file: I left it) should say it shows the
  graph too.
- Leases, the board, `/tasks`, the Discord card and the cockpit's graph are 39b's; untouched. The cockpit compiles
  with the new `records` field and does not read it yet.
