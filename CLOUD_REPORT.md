# CLOUD_REPORT: progress (theseus-xlias)

Branch `cloud/20261013-progress`. Commits: 0adc3021 (the feature and its tests), ca5fe7a3 (reader tests).

## What I found
- work-board had already joined: `WorkView.progress` counted plan steps (`push/work/settle.rs`, `roll`), but `said` was always `None`.
- A task session may already create plan items (`task_create` with no `brief` runs before the depth check), so the guidance needed no tool change.
- `theseus status` and the TUI read `executions.watch` (`ExecutionView`), not the work board. I read the work board from them (one `work.watch` snapshot in `status`; `work.watch` + `work.changed` in the TUI) rather than copy step counting into the old push.

## What I changed
1. `say`: `loop.ended` gains `say` (first non-empty line of the loop's text when it made a tool call; scrubbed with the session's scrubber, then cut at 120 chars at a word, `…`). `turn/advance_step.rs` (`say_of`; `advance` moved here from turn.rs, which is at its ceiling: 3524 -> 3467 lines), `fact/turn.rs` (`LoopEnded.say`, in the row only when set). The protocol's wire `LoopEnded` is unchanged.
2. Board: `ExecIn.said` from the newest `say`; `progress/` module `push/work/progress.rs` joins plan count and line (a task with no plan shows its line alone). The count stays the task's own (`owned_by`).
3. Wording: `Progress::line()` in theseus-protocol: `3/5 · Running the tests again with the fix…`. Readers on the same commit: `theseus work` tree (`[1/3 · line]`; was `[1/3 steps]`), `theseus status` long form (`status_progress.rs`), TUI rows (`board.rs`, `app.rs`, `ui.rs`).
4. Guidance, one sentence each, own lines: `task_graph::PLAN_GUIDE` in the task brief (names the task's own id) and `PLAN_NOTE` in the tools note (`toolrun.rs`, private places).
- Store format: **no bump**. `say` is a key in ledger-row JSON, not a stored record's layout; `tests_layouts` is untouched.
- Token cost: `PLAN_NOTE` is about 240 chars, about 60 tokens in the system block (every private-place request); `PLAN_GUIDE` about 50 tokens once per task, in its first message. Route-pack keep budget: `tests_route_keep` passes unchanged (its WINDOW has slack). `tests_tiering`'s window needed +60 (41,100 -> 41,160); two tests failed at 41,100 and pass at 41,160 and above. First request of a fresh default session: 38.8 KB (budget 40 KB).

## How I proved it
- New: `tests_progress` (3: a task with three plan items reads 0/3 then 3/3 with its lines, the conversation above counts none; only a loop with a call writes `say`; the guidance is in the brief and the note), `advance_step::tests` (3: first line, scrub-before-cut with a planted token, 120-char word cut), `status_progress` (1), TUI `tests_progress` (1).
- Planted reverts: `say_of` without the scrub -> `a_say_is_scrubbed_before_it_is_cut` and the scenario fail; `owned_by` dropped from the step count -> the scenario fails (conversation counted the task's steps). Both restored and touched.
- `tests_output` golden rewritten: only the six `loop.ended` rows with tool calls changed (the added `say`). `tests_work`'s golden helper now prints a step count only when there is a plan (said-only progress is timing-dependent; it failed once under full-suite load before that).
- Workspace nextest: 4524 tests, all pass but the known L1 set (theseus-sandbox contract/bench, theseusd sandbox: 33) and the three above, fixed since. `cargo clippy --workspace --all-targets -D warnings`, `fmt --check`, `shape.sh` pass. `theseus-sim bench turn --check` (debug): plain 4 frames, tool-call 8 (both at budget; the brief's 5/9 does not match this tree's budgets 4/8), batch 14.
- Live check on a scratch daemon with a stdlib Python stand-in (scratchpad, not committed; the fake-model rules cannot read the plan items' ids to close them): `theseus work --watch` showed `[0/1]`, `[0/2]`, `[0/3]`, `[0/3 · Laying out the three steps first.]`, `[1/3 · Running the tests again with the fix…]`, `[2/3 · Checking …]`, `[3/3 · Writing the last step down and closing it.]`, then the task `done`.

## Live check for the maintainer (real models; needs a key)
Run the same five-task brief on `glm-5.3-flash` and `claude-sonnet-5-5`, each task of more than two steps, then count:
```
theseus work --watch > work.txt            # in a second shell; keep it open
theseus ask "<the five-task brief>"
grep -c '\[0/' work.txt                    # tasks that laid out a plan
theseus ledger ... | jq 'select(.kind=="loop.ended" and .data.say) ' | wc -l   # loops with a say
```
(any way to count `loop.ended` rows with `say` over loops with `tool_calls > 0` works). The owner decides (D4) whether a `progress` tool is needed if either model keeps few plans or says little.

## Gate
`scripts/gate.sh` stops at **keel**, on a finding not from this branch: `crates/theseus-sim/src/lifecycle.rs:155` `budget-raised "seed" => Some(250.0)`, whose ack commit (1c2beb27) is signed with a key this VM cannot verify (`%G? is E`). My diff has no keel finding. I ran the later phases by hand: fmt, shape, clippy, the full suite (above), cockpit build all pass. `cargo deny check`: advisories FAILED (no dependency changed by this branch; Cargo.lock untouched), bans/licenses/sources ok.
Keel findings expected from my changes: none. Ceilings: turn.rs lowered, none raised.

## Left / uncertain
- Timing: `tests_work::the_substrate_scenario...` failed once under the full run; fixed as above and passed 3 of 3 alone since.
- A conversation also shows its own `said` on the board (e.g. `[Starting it.]`); I kept it, since a conversation with a running task benefits too. Say if it should be tasks only.
- Docs to update at review: the spec Part III item for this step; `docs/status.md`; `crates/theseus-core/AGENTS.md` ("The work board": progress.said from `loop.ended`'s `say`; "The turn": `turn/advance_step.rs`); the TUI/CLI guides (the work line wording).
