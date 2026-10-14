# CLOUD_REPORT: tool-words (theseus-z6au)

Branch `cloud/20261013-tool-words`, built on the branch's base (store format 33, which this row does not change).
Commits: `81af885` (the whole row: protocol, tools, core, CLI, TUI, Discord, cockpit, tests, goldens, AGENTS.md notes)
and `3467a73` (a late-start fix found in the live check, and the colour rule's test). Nothing in the spec,
`docs/status.md`, the README or `docs/design/` was edited (see "Docs to change").

## What was wrong

A CLI tool line named only its tool (`→ fs.read`), `tool.proposed`'s input was printed for AWS and terminal calls only,
the result's `preview` was never shown (a failing test's output was invisible), and an edit was approved blind
(`? fs.edit needs your confirmation: <reason>`) though `fs.edit` already made the diff for the model. Three summarizers
(Discord's `summarize`, the TUI card's `summary`, `term::summary`) said different things.

## What changed

1. **One summarizer** (`crates/theseus-protocol/src/term/words.rs`, declared from `term.rs`; lib.rs gained only
   `pub mod diff;` and one `pub use` line). `summarize(tool, input)` says a call in words: `Run `python3 -m unittest -v``,
   `Edit tides.py`, `Read README.md`, `Search for `retry` in src/`, `Patch 2 files`, … and an unknown tool is its name and
   argument. It is built on `argument(tool, input)`, which is Discord's old algorithm moved verbatim (clip at 90, backticks
   to quotes): Discord's `summarize` now calls it, so its bytes and its tests are untouched. The TUI card's `summary` is gone
   (the card line is `⏸ confirm fs.edit: Edit tides.py`); the CLI's lines use `summarize`.
