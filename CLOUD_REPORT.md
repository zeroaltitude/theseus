# Cloud report: Jev's rerank of recall's top 20, step 32c (theseus-6fn.3)

Branch `cloud/20261004-rerank-arm`, from `main` at 3add3f5 (the Jev wire-in, 23a, already merged). Two code commits
and this report. Started 08:45 UTC; written 10:30 UTC.

## Step 1: `rerank.v1`, its builder, and the pure reorder (8755976)

**Found.**
- The loader caps a per-item Noul at ten items (`pack.rs`: "a per-item Noul asks 1 to 10 items"). The design wants 20
  Nouls in one request.
- The closed sets had no point for a recall, no builder, and no baseline for the fused order.
- `theseus_memory::recall::recall` exposes no "what passed the filters" list. Its filters are private.

**Changed.**
- `crates/theseus-judge/packs/rerank.v1.toml`, embedded:
  - Point `recall` (new), builder `rerank` (new), baseline `fused` (new), action `none`, cap 8,000 tokens, sample 1.0,
    thresholds 0.90/0.60.
  - Two per-item Noul questions with the same wording: `helps` over source `notes` (the first ten) and `helps_more`
    over `more_notes` (the next ten). This stays within the loader's ten-item rule without changing it. The wording:
    "Does the note numbered {item} hold information that would help answer the message?", with both criteria given.
- `builders/rerank.rs` (`RerankInput`, `RerankNote`, `RERANK_NOTES = 20`, builder version 1):
  - The state is `message` (trimmed, clipped to 4,000 characters, then to 15% of the cap) and `notes`: up to 20
    `{note: n, text}` entries in the fused order, each scrubbed and clipped to 1,000 characters, at 80% of the cap.
  - The dynamic items are the notes the built state kept, keyed by `<node>#<chunk>` and named by their number. A note
    the cap dropped gets no Noul.
  - Fixture `fixtures/inputs/rerank.json`, with goldens `rerank.state.json` and `rerank.v1.request.json`.
  - The shape, loader-rules, golden and huge-input tests now include it. Three builder tests are new: 20 asked of 25,
    every Noul's note is in the state, and the notes are scrubbed.
- `Ask` gains `pub id: Option<String>` (the six-session convention). `Ask::new` sets `None`, `Judgment::pending` uses
  it when set, and `judge::new_id()` mints `jdg_<uuid v7>`. New test: `tests::a_minted_id_is_the_judgments_own`.
- `crates/theseus-memory/src/rerank.rs` is pure:
  - `eligible(science, asker, candidates, params)`: the candidates that pass every filter, the place rule first, in
    the science's order. Each candidate runs through `recall::recall` alone with an open budget, so `recall.rs` is
    unchanged and the filters cannot drift.
  - `reorder(fused_keys, probabilities)`: the top 20 re-sorted by Jev's probability, highest first. Ties keep the
    fused order. An unanswered key (or NaN) keeps its fused slot. The rest follow in the fused order.
  - `Reranked`, a `MemoryScience` whose `rank` is a given order. `repack(...)` is `recall::recall` with that science,
    so the same filters and the same pack apply.
  - Tests: reorder, rest-in-fused-order, eligible-and-repack, and a property test, `the_place_rule_holds_through_the_rerank`.

## Step 2: the core's rerank in shadow, and `theseus judge log` (c12df91)

