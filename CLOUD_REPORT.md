# Cloud report: step 30b, recall in front of the model (theseus-6fn.2)

Branch `cloud/20261004-recall-node`, cut from `175318e` (main as cloned, store format 7). One step commit,
`c2f1f05`, then this report. Started 08:28 UTC and finished about 09:35 UTC, well inside the 5-hour deadline.

## Step 30b: the `Recall` node, canary and live, the BudgetReport, `memory.label`, the footer

### What I found

- 30a's recall step began the index's query as the first loop's call went out, and read it after the call. For
  canary and live, the read had to finish before the compile, and the node it makes had to reach the plan frame.
  `plan_and_dispatch` already takes extra records for that frame (the closure was `|_| Ok(vec![])`), and the kernel's
  turn handle adds every node a frame writes to the turn's kept transcript. So the node can ride that frame with no
  new frame and no transcript work of its own.
- The compiler is pure over the session's nodes. A `Recall` node renders text from nodes in *other* sessions, so
  the compile needs those sources passed in. Sources are immutable, so a cache by node id is safe.
- The index's extractor already skips `kind = "recall"` (29b), and `node.reach` already follows `derived_from`
  edges of any `via`. So edges with `via = "recall"` are counted with no change to reach.rs.
- The index's chunk text is not always a substring of the core's `text_of` (a user message with attachments joins
  them differently). So the frozen range is wherever the excerpt is found in the source's text, and otherwise the
  source's start, cut to the same length.
- **Trap I hit, which may help the next session:** I built main's tree (a temporary worktree) against this
  checkout's `target/` to prove the old layout sample. The test binaries that followed carried the worktree's paths
  (theseus-judge's and theseus-aws-guard's fixture paths) and a stale theseus-memory. The first gate failed on
  those, and `cargo clean -p <every workspace crate>` fixed it. This is exactly what AGENTS.md warns about ("a
  shared one poisons both").
- **This VM's time zone is UTC.** `tests_output::the_cores_output_matches_its_golden` fails here on the wake
  line's offset (`+#:#` instead of `-#:#`). The golden was written in the owner's zone. That is unrelated to this
  step: I ran every gate here with `TZ=America/Los_Angeles`, and it passes. The golden could normalise the offset's
  sign; I left it, since it is not mine.

### What I changed (`c2f1f05`)

- **Config** (`config/memory.rs`, the template): `mode` gains `canary` and `live`. New keys: `arm` (`none` or
  `baseline`, default `baseline`), `canary_fraction` (0.5), `experiment` (`m6-1`), and `session_recall_cap_tokens`
  (12,000), each with a template line. Validation: the fraction is in 0 to 1, the experiment is not empty, and the
  cap is at least `recall_budget_tokens`.
  - `MemoryConfig::assign(session)` takes the first 8 bytes of SHA-256 of `experiment\nsession` and compares them
    with `canary_fraction`.
  - Canary sends that share to `arm` and the rest to `none`, which runs live with `baseline` in shadow. Live sends
    every session to `arm`. Off and shadow assign nothing.
- **The arm row:** each session's arm is recorded once as a `memory.arm` row (mode, arm, live, experiment,
  science), scoped `recall:<session>` and deferred into the turn's next frame. Duplicates are prevented by an
  in-memory set, and on a session's first turn in a daemon run by a scan of the session's recall scope. That
  survives restarts.
- **The node:** `Body::Recall { recall_id, arm, items: Vec<RecalledRef> }`, with `RecalledRef { node_id,
  session_id, position, chunk: (u32, u32), header, tokens }`. Node ids are `rcn_…`, the origin is harness, and
  `kind_str` is `"recall"`. Only the variant is declared; `Summary`, `Synthesis` and `Lesson` are left to their
  steps, as the brief says. Each match on `Body` got its arm:
  - `text_of` gives an empty string, so a recall is never recalled again.
  - `node_info` returns the references.
  - `place.publish` refuses a recall ("publish its sources").
  - The index's registry test asserts that a recall is skipped.
