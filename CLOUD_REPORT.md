# Cloud report: projects (theseus-n5ui2), branch cloud/20261013-projects

## What I found
`routines` was on main (store format 34). `owner_ask` is not on main, and neither is `ProjectView` / `WorkView.project` (horizon-board): **the horizon-board joiner fills `WorkView.project` from these records** (`project::progress`, `project::info`).

## What I changed (commits a9fe5cdb, 10a22931, and the clippy/alias commit after it)
- **Record + format 35**: `TaskRecord.kind` (item default), `due_at_ms`, `review`; `SessionRecord.project`. Layout samples `TASK_BEFORE_PROJECTS`, `SESSION_BEFORE_PROJECT` in `tests_layouts.rs`; version tests moved to 35 (and 36 for the "newer store" case). Renumber at the join if another row bumped.
- **Create**: `task_create { kind: project|milestone, due, milestones:[{title,due}], review }`; project + milestones written in one frame. A project with a review waits on one card (the review is a routine, spends money). `project.create` (owner, `Act::Routine`), `project.list|show|attach`; the model's `project_attach`; `task_update` patch takes `due`.
- **Slice** (`project/slice.rs`): goal, open milestones with dates, next steps, ≤300 tokens, last block after commitments, from memory every loop; `context.compiled.project` + a line in `theseus why`.
- **Reminders** (`project/remind.rs`): two notice wakes per dated open milestone (`⚑ due today: …`, `⚑ overdue: …` after `[projects] overdue_after_minutes`=1440); moved with the date, cancelled on close/project close. `notices.rs`: `Why::Overdue`, Interrupt with no sound, milestone lines without the clock icon.
- **Weekly review** (`project/review.rs`): routine "Review: <title>", default `mon 08:00`; brief says `owner_ask` is absent, so decisions go in the report under "Decisions for you". Run log = `theseus routine runs <review>`.
- **CLI**: `theseus projects` (alias of `project`), `project show|new|attach|detach`; bar `▰▰▰▱▱▱▱ 3/7`.
- `[projects]` config + template; tool `project.attach` in the template (`policy.tools` 58).

## Proof
- `cargo nextest run --workspace`: 4841 tests, 4807+ pass. Failures: the ~33 known L1/sandbox tests (root VM) and timing tests that passed alone (`tests_work::the_substrate_scenario…`, `herdr … ten_ms`, `markdown rendering_costs_little`, `judge builders …`, `push … a_fresh_seed…`).
- New: 10 tests in `tests_projects.rs` (create+reminders, moved date/close, a date firing with no model call, overdue notice without sound, review run in its log, layer 1 guard, model's card yes/no, no-review project, attach, 3 compactions with the slice in every later request and in `context.explain`), 5 in `project/tests.rs` (slice bench: see below), CLI 3, layout 1, protocol 1.
- Slice bench: median reported by `project::tests::the_slice_is_built_from_memory_in_microseconds` (`--no-capture`); bound 50 µs release / 1 ms debug.
- Planted reverts: slice attach removed in `compile_step.rs` → `an_attached_conversation_keeps_the_slice_through_three_compactions` fails; the snooze of a moved date removed in `remind.rs` → `a_moved_date_moves_its_reminder_and_a_close_cancels_it` fails. Both restored and touched.
- Live check on this VM (`scripts/projects-live.py`, stand-in model, window 100k): project made with three milestones 1/2/3 min out and `--review "every 2m"`; session attached and 10 turns → 4 compactions (requests with no tools), **every request after attach carried the slice** (dump), `theseus why` printed `project … slice … 5 lines · 134 tok`; the review ran (`#1 complete`) and landed in `routine runs`; the three `⚑ due today` notes and wakes are in the WAL (counted by string; I did not watch the notice on a surface).
- Plain turn frames: slice/after hooks return before reading anything when no project/long kind is involved; I did not run `theseus-sim bench turn` (debug disk constraints).
- Gate: ran fmt, shape, clippy -D warnings, cockpit lint/test/build, the full suite; I did **not** run `scripts/gate.sh` itself (disk allowance on this VM ran out twice: use CARGO_INCREMENTAL=0).

## Keel findings expected
- `keel-guard.py` reports one finding, `crates/theseus-sim/src/lifecycle.rs:155 budget-raised` — not from this branch (the base is older than origin/main); none of mine.
- Window constants in tests_tiering (42_500), tests_route_keep (43_500), tests_compaction (48_400) were raised, with comments, for the new tool text (as earlier rows did); not budgets, but the maintainer may want to look.

## Left / uncertain
- Reminders from the model's tool path are set in the calling conversation, not Home (no `Core` there); the owner's verbs use Home.
- The goal-vs-dates ownership: `due` edits are not layer 1 even on an owner's milestone.
- Orphan risk: the review routine is made before the project frame; a crash between them leaves a routine without a project.
- Docs to update at review: spec Part III item, status.md, `docs/technical-overview.md`.
