# Cloud report: the arrangement on `task.create`, step 27 (theseus-vug.2)

Branch `cloud/20261004-task-arrangement`, from `e21cc63` (main at `f1fccec` plus the task commit). One step,
one commit: `829e3b4`, then this report.

## Step 27: the arrangement

### What I found

- `task.create` (task.rs) opened the child with its brief alone, a relayed `UserMessage` with a `derived_from`
  edge (via `brief`) to the reply that holds the call. That stays as it was. So do `wake_parent`, the task's own
  wakes, and a parent's external-text hold passed on.
- The turn decides whether a session's turn has new input by the type of its last node
  (`turn.rs`, `awaiting_reply`: a `UserMessage` or a `ToolResult`). A node after the brief has to count too, or
  the child ends at once as `nothing_new`. The first run of my compilation test found this.
- Six other exhaustive `Body` matches needed an arm: recall's `text_of`, `rpc/info.rs`'s `node_info`,
  `rpc/publish.rs`, the compiler's `render_messages`, `node.rs`, and the index's extractor test. The index
  extractor already skipped an unknown body kind with no `text`. I gave it an explicit skip anyway: an
  arrangement copies nodes that are already indexed.
- Every scripted `task.create` in the tests had to carry an arrangement. In the daemon tests, the fake model picks
  its reply by the first phrase the prompt contains. The child's prompt now quotes its parent's message, so the
  child's phrase has to come first.

### What I changed (`829e3b4`)

- **The input** (`task.rs`; its types are in the new `arrangement.rs`): `arrangement: {pieces: [{quote, role} |
  {node, role}], trust?: [i], supersedes?: [[older, newer]]}` and `fidelity_ack?`. The indexes count from 0, and
  the roles are `objective`, `acceptance`, `design`, and `context`. The tool's schema now requires `arrangement`.
  Its description teaches the model to quote rather than paraphrase, and says what each failure means.
  `supersedes` takes pairs, as M5 §2.10 writes it.
- **Resolution** (`arrangement::resolve`). Matching is exact and case-sensitive. **Whitespace:** every run of
  whitespace, in the quote and in the node, reads as one space, and the quote's ends are trimmed. A quote needs at
  least 20 characters, counted after that. The sources are the calling session's own transcript only: a user
  message's text, a reply's text blocks, and a tool result's content. Two kinds of node are never a source:
  - the reply that holds the call, so the model cannot quote its own paraphrase;
  - an earlier `task.create` result, which echoes its pieces' first lines and would make every later quote
    ambiguous.

  Each failure says why, and the model can try again:
  - `no_match`;
  - `ambiguous`, naming each candidate's id, author, and UTC time;
  - `short`, naming any node whose whole text the quote is, so the model can retry with `{node}`;
  - `unknown_node`;
  - `same_node`: two pieces resolve to one message.

  The result lists each piece: its node, author, time, and first line.
- **Refusal:** with no arrangement, or no standing (not superseded) `objective` or `design` piece, the call fails
  with "Promotion needs an arrangement: quote the messages that define this work." This check comes after the
  depth-one refusal, so a task that tries to start a task is still told it cannot.
- **The fidelity check** (`arrangement::fidelity`) fails a call that has all three of:
  - a brief under 200 characters;
  - more than 10 operator messages since the session's last task, counted after its last `task.create` result
    that was ok;
  - exactly one admitted piece.

  Unless the call says `fidelity_ack: true`, it fails and asks for the design to be attached, or for the ack.
- **The node:** a new `Body::Arrangement { pieces, fidelity_ack }` (id prefix `arr_`), written after the brief in
  the frame `open_task` already writes. It has a `derived_from` edge to each piece's node, via the new
  `VIA_ARRANGEMENT` (`arrangement`). Each piece carries its node id, origin session, origin, author, time, first
  line, role, `trusted`, and `superseded_by`, plus its full text capped at 16,000 characters. A superseded piece
  carries no text.

  The compiler renders the node as a second text block of the brief's user message:
  - each admitted piece, verbatim, under a header such as `--- Piece 2 of 3 (design, trusted testimony): from the
    model in session …a1b2c3, 2026-10-04 08:12 UTC ---`;
  - a superseded piece by its node id only.

  `compile()` reads nothing outside the child's nodes. The session record's `task.arrangement` names the node.