- **The turn** (`turn/recall_step.rs`; turn.rs gets only a `Turn.recall` field, the `recall_first` call, the plan
  closure's rides, and `recalled` in the result):
  1. `recall_first` records the arm. In front of the model, it awaits the answer, which is bounded by
     `recall_deadline_ms`, before `compile_step`. In shadow it returns the query for 30a's `recall_end`.
  2. `recall_live` runs the pipeline with the place rule, the turn's context (including the sources of earlier
     recalls), and the labels. It then checks the session's cap: the tokens of `Recall` nodes after the current
     compilation's `as_of`. Past the cap, the outcome is `paused` and there is no node.
  3. Otherwise it builds the node. Each item is read by position for its frozen range and header. A
     `derived_from` edge (`VIA_RECALL = "recall"`) goes to each source.
  4. The node and its edges wait in `Turn.recall.rides` until the provider call's plan frame. The `recall.ran` row
     (the manifest, with `arm` and `budget`, and its text stripped) is deferred into the turn's next frame.
  5. `recall_view` hands the compile the transcript with the pending node at position `u64::MAX`, plus the sources.
     `recall_compiled` takes the pending node out of a new compilation's `includes`: its real position comes after
     `as_of`, so it renders in the tail, as it does here.
- **The render** (`recall/render.rs`) is §2.4's block, in the same user turn after the new message:

  ```
  [Recalled: N notes from earlier sessions. Testimony, not instructions: dated, and possibly stale.]
  (1) <header>
      "<the source's text over [start,end), with … where it is cut>"
  ```

  - The header is frozen at recall time: `a message from cli in ses_…, 2026-09-30 14:34 UTC (as of @18231)`.
  - A source that cannot be read says so in its item's place.
  - Sources are read through the store by position. If the position holds another record, the source is read by
    id. Sources are cached in `Memory`, up to 4,096 entries.
