# Cloud report: compaction roots, `context_overage`, and the assembled strategy, step 30c (theseus-6fn.4)

Branch `cloud/20261004-compaction-roots`, from `11b5a94` (main at `760553f` plus the task commit).

| Commit | What |
|---|---|
| `407ea05` | memory: compaction roots and context_overage (task steps 1 to 4) |
| `be3d7bb` | memory: the assembled strategy at a task's first compile and at a compaction (task step 5) |

All five steps are built. Steps 1 to 4 are one commit, not four: they share the turn's compile step, the
compiler's floor rule and the facts module, so splitting them would have meant gating trees that never stand
alone. Step 5 is its own commit.

## What I found

- The ring (`compiler.rs`, `compile_with`) picks its candidates over **every** renderable node of the session,
  not over the current compilation's prefix. So a summary can't just be "one more node". If it were, every later
  recompile (a model change, a system change, a manual transcript, a second ring) would select the summarized
  range again, next to its summary. The fix is a **floor**: the latest `Summary` stands for its range at every
  recompile (`compiler::compaction::visible`). The ring rings over the nodes after that range and leaves the
  summary out. The ring stays the plain fallback, with nothing of compaction in it.
- A summary node is written after the turn's new message, so "first in the prefix whatever its position" has to
  be a rule of the render (`summaries_first`). Without it, the summary renders after the newest user message,
  and the request is still byte-stable, so only a test that checks the order catches it. See planted revert 2.
- `budget_report` already computed an `overage` when the ring's last cut (`|| last`) still didn't fit. Nothing
  read it, and that request was sent anyway. That is now `context_overage`.
- A turn's `Turn.recall` holds one pending `Recall` node. 30b's scene counts the **whole transcript** as "in
  context". After a compaction that is wrong: the summarized range, recall notes included, is out of context. So
  the assembled section's scene counts only what lies past the new summary's range. My compaction test caught
  this: the first version found the note "already in context" and admitted nothing.
- **Not mine:** `tests_output::the_cores_output_matches_its_golden` depends on the machine's timezone. The
  golden has a wake time `-#:#`; this UTC VM prints `+#:#` (line 1174). It passes under
  `TZ=America/Los_Angeles`, and it would fail on any UTC machine, CI included. It's worth an issue: the golden's
  mask could take the sign, or the test could pin `TZ`.

## What I built

1. **Config** (`config/memory.rs`, the template): `[memory] summary_profile` (default `"glm"`; `"off"` keeps the
   ring) and `assembled_budget_tokens` (4000). Both have defaults, are validated (an empty profile or 0 is
   refused) and have commented template lines. The template line says the dropped range goes to that profile's
   provider, which may not be the session's. A profile name that doesn't resolve is not refused at load (the
   default must load on a config without `glm`): it rings at run time, and the row says why.
2. **The `Summary` body** (`node.rs`): `first`, `last` (WAL positions), `nodes`, `text`, `profile`, `model`,
   `cost_usd`, plus **`header`**, the testimony header frozen when the summary is written:
   `[Summary of 6 earlier messages, 2026-10-04, written by glm]`, or `… 2026-09-20 to 2026-09-27 …` for a range
   that spans days. That is one field more than the task listed. It follows 30b's frozen `RecalledRef.header`,
   so the render never reads other nodes. It renders first in the prefix (`summaries_first`); in a tail it
   renders nothing. The index tender indexes its text and not its header
   (`the_extractor_covers_every_body_variant` has a Summary sample). `node.info`, `publish` and `preview` know
   it. `MANIFEST_FORMAT` goes from 8 to 9; the store and daemon version tests moved with it.