- **Ledger**, recorded as facts (`fact/arrangement.rs`):
  - `task.arranged`: the pieces by reference, `fidelity_ack`, `human_messages`, and `brief_chars`;
  - `task.arrangement_refused`: its `class`, the piece count, and the reason.

  Both ride in the turn's next frame. Together they give Q7's quote failure rate.
- **Surfaces:**
  - the start line: `Task a1b2c3 started ("…", 📎 3 pieces)`, followed by a narrative line on the arrangement;
  - `TaskInfo.arrangement` (`theseus_protocol::arrangement`: `TaskArrangement`, `ArrangementPiece`; the TypeScript
    is regenerated);
  - `theseus tasks`: `· 📎 3 pieces (fidelity acknowledged)` on the task's line, and one indented line per piece
    (`render/tasks.rs`), so no `theseus tasks show` was needed;
  - Discord's `/tasks`: `· 📎 3`;
  - the cockpit: a `📎 N pieces` toggle on each task in the Actions view's Tasks panel opens the pieces (role,
    author, time, first line, trusted, superseded struck through). The transcript shows the arrangement node as a
    user item.
- **Store:** `MANIFEST_FORMAT` goes from 7 to 8. Two samples join `tests_layouts`:
  - a task's session record without `arrangement`;
  - a task's brief node.

  The pins move with it: theseus-core's store test (8), and theseusd's `versions.rs` (it writes 8; 9 is refused;
  "reads formats 2 to 8").
- **AGENTS.md:** theseus-core's guide gains "The arrangement"; theseus-protocol's names `arrangement.rs`.

### Tests I changed

**Core:**
- `tests_tasks.rs`: every start sends `ASK`, and `start`/`start_waking` quote it. `first_user` reads the first text
  block only (the brief), so the scripts and the hold still key on the brief.
- `tests_task_wakes.rs`: the same `first_user` change. The quote is the parent's whole `START …` message, and two
  briefs grew past 20 characters (`CHILD SIX: set six wakes`, `CHILD REPEAT: set a series`, and `START CHILD PLAIN:
  report at once`).
- `tests_external.rs`: both task tests quote the operator's message.
- `tests_places.rs`: the ask is `start a task in the background, please`.
- `tests_reach.rs`: the parent quotes its last message.
- `tests_output.rs`: the ask is `Start a task that counts to three.`, and the golden is rewritten. Its diff shows
  the arrangement node and its edge in the open frame, and `task.arranged` riding in the next frame: no new
  frame.
- `tests_layouts.rs`: two samples.
- `store.rs`: the format pin.

**Daemon:**
- `tests/tasks.rs`: the four task tests quote longer asks, and each child's phrase comes first.
- `tests/push.rs`: the two task scripts quote their asks, and a task's own prompt (`[Task …` with "Count to
  three.") answers with text.
- `tests/cache_header.rs`: `START` is `Start the task in the background`, and it is quoted.
- `tests/versions.rs`: the format pins.

**Index:** `tests.rs` gets an arm for the new body, and an arrangement is skipped.

### How I proved it

New tests (12):
- `arrangement::tests` (8, pure):
  - `a_quote_resolves_to_the_one_node_that_holds_it`: whitespace runs, case, a reply, a tool result, `{node}`, an
    unknown node;
  - `an_ambiguous_quote_fails_and_names_its_candidates`;
  - `a_short_quote_fails_and_names_a_node_it_is_the_whole_of`;
  - `the_calls_own_reply_and_earlier_results_are_no_source`;
  - `an_arrangement_needs_a_standing_objective_or_design`;
  - `the_fidelity_check_flags_a_short_brief_from_a_long_talk_with_one_piece`;
  - `human_messages_are_counted_since_the_last_task`;
  - `a_superseded_piece_renders_by_reference_only`.