2. **`tool.ended.diff`** (`ToolDiff {path, added, removed, unified, truncated}`, optional, serde default, absent from the
   bytes when unset) for `fs.edit`, `fs.write` and `fs.patch`. The tools put the diff in their result `meta["diff"]`
   (`crates/theseus-tools/src/change.rs`; fs.rs changed only the three returns and the patch loop's one push); the core
   scrubs it (`tool_words::scrub_diff`, called in the line that took `r.meta` in `result_node_in`) with the same scrubber as
   the model's copy and `fact::tool::ToolEnded` pushes it. A write is a diff against the old text, or against nothing (`--- /dev/null`)
   for a new file; a patch of several files is the files' diffs in order with path `N files`. Capped at 16 KB on a line
   edge (`diff::cap`), counted over the whole diff.
3. **`confirm.requested.preview`** (`ConfirmPreview {kind: diff|command|text, body}`, optional): an edit's diff against the
   file as it is when the call is planned (`change::preview` uses the same `edit_after` and `file_diff` the run does), a write's
   diff, a patch as given, a `proc.run` as a shell line with `in <dir>`; scrubbed. In `crates/theseus-core/src/tool_words.rs`.
   `toolrun.rs` got one call (`preview: self.preview(tc.dir, tool, &call.input)`) after I moved its `subject` fn (14 lines) into
   `tool_words.rs` (2,477 lines at the base, 2,463 now). `confirm.list` (`rpc/confirms.rs`) makes the preview too, from the files as they
   are at the read, so a client that connects after the question still gets it.
4. **Styled diffs** (`crates/theseus/src/render/diffview.rs`, pure; `Tag::Del`/`Tag::Add`). Rows are `file:line (+n −m)`, the
   hunk header, then `{number} {-|+| } text` (the old number for a removed line, the new for the rest); the changed words of
   a paired −/+ line sit between `MARK_ON`/`MARK_OFF` (a token-level LCS in `theseus_protocol::diff::words`, no dependency).
   The CLI paints them with its own SGR (`31`/`32`, and `1;97;41`/`1;97;42` for the marked words) only when stderr is a terminal,
   `NO_COLOR` is unset or empty and `TERM` is not `dumb`; `print::lines` and a pipe strip the marks. The TUI draws them with
   ratatui styles (`ui::put`; `ui::wrap` carries a mark across a cut). There is no `--plain` flag in this CLI, so none is honoured.
5. **Readers.** CLI (`ask`, `watch --interactive`): `Flight::event` words a call from its `tool.proposed`, drops the start line of
   a call already worded (a job's start becomes `started · pid N`), and puts the result behind a `⎿` gutter under the (kept) `←`
   end line; a question shows its preview in place of the JSON input line; a finished edit shows `⎿ tides.py  +1 −1` and its
   numbered diff. TUI: the card shows the preview (at most 12 rows), the pane shows the same lines as the CLI. Discord: an edit's
   approval card gains the hunk in a ```diff block (`render/preview.rs`, 800 characters, cut with a marker). Cockpit: `ConfirmCard`
   prefers the daemon's preview (`previewFor` in `lib/toolwords.ts`).

## How it was proved

Tests added (all pass):
- theseus-protocol: `diff::tests` x4 (numbered rows and word marks; a dashed removed line is not a header; the cap; a new
  file), `term::words::tests::a_call_is_said_in_words`; the generated TypeScript is rewritten and staged.
- theseus-tools: `change::tests` x2 (an edit's preview equals the diff its run makes, an unappliable edit has none; a patch and
  a write end with their diffs, a new file diffs against `/dev/null`).
- theseus-core `tests_tool_words.rs` x6: preview == the run's diff; the preview reads the file as it is when planned; a planted
  secret (`Scrubber::with_values`) in the edited file shows in no `confirm.requested` and no `tool.ended` while the diff is still
  there; writes end with their diffs; a 3,000-line write is cut at 16 KB with `truncated` and still counts 3,000 added; old
  peers decode without either field and re-encode without them.
- theseus (CLI): `diffview::tests` x3; `print::tests` x3 (tool line in words + diff on a pipe with no escape or mark; a terminal
  paints the removed line `\x1b[31m` with the marked word `\x1b[1;97;41m`; the colour rule); `flight::tests::a_start_without_its_proposal_is_worded_from_its_question`.
- theseus-tui `tests_diff.rs` x4 (the card shows words + the hunk; the changed word has its own background; a wrap keeps a
  marked word marked on both rows; the pane keeps the words, a failed command's last lines and the diff).
- theseus-discord `render/preview.rs` x2; Discord's own `summaries_pick_the_argument_a_person_looks_for` passes untouched.
- cockpit: `previewFor` test; `npm run lint` (only the repo's existing warnings), `npm run build` (tsc -b + vite), `npm test` 378/378.
- Goldens: `core_output.txt` gains the preview/diff keys; the CLI's six goldens only gain `⎿` lines (no existing line changed).
  `scripts/f10/test_f10.py`: 20 tests pass, no fixture re-recorded.

**Planted reverts** (each file restored and `touch`ed, `git status` clean after):
| plant | what failed |
|---|---|
| `scrub_diff` returns the meta unscrubbed | `tests_tool_words::a_planted_secret_shows_in_no_diff_and_no_preview` (the secret was in the `tool.ended` diff) |
| the preview body not scrubbed | the same test (the secret was in `confirm.requested`) |
| the preview diff made from the wrong state (`after`→`before`) | `change::tests::an_edits_preview_is_the_diff_its_run_makes`, `tests_tool_words::an_edits_question_previews_the_diff_its_run_makes`, `::a_preview_reads_the_file_as_it_is_when_planned` |
| the CLI ignoring `tool.proposed` (no words line) | `print::tests::a_tool_line_is_words_and_a_finished_edit_shows_its_diff`, `theseus-tui tests_diff::the_conversation_keeps_the_words_the_failure_and_the_diff` |

**FAST.** A plain turn keeps its frames: `theseus-sim bench turn --check --runs 5 --burst 0` passes (frames_plain 4/4, frames_tool 8/8,
frames_batch 14/14, syncs 2/2). `theseus-sim bench lifecycle --runs 10 --check` (debug build, this 4-core VM): LIFECYCLE OK in 52 s;
cold start p50 19.7 ms / p95 24.8 ms (budget 50 + 7), clean shutdown p95 17.4 ms (100), SIGKILL restart p95 24.6 ms (150), binary
swap p95 28.4 ms (200). A second run missed `health` after the seed by 0.2 ms (p95 5.2 ms vs 5 ms; the first run read 4.9 ms):
noise on this VM, a path this row does not touch. The diff is a field of an event and of the result node's meta, at most 16 KB, and
no frame was added.

## Live check on a scratch daemon (stand-in model, `$0`)

`/tmp/live/live.py` (kept out of the tree) starts the F10 arm's scratch daemon and stand-in, asks "fix the failing test" and
watches it. `theseus ask` (stderr, a pipe, no escape):

```
  ⟳ context recompile (new_session, transcript) · 1 message(s) · ~16026 tokens
  → Edit /tmp/live-werpbrbz/project/tides.py
  ? fs.edit needs your confirmation: edit /tmp/live-werpbrbz/project/tides.py: fs.edit — approve ([policy.tools] "fs.edit" = approve)
      /tmp/live-werpbrbz/project/tides.py:50  (+1 −1)
      @@ -47,7 +47,7 @@
        47       """How far the water moves over the table: highest less lowest, in metres."""
        48       highest = max(r.height_m for r in readings)
        49       lowest = min(r.height_m for r in readings)
        50 -     return round(lowest - highest, 2)
        50 +     return round(highest - lowest, 2)
        51   
        52   
        53   def format_height(height_m):
      approve: theseus confirm act_01a139725e387000a523026911888a8e
      decline: theseus confirm --decline act_01a139725e387000a523026911888a8e
```

`watch --interactive`, after `y` (the daemon had already proposed the call, so the line is worded from the question; before
`3467a73` it read `→ fs.edit`):

```
  ✓ approved act_01a139725e387000a523026911888a8e (by theseus watch)
── turn turn_01a139726e0877b0a9999aaa59ed6b82 (continuation)
  → fs.edit
  ← fs.edit ok · 1 ms · 452 B
  ⎿ /tmp/live-werpbrbz/project/tides.py  +1 −1
    @@ -47,7 +47,7 @@
      47       """How far the water moves over the table: highest less lowest, in metres."""
      48       highest = max(r.height_m for r in readings)
      49       lowest = min(r.height_m for r in readings)
      50 -     return round(lowest - highest, 2)
      50 +     return round(highest - lowest, 2)
      51
      52
      53   def format_height(height_m):
  ⟳ context append · 3 message(s) · ~13406 tokens
Fixed: the range is the highest less the lowest.
[f10 → anthropic/claude-sonnet-5-5 · 1 loop(s) · continuation · no_tool_calls · tokens in 13448 out 12 · $0.0535 total (
$0.0270 this reply) · 28 ms (first token 2 ms) · session ses_01a139725e127484a8ce64b718e90f9b]
```

On the same capture the terminal's removed/added lines are `ESC[31m … ESC[1m ESC[97m ESC[41mlowest ESC[0m ESC[31m …` and
`ESC[32m … ESC[1m ESC[97m ESC[42mhighest ESC[0m ESC[32m …`. The TUI's card and conversation (F10's S4 and S3 at 80x24):

```
⏸ confirm fs.edit: Edit /tmp/f10-theseus-80x24-ybvk6ilq/project/tides.py
   expires in 14:59
   why: edit /tmp/f10-theseus-80x24-ybvk6ilq/project/tides.py: fs.edit — approve
   ([policy.tools] "fs.edit" = approve)
       tides.py:50  (+1 −1)
       @@ -47,7 +47,7 @@
         47       """How far the water moves over the table: highest less
         lowest, in metres."""
         48       highest = max(r.height_m for r in readings)
         49       lowest = min(r.height_m for r in readings)
         50 -     return round(lowest - highest, 2)
         50 +     return round(highest - lowest, 2)
         51
         52
         53   def format_height(height_m):
   [y] approve  [n] decline
                                                             [?] help  [q] quit
...
← fs.edit ok · 1 ms · 491 B
   ⎿ tides.py  +1 −1
     @@ -47,7 +47,7 @@
       47       """How far the water moves over the table: highest less lowest,
 in metres."""
       48       highest = max(r.height_m for r in readings)
       49       lowest = min(r.height_m for r in readings)
       50 -     return round(lowest - highest, 2)
       50 +     return round(highest - lowest, 2)
       51
       52
       53   def format_height(height_m):
   
...
← proc.run error · exit 1 · 70 ms · 879 B
   ⎿   File "/tmp/f10-theseus-80x24-ybvk6ilq/project/test_tides.py", line 33, in
 test_tidal_range
         self.assertEqual(tides.tidal_range(self.readings), 2.8)
     AssertionError: -2.8 != 2.8
     ----------------------------------------------------------------------
     Ran 5 tests in 0.001s
     FAILED (failures=1)
     … all 18 lines
   
```

## F10 (debug build: `python3 scripts/f10/driver.py theseus --bin-dir <debug build> --out /tmp/f10b`, both sizes)

A debug build ran, not `release-thin` (this VM's disk allowance cannot hold a second profile's build), so the timing checks S1b and
B1 do not count (B1 still read 61 to 88 ms). **Match 17 of 42 at both sizes, Beat 5 of 5**; main's baseline is 11 of 42, and all
11 still pass. New: S2b, S3c, S4a, S4b, S4c, S4d. One run showed S6a yes at 80x24 (an elapsed time caught on a slow machine); the final run
has it no at both, so I count it as no.

| step | main (11) | this branch (17) | the checks now yes |
|---|---|---|---|
| S1 Open | 1/4 | 1/4 | d |
| S2 Ask | 0/4 | **1/4** | **b** |
| S3 Run the tests | 3/4 | **4/4** | a, b, **c**, d |
| S4 Fix, reviewed edit | 0/4 | **4/4** | **a, b, c, d** |
| S5 Don't ask again | 0/3 | 0/3 | |
| S6 90-second command | 2/4 | 2/4 | c, d |
| S7 Interrupt | 1/3 | 1/3 | c |
| S8 Paste | 2/3 | 2/3 | a, c |
| S9 Undo | 0/3 | 0/3 | |
| S10 Context and cost | 1/3 | 1/3 | b |
| S11 Leave and come back | 1/4 | 1/4 | a |
| S12 Plan first | 0/3 | 0/3 | |
| Beat | 5/5 | 5/5 | B1 to B5 |

Evidence lines from the run: S2b `→ Read …/project/README.md`; S3c `AssertionError: -2.8 != 2.8`; S4a `50 - return round(lowest - highest, 2) →
50 + return round(highest - lowest, 2)`; S4b removed `['38;5;1','38;5;15','48;5;1']`, added `['38;5;15','38;5;2','48;5;2']`; S4d `tides.py:50 (+1 −1)`.
No check, word, weight or scorer was changed; `arm_theseus.py` was not edited (the row names no step method).

## Defects found and fixed on the way

- A `watch --interactive` started after a call was proposed worded the approved edit as `→ fs.edit` (the proposal had gone by).
  The question and the start share a correlation id: `Flight` keeps the question's words (`3467a73`, with a test).
- `ts.rs`'s `declared()` demands TS on every `pub struct` of the sources it lists, so `diff.rs` (which also holds non-wire row types) is
  not in that list; `ToolDiff` and `ConfirmPreview` are in the export list, which generates their files.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0`, the VM's disk allowance filled twice): door, keel (0
findings), keel tests, fmt, shape, features, clippy, cockpit (lint, tests, build), test build, the reader rule: **pass**. The suite phase: 4,500
tests, 4,466 pass, 34 fail:
- 33 are the known L1 failures (theseus-sandbox's `contract` x22 and `bench spawn_100`, theseusd's `sandbox` x10; the VM runs as root, theseus-pv6i).
- 1 is outside the brief's lists: `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
  times out at 21 s, alone and under the gate. Its failure is exactly what theseus-wpv2g
  (`b495389`, joined on main after this branch's base) describes: the turn ends before the lane writes the process message, so the
  hold on "💭 thinking" never fires (the lab holds `-# 💭 thought for 1 s` as the first create). It does not touch tool lines, cards or
  any file this row changed. I could not run the base to prove it (the disk allowance holds one target), so this is the explanation, not a
  measurement.
Then the phases after the suite, run by hand: protocol types (`cockpit/src/protocol.gen` staged, no diff), `cargo deny --offline check`
(advisories, bans, licenses, sources ok), the turn bench (above); the lifecycle bench run once as the brief asks (above).

**Keel findings expected:** none. `scripts/keel-guard.py`: `0 findings acked` (it prints one `same test` pairing from the base, a store test
renamed before this branch).

## What is left, uncertain, and choices the owner should hear about

- **The `←` end line stays** (status, exit code, time, bytes), with the `⎿` gutter under it, so "exit code and time on the right of the
  gutter" is not literal: keeping it kept every existing golden line and the twin-naming test (`[sleep 2]`). Moving them is a render-only change in `diffview::ended_lines`.
- **A failed result shows its last 8 lines, not its first 3** (the brief says first lines): a failure's cause is at the end (the AssertionError is
  the last line of a unittest run), which S3c needs. The `… all N lines` line says the rest is cut (`+` when the 2,000-character preview was).
  `fs.read`/`fs.write`/`fs.edit`/`fs.patch` that succeed show no gutter text (their size is on the `←` line; an edit shows its diff).
- **Storage.** The diff rides the result node's `meta` (and the completion's `detail.meta`), so up to 16 KB of redacted diff is stored with an edit's
  result, and a question's preview is stored in the `tool.confirm_requested` ledger row (goldens show it). No format bump: `meta` and the row are free
  JSON and the new fields are optional. If the owner would rather not store them, the diff can move to a side channel read by `ToolEnded` only; say so.
- `confirm.list` recomputes the preview at read time (files as they are then), the live event's was made at the plan: they differ only if the file changed between.
- **Not done:** Discord's finished edit does not show its diff (the brief says "may"); chat's `render_seam.rs` does not exist on this base, so nothing points at `summarize`
  yet (`theseus_protocol::term::words::summarize` is the function); the TUI's session history (`session.history` nodes) still renders results as before, so the
  diff and gutter show for live events, not after a reload; a card's preview is cut to 12 rows (`… N more rows`).
- A job's start line reads `started · pid N` under the worded call; a start the TUI sees without the proposal (the question came by snapshot) still prints the old
  `→ proc.run [argv] pid N` (not a bare tool).
- The path in a diff is relative to the session's directory when under it, else absolute (the live check's session ran outside the project).

## Docs to change (for the maintainer)

- Spec Part III: a new item for theseus-z6au (what is above), the version line in `docs/spec/01-front.md`, and `docs/status.md` (the roadmap row).
- `docs/design/roadmap-v2.md`: the tool-words row done; F10's report (`docs/benchmarks/f10/`) can quote the table above after the maintainer re-records
  `scripts/f10/fixtures/` on a release-thin build.
- AGENTS.md notes were added in the six crates' guides in `81af885`.
