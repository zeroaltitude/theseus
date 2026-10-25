# CLOUD_REPORT: p3f-clear-by-tokens (theseus-5lzox)

Branch `cloud/20261025-p3f-clear-by-tokens`, based on `b69236c1`. Commits on top of the task commit:

| commit | what |
|---|---|
| `4ed4d359` | early-push marker (empty). Its subject says "first build done"; that was wrong when I pushed it: the first `cargo build` was still running. The build finished a few minutes later. |
| `5cf9dba5` | the token rule, stubs that remember, the rewritten ring tests, `tests_clear.rs` |
| `1bc9e022` | the replay (`tests_replay.rs`), `KEEP_TOKENS` chosen from its grid |
| `d51ce6cb` | core `AGENTS.md`, "One long instruction" paragraph only |
| `9c5dde18` | three other tests that encoded the 20-exchange rule, rewritten |
| (this) | `cloud report (not for main)` |

## 1. For the maintainer

- **Store format: 51, untouched.** The cleared results are still drops of tier `cleared` in the stored report. The new parts of the stub (line range, file length, note) are derived at render time from stored nodes.
- **Ceilings:** none raised. `turn.rs` untouched. `compiler.rs` +4 lines (two `mod` lines), `kept.rs` +2, `render.rs` +13 net. `clearing.rs` is 419 lines.
- **No OWNER CALL, no STOP.** `keel-guard.py --base b69236c1 --head HEAD --quiet` prints nothing and exits 0 (section 8).
- **Chosen rule: trigger 150K tokens of not-yet-cleared results, keep 40K.** The owner's example was keep ~60K. The replay says 40K is cheaper (section 4); the owner can move either constant.
- **No config key added.**
- **A finding the owner should hear first:** the brief's sizes (reads of 12-25K tokens) do not reproduce the owner's own sawtooth. At those sizes the base rule never passes (20 exchanges hold ~520K, over the 50 % line, so its guard refuses every pass and the ring runs 48 times). The base only reproduces the hunt (mean 318K, max ~430K, 7-8 passes, ~$16) with reads of about 2-8K tokens (avg ~5K, about 13K per exchange). I calibrated the replay to that and report both.
- **Token rule vs the brief's expectation.** The brief expected mean context 318K -> ~120K and about -60 % per call. The replay gives 321K -> 175K and -33 % in dollars ($17.67 -> $11.88). See section 4 for why it can't reach 120K.
- **The gate could not run end to end here.** `scripts/gate.sh` fails in its `door` and `keel` phases on this clone, for reasons that are not this branch's (section 7). I ran the phases myself.

## 2. The rule

`compiler/clearing.rs`:

- `CLEAR_AT_TOKENS = 150_000`, `KEEP_TOKENS = 40_000`; `CLEAR_AT = 50` and `CLEAR_TO = 35` stay as the small-window lines. `KEEP_EXCHANGES` is deleted (no floor left: the floor is "the newest exchange's results", see below). Each constant's doc comment gives the evidence.
- **Trigger.** A pass is considered when the estimate passes `CLEAR_AT_TOKENS` (a cheap precheck) and, after summing `live` (the tokens of results not yet cleared and not late, by the per-byte estimate `pass()` always used), `live > CLEAR_AT_TOKENS` **or** `est > CLEAR_AT % of budget`. So a small window clears no later than at the base.
- **Keep.** `overhead = est - live`; `keep = min(KEEP_TOKENS, CLEAR_TO % of budget - overhead)`. Walking back from the newest result, results stay whole while their tokens sum to at most `keep`; the first one past it and every older one longer than `MIN_CHARS` is cleared. Results after the last answer (the newest exchange) always stay whole, however large. `late` results and the ones already cleared are untouched.
- **The savings guard.** `line = min(overhead + CLEAR_AT_TOKENS, CLEAR_AT % of budget)`; the pass runs only if `est - saved <= (line + overhead + keep) / 2`, i.e. it must take the request at least halfway from the line to the keep. For a small window this is the base's midpoint of 50 % and 35 %. The formula is in `clear`'s doc comment.
- **Why passes stay rare.** After a pass the results are at most ~`keep` (+ the newest exchange), so the next pass is `(trigger - keep)` tokens of new results away (110K). Guard: a pass that can't reach the midpoint doesn't run (the ring makes room).
- **Cache stability.** A pass is one new compilation. Its cleared results are drops in the stored report; the render reads them, so every later append renders the same stub bytes. The note comes from the next answer after the result, which exists when the result is cleared (the newest exchange never is) and never changes.
- **Cost of the check.** `clear` runs on every compile as before; it returns before decoding any node unless the estimate passes 150K tokens. Above that it decodes the shown nodes (the heat cache holds them), as the base did above 50 %.