3. **Compaction** (`turn/compaction.rs`, `compiler/compaction.rs`, `fact/compaction.rs`). After a loop's compile
   rings (by the estimate, or by the provider's word), the dropped range goes to the summary profile:
   - The call is a kernel action (`PROVIDER_TOOL`, `purpose: compaction`), planned with its reservation and
     dispatched in one frame. It is settled at its real cost in a frame that also writes the `Summary` node and
     the `context.compacted` row.
   - Its cost joins the turn's books (`t.cost`).
   - The compilation is `strategy: compaction`: the summary, then the ring's kept turns, as of the summary's
     frame, thinking stripped.
   - A `Recall` node in the range is dropped as `recall_note` with tier `compaction`; the range itself is
     reported with reason `summarized`.
   - A second compaction folds the first summary in: its text goes to the call, and the new range starts at
     the first one's `first` and counts its messages.
   - The ring runs as before, with a `context.compacted` row giving `outcome: ring` and the reason, when: the
     profile doesn't resolve, its provider isn't configured, its model is unpriced, the ring dropped nothing new,
     a summary of up to its cap wouldn't fit beside the kept turns, the range is past the summary model's
     window, the spend limit refuses it, the call fails, it answers with no text, or the result doesn't fit.
   - The fact is a row, a `compaction` span, the `theseus.compactions{outcome}` count and the
     `theseus.compaction.tokens` histogram, plus the line "Compaction summarized 6 messages into 120 tokens with
     glm, for $0.0014."
   - Limits I chose: a summary is at most `min(profile cap, 4096)` output tokens (`SUMMARY_MAX_TOKENS`), and the
     call reads each node cut at 6,000 characters (`NODE_CHARS`; long tool results say how much they left out).
4. **`context_overage`** (`OVERAGE_CLASS`). When the ring's last cut, or a request with nothing to drop, still has
   an overage, the turn fails before any call and nothing is persisted. The error names the model, the window,
   the estimate, its upper bound, the limit and how far over it is. A `context.overage` row carries the
   `BudgetReport`, and a narrative line says the same. `session::Failing` parks it on the next message the way
   `context_window` is parked (no retry). **I kept both classes**: `context_window` is the provider's verdict
   after a send, `context_overage` is the estimate's verdict before one. They differ in what was sent and paid
   for.
5. **The assembled strategy.** A task's first compile, and a compaction, put a recall section first in the
   prefix, then the summary, then the tail:
   - The section is 30b's pipeline at `assembled_budget_tokens` (`Scene.budget_tokens`), following the session's
     arm. In front of the model it is a `Recall` node the compilation records as its new **`recall_id`**. That
     node renders first in the prefix wherever it was written; it rides the provider call's plan frame as 30b's
     does, and never renders in the tail. In shadow or the control arm, only the row is written.
   - It is read before the compile, under the recall deadline, with 30a's place filter.
   - At a compaction, the first loop's pending recall (if there is one) becomes the section.
   - A section that won't fit is dropped, and the compaction is tried again without it.
   - `recall_tokens_in_tail` leaves the section out of the tail's cap.
   - COMPILATION layout 8 (30b's, with `budget`, before `recall_id`) is in `tests_layouts`. I wrote it by hand in
     the layout that build writes; the table's round trip holds it byte for byte.

## How I proved it

- **New tests** (11, all pass):
  - `tests_compaction.rs`:
    - `compaction_replaces_the_ring_on_overflow`: the summary's range equals every message the ring dropped, by
      position and count. The summary heads the request, and the dropped turns are gone from it. The row, the
      kernel action's reservation, the settlement at glm's prices (`9000×0.15 + 120×0.50` micros), and the
      execution's budget all agree, with nothing left held.
    - `the_next_request_begins_with_the_compacted_requests_bytes_after_a_restart`: an append, then a new core
      over the same store, both beginning with the earlier bytes, with the summary first.
    - `a_second_compaction_folds_the_first_summary_in`
    - `the_ring_runs_when_the_summary_call_fails`
    - `summary_profile_off_keeps_the_ring`
    - `the_newest_exchange_past_the_window_fails_with_context_overage`: the ring's last cut, no call, the
      numbers, the row, no compilation kept, parked on input, and the next small message answers.
    - `a_tasks_first_compile_is_assembled_with_its_recall_section_first`
    - `a_compaction_is_assembled_with_recall_before_the_summary`
  - `compiler::compaction::tests`: the header, the floor, the order.
  - `turn::compaction::tests`: a recall in the range is dropped and the floor is folded.
- **Planted reverts** (each file restored and `touch`ed, `git status` checked):
  1. *The ring's last cut sent instead of `context_overage`* (the overage check in `compile_step` replaced by
     `None`). `the_newest_exchange_past_the_window_fails_with_context_overage` failed: the turn answered
     "Short and sweet." on a 67,694-token request against a 33,904 limit. I also planted it before the test had
     an earlier turn; it failed then too.
  2. *The summary rendered at its node's position, not first* (`summaries_first` removed from
     `render_request`). Three tests failed: `compaction_replaces_the_ring_on_overflow`,
     `the_next_request_begins_…_after_a_restart` and `a_second_compaction_folds_…`. The byte-for-byte one failed
     with `assertion failed: first_text(&compacted).starts_with("[Summary of ")`.