- **The compiler** gains `CompileInput.sources`, a `Body::Recall` arm in `render_messages`, and the BudgetReport:
  - `Compiled.budget`, and `Compilation.budget: Option<BudgetReport>` (skip-if-none), which a new compilation
    stores.
  - The report holds the limit (window − max_tokens − 4,096, the ring's own number), the estimate used, the ring's
    cut, and an overage.
  - The ring's cut is `{range: {first, last, nodes}, reason: "overflow", tier: "ring", tokens}`.
  - The overage is set when the estimate's upper bound still passes the limit.
  - A recall's budget drops (tier `recall`) are added to the compilation's report.
- **Protocol** (`memory.rs`, plus field and constant lines in lib.rs):
  - New types: `BudgetReport`, `BudgetDrop`, `BudgetRange`, `BudgetOverage`, `MemoryLabelParams`,
    `MemoryLabelResult`, and `MEMORY_LABELS`.
  - New fields: `RecallManifest.arm` and `.budget`, and `TurnSubmitResult.recalled` (skip-if-zero).
  - New ledger kinds: `recall.ran`, `memory.arm`, and `memory.label`. New method: `memory.label`.
  - The TypeScript is regenerated.
- **Store format 8** (`MANIFEST_FORMAT`, with its doc line). tests_layouts gains format 7's compilation, with
  `memberships` and `guidance`. I checked that the build before this step (`175318e`) writes that sample back byte
  for byte, by running `every_old_layout_on_disk_still_reads` with this sample on a checkout of `175318e`.
  `theseusd/tests/versions.rs` and `store::tests::a_write_moves_an_older_store_to_this_builds_format` now expect
  format 8, and the "newer store" fixture is 9.
- **Labels:** `memory.label` lives in `rpc/memory.rs`.
  - It is judged by `judge_act(Act::Label { what })`: the owner, from a private place. That added a new `Act` arm,
    with its refusal row and line.
  - It writes a `memory.label` row scoped `memory`.
  - The set it keeps out is `recall/labels.rs`. It is built after serving (`Core::warm_labels`, called in theseusd
    beside `warm_ontology`) or by its first reader, and kept current on each write. A node's latest label decides:
    `wrong` and `stale` exclude it, `useful` lets it back, and `should_have` changes nothing.
  - theseus-memory gains `Reason::LabeledWrong` and `Asker.labeled`. The filter runs after `untrusted`; the place
    filter is still first.
  - CLI: `theseus memory label <node> <label> [--recall R] [--note N]`. `MEMORY_LABEL` is added to the client's
    `OPERATORS`, so the command is refused inside a job.
  - `memory recalled` shows the arm, "admitted" in place of "would admit" in canary and live, and `paused`.
- **Discord:** the reply footer adds `🧠 N recalled` when `recalled > 0`, so nothing appears in shadow. The change
  is in `footer()` in render.rs (2,786 lines, under its 2,930 ceiling).
- **AGENTS.md:** the core's Recall bullet gains a 30b sub-bullet, and theseus-memory's filter list gains
  `labeled_wrong`.
- **Shared and long files:** turn.rs got field, call and closure lines only (3,460, under its 3,500 ceiling).
  compiler.rs is at 2,436 (it has no entry in long-files.txt; it must stay under 2,500). protocol lib.rs got two
  field lines, one helper and one method constant. The CLI's `main.rs` (a join-time file) gained the `Label`
  subcommand.

### How I proved it

- **New tests** (`tests_recall_node.rs`, 7 tests, through whole cores):
  - `a_canary_turn_puts_its_recall_in_front_of_the_model`:
    - The node comes right after the input, with arm `baseline`, a reference, a range of `(0, len)`, and no copied
      text.
    - The request's last user message is `[question, note]`, and the note quotes the source.
    - The `recall.ran` row carries the node's `recall_id`, mode `canary`, arm, budget, and no text.
    - There is one `derived_from` edge scoped `in:<source>` with `via = "recall"`, and `reach` lists the node as a
      descendant (route `recall`) with at least one loop of exposure.
    - `recalled = 1`.
    - **The frames this turn writes equal memory-off's** (the node rides existing frames).
  - `the_next_request_begins_with_the_previous_requests_bytes`: a later record under the source's key (as a
    redaction would write) leaves the second request beginning with the first request's message bytes.
  - `the_arm_is_sticky_and_recorded_once`: canary at fraction 0 gives arm `none`, live false, and one row over two
    turns. The control writes shadow rows and no node. A fresh `Memory` finds the row in the store.
  - `a_wrong_label_keeps_a_node_out`:
    - After `wrong`, a new session's recall drops the node as `labeled_wrong` and writes no node.
    - The set is rebuilt from the rows.
    - A non-owner in a shared Discord channel is refused and changes nothing.
    - A bad label or an unknown node is refused.
    - `useful` lets the node back.
  - `a_stalled_index_holds_a_canary_turn_no_longer_than_its_deadline`: with a 50 ms deadline and a pending index,
    the turn takes under 2 s, the row says `canary`/`deadline`, and no node is written.
  - `past_the_sessions_cap_recall_pauses`: the second recall says `paused`, and its why names the cap.
  - `shadow_still_writes_no_node`: no node, no arm row, and no note in the request.
- **Other new and changed tests:**
  - `recall::render::tests`: the render's golden bytes (three items: a multi-line one, a cut range with `…` on both
    sides, and an unreadable source), the frozen range, and the header.
  - `config::memory::tests::a_sessions_arm_is_sticky_and_canary_takes_its_fraction`: at 0.3, 2,000 sessions give
    500 to 700 live; the assignment repeats; 0 and 1; another experiment draws again; live; shadow.
  - The template and validation tests, which take the new modes and keys, and their bad values.
  - The compiler's `fresh_drops_the_past_and_overflow_rings_at_a_user_boundary` now asserts the ring's cut in the
    report: it is stored on the compilation, its range is first to last of the nodes dropped, and its count and
    tokens are there.
  - theseus-memory's `each_filter_drops_with_its_reason` gains `labeled_wrong`.
  - Discord's `a_turn_streams_as_edits_and_ends_with_a_footer` checks the `🧠 2 recalled` bit and that it is absent
    otherwise.
  - The index's registry test checks that a recall is skipped.
- **The place property test through canary:** `tests_recall::the_place_rule_holds_over_generated_stores` now draws
  `canary in bool`. Under canary it also checks that the bytes the model got contain no note of a session the
  asking place may not draw on (and, in shadow, no note at all).
