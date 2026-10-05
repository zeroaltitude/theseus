# CLOUD_REPORT: the incidental-recall bench (theseus-7gir.17)

Branch `cloud/20261005-bench-recall`, on top of `86d95501` ("cloud task (not for main)", over main's `80ef1dea`).
Started 08:35 UTC and reported at 09:49 UTC on 2026-10-05, well inside the 5-hour deadline. Everything is under
`bench/recall/`: Python, standard library only. No Rust, no other directory, and no dependency changed.

## Commits

| # | Commit | Step |
|---|---|---|
| 1 | `908d32b6` | The format: `progression.py`, `checks.py` |
| 2 | `2915f311` | The generator: `generate.py` |
| 3 | `bbda2334` | The drivers: `drive.py` |
| 4 | `b255a871` | The scorer: `score.py` |
| 4a | `48af0bed` | The window leaves room for Theseus's output cap and margin (found while proving step 3) |
| 4b | `2ac2ec3f` | Compactions read by ledger position; leftovers found by working directory |
| 4c | `30ec2842` | The checks take `v6.13.15`, "November 3rd", and more ways of not knowing |
| 4d | `027d9d73` | A turn past its timeout is killed with its children, and Theseus's is stopped |
| 5 | `b0397246` | `bench/recall/README.md` |

Each commit's suite ran on that commit's own tree, in a scratch `git worktree`:
`python3 -m unittest discover -s bench/recall`, with `THESEUS_RECALL_BIN_DIR` pointing at a copy of this
workspace's debug binaries. The counts were 15, 31, 35, 42, 43, 44, 45 and 47 tests, all OK. Step 5 adds no
test. One run of the whole suite under load also passed: 47 tests at `nice -n 19` beside four busy loops at nice 0
(Python loops, since the safety check refused an inline `sh -c` loop), taking 482 s.

## 1. The format (`progression.py`, `checks.py`)

**Found.** theseus-exam's families, checks and SplitMix64 suit the format, but its `exam-v2.toml` carries names
from a real history. None of them is used here.

**Changed** (`908d32b6`, `30ec2842`, and the window rule in `48af0bed`).
- **Sessions** have a label, a date and a weekday. **Turns** have the user's text, a block, a role, an estimate,
  the workspace edits made before them (`before`), and `mark`.
- **Facts** have a subject, a kind (port, path, version, host, ticket, date), a value, a salience, a family, a
  carrier (output, error, aside, remark, topic), a turn, a source, a delivery marker, and what supersedes them.
- **Probes** have a fact (none for an abstention), a kind, a planned bucket, a salience, an anchor (the fact the
  distance runs from: an abstention's distractor), a check, the indirect probe's file, and the stale value.
- `bucket_of` measures distance from any list of compaction turns. A compaction at turn m lies between a fact and
  its probe when f < m ≤ p, because turn m's request is the compacted one.
- `validate` refuses a fact probed twice, a probe before its fact, a value said outside its turn or inside a probe,
  an abstention's subject said anywhere, and a bucket the marks contradict.
- `checks.py` is check.rs's language in Python: strings, whole words, regexes, `file` subjects, `or`, and strict
  parsing.

**Proved.** `test_progression.py` has 16 tests: the check language as check.rs reads it, strict parsing, files,
buckets by actual compaction, the date and version forms, the abstention check, and the validator.

**Differences from the brief, and choices.**
- **Families.** Five are theseus-exam's, used for their kinds and checks. A sixth, `said`, covers the user's own
  words (an aside, a side remark, the topic). The exam's `preference` and `decision` would fit them, but the brief
  named five.
  - `needs_nothing` here is an abstention's subject (nothing to recall, nothing to invent). In the exam it is a
    task answerable with no memory at all.
  - `time` here is a dated event (the value is its session's date). The exam's `time` items carry stale values
    stated over 90 days.
- **The check language** uses Python's `re`, not Rust's `regex`; the kind regexes use look-arounds, which only
  Python's has. It has no `calls` subjects: the arms name their tools differently, so a probe is scored by its
  reply or by its file.
- **Stale is strict.** A supersession's check wants the old value absent, as the exam's superseded items do. So
  "it moved from 27340 to 38013" is wrong, and counted stale. The owner may prefer to accept history; that is a
  one-line change in `value_check`.

## 2. The generator (`generate.py`)

**Changed** (`2915f311`, `48af0bed`).
- SplitMix64 is ported from `rng.rs`. Topic blocks: a block's central facts are about its topic, and its incidental
  facts are about something else.
  - An incidental fact goes in a script's output, an error message, an aside, or a side remark by an invented
    teammate.
  - A script that showed a value is rewritten before the next turn (the log rotated, the error fixed), so the value
    can't be read again.
- Supersessions: a stale value said in passing in an earlier block of the same session, then the new value.
  Abstentions ask after a pair nobody says, beside a same-kind distractor that shares a word with it.
- **Smoke**: two sessions of 15 turns three days apart, one mark at turn 10, and 8 probes in 8 fixed cells
  covering every kind.
- **Full**: three sessions of 200 turns (two on one day, the third nine days later), 10 blocks each, a mark at turn
  120 of each, and **6 probes in each of the 34 cells: 204 probes and 228 facts**.
- It prints the budget at the catalog's price. For seed 7 at Sonnet 5.5 that is **$0.48 an arm for the smoke and
  $11.25 for the full** (this is an estimate).

**Proved.** `test_generate.py` has 17 tests:
- Vigna's reference stream, the smoke's pinned digest (`d30943acf7bfd1ac`), the full built twice and equal, and
  other seeds different.
- Every cell filled to 6, each fact before its probe and probed once (seeds 7 and 11, full and smoke), the shapes of
  both sizes, and each session opening with its date.
- Each tool-carried value gone from its script after its turn, the abstentions' distractors, and the supersessions'
  stale values.
- No name from any list, and no generated value, in `exam-v2.toml`.
- The price table matches `catalog.rs`'s rows (parsed from the Rust), and the window matches the budget rule.

**The window.** The brief's 32000 was too tight. Theseus lets a request hold the window less the output cap and
4,096 tokens (`request_budget`, `compiler.rs:883`).
- At 32000 with the driver's 8000 cap, that leaves 19,904 tokens. The smoke's turns before the mark plus about
  12,000 tokens of system prompt and tools need about 18,500. A run 7% heavier than the guess would compact before
  the mark.
- Against the stand-in model, a 20000 window refused every turn: "its request is estimated at 12,060 tokens …
  against the 10,904 the window leaves after the output cap".
- The generator now takes the smallest window whose budget holds each session's pre-mark turns with 15% to spare,
  and makes each mark's bulk read cross it by 15% more. That gives **35000 for the smoke and 124000 for the full**.
- The driver caps output at a quarter of the window, at most 16000 (`progression.output_cap`). That overrides the
  bench profile's "the model's whole output cap", for a scratch window only.
- In the smoke, the turns after the mark may not fit beside the kept bulk read and a summary, so an arm may compact
  twice. The scorer measures by where it did.

**Uncertain.** The per-turn token guess (650) and the 12,000-token overhead are guesses; the overhead matched the
compiler's 12,042-token estimate seen in the offline runs. Tuning the full size against a live run is left.

## 3. The drivers (`drive.py`)

**Changed** (`bbda2334`, `2ac2ec3f`, `027d9d73`).
- **Theseus**: a scratch `theseusd`, with `theseus-index` beside it, configured as `daemon.rs`'s `config_for` does.
  - That is the bench profile with the run's lines, `[memory] mode = "live"` with `arm` (any string), Discord, the
    web UI and the MCP server off, and the index on except for `none`. The brief said "the index on"; I followed
    daemon.rs, which turns it off for `none`.
  - One `sessions open` per session, and each turn `theseus --json ask -s <id> -`.
  - Compactions come from `ledger -k context.compacted` row positions after each turn, since `ledger.tail` returns
    1000 rows at most.
  - At the end it saves `history --full` and `memory recalled`, stops the daemon cleanly, and fails if any process
    names the run directory or works inside it.
  - A turn past its timeout has its process group killed, and then `theseus stop <session>`.
- **Claude Code**: `claude -p --output-format json`, a scratch `CLAUDE_CONFIG_DIR`, and the workspace as its
  working directory.
  - `--session-id <uuid>` at each boundary, then `--resume` with the id the result carries.
  - `--tools` and `--allowedTools` set to Bash, Read, Write, Edit, Glob and Grep, with `--permission-prompts none`.
  - Compaction: `--autocompact <window>k` at 100k or more. Below that, `/compact` through `-p --resume` after each
    mark's turn. A compaction counts only where its session log has a `compact_boundary`.
  - Its memory files are kept. The parent session's variables (`CLAUDECODE`, …) are taken out of its environment.
- **Delivery**: each fact's marker is looked for in the transcript.

**Proved.** `test_drive.py` has 7 tests:
- Against a stand-in `claude` on PATH: one call per turn, `--session-id` at each session's first turn and
  `--resume` with that id after, two different ids, `/compact` sent once right after the mark, the compaction
  recorded at mark + 1, every fact delivered, and the parent's `CLAUDECODE` taken out. A perfect stand-in scores
  100%.
- The Theseus driver end to end on this workspace's binaries and `theseus-sim fake-model --rules`: 30 turns, all
  exit 0, every fact delivered, and nothing left running or killed. It scores 100%, and is skipped without the
  binaries. It takes about 6 s.
- A turn past an 8 s timeout (a `sleep 30` job) is recorded as timed out, the next turn answers, and nothing is
  left.
- The config round-trips through TOML for `none`, `bm25`, `baseline` and `+synthesis`. Leftovers are found by
  command line and by working directory, and a timeout kills the whole process group.

**Checked by hand, with the real `claude` (2.1.289) against `theseus-sim fake-model`** (`ANTHROPIC_BASE_URL`):
- The driver's flags all parse and run. The session keeps its `--session-id` across `--resume`, and the logs land
  in `projects/<cwd with non-alphanumerics as ->/`, where the driver reads them.
- **Print mode runs `/compact`**: the log gains `{"type":"system","subtype":"compact_boundary"}`, the session id
  stays, and the result is empty with `num_turns` 0.
- `--autocompact 145k` is accepted, and `50k` refused ("between 100k and 1M").
- The whole smoke through the real claude delivered 9 of 9 facts and recorded the compaction at turn 11 from the
  real boundary.
- A first try whose `/compact` failed against the stand-in ("summarization produced empty response") recorded no
  compaction, as it should.
- Some turns run to their timeout against the stand-in: it re-asks a tool call after some of Claude Code's
  requests, and the session logs show the same call repeated. It decides by a request's last message alone. That is
  why this is a check by hand, not a test.

**Planted reverts.** Each was restored and `touch`ed, and `git status` was clean after.
- **`theseus stop` after a timeout removed.**
  `test_a_turn_past_its_timeout_is_stopped_and_the_next_one_runs` failed: the next turn timed out too
  (`[29, 30] != [29]`).

**Left, and uncertain.**
- Offline, Theseus never compacts. The stand-in reports 40 to 60 input tokens a call, and Theseus's estimate counts
  from the provider's numbers, so only one huge exchange could cross, and that is an overage, not a compaction. The
  Theseus compaction path is proved only by the scorer's tests and the ledger read. The live check must show it.
- The live check is also where Claude Code's `usage` in `-p` JSON will show whether it is the whole turn's
  (assumed).

## 4. The scorer (`score.py`)

**Changed** (`b255a871`, and the carrier and value-kind breakdowns in it).
- Each probe is checked by its own check: an indirect probe by its file, never the arm's words. Undelivered and
  failed probes are excluded, and counted.
- The bucket comes from `run.json`'s compactions. Turns and tokens since the fact use each turn's input, cache
  writes and output.
- The curve covers each salience × bucket, with abstention separately. The half-life comes in turns and tokens,
  linear between the buckets around the crossing, with "not reached" and "undefined".
- It also reports confident-wrong, stale, cites where and when, $ and ms per probe, accuracy by kind, carrier and
  value kind, and how many probes each arm's compactions moved.
- Output: `report.md`, `curve.svg` (drawn by hand) and `scores.json`. Runs of different progressions are refused.

**Proved.** `test_score.py` has 7 tests, against numbers worked by hand:
- A known curve: near 4/4 at 2 turns, topic shift 3/4 at 10, compaction 1/4 at 30, session 0/2 at 100. Its
  half-life is 10 + (0.75 − 0.5)/(0.75 − 0.25) × 20 = **20 turns**, and 3000 tokens. Also checked: never halving,
  starting at nothing, exactly half, and unordered points.
- The smoke with a reply set by hand:
  - recall 3/5 and abstention 1/2;
  - confident-wrong on p001 (the stale value) and p007 (a wrong path), but not p003, which is hedged;
  - stale 1/1, and cites where and when for both right direct answers;
  - p002 right by its file although its words give a wrong ticket, and p008 excluded as undelivered;
  - p005's fact at turn 5 and probe at 12: 7 turns and 120 tokens since.
- An invented value is refused as an abstention. A failed turn is excluded and counted.
- **Distance where the arm compacted**: p005, planned `compaction` past the mark at turn 10, is `topic_shift` when
  the arm never compacted, `compaction` at [12], and `topic_shift` at [13]. p007, planned `near`, moves to
  `compaction` at [22].
- The CLI writes the report, the SVG and the scores.

**Planted reverts.** Each was restored and `touch`ed, and `git status` was clean after.
- **An invented value scored as an abstention** (`kind_check` without its `lacks` line). These failed:
  `test_the_abstention_check_wants_no_value_and_an_admission`, `test_an_invented_value_is_not_an_abstention`,
  `test_each_probe_is_scored_as_worked_by_hand`, `test_a_failed_turn_is_excluded_and_counted`, and, through the
  digest, `test_the_smokes_digest_is_pinned` and the CLI test.
- **Distance from the marks, not the arm's compactions** (`score_probe` using `prog.marks()`):
  `test_distance_is_where_the_arm_compacted_not_the_marks` failed.
- **A fact probed twice** (the validator's check removed): `test_a_fact_probed_twice_is_refused` failed.
- **`random` in place of SplitMix64** (`Rng.below` through `random.Random`): `test_the_smokes_digest_is_pinned`,
  `test_the_generator_draws_from_splitmix64_alone` and the CLI test failed, and with them every test that builds a
  full progression. Under `random`'s draws the full no longer fit its cells: "no room".

## 5. The README (`b0397246`)

It covers the format, the commands, the sizes and their budgets, each arm's memory (the window rule, Claude Code's
compaction, OpenClaw's room), what dated text can't measure (both arms also see the real date, and a retention
memory sees minutes, not days), delivery, the run directory, each score, and the tests.

**Docs the maintainer should change** (I didn't touch them):
- `bench/README.md`: add a line or table row linking `recall/README.md`, "the incidental-recall bench: what an agent
  remembers from its own work, Theseus against Claude Code".
- `docs/benchmarks.md`: a section for its results once the full has run.
- Part III and `docs/status.md`: the usual record.

## The live check (the maintainer's)

Run it in a throwaway container, with `ANTHROPIC_API_KEY` set, Claude Code installed, and a release build of
`theseus`, `theseusd` and `theseus-index` in `target/release`.

1. `python3 bench/recall/generate.py --seed 7 --size smoke --out /tmp/rc-smoke` should show:
   - digest `d30943acf7bfd1ac`; 30 turns (bulk 1, fact 9, filler 10, opener 2, probe 8); a mark at turn 10;
     window 35000;
   - 9 facts; 8 probes (direct 4, indirect 2, abstention 2); about $0.48 an arm.
2. `python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir target/release --model
   anthropic/claude-sonnet-5-5 --progression /tmp/rc-smoke --out /tmp/rc-th` should end with "ran 30 turns; 9 of 9
   facts delivered; compacted at turns [10] (or 11 …); left running: nothing" and exit 0.
   - Look at `/tmp/rc-th/run.json`'s `compaction_rows` (outcome `compaction`, not `ring`) and `turns.jsonl`'s exits.
   - An undelivered fact means the model didn't run its script. Its probe is excluded, which is not a bug, but say
     how many.
   - Then repeat with `--memory-arm none` and `--memory-arm bm25` (one run each, to fresh `--out`s).
   - `pgrep -f /tmp/rc-th` should print nothing.
3. Then `--arm claude-code --out /tmp/rc-cc` should show 30 turns and `cc_compact: "marks"`. Its `compact_calls`
   should have one entry after turn 10 with an empty result, and `compactions` should be `[11]`.
4. `python3 bench/recall/score.py /tmp/rc-th /tmp/rc-cc --out /tmp/rc-report`: each arm's line, then `report.md`
   (the curve, half-lives, abstention, supersession, carriers) and `curve.svg`.
5. With both smokes clean, do the same with `--size full` (204 probes, window 124000, about $11 an arm; Claude Code
   then uses `--autocompact 124k`). Budget about 3 hours per arm, at 600 turns.

## Left, and design choices for the owner

- **The full size isn't tuned against a live run.** The window, the bulk reads and the per-turn guess come from
  estimates. If Theseus compacts far from the marks, the scorer still measures correctly, but cells may thin out.
  The report's "moved" count shows it.
- **Claude Code's window semantics differ.** `--autocompact` sets its own threshold, and its system prompt is not
  Theseus's. On the smoke, `/compact` forces it at the mark instead. So the two arms' compactions land differently,
  and that is why buckets are measured per arm.
- **Abstention is strict:** no value of the kind at all, plus an admission. "I don't know, but 6.13.15 is the other
  pin" fails.
- **Indirect probes can be answered from the workspace** when an arm wrote the fact down itself, in notes or memory
  files. That counts as the arm's memory working. The run keeps Claude Code's memory files so it can be seen.
- **OpenClaw**: a driver is a class with `drive()` writing the same run directory; the scorer reads any arm's.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran twice: before the first commit (`/tmp/gate1.log`)
and before the last code commit, the README (`/tmp/gate2.log`).
- Each time fmt, shape, features, clippy, cockpit, test build and the reader rule passed.
- The suite failed on the same 33 tests both times, all the known L1-as-root ones (theseus-pv6i):
  - theseus-sandbox's `bench spawn_100` and its `contract` clauses;
  - theseusd's 13 `sandbox` tests (`a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace` …
    `unlisted_and_private_hosts_are_refused_with_their_reasons_and_hold_nothing`).
- 2562 tests ran each time: 2529 passed and 19 were skipped. The first run had one flaky pass on its retry,
  theseus-sim's `the_kernel_holds_its_invariants_under_seeded_faults` (theseus-81ig, on the flaky list). No timing
  test from the brief's list failed, and the kernel's `the_deadline_stops_the_whole_tree_too` passed.
- I ran the phases after the suite myself, and they passed:
  - protocol types: `cockpit/src/protocol.gen` unchanged;
  - the turn bench: `theseus-sim bench turn --check --runs 5 --burst 0` gave frames 5/5 and 9/9, ok;
  - `cargo deny --offline check`: advisories, bans, licenses and sources ok (the database was fetched at setup);
  - lifecycle and jobs are skipped under `THESEUS_GATE_NO_BENCH`.

So the commits count green. Nothing the gate builds changed: only Python and Markdown under `bench/recall/`.