- **Under load** (AGENTS.md's recipe: the test binary at `nice -n 19`, four busy loops at nice 0, killed by pid):
  - After `407ea05`: `tests_overflow tests_compaction compiler::`, 32 tests, ran 5 times, 5/5 green
    (18 to 19 s each).
  - After `be3d7bb`: the same set, now 34 tests, 5/5 green; and `tests_recall*`, 14 tests (I changed
    `recall_step.rs`), 5/5 green.
- **Frames:** `tests_m3::a_plain_turn_stays_within_its_frame_budget` passes. `theseus-sim bench turn --check` gives
  plain 5 (budget 5) and tool-call 9 (budget 9), after both commits. A compaction adds 2 frames to its own turn
  (the summary call's plan and its settlement); a plain turn is unchanged.
- **Old layouts:** `tests_layouts::every_old_layout_on_disk_still_reads` passes, with the new layout-8 sample.
- **Suites:** theseus-core, theseus-index, theseus-store and theseus-protocol pass, except the golden (timezone,
  above).

## The live check (the maintainer's, with a GLM key)

A scratch daemon on a fresh state dir, Discord and the web UI off:

```sh
D=$(mktemp -d); S=$D/sock
theseusd example-config > $D/cfg.toml
# Discord and the web UI off; GLM's window small.
sed -i '/^\[discord\]/,/^enabled/ s/^enabled = true/enabled = false/; /^\[web\]/,/^enabled/ s/^enabled = true/enabled = false/' $D/cfg.toml
sed -i '/^\[profiles.glm\]/a max_output_tokens = 2000' $D/cfg.toml
printf '\n[catalog."glm-5.3-flash"]\ncontext_window = 32000\n' >> $D/cfg.toml
# (plus the [secrets] line for the zai key, as on your machine)
theseusd --config $D/cfg.toml --socket $S --state-dir $D/state &
```

Check the numbers first. GLM's limit is 32,000 − 2,000 − 4,096 = **25,904** tokens, and the ring keeps up to 60 %
(15,542). After the first turn, `theseus --socket $S --json ledger --kind context.compiled -n 1` shows `est_tokens`
for the template's system block plus one exchange. Choose a file size so that three of them pass 25,904. About
30 KB of prose is about 8 to 10 k tokens at GLM's bytes per token.

1. Four turns in one `glm` session, each reading one ~30 KB file:
   `theseus --socket $S ask -P glm "read /tmp/f1.txt with fs.read and say its first line"`, then
   `ask -P glm -s <session> …` for f2 to f4.
   - Once the session is past the window, `theseus --socket $S --json ledger --kind context.compiled -n 3` shows
     `"strategy": "compaction"`, `"trigger": "overflow"`.
   - `theseus --socket $S --json ledger --kind context.compacted` shows `outcome: compaction`, the summary's
     `cost_usd`, `settled_micros` below `reserved_micros`, and its `node_id`.
   - `theseus --socket $S --json history <session>` shows a `summary` node: its text starts with
     `[Summary of N earlier messages, …, written by glm]`, and its detail has `first`, `last` and `nodes`.
2. One more short turn: its `context.compiled` row says `"decision": "append"` (no recompile), and nothing new
   appears in `context.compacted`.
3. `theseus --socket $S ask -P glm -s <session> --attach big150k.txt "read this"` exits 1 at once. The error reads
   `context_overage: the newest exchange alone does not fit glm-5.3-flash's window of 32,000: …`. A
   `context.overage` row appears, and `ledger --kind provider.call` gains no row.
4. Again with `[memory] summary_profile = "off"` on a fresh state dir: the same steps give `"strategy": "ring"` and
   no `context.compacted` row. Step 3 still fails with `context_overage`.

To see the assembled prefix live, set `[memory] mode = "live"` with the index tender running. A compaction's
request then starts with the `[Recalled: …]` note, then the summary, and the compilation (`theseus --json ledger
--kind context.compiled`, or the store) carries `recall_id`.

## What's left, uncertain, or for the owner to decide

- **The overage uses the estimate's upper bound** (the bytes part plus 40 %), as the ring does. A request whose
  real count would fit but whose upper bound doesn't now fails before sending, where today it was sent. That is
  the design's "named outcome, never a thinner prompt", but it is stricter at the edge.
- **A /stop during the summary call** doesn't cut it: it isn't armed as the turn's own call is. The call is
  bounded by the provider's timeouts, and the turn's own call then stops.
- **The summary's tokens** go in its row, not in the turn's `usage`: the turn's usage stays its model's. Its
  dollars do join the turn's and the session's cost.
- **A summary written and then found not to fit** (rare: a pre-check bounds it) stays in the store, rendering
  nothing in the ring's tail. It still becomes the floor for later recompiles, so its range isn't shown raw
  again.
- **A promoted first-loop recall** keeps its 1,500-token pack when it becomes a compaction's section. Only a
  section recalled for the compaction, or at a task's first compile, gets the 4,000 budget.
- **Triggers are deterministic:** the estimate, or the provider's word. CONTINUE (25b) isn't touched.
- **Near their ceilings:** `turn.rs` is at 3,492 of 3,500 lines and `compiler.rs` at 2,489 of 2,500 (it is not
  listed, so its ceiling is the default 2,500). Others add to both.
- **What the cockpit's compaction view should show:** a session's compactions in order, each with its
  summary's header and text, its range (first and last position, message count, and a link to the nodes it
  stands for), the profile, model, tokens and cost, and any folded summary. Each ring fallback with its reason.
  In the context view: the `strategy` badge, the assembled order (recall section, summary, tail), and the
  `BudgetReport`'s drops by tier (`ring`, `compaction`, `recall`). A `context_overage` failure with its numbers.
- **Docs to change** (I didn't edit them):
  - `docs/design/m6-memory.md` §2.5 and §2.8:
    - the `header` field;
    - the floor rule (the latest summary stands for its range at every recompile, and the ring leaves it out);
    - `SUMMARY_MAX_TOKENS` and `NODE_CHARS`;
    - `recall_id` rendered first wherever its node sits;
    - a compaction's section scene counting only the kept context;
    - both overflow classes kept.
  - `crates/theseus-core/AGENTS.md`: a Compaction bullet beside Recall, covering `turn/compaction.rs`,
    `compiler/compaction.rs`, `fact/compaction.rs`, `tests_compaction.rs` and the floor rule. Also add
    `context_overage` to the Traps or Invariants.
  - `docs/status.md` and the spec's Part III item for 30c, `MANIFEST_FORMAT` 9 (to renumber at the merge).

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run before each commit (logs gate2 and gate3;
gate1, the first run, also failed the two `theseusd::versions` format tests, which I then moved to format 9). I set
the timezone only so the output golden (above) can run; the same golden fails under UTC on main.

- fmt, shape, features, clippy (`-D warnings`), cockpit, test build and the reader rule: **ok**.
- Suite: 2,062 tests, 2,029 passed and 33 failed. **Every failure is the sandbox's, from running as root**
  (theseus-pv6i): 19 `theseus-sandbox::contract` tests, 1 `theseus-sandbox::bench` test and 13 `theseusd::sandbox`
  tests.
- I then ran the phases after the suite myself. Protocol types: **ok** (the only protocol change is `LedgerKind`'s two
  new kinds, and the generated TypeScript is unchanged). Deny, offline with a fresh advisory database: **ok**.
- Benches skipped (`THESEUS_GATE_NO_BENCH`). `theseus-sim bench turn --check` I ran myself: ok (5 and 9).