- **Unchanged and passing:** the frame-budget tests (`a_plain_turn_stays_within_its_frame_budget` and the rest),
  `shadow_writes_no_frame_and_changes_no_request_byte`, `bench turn` (5 frames plain, 9 with a tool call), and
  `every_old_layout_on_disk_still_reads`. An older build refusing the newer store is held by `theseusd::versions`
  (with fixture format 9) and the store's own test.
- **Under load** (AGENTS.md's recipe: `nice -n 19` and four busy loops at nice 0, killed by pid):
  `-E 'test(recall) | package(theseus-memory) | test(memory) | test(fresh_drops_the_past)'`, 5 runs. Each run was
  **75 of 75 passed** (59 to 63 s each), with no FLAKY.
- **Planted reverts** (each restored by copy and `touch`, with `git status` clean afterwards):
  1. `render::source` read the source's current record by id instead of by position, and `shown` rendered the whole
     text instead of the frozen range. Then `the_next_request_begins_with_the_previous_requests_bytes` **failed**:
     "the second request does not begin with the first's bytes".
  2. In `recall_live` I set `scene.place = Place::Private`, skipping the place rule in canary. Then
     `the_place_rule_holds_over_generated_stores` **failed**: "AliceDm asked of Cli". A shared DM's canary turn drew
     on a CLI session. I deleted the proptest-regressions file that run left.

### The live check (the maintainer's, with a model key)

On a debug build, with `theseus-index` beside `theseusd` (as `target/debug` has them). The state dir is fresh and
short, so the tender's socket path fits.

```sh
B=$PWD/target/debug
S=/tmp/rn; rm -rf $S; mkdir -p $S
$B/theseusd example-config > $S/config.toml
# In config.toml: the model profile and key you use (GLM or Anthropic), Discord and [web] off, and append:
cat >> $S/config.toml <<'T'
[memory]
mode = "canary"
canary_fraction = 1.0
recall_deadline_ms = 2000
T
$B/theseusd --config $S/config.toml --socket $S/s --state-dir $S/state &
T="$B/theseus --socket $S/s"
sleep 4   # the tender starts 2 s after serving; `$T index status` says running
A=$($T --json ask "Remember: the grey heron nests by the old weir at Millbrook." | jq -r .session_id)
sleep 2   # the index follows the WAL within a second
$T ask "Where does the grey heron nest?"        # a new session, B; note its id (or use --json and .session_id)
$T memory recalled <B>
$T history <B>                                 # the recall node after the question
```

1. **The reply names the weir.** `memory recalled B` shows a recall with mode `canary`, arm `baseline` and outcome
   `ran`, with A's message admitted. `theseus history <B>` shows a `recall` node in B
   after the question.
2. `$T reach <A's message node id>` lists B's `rcn_…` node as a descendant with route `recall`.
3. `$T memory label <A's message node id> wrong` prints "recall leaves it out from the next turn on". Then:

   ```sh
   C=$($T --json ask "Where does the grey heron nest?" | jq -r .session_id)
   $T memory recalled $C
   ```

   C's reply does not use the note, and its recall shows `dropped 1 for labeled_wrong` with A's session.
4. **Discord:**

   ```sh
   $B/theseus-sim discord rig --dir /tmp/rr
   ```

   Follow the commands it prints (fake-discord with its gateway, the model stand-in, and the daemon), after
   appending the same `[memory]` table to `/tmp/rr/config.toml`. Write step 1's note in a CLI session of that
   daemon, wait a second, then say the question in `#lab`:

   ```sh
   $B/theseus-sim discord say --channel <#lab's id from the rig> --user <the owner's id from the rig> "Where does the grey heron nest?"
   $B/theseus-sim discord read
   ```

   The reply's last part ends in `· 🧠 1 recalled`. `#lab` is bound private, so a CLI session's note may be drawn
   on there. A shared channel would draw nothing from the CLI session.

### What is left, or uncertain, and choices the owner should hear about

- **Pending position.** For the compile, the pending node sits at position `u64::MAX`, and a new compilation's
  `includes` leaves it out. Its bytes are the same whether it renders as prefix or tail, since nothing strips a
  recall. A recompile on a later turn puts it in the prefix, which is correct.
- **A plan that fails** (over budget, stopped) never writes the node. The `recall.ran` row is still written, and it
  names a `recall_id` with no node. The turn ends or parks there, so the next turn recalls afresh.
  `TurnSubmitResult.recalled` then still counts the notes that were held.
- **The cap** counts `RecalledRef.tokens` (the pack's estimate) of the `Recall` nodes after the current
  compilation's `as_of`. It resets at any recompile, as §2.4 says. 30c's compaction should drop old notes first.
- **`memory.label`** takes no `author` or `discord` params (unlike the ontology's writes), so Discord cannot send it
  yet. Reactions as labels are filed in the design. Labels are global, not per session.
- **The arm-row check** scans the session's `recall:<session>` scope once per session per daemon run. That is
  linear in the session's recall rows, and off the start path.
- **One `BudgetDrop` per recall item dropped for the budget.** Other recall drops (place, threshold, …) stay in the
  manifest's `dropped`, not in the budget report, since they are not budget decisions.
- **`context.compiled`** does not carry the report yet (its protocol struct is in the long lib.rs). The report is
  on the stored compilation and on `recall.ran`. The design's `context.compiled` `recall` summary would be the next
  small add.
- **Docs to update at review:**
  - The spec's Part III item for 30b.
  - `docs/status.md`.
  - `docs/design/m6-memory.md`:
    - §2.8 now says "NODE schema 2 to 3". The actual change is store format 8 with only `Recall` declared.
    - §2.4's render header shape (mine names the kind, author and session, not the place).
    - `memory.arm`'s row fields.
    - §2.14: `memory.label`'s params have no `author`.
- **The cockpit's per-turn recall view** (not built, as the brief asked) should show, per turn:
  - the `recall.ran` or `recall.shadow` manifest: mode, arm, outcome and why, candidates by source, and timings;
  - the admitted items with each source's rank and fused score, and their header and text through
    `memory.recalls`;
  - the drops grouped by reason, `labeled_wrong` included;
  - the budget report (limit and used, and the dropped items);
  - the `Recall` node's link to `node.reach` for each source;
  - label buttons (useful, wrong, stale, should have) calling `memory.label` with the `recall_id`;
  - the session's arm, from `memory.arm`;
  - per compilation, its `budget` (the ring's range and an overage, as a "never silently thinner" banner).

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run on the committed tree: it **failed in suite
on the known cases only**. 1,951 tests ran: 1,918 passed, 33 failed, 17 skipped. All 33 failures are the root-VM
sandbox cases (theseus-pv6i: the daemon runs as root, and Linux exempts root from `RLIMIT_NPROC`):

- the 20 `theseus-sandbox::contract` tests;
- 13 `theseusd::sandbox` tests: `a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace`,
  `a_deadline_ends_a_job_mid_tunnel_…`, `a_granted_program_in_l1_…`, `a_job_with_no_list_has_no_proxy`,
  `a_listed_host_is_reached_through_the_proxy_…`, `a_probe_script_in_l1_…`, `an_aws_granted_l1_job_…`,
  `health_reports_the_last_real_l1_launch`, `the_jobs_bench_l1_row`, `l1_argv_routes_a_call_to_l1`,
  `unlisted_and_private_hosts_are_refused_…`, `a_running_l1_jobs_command_is_read_before_its_row_…`, and
  `a_host_beyond_the_list_once_approved_is_outside_text`.

The phases before the suite passed: fmt, shape, features, clippy, cockpit, the test build, and the reader rule.

I ran the phases after the suite by hand:

- **protocol types:** ok, once the generated files were added.
- **compiled under lock:** nothing compiled.
- **lifecycle and jobs benches:** skipped under NO_BENCH.
- **turn bench:** `theseus-sim bench turn --check --runs 5 --burst 0` gave `frames_plain` 5 (budget 5) and
  `frames_tool` 9 (budget 9): ok.

Without `TZ` set, `tests_output::the_cores_output_matches_its_golden` also fails here, on the wake line's UTC
offset. That is this VM's zone, not this change.

The deny phase did not appear in this gate's phase table. `cargo deny fetch` ran in setup.