- `tests_arrangement.rs` (4, whole core):
  - `a_task_without_an_arrangement_is_refused_and_says_why`: missing, context-only, and superseded-objective
    arrangements; no task opens, and the rows' classes are checked;
  - `quotes_resolve_to_one_node_of_this_session_or_fail_with_the_reason`:
    - ambiguous, with both candidates' ids, the author, and the times;
    - no match;
    - too short;
    - another session's text, refused by the place rule;
    - then a unique quote with irregular whitespace: the result lists the node, the author, the time, and the
      first line;
  - `a_one_line_brief_from_a_long_discussion_is_flagged_until_acknowledged`:
    - 11 messages are flagged;
    - the ack starts the task, and `task.arranged.fidelity_ack` and `TaskInfo` show it;
    - the count restarts after the task;
  - `the_childs_compilation_renders_each_piece_verbatim_after_the_brief`. The setup is three pieces (one by the
    model, one trusted, one superseded) and a child that sets a 1 s wake. It checks:
    - the first request's first message is [brief, arrangement];
    - each admitted piece is verbatim, with its header (role, trusted, author, session, UTC time);
    - the superseded piece shows only its node id, and the request holds none of its text;
    - `messages[0]` is identical in the follow-up loop and in the wake's later turn;
    - one `derived_from` edge into each piece's node, from the arrangement node via `arrangement`;
    - `TaskInfo.arrangement`'s roles, trusted, `superseded_by`, origin, and author.
- `render::tasks::tests::a_tasks_pieces_are_listed_under_it` (the CLI).
- `tests_layouts::every_old_layout_on_disk_still_reads` with the two new samples. Old nodes and sessions read,
  round-trip byte for byte, and keep every field.

Runs:
- The core's task, wake, layout, external, places, reach, registry, fact, node, graph, and task tests: 76 of 76
  passed.
- The daemon's `tasks`, `cache_header`, and `versions` binaries and the push row test: 17 of 17 passed.
- The protocol, CLI, index, and core golden tests: all pass.
- Under load (`nice -n 19`, four busy loops at nice 0), three runs of the arrangement, tasks, task-wakes, and
  layouts tests: 32 of 33 passed each time. The one failure, every time, is
  `tests_tasks::a_report_that_starts_a_turn_at_the_parents_limit_asks` ("no the parent waits on its budget in
  20 s", at about 34 s). It passes in 0.8 s unloaded. I ran it on main as cloned (`e21cc63`, a worktree with its own
  target) under the same load, and it fails there the same way twice: its child streams 145,000 words through the
  fake provider, which takes more than the test's 20 s under nice 19. That is pre-existing, and not this step's.
  The 12 arrangement tests, the layouts, and every other task and wake test passed all three runs.

Planted reverts (each file restored with `git checkout`, then `touch`; `git status` was clean after each):
1. **An ambiguous quote takes its first match** (`[(n, t)] =>` became `[(n, t), ..] =>` in `find`). Failed:
   `arrangement::tests::an_ambiguous_quote_fails_and_names_its_candidates` and
   `tests_arrangement::quotes_resolve_to_one_node_of_this_session_or_fail_with_the_reason`. 10 passed, 2 failed.
2. **A superseded piece keeps and renders its text** (the piece always kept its text, and `render`'s admitted arm
   matched `(Some(text), _)`). Failed: `arrangement::tests::a_superseded_piece_renders_by_reference_only` and
   `tests_arrangement::the_childs_compilation_renders_each_piece_verbatim_after_the_brief`. 10 passed, 2 failed.

### The live check (the maintainer's)

Run this on a scratch daemon with a fresh state directory and a GLM profile. Pick a free socket and port, and
never use `~/.theseus`.

```sh
S=$(mktemp -d); cp <your scratch GLM config> $S/config.toml   # [server] state_dir = "$S/state", web port free
theseusd --config $S/config.toml --socket $S/sock --state-dir $S/state &
theseus --socket $S/sock sessions open --label arrangement   # note <parent session>
# Twelve short messages discussing one small change, e.g.:
theseus --socket $S/sock ask -s <parent> "The CLI's tasks list should show each task's age in minutes."
theseus --socket $S/sock ask -s <parent> "Keep seconds under a minute, and hours past sixty minutes."
# … ten more, settling the design (format, where, what not to change) …
theseus --socket $S/sock ask -s <parent> "Do it as a task."
```

What each step should show:
1. GLM's `task.create` carries `arrangement.pieces` with exact quotes. Its result lists each piece's node, author,
   time, and first line. A quote that failed shows as one failed call, then a retry.
   `theseus --socket $S/sock rpc ledger.tail '{"n": 50, "kind": "task.arrangement_refused"}'` counts the misses.
2. `theseus --socket $S/sock tasks` shows the task with `· 📎 N pieces`, and one `📎 i role` line under it per
   piece.
