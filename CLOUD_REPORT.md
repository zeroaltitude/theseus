# CLOUD_REPORT: p3f-recall-now (theseus-mo1vm)

Branch `cloud/20261025-p3f-recall-now`. Commits: `b885ee44` (early push, empty), `9200f7b8` (the row),
`693b013a` (threshold fix found by M2), and this report.

## 1. For the maintainer

- **Store format: 51, untouched.** `RecallItem.superseded` is `#[serde(skip)]` (and `ts(skip)`): never stored, no
  TypeScript change; the mark lives in the frozen header string of the `Recall` node, which already exists.
- **No ceiling raised.** `temporal.rs` is exactly 560 (I trimmed doc comments to make room), `tender.rs` exactly 969.
  `scripts/shape.sh`: ok. The new code is in two new files, `recall/current.rs` and `recall/tests_current.rs`.
- No OWNER CALL, no STOP.
- **The gate was NOT run whole.** I ran the pieces listed in section 6 and not `scripts/gate.sh` (the VM ran out of disk
  twice; see 6). Please run it.
- **The acceptance is only partly met by M2** (section 4): gold@1 improved on 1 of 8 time items, and the other seven are
  limited by things this row does not touch. Read section 4 before merging.

## 2. The read

`temporal::read` (`recall/temporal.rs`, the end of `read`) used to return `None` for any `CURRENT` word. It now runs the
event reader as before; if that finds an event kind AND a `now` word it still returns `None` (unchanged); if it finds no
event kind it hands the text to `current::read` (`recall/current.rs`, `read`). `Kind::Current` ("current") is new.

`current::read` returns `Current` when the text has a `now / currently / nowadays / anymore` word, or `still`, and:
- the question asks with `what / which / where / who / whats` in its first four words, or `how many / how much`;
- and it is not an activity question (`doing, happening, going, working, think, should, shall, could, would, can, am,
  will`, or "what is/are we/I/you"), not the present-tense "what time/day/date/year does/do …" (the existing table in
  `tests_time.rs` keeps "What time does the nightly backup run now?" as `None`; this means the exam's own `superseded-1`
  shape stays unread, see follow-ups), and not "is it".
- `still` alone (no `now`): `Current` only with a digit in the question or an ask word. "Do we still keep 30 days?" is
  `Current`; "Do we still use the old runner?", "Is the cache still enabled?", "Why is the build still slow?" are `None`.

## 3. Ranking and marks

- **Statement** (`current::statements`): a hit that shares at least half (rounded up, min 1) of the question's content
  words (stop words and `now` words out, light plural stem) and holds a number (a date is not one).
- **Order** (`current::order`, called from `temporal::pack` for `Kind::Current`, so the place rule and the filters run
  first as for the other kinds): statements first, then everything else in relevance order. Among statements: a
  `supersedes` link between two of them wins over time (a statement some other statement supersedes, by any chain, goes
  after it: sort key `(depth, newest time first)`); with no link, newest `time_ms` first. Links come from the asker
  (`asker.links`), which recall fills only when the science `prefers_newer`; otherwise time decides. The exam's probe has
  no links.