**Changed.**
- `crates/theseus-core/src/judge/rerank.rs` (`mod rerank` and one `WIRED` line in `judge/mod.rs`; the service gains a
  single field, its deadline):
  - `JudgeService::at_recall(&mut Trace, Recalled)` runs on the turn's path. It checks the pack's mode (`mode_of`,
    with `Off` returning at once), checks the sample, and computes `eligible` (pure, at most the index's 40 hits).
    When nothing is eligible it returns. Otherwise it mints the id, marks the trace (span `judge`, kind `mark`,
    `{pack, point: "recall", mode, judgment}`) and spawns. That is all it does on the turn's path.
  - The spawned task:
    - On a blocking thread: builds the state from the top 20 eligible (through the core's scrubber), writes its blob,
      and reserves from the judge's day budget. If the day's limit stops it, nothing is sent and no row is written.
      The skip is counted and `judge.paused` is written once, as for every shadow judgment.
    - Then calls Jev with `Urgency::live(600 ms)`, the client's own deadline, through `Recording`'s inner judge. On
      success it runs `reorder` and `repack`, adds `context.rerank`, hands the judgment to the sink, and settles the
      budget.
  - The row is `judge.call`, scoped `judge:rerank`, with `budget: "shadow"`. Its context holds
    `session, turn, recall, purpose: "recall", arm: "+rerank", baseline: "fused", blob, deadline_ms, on_path_ms: 0`,
    plus `rerank`:
    `{recall, eligible, asked, fused_admitted, reranked_admitted, changed, order_changed, top, fallback, latency_ms,
    deadline_ms, within_deadline, cost_micros}`.
  - `fallback` is null when answered by the pinned model. Otherwise it names the cause: `model_drift`, a skip reason
    (`shed`, `circuit_open`, `no_key`, `unpriced`, `no_questions`) or a failure class (`timeout`, `rate_limited`,
    `network`, …). On a fallback the reranked order is the fused one.
  - `pub fn probabilities(&Judgment)` and `pub fn fallback(&Judgment)` are there for 30b.
- `recall.rs`:
  - `Memory::manifest_with` returns the manifest and the candidates its pipeline read. `manifest` now wraps it, so
    callers are unchanged.
  - `Memory::science_owned()` is new.
- `turn/recall_step.rs`: uses `manifest_with`, plus the one call `self.judge.at_recall(...)`.
- Config template: the `[judge]` comment names both packs, and adds `# [judge.packs."rerank.v1"]` / `# mode = "off"`.
  `the_templates_judge_section` asserts it.
- `tests_judge.rs` (23a's): two assertions that health's pack list was exactly `["loop.v1: …"]` now check that the list
  contains it. A small edit in another step's test, needed because `WIRED` grew.
- CLI `render/judge.rs`: a rerank's log line replaces the 20 answers. It reads
  `… rerank.v1 (shadow) <session> · recall rcl_… · 3 notes · changed what would be admitted (+1 −1) · $… · 341 ms of 600`,
  or `kept what would be admitted`, or `fell back to the fused order (timeout)`. A unit test covers all three.
- The AGENTS.md files for theseus-core and theseus-memory are updated.

**How it was proven** (offline, against the fake Jev and a stand-in index; `tests_rerank.rs`):
- `a_fake_jev_reorders_what_would_be_admitted`:
  - The baseline admits the fused first note. `+rerank` admits the third, which Jev scored 0.97.
  - The row is keyed and scoped, and names the recall row's id. `fused_admitted` equals the recall row's admitted
    keys, `changed` is true and `fallback` is null.
  - Jev saw the message and the three notes in fused order. The state's blob is named.
  - Cost is above 0, recorded as `purpose: recall`, and health's spend equals it. The session's cost is unchanged.
  - The trace has exactly one `judge` mark, whose `judgment` is the row's id.
- `a_timeout_falls_back_to_the_fused_order`: Jev is slow for 5 s. The outcome is `failed`/`timeout` and `fallback` is
  `timeout`, with both admitted lists equal. Latency is between 550 and 3,000 ms against 600.
- `the_days_limit_skips_a_rerank`: with the limit at 0, one skip is counted, one `judge.paused` row is written, there
  is no rerank row, and Jev gets no connection.
- `mode_off_calls_nothing`: with `rerank.v1` off, the recall runs and `loop.v1` still judges. There is no rerank row,
  Jev sees only `loop.v1/` questions, there is no mark, and health says `rerank.v1: off`.
- `a_slow_or_failing_jev_changes_no_turn`: for Jev slow (10 s, with the rerank deadline raised to 5 s), down,
  rate-limited and malformed, the model's requests are byte-identical to a judge-off core. Result and cost are equal,
  the turn stays under 3 s, and every row has a fallback.
- `a_reranked_turn_keeps_its_frame_budget`: the measured turn writes the same number of frames as with the judge off,
  and at most 5.
- `a_reranks_state_holds_nothing_the_place_filter_dropped` is a property test with 16 cases over generated stores and
  places (tests_recall's `Spot`s). The notes in the state the fake Jev received are exactly the sessions the place rule
  allows. When none is allowed, nothing is sent.

**Planted reverts:**
1. Handing Jev the candidates from before the filters (`let eligible = recalled.candidates.clone()`) made the place
   test fail. The minimal case was a CLI asker with one session in someone else's DM: "a rerank with nothing
   eligible", meaning Jev was sent that DM's note.
2. Awaiting the rerank in the turn (`block_in_place(block_on(judge_rerank(…)))`) made
   `a_slow_or_failing_jev_changes_no_turn` fail: "Slow(10s): the turn took 5.103434536s".

After each revert I restored the file from a copy, touched it, removed proptest's regression file, and checked
`git status` was clean apart from the step's files.

**Runs under load** (nice 19, beside four nice-0 busy loops, five runs). The filter covered the rerank, `tests_judge`
and `tests_recall` tests, plus every theseus-memory and theseus-judge test.
- Runs 1 and 2 of the first pass, over the rerank, tests_judge and tests_recall tests: all 31 passed.
- Run 3 of the first pass: `the_days_limit_skips_a_rerank` failed. That was a race in my test: the skip is counted
  under the budget's lock before `judge.paused` is appended. The fix waits for both. After the fix, runs 1 to 5 over
  192 tests passed every rerank test.
- Failures under load that are not this step's, none of them on the flaky list:
  - `theseus-judge builders::security2::tests::the_builder_is_fast` failed in all five runs, and
    `builders::tests::every_builder_stays_under_its_cap_on_huge_inputs` in runs 1 to 3. Both hold a 500 ms bound in a
    debug build, and nice 19 under four busy loops starves them. They failed before I touched that test.
    - Adding `rerank.v1` to the huge-input test first cloned a 1.1 MB string 2,000 times, which made runs 4 and 5 time
      out. It now uses 100 notes (25 big). Unloaded it takes 0.58 s against 0.49 s without rerank. Under load it is
      back to its earlier failure at about 18 s, which is the same timing bound, not a hang.
  - `theseus-judge tests::each_fake_mode_maps_to_its_error_class`: once, in run 3.
  - `theseus-core tests_judge::a_tiny_day_limit_pauses_shadow_with_one_row` (23a's): once, in run 1. Its rig has
    memory off, so no rerank code runs there. It may share my test's race between the skip count and `judge.paused`;
    worth a look by 23a's owner.

## The gate

Each commit's tree was gated with `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. The gate ran in a worktree of that
exact tree with its own `CARGO_TARGET_DIR`, and the commit's files were compared byte for byte against it.

- **Step 1 (8755976).** These phases passed: fmt, shape, features, clippy, cockpit, test build, and the reader rule.
  The suite ran 1,969 tests: 1,935 passed, 34 failed, and one was flaky
  (`theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` passed on retry). The 34 failures:
  - 33 are `theseus-sandbox::contract` (`clause_01`–`12`, `egress_18b_*`, `exit_status_and_signals`,
    `sigterm_is_forwarded_to_the_command`, `scratch_is_reported_and_discarded`, `a_job_that_cannot_start_says_why`),
    `theseus-sandbox::bench spawn_100`, and `theseusd::sandbox` (its L1 tests). They are the known root-VM cases
    (theseus-pv6i).
  - One is `theseus-core tests_output::the_cores_output_matches_its_golden`. Its golden expects a negative UTC offset
    in a `wake.at` preview (`-#:#`), and this VM runs in UTC (`+#:#`). It passes with `TZ=America/Los_Angeles` and
    fails with `TZ=UTC` on the same build. This is not from this branch: the golden assumes the owner's timezone, and
    a fixed TZ in that test would fix it. Gate 2 ran with `TZ=America/Los_Angeles`.

  After the suite I ran its later phases myself. Protocol types were unchanged. The turn bench (5 runs, no burst)
  gave frames_plain 5 against a budget of 5 and frames_tool 9 against 9. `cargo deny --offline` passed advisories,
  bans, licenses and sources. The lifecycle and jobs benches are skipped in a lane gate.
- **Step 2 (c12df91), first run.** It failed `theseusd::judge a_start_with_the_judge_on_builds_nothing_of_it`, which
  pinned health's pack list to `["loop.v1: shadow"]`. I fixed it to expect both packs. The same run failed
  `theseusd::bench_profile theseusd_check_passes_on_the_bench_profile_with_no_vault` once, with "L1: the self-test
  failed: … a workspace root, /, is the root". It is an L1 self-test on a root VM. It passes alone (with either TZ)
  and in step 1's gate and gate 2's rerun.
- **Step 2, rerun on the final tree.** These phases passed: fmt, shape, features, clippy, cockpit, test build, and the
  reader rule. The suite ran 1,977 tests: 1,944 passed and 33 failed, all of them the root-VM sandbox tests named
  above. Then I ran the later phases by hand: protocol types were unchanged, the turn bench gave plain 5/5 and tool
  9/9, and deny passed. **Green by the brief's rule.**
- `cargo deny fetch` ran in the setup, so the deny phase ran offline against a fresh database.

## The live check (the maintainer's, with `[secrets] jev_api_key` and a model key)

Run it on a scratch daemon with `theseus-index` beside `theseusd` (the install build). Use a short, fresh state dir
(the socket path must stay short).

```bash
d=/tmp/rr; rm -rf $d && mkdir -p $d
theseusd example-config > $d/full.toml   # start from the template; then write $d/theseus.toml as below
cat > $d/theseus.toml <<'EOF'
# your usual provider/model and [secrets] lines (a model key and jev_api_key), plus:
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = true
[memory]
mode = "shadow"
[judge]
enabled = true
EOF
theseusd --config $d/theseus.toml --socket $d/s.sock --state-dir $d/state &
T="theseus --socket $d/s.sock"
```

1. Run the first exchange:

   ```bash
   $T ask "Remember: the grey heron nests by the old weir at Millbrook."
   sleep 10          # the tender indexes it
   $T ask "Where does the grey heron nest?"        # session B (a new session)
   B=<B's session id, from `$T sessions` or the ask's output>
   $T judge log
   $T memory recalled $B
   ```

   `judge log` should show one line `rerank.v1 (shadow) <B> · recall rcl_… · N notes · … · $… · <ms> ms of 600`.
   Its `rcl_` id should be the one `memory recalled $B` shows for B's turn. Expect `changed`/`kept`; the heron note
   should be in `reranked_admitted`. If the line says `fell back … (no_key)` or `(unpriced)`, the key or the price is
   missing.
2. Run ten more questions across sessions (`$T ask "…"`, some with `-s <session>`), then:

   ```bash
   $T --json ledger --kind judge.call -n 100 | jq '[.[] | select(.data.pack=="rerank.v1") | .data.context.rerank]'
   ```

   From those rows, take the p50 and p95 of `latency_ms` against `deadline_ms` 600 (the design expects about 350), the
   cost per recall (`cost_micros`), and the share where `within_deadline` is false. If the CLI's `--json` shape
   differs, read the rows' `data.context.rerank`.
3. Add `[judge.packs."rerank.v1"]` with `mode = "off"` to the config, restart (`$T shutdown`, then start again), and ask
   one more question. Expect no new `rerank.v1` row, and health's `judge:` line should show `rerank.v1: off`.

Stop the daemon with `$T shutdown`.

## Left open, and choices the owner should hear about

- **The live arm (30b):** 30b should call the same pure functions before its pack, on the turn's path, under the
  600 ms deadline:
  1. `eligible`;
  2. one `rerank.v1` Ask with `Urgency::live(600 ms)`;
  3. `reorder(fused, probabilities(&j))`, or the fused order when `fallback(&j)` is `Some`;
  4. `repack`, whose `Pack` replaces the fused pack.

  `at_recall`'s spawn becomes an await there, and only there; my planted revert shows the cost. The recall step's
  deadline becomes 600 ms under `+rerank` (§2.12).
- **Who pays when the arm is live:** shadow is paid from the judge's day budget, as `purpose: "recall"` in the row's
  context. The design's session-budget path for a live arm (reserve from the session, `purpose: recall`) waits for
  26b's live judgments as kernel actions. It is not built.
- **Urgency:** the shadow rerank calls with live urgency, so it waits for an in-flight permit up to 600 ms instead of
  being shed. That makes its latency what a live arm would see. Its timeouts count toward the shared breaker
  (`loop.v1`'s too), as any transient failure does.
- **No row at the day's limit:** a rerank the day's limit stops writes no `judge.call` row. The budget's
  skipped count and `judge.paused` say it, as for `loop.v1`. `Skip` has no day-limit reason, and I did not add one.
- **The message Jev reads** is the recall's query: the new message's text, its attachments' names, and the first 500
  characters of the reply before it, clipped to 2,000 characters. The query gives "yes, do that" a subject. If the
  design meant the bare message, it is a one-line change in `prepare_rerank`.
- **Twenty Nouls as two questions** (`helps`, `helps_more`) keep the loader's ten-item rule. The alternative is to
  raise the rule to 20 for this pack.
- **Per-recall cost: not measured.** A rerank asks 20 questions in one request. I could not measure what Jev bills
  for one offline. The fake Jev bills each question for the whole state, and if Jev does the same, a rerank costs
  about 20 times a one-question judgment of the same state. That is still a fraction of a cent at $0.042/Mtok. The
  shared `shadow_limit_usd_per_day` covers it beside `loop.v1`. Step 2 of the live check gives the real number. If it
  is too dear, `sample` on the pack's config line thins it.
- **Docs for the maintainer to write:**
  - Part III: the 32c item.
  - `docs/status.md`: the row, and "rerank in shadow".
  - `docs/design/m6-memory.md` §2.7: note the two-question split and the shadow spend. Its "the session's budget"
    applies when live.
  - `docs/design/m5-judgment.md` §2.3/§2.4: the `recall` point and the `rerank` builder in the closed sets.
- **Edits outside my area, all small:**
  - `tests_judge.rs` (two assertions) and `crates/theseusd/tests/judge.rs` (one) pinned health's pack list. A sibling
    pack session adding a `WIRED` line will meet the same edits at the merge.
  - `recall.rs` (`manifest_with`, `science_owned`) and `recall_step.rs` (the one call), which 30b also touches.
- **Not done:** 23b's `judge` spans and cockpit surfaces. The rerank has no narrative line or notification of its own
  beyond the trace mark and the row.