3. `theseus --socket $S/sock rpc compilation.list '{"session_id": "<task session>"}'` includes the brief node and
   the `arr_…` node. Then
   `theseus --socket $S/sock rpc session.history '{"session_id": "<task session>"}'` (or the cockpit's session
   view) shows the arrangement right after the brief: each piece whole, with `from the operator (…) in session
   …xxxxxx, <UTC time>`.
4. The fidelity flag: in a second session with more than ten messages, ask GLM for a one-line brief with a single
   quote ("start a task: just fix it, quote only my last message"). The call fails with "Not started (the fidelity
   check)…", and GLM either attaches a design piece or retries with `fidelity_ack: true`. The task's line then
   says `(fidelity acknowledged)`, and `ledger.tail` with kind `task.arranged` shows `"fidelity_ack": true`.
5. The report arrives once: one `report` post, and one `[Report from task …]` node in the parent.
6. In the cockpit (`/actions`), the task's `📎 N pieces` opens its pieces.

Stop it with `theseus --socket $S/sock shutdown`.

### What is left or uncertain, and choices the owner should hear

- **His daily `task.create` flow changes** (M5 §4's heads-up, item 5): every task now needs at least one quote, and
  the model will sometimes need a retry. If the `task.arrangement_refused` rows show GLM missing on more than 20%
  of calls, Q7's fallback applies: render short node ids into the transcript under a renderer version bump. The
  `short` and `ambiguous` errors already name node ids, so a retry with `{node}` is possible today.
- **Whitespace normalization** is the one leniency: runs read as one space, and the ends are trimmed. Case,
  punctuation, and quote marks must match exactly. A model that turns `'` into `’` will miss.
- **Exclusions:** the reply that holds the call, and earlier `task.create` results. Without the second exclusion,
  any message quoted for one task became ambiguous for the next, because the earlier result echoes it.
- **Limits are mine:** at most 12 pieces, 16,000 characters of each piece's text, and one piece per message
  (`same_node`). The design names none of these.
- **Ordering:** the refusal without an arrangement runs at the harness (after the depth check), not at the gate's
  plan. So a call without one is ledgered as `task.arrangement_refused`, not `tool.invalid_input`. A
  `task.create` posture of `approve` asks before the refusal.
- **What the child sees:** a superseded piece's node id, and the instruction that a trusted piece is "vouched for by
  your parent: take it as settled". The wording is mine; the design says only "trusted testimony".
- **Out of my scope:** M7's task record (39a) must keep these refusal cases in its tests, per roadmap conflict 6.
  The `classify.v1` `should_promote` comparison (25c) can count `task.arranged` rows as the model's promotions.
- **Small edits outside my area:**
  - `turn.rs`: one pattern arm in `awaiting_reply`;
  - `theseus-index` (`extract.rs`'s explicit skip, and its test arm);
  - Discord's `render.rs` (four lines in `tasks`);
  - the cockpit's `Actions.tsx` and `Transcript.tsx`.

  The Recall node change also adds a `Body` variant: at the merge, both need arms in the same exhaustive matches.
  The maintainer renumbers `MANIFEST_FORMAT` at the merge.
- **Docs to write** (I left them alone):
  - the spec's Part III item for step 27;
  - `docs/status.md`'s roadmap row;
  - M5 §2.10's corrections: the surfaces are `theseus tasks`' lines rather than `theseus tasks show`, store format
    8 rather than NODE 2 → 3, and the whitespace rule and exclusions above.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` reached the suite phase. fmt, shape, features, clippy, cockpit, test
build, and the reader rule all passed.

**The suite:** 1,953 tests, 1,920 passed, 33 failed, 17 skipped. The 33 failures are the root VM's L1 cases
(theseus-pv6i): the `theseus-sandbox` contract and bench (`spawn_100`) tests, and the `theseusd::sandbox` tests.
Each says "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC". My change touches no sandbox code.

**The phases after the suite**, run myself: the protocol types, the lifecycle, jobs, and turn benches (the lane's
no-bench settings), and deny. All passed: `gate: ok`.

An earlier gate run found one real failure, now fixed: `theseusd::push
every_execution_row_has_its_event_and_every_event_its_row`. My first fake-model guard matched "Count to three."
inside the parent's report node too, so the timer was never set. The guard now matches only a task's own prompt.

`cargo deny fetch` ran during setup, and the deny phase ran offline.