- **Mark** (`current::marks`, `with_mark`): a statement whose numbers do not include all of the newest statement's gets
  `, superseded: older than the <YYYY-MM-DD> statement` at the end of its frozen header (the day is the newest
  statement's, UTC, `render::utc_day`). The newest, a statement repeating the newest's values, and every non-statement
  get none. Example header, before: `a message in #x, said … (as of @7)`; after, for the 14-day note when a 21-day note
  is newest: `a message in #x, said … (as of @7), superseded: older than the 2026-09-04 statement`.
- A pack with no `Current` question is byte for byte as before (`with_mark` returns the header untouched; tested).
- A live rerank (`Memory::refill`) rebuilds the items and does not carry the marks (no `begun` there). Follow-up.

## 4. M2 (the gold probe, model-free)

How it ran: `theseus-exam write-store`, a scratch `theseusd` (own config, `DUMMY_KEY` env secrets, Discord/web off) on a
copy of the store at `<state>/store`, then `theseus-exam probe --arm bm25,entity --items --k {6,40}
--half {in,out}`, before = no `--temporal` (identical to the base for every time item, since the base read returned
`None` for all eight: each contains "now"), after = `--temporal` on this branch. **Only the BM25+entity arm ran: no
embedding model in this session, so no vector or fused arms.** k=6 is the brief's; k=40 is `recall::K`, the hits a real
recall pulls before it packs 6, which is the realistic one: at k=6 the newest statement is almost never among the 6 hits
the index returns, so no reordering can move it. Gold of every time item is its NEWEST statement (time-4 is the one
with two gold, b.1 and c.1), and the probe counts exactly that, so no extra gold@1-newest read was needed; the table's
rank is the newest statement's rank (`-` = not in the k hits).

| item | k=6 before | k=6 after | k=40 before | k=40 after |
|---|---|---|---|---|
| time-1 (in) | - | - | 29 | **1** |
| time-2 (in) | - | - | - | - |
| time-3 (in) | - | - | - | - |
| time-4 (in, gold b.1 / c.1) | 2 / - | 2 / - | 2 / - | 10 / - |
| time-5 (out) | - | - | - | - |
| time-6 (out) | - | - | - | - |
| time-7 (out) | - | - | 23 | 23 |
| time-8 (out) | - | - | 39 | 38 |

(The `in` half was used to choose the one rule that changed, the 60% to 50% word share; `out` was read after, once
per k, with the final rule. Before that rule change the `out` numbers were the same as the final ones.)

What the table says, plainly:
- time-1 is fixed (29 to 1): the newest statement says "cut WAL retention to 21 days", sharing 2 of 4 question words;
  the first version of the rule (60%) missed it, the probe showed it, the rule moved to half. That is a rule chosen on
  `in` data (time-1 is an `in` item); no `out` item changed because of it.
- time-2, 3, 5, 6: the newest statement is not among the 40 BM25+entity hits at all (its words differ from the
  question's: "Thursdays at 16:30", "bring the alert down to $35 a day", "hold 13 snapshots", "goes to 9291"). A reorder
  cannot recover it; that needs the vector arm or better retrieval.
- time-4: the correction is relative ("five more minutes") with no number; the newest statement is not a numeric
  statement, so it is not found. The older gold b.1 got worse (2 to 10) because newer chatter that repeats `600` outranks
  it as a statement.
- time-7, 8: the values are text and paths, not numbers; this rule reads numbers only. I did not widen it to quoted
  strings or paths: that would be fitting the exam (rule before data).
- Other families, same probe, `--temporal` vs none at k=40, all halves: the diffs in `episode-1`, `superseded-2`,
  `private-2`, `distractor-3` are not from this row, except `superseded-2`: `private-2` and `distractor-3` are "When
  is …" questions, which the existing 10-14 reader (q1tbn) reorders; `episode-1` likewise. `superseded-2` ("What's the
  per-session spend limit now?") IS now read as `Current`: gold ranks go from (1, 2) to (3, 1): b.1, the newest, is rank
  1; a.1 is rank 3, behind a.2 (a one-minute-newer note stating the same value). Both gold stay in the top 3.

So: **M2 gold@6 on the time items does not improve** at k=6 (the newest is not retrieved). At k=40 (the pack's pool)
the newest statement moves to rank 1 on 1 of the 4 `in` items and to 1 of the 4 `out` items that were retrieved at all
(time-8, 39 to 38, not a real gain). The acceptance ("improves") is met for the one item the rule can reach and not
beyond it. I would not expect M8 to move much on this row alone.

## 5. Tests and planted reverts

New `recall/tests_current.rs`, 9 tests: reads (the nine "now" shapes in invented wording, the ten that must not read as
`Current` incl. "What are we doing now?", "When did we first …", the `still` cases and guards), the subject without `now`,
newest-first with the other subject after, a `supersedes` edge beating time, marks (older marked, newest not, another
subject never), the header mark and the unmarked pack untouched, and the probe's order equal to recall's. Existing
`tests_time.rs` is unchanged and passes. All 9 are red at the base (the module does not exist) and green after.

Planted reverts (each: plant, `nextest run -E 'test(/recall::tests_current/)'`, restore from a copy, `touch`):
- **P1** (the reader hands nothing to `current::read`, i.e. the old early `None`): 3 red:
  `a_value_asked_about_now_reads_as_current`, `still_reads_as_current_only_when_it_asks_about_a_value`,
  `the_subject_leaves_out_the_now_word`.
- **P2** (drop the newest-first time order from the sort): 2 red:
  `the_newest_statement_of_the_subject_ranks_first_and_others_follow`, `older_values_are_marked_and_the_newest_is_not`.
- **P3** (`with_mark` returns the header unmarked): 1 red: `the_mark_rides_the_header_and_an_unmarked_one_is_untouched`.

All three ran against the 60% rule version of `current.rs`; the 50% change (`693b013a`) only touches the word share
and was re-tested after (the 9 new tests and the 98 recall tests). I did not re-plant after that commit.
Not tested: the `hold_node` hunk itself (it is one call to `with_mark`) in a full turn, and the load runs (the brief
asked for none for this row). I did not run a live turn against a scratch daemon with a model (no key).

## 6. Gate: what I ran

- `cargo fmt --check`: ok.
- `cargo clippy -p theseus-core -p theseus-exam -p theseus-protocol --all-targets -- -D warnings`: clean (one
  `too_many_lines` on `manifest_ranked` along the way, fixed by moving the work into `current::mark`).
- `cargo nextest run -p theseus-core -E 'test(/recall::|tests_recall/)'`: 98 passed (the 9 new among them).
- An earlier broad run of theseus-core (about 4,000 of 4,786 tests, cut short by my own timeout): no failure.
- `cargo nextest run -p theseus-exam -p theseus-protocol`: 280 passed, 1 skipped. The protocol's generated TypeScript:
  `git status` shows no change under `cockpit/`.
- `scripts/shape.sh`: ok.
- NOT run: `scripts/gate.sh` whole (its suite, the theseusd tests, deny). The disk filled twice (`target/` reached
  28 GB on a 252 GB device with ~30 GB free); I deleted build artifacts under `target/` (incremental caches and large
  test binaries) and built with `CARGO_INCREMENTAL=0`. The known failures on this VM (L1, the golden's TZ, hohs/1n2y)
  were not exercised.

## 7. Keel

`python3 scripts/keel-guard.py --base b69236c1 --head HEAD --quiet`: no output.

## 8. DM golden

`git diff --exit-code b69236c1 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no difference.

## 9. Join notes (with `p3a-recall-floor`)

Files: `recall/current.rs` (new), `recall/tests_current.rs` (new), `recall/temporal.rs`, `recall.rs`,
`turn/recall_step.rs`, `theseus-protocol/src/memory.rs`, `theseus-exam/src/tender.rs`, `theseus-core/AGENTS.md`.
Hunks I own (line numbers at my head):
- `recall/render.rs`: **untouched.** The mark is in the frozen header, written in `hold_node`.
- `turn/recall_step.rs`: one line, 566: `header = crate::recall::current::with_mark(header, item);`, placed after the persona
  mark and before `items.push`. It does not rewrite any `header` line. p3a's `neighbour::with_note(header, item)` goes
  near the `moved` lookup (~553-556); the two lines do not overlap, and the order of the two suffix/prefix edits is
  free (mine appends at the end, p3a's note position is theirs).
- `recall.rs`: `pub mod current;` (35); `#[cfg(test)] mod tests_current;` (~46); in `manifest_ranked`: 861
  `let sci = &*scene.science;`, 862-865 the `pack` match (arm now `temporal::pack(k, &begun.said, sci, …)` and the
  `None` arm uses `sci`), 868 `current::mark(&mut m.admitted, begun, &links, &candidates);` right after
  `fill(&mut m, pack, …)` (p3a adds its `neighbour::apply(...)` after each `fill(...)` too, ~861 and ~897 in its
  numbering: keep both lines); in `fill`'s item literal, line 1031 `superseded: None,` BEFORE `retention: None`.
  `refill` is untouched.
- `theseus-protocol/src/memory.rs`: `RecallItem.superseded` (518-523), **before `retention`**, `#[serde(skip)]`; `fill`
  sets it (to `None`; `current::mark` sets it after). p3a's `neighbour` goes at the end; no overlap.
- `recall/temporal.rs`: `cut` is `pub(super)`; `Kind::Current` and its `as_str`; `read` (the `now` flag and the final
  `kind` match); `by_kind` gets `Kind::Current => tier`; `pack(kind, asked, …)` takes the question and calls
  `current::order` for `Current`. Doc comments trimmed to stay at the 560 ceiling.
- `theseus-exam/src/tender.rs`: `temporal_ranks(kind, q, hits)` and one call, via `current::order_of`; the import line.
- `theseus-core/AGENTS.md` lines 2152-2163: one new paragraph "Current-value questions", inserted before the
  `memory.lookup` bullet, nothing existing rewritten.
- Docs to change at the merge (I did not touch them): Part III item for this row; `docs/status.md`; the exam README/design
  note should say the probe's `--temporal` now orders a `Current` question and that `--k 40` is the realistic k.

## 10. What M8 should run

The GLM exam on the `time` family, arms none / bm25 / baseline, with this build in the arms' daemons, and (to see the
retrieval limit above) the same family under the vector arm's fused order once the embedding model is available.

## 11. Follow-ups

1. Retrieval, not order, is the limit for 5 of 8 items: the newest statement is not among the 40 hits. Vector/fused arms
   may change this; the probe's `--arm` with vectors was not available here.
2. Non-numeric values (time-7 a quoted status line, time-8 a path) and relative corrections (time-4) are not
   "statements" to this rule. Widening needs a rule chosen before the data.
3. `refill` (the live rerank) drops the marks.
4. The present-tense "what time does X run now?" stays unread because `tests_time.rs` pins it as `None`; that is a
   current-value question too (the exam's `superseded-1`). Changing it means editing a pinned test: the owner's call.
5. Statements are matched by word overlap plus a number: another subject's note sharing half the words and holding a
   number would be ranked as a statement (newest first). It was bounded by the hits being the subject's retrieval.

## 12. Data floor

Nothing new is sent to a model or routed anywhere; only the order and a short mark on already-recalled text change.