## 3. The stub

Before (`b69236c1`), for a read with no offset or limit in the call, and for one with `offset 1, limit 400`:

```
[Result cleared to save context: fs_read src/big.rs (6,003 characters). Read it again if you need it.]
[Result cleared to save context: fs_read src/f0.rs, lines 1-400 (6,003 characters). Read it again if you need it.]
```

After (exact text pinned in `tests_ring::clearing_stubs_only_old_large_results_and_each_says_how_to_get_it_back`):

```
[Result cleared to save context: fs_read src/f0.rs, lines 1-400 (6,003 characters). You read it, and your notes after it still stand; read again only the line range you need. After it you wrote: "step 1".]
```

A window result (`[showing lines 1-400 of 1210; …]` footer) gives `lines 1-400 of 1,210` whatever the call's input said (`remember::read_lines`: first and last numbered rows of the result, the length from the footer, else the tool's `meta.lines_total`; a result with no numbered rows falls back to the call's offset/limit as before). The head `[Result cleared to save context:` is unchanged.

- **The note** is the first sentence (cut at `.!?:` or the end) of the first text block of the next answer after the result, whitespace folded, `"` -> `'`, at most 120 characters, cut at a word with `…`. None if that answer has only tool calls. `remember::notes` walks the nodes the render already holds (`render_messages`, and the kept render for its new nodes) and hands the map to `clearing::block` through `Ctx.notes`. No model call, nothing stored.
- `proc_run` and the generic stub keep today's text and take the note before the closing bracket (`... Run it again if you need its output. After it you wrote: "…".]`); the full-output form keeps `the whole output is at <path>.`
- The test-only `clearing::stub` (no note) is kept for `a_programs_stub_names_its_kept_output`, whose text is unchanged.
- **The optional running "checked" note: not built.** Reasons: it would be text in the uncached tail that changes at every pass, and the "files never opened" list needs a work-dir scan on the turn path (against FAST). The per-stub range and note carry the memory. A follow-up could add it once the verify rerun says whether re-reads fall under 20 %.
- Stub size grows by ~110-250 bytes. `ring`/`cold` estimate a stub at 200 bytes (`pass()`: `len - 200`; `cold.rs`: `len / 4`); I did not change cold.rs. The replay counts the real stubs.

## 4. The replay (model-free, `compiler/tests_replay.rs`)

207 calls through the real `compile` (append loops, each compile given the last compilation), `claude-sonnet-5-5` on a 1M window (budget 867,904; 50 % = 433,952), a ~7.5K system block, a ~450-token answer per call kept in the context, 1.45 calls per answer on average (~300 calls), 90 % of them `fs_read` results of numbered rows (300-800 lines), 65 distinct files read in a first sweep and the rest re-read, seeded. Accounting: a request that appends reads the previous request from the cache and writes the delta; a recompile (a pass or the ring) reads only the system block and writes the rest; output 450 tokens a call. Prices: read $0.10, write $2.50, output $10 per million.

**Calibration.** Reads of 2-8K tokens (avg 5K). With these the 20-exchange rule gives 9 passes, mean 321K, max 433K, $17.67: the owner's hunt (7-8 passes, mean 318K, max ~430K, ~$16). The brief's 12-25K-token reads are the second table.

Calibrated shape (reads 2-8K tokens), `TZ` irrelevant:

| rule | passes | rings | mean | max | rewrite tokens at passes (max one) | cache read | $ |
|---|---|---|---|---|---|---|---|
| base (50 % / 20 exchanges, `b69236c1`) | 9 | 0 | 321,508 | 433,338 | 2,479,623 (281,342) | 62,350,444 | **17.67** |
| near-window 82.5 % (comparison only; the base rule on a window 1.65x the budget, so line 82.5 %, to 58 %) | 5 | 0 | 489,089 | 715,908 | 2,320,646 (470,248) | 97,172,276 | 20.82 |
| trigger 120K, keep 40K | 16 | 0 | 159,120 | 277,452 | 1,820,210 (173,956) | 29,459,414 | 12.57 |
| 120K / 60K | 20 | 0 | 168,095 | 275,818 | 2,729,871 (201,657) | 30,462,124 | 14.81 |
| 120K / 80K | 28 | 0 | 181,245 | 278,779 | 4,500,469 (223,366) | 31,495,431 | 19.14 |
| **150K / 40K (shipped)** | 12 | 0 | 175,200 | 316,445 | 1,395,479 (181,350) | 33,217,600 | **11.88** |
| 150K / 60K (the owner's example) | 14 | 0 | 184,314 | 303,252 | 1,922,139 (199,848) | 34,567,919 | 13.35 |
| 150K / 80K | 17 | 0 | 196,490 | 317,856 | 2,708,521 (219,504) | 36,349,748 | 15.38 |
| 200K / 40K | 8 | 0 | 200,981 | 357,377 | 916,669 (172,135) | 38,953,441 | 11.45 |
| 200K / 60K | 9 | 0 | 210,468 | 363,090 | 1,225,928 (194,007) | 40,630,031 | 12.34 |
| 200K / 80K | 10 | 0 | 222,109 | 370,041 | 1,581,528 (217,505) | 42,699,824 | 13.39 |

"Rewrite" is the tokens written at the passes only; the total `write` also holds each call's new tokens (about 2.6M at the best cells).

**Choice.** I chose the rule before reading the table: the cheapest cell of the owner's ranges (trigger 120-200K, keep 40-80K), a cell within 5 % of it winning if its mean context is lower. 200K/40K is cheapest ($11.45), 150K/40K is +3.8 % with a mean 26K lower, so 150K/40K ships. The other cells are above.

Brief's sizes (reads of 12-25K tokens, ~26K of results a call): base 0 passes, 48 rings, mean 528,853, max 619,742, **$81.08**; 150K/40K: 47 passes, mean 169,140, max 301,905, $28.82 (150K/60K: 51 passes, mean 177,675, $31.44).

**Against the brief's expectation (318K -> ~120K, -60 % a call).** Mine: 321K -> 175K (-45 %) and $17.67 -> $11.88 (-33 %). It can't reach 120K here: after a pass the context is the kept results (40K) plus every answer so far (~450 tokens x calls, up to ~93K by the end) plus the stubs (~100 tokens each, up to ~27K) plus the system block, and it grows 90-110K before the next pass. The answers are modelled as fully retained; if the hunt's thinking is dropped from later requests, both rules' means are lower. Most of the remaining cost is the new content every call writes (the same in every row), so no rule can take it down by 60 %.

**What the replay cannot show:** whether the model re-reads less. The re-read share (66 % of lines in the hunt) needs a model; see section 11.

Asserted properties (`the_bug_hunts_replay_holds_its_context_near_the_keep`): passes 6-30, mean < 200K, max < 360K, one pass's rewrite < 200K, $ < 14.00 (shipped: 12 passes, 175,200, 316,445, 181,350, $11.88). The brief's "each pass's rewrite under ~100K" does not hold on this shape: the answers and stubs alone put a late pass's rewrite at 150-190K; it is 3.8x smaller than the base's total anyway. `the_replays_grid` (ignored; `cargo test -p theseus-core --lib the_replays_grid -- --ignored --nocapture`) prints the 3x3 table above.

**The near-window rule** (pass only at 80-85 %) is dearer on this shape ($20.82 vs $17.67): its mean context is 489K. No follow-up on it. (It is approximated by the base's code on a larger window, so the comparison is only roughly the analysis' rule.)

No bench in `theseus-sim` exercises clearing (`perf/long.rs` and `synth::long_session` don't), so there is nothing to rerun there; `tests_one_turn.rs` and `tests_ring_sums.rs` are the closest and are covered below.

## 5. Goldens

Every golden in the workspace that reads a compiled request or a stub is in theseus-core; the whole core suite passes with `TZ=America/Phoenix` (4504 passed, 26 skipped, none failed). No golden moved; I updated none. The DM golden is untouched (section 9). I did not run the other crates' suites (theseusd, theseus-sim, theseus-discord): nothing outside theseus-core reads a stub or `clearing::`.

## 6. Tests and plants

New: `compiler/tests_clear.rs` (3 tests), `compiler/tests_replay.rs` (1 test + the ignored grid). Rewritten in place (names kept): `tests_ring::clearing_stubs_only_old_large_results_and_each_says_how_to_get_it_back` (item 1: the expected cleared set is worked out again in the test from the token walk, and the stub text is exact), `tests_ring::no_pass_under_the_line_or_without_old_results` (item 2: same assertions plus "the newest exchange's results are whole"). The new tests can't be run at `b69236c1` (they use `stub_noted` and `remember`), so "red at the base" is shown by the plants.

| plant | what | test that went red |
|---|---|---|
| P1 | the 20-exchange keep put back in `pass()` | `tests_ring::clearing_stubs_only_old_large_results_and_each_says_how_to_get_it_back` |
| P2 | trigger at `CLEAR_AT` % only (`rule()` returns `u64::MAX`) | `tests_clear::a_long_session_clears_at_the_token_line_and_keeps_its_prefix_between_passes` |
| P3 | the `CLEAR_AT` % arm dropped (`by_percent = false`) | `tests_clear::a_small_window_clears_no_later_than_at_the_percent_line` |
| P4 | the range from the call's input only (`read_lines` -> None) | `tests_clear::a_reads_stub_names_the_lines_its_result_held_and_the_note_is_cut_on_a_word` |
| P5 | the base's rule (percent only, keep 270K) | `tests_replay::the_bug_hunts_replay_holds_its_context_near_the_keep` (mean-context bound) |

Each was planted with `/tmp/plant.sh`, the test run, the file restored from a copy and `touch`ed; `git status` was clean after.

Other tests changed because they encoded the 20-exchange rule. None lost an assertion or had one weakened (the keel guard agrees):

- `tests_cold::a_cold_resume_of_a_long_thread_fits_its_working_tokens_with_its_stubs`: asserts `est_tokens > 300_000` of a warm 200-turn thread. By tokens a warm thread can't stay that big through its results, so the person messages in this test are now long (`" and the gauge on the north pier again" x 125`); the assertions are as they were.
- `tests_one_turn::a_single_instruction_reads_300_large_results_without_an_overage`: the window is 80,000 (was 100,000). At 100K the token keep takes the 300-turn read to one compaction (the test wants two) with 21 passes; at 80K it is two compactions and under 30 passes. Assertions unchanged. (This is also an honest finding: on a small window the new rule needs fewer compactions.)
- `tests_bg_gate::a_later_loop_compacts_over_this_turns_screened_results`: 16 reads of 6,500 tokens fit under the new rule by clearing, so no compaction ran. It now reads 16 loops x 8 short files (about 1,500 characters each, under `MIN_CHARS`), so only the ring and the summary make room; assertions unchanged.

## 7. Gate

- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **fails in `door`/`keel` before building**, with errors that are not this branch's: it picks its own base (`2d1d8d6f`, not `b69236c1`) and refuses the owner's earlier acks ("its signature does not verify (%G? is E)": no key here) for erosions in `persona_cmd.rs`, `keel-guard.py`, `test_keel_guard.py`, `task_history_tests.rs`, `voice/flux.rs` that I did not touch. With `--base b69236c1` the guard is clean (section 8).
- So I ran the phases by hand:
  - `cargo fmt --all -- --check`: ok.
  - `scripts/shape.sh`: ok (no file over 2,500 lines but the 8 listed).
  - `cargo clippy -p theseus-core --all-targets -- -D warnings`: ok (twice, the last after the final commit's code).
  - `TZ=America/Phoenix cargo nextest run -p theseus-core --no-fail-fast`: 4504 run, 4504 passed, 26 skipped. (The first full run had two failures, `tests_bg_gate::a_later_loop_compacts_over_this_turns_screened_results` and `tests_one_turn::a_single_instruction_reads_300_large_results_without_an_overage`, fixed in `9c5dde18`; `tests_cold` I had found in the lib run.)
  - Not run: the full-workspace clippy, `cargo deny` (I did not run `cargo deny fetch` or install it; the nextest install was the only long one), theseusd/theseus-sim suites, the benches (no budgets measured).
  - None of the known-failing cases (L1, `the_cores_output_matches_its_golden`, `a_filtered_page_equals_the_scans_answer`, `python3s_repl`) failed in the core suite.

## 8. Keel (whole output)

```
$ python3 scripts/keel-guard.py --base b69236c1 --head HEAD --quiet
(no output)
exit 0
```

## 9. DM golden

```
$ git diff --exit-code b69236c1 -- crates/theseus-discord/src/golden/dm_flow.txt
(no output)
exit 0
```

## 10. Join notes

Files (line ranges at the branch head; all additive unless stated):

- `crates/theseus-core/src/compiler.rs`: `mod tests_clear;` after `session_block` (:52-53, with `#[cfg(test)]`), `mod tests_replay;` after `tests_ring_sums` (:60-61). Nothing else. `tests_cold` keeps its own `#[cfg(test)]`.
- `compiler/clearing.rs` (rewritten around the gate: doc header, constants, `rule()`/`TRIED` test hook, `clear`, `live_tokens`, `result_tokens`, `pass`, `any_old`, `block`, `stub`, `stub_noted`); `compiler/clearing/remember.rs` (new: `note_of`, `notes`, `read_lines`). I own these files per the brief.
- `compiler/render.rs`: `Ctx.notes` (:99-100), the `notes` call in `render_messages` (:140, :149) and the `clearing::block` call (:215-220). `compiler/kept.rs`: two lines (:225, :234): the same `notes` map for the nodes it renders. Neither is touched by the other rows I was told of.
- Tests: `compiler/tests_clear.rs`, `compiler/tests_replay.rs` (new), `compiler/tests_ring.rs` (`compile_in` is now `pub(super)`; two tests rewritten), `compiler/tests_cold.rs` (:166-175), `tests_one_turn.rs` (:164-169), `tests_bg_gate.rs` (:284-300).
- Core `AGENTS.md`: the "One long instruction" bullet only (:119-137). The line near :160 doesn't name `KEEP_EXCHANGES`.
- `compiler/clearing.rs::block` has a new fourth argument (`note: Option<&str>`); its only caller is `render.rs`.
- **Not touched:** `turn.rs`, `turn/`, `turn/compile_step.rs`, `progress_step.rs`, `checkpoint_step.rs`, `tier.rs`, `theseus-tools`, `toolrun/`, `fs.rs`, `docs/`, the cockpit and the protocol types.
- **No `Execution { .. }` literal** is built by the replay, so no `helper: None` is needed from `p3e-fanout-kernel`.
- **narrow-reads:** the stub reads the range from the result's own footer / numbered rows, so a 400-line default window shows as `lines 1-400 of N`. `remember::read_lines` needs the row format `{:>6}\t` and the footer text `[showing lines a-b of N;` to stay as they are in `fs.rs`; `remember.rs` and `tests_clear::a_reads_stub_names_the_lines_…` pin both.
- The suggested docs changes (not made): spec Part III item for this step: the token rule, the stub, the calibration finding; `docs/status.md`: the recently-landed line.

## 11. What the verify rerun should read

- `efficiency.json` mean context (base 318K; the replay predicts ~175K on the same hunt) and the maximum.
- Clearing passes and their rewrite $: the `context.cleared` rows (`tokens_before`, `tokens_after`) and the cache-write tokens of the call after each (base 7-8 passes of ~265K; replay predicts ~12 passes of ~115K).
- **Re-read share by line coverage** (the metric the replay can't give): the share of `fs_read` lines already read earlier in the same hunt (base 66 %, goal under 20 %), and files opened per 4-minute window after minute 8 (base 1-2). Also whether the file with the planted bug that was never opened in any trial gets opened.
- $ per bug fixed (base about $16 / 4.3 bugs).
- If the re-read share doesn't fall, the first lever is `KEEP_TOKENS` (60K or 80K costs +12 % and +29 % in the replay), then the "checked" running note (not built).

## 12. Follow-ups

- The "checked" note (files read, ranges merged, files never opened), if the re-read share needs it.
- The stub is ~110-250 bytes bigger than the 200 that `pass()` and `cold.rs` assume; neither is wrong enough to matter, but `cold.rs`'s `content.len() / 4` could use the real stub size.
- `THESEUS_GATE` here: the gate's base should follow `--base`/the cloud task's base, or the keel phase can't judge a cloud branch.
- The brief's read sizes (12-25K tokens) should be re-derived from the hunt's `efficiency.json` before the next replay is used for a decision.

## 13. Data floor

Nothing new is sent to a model or routed anywhere: a stub's note is the model's own earlier words from the same session's stored nodes, in the same session's request.
