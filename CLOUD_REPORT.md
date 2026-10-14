# CLOUD_REPORT: long-session-fast (theseus-0l3g)

Branch `cloud/20261013-long-session-fast`, built on `e9ea344` (store format 32, unchanged: no stored record changed;
no bump). Started 04:12 UTC, report at 08:20 UTC. A 4-core VM, debug builds throughout and one release-thin build at
the end (its numbers are marked).

## Step 1: what grows, with numbers

`compiler/phases.rs` times each compile's phases, and `turn/compile_step.rs` the step around it, at debug level:
`THESEUS_LOG=info,theseus_core::compiler::phases=debug`. The ring logs its renders (`ring: cut i of n renders=k`).
`bench turn --session-nodes` now takes a list (`2000,5000,10000`, a fresh daemon each).

Found:
- **The ring's candidate search** was the quadratic part, as Oct 8 said: from the earliest cut on, each candidate a
  whole render and estimate. On 2,400 exchanges (9,600 nodes) the pure compile alone did not finish inside nextest's
  120 s (the planted revert below). It now renders **3 candidates** at 10,000 nodes (`ring: cut 1859 of 2000 ...
  renders=3`), 83 ms of the first compile in debug.
- **The first compile of the synthetic session decodes the whole transcript once** (8,002 of 10,001): the synthetic
  session has no stored compilation, so the first compile is `new_session` over everything, and its first render and
  the clearing pass (which runs first: at 2,000 nodes clearing alone fits the session, and no ring happens) read every
  node. That is linear and stays. A session with a stored compilation (every real one) decodes only what lies past
  the stored cut (the test below).
- **The rest of the first turn** at 10,000 nodes: the clearing pass (850 ms debug: it decodes the shown nodes and
  renders once), the first render (290 ms), and the compaction's summary call over the 9,000 dropped nodes (about
  1.4 s debug, its request built from the dropped range: linear, and a model call).
- **Per turn** (the soak, below): a turn's first reader re-read the session's whole record range from the store each
  turn (`Store::scan_kept`), the render re-rendered every node, the request was copied whole to take its digest and
  again to send it (`ProviderRequest::body`). Still O(history) per turn after this branch: the estimate's census, the
  digest and the send's serialization (now without the copy), the kept render's id comparison and clone, recall's
  `in_context` set (every node id of the session, a `BTreeSet<String>`, each turn), and in debug builds only the
  transcript's check against a whole read of the store (`Store::transcript`'s `debug_assertions` block), which is
  most of the debug `transcript` phase (26 ms at 776 nodes).
- **RSS**: the soak's RSS growth is the allocator holding freed memory, not kept memory. After 300 turns the daemon,
  left quiet 12 s (its `resident` tender trims), was at 117.6 MB; at turn 100 it had been at 117.9 MB. The sampled
  ~200 kB a turn is the high-water of each turn's temporaries (a few copies of a request that grows with the session)
  between trims, which never come while turns run back to back (the tender trims after `QUIET`, 10 s, or at most
  `MAX_DEFER`, 60 s).

## Step 2: a cold first compile in linear time

- `compiler/ring/sums.rs` (new) and `compiler/ring.rs`: the cut before a user message is **guessed** from the
  segments' census sums (the nodes from one cut to the next, each rendered alone once, walked from the last cut back
  until the sum passes the target), then **settled** by rendering the guess and its neighbour whole (`sums::settle`):
  the earliest cut whose own render's estimate is under the target, the rule the walk over every cut applied, as long
  as a later cut never estimates more than an earlier one. The ring's rules are unchanged: it cuts only before a user
  message, rings on the upper bound, keeps turns under 60 %, the last cut when none is under and the range holds more
  than one message, and the in-turn search (pinned instruction, by halves) is untouched.
- Decodes: the walk renders only the segments it passes, so after a start the ring decodes what lies past the stored
  cut plus a segment, never the session.
- The stored compilation's own append (`strategy` ring, `includes`) already rendered only its prefix and tail; with the
  ring fixed, nothing else in the compile reads the whole transcript's bodies.

## Steps 3 and 4: an incremental compile, flat per turn

- `compiler/render.rs` (moved out of `compiler.rs`, behaviour unchanged): the render is a state (`State`) nodes are
  rendered into in order (`State::item`), `render_messages` its whole run.
- `compiler/kept.rs` (new): each session's render state kept across loops, bounded by `[context] kept_mb` (64 MiB; 0
  none; `config/context.rs`, documented in the template) with least-recently-used eviction across sessions. An append
  takes up the kept state and renders only the nodes written since, when: the key matches (compilation id, its strip,
  provider, model, fallback model, how images and PDFs show, the refused images, and the caller's view: the old
  attachments a cold rewrite stubbed, from `compile_step`); the kept node ids are the first of this render's, the
  prefix the same length; it was closed when kept (no repair, no cut answer open, nothing retried); and no new node is
  a result for, or a call by, an id it holds a result for. Else a whole render, kept. The compilation id keys it, so a
  ring cut, a compaction, a clearing pass, a recompile or a system/tools digest change (each a new compilation) start
  it over; the recall cap pauses recall, which only changes what new nodes come. A Recall node renders by position
  from frozen sources and is the same node by id, so the kept state reads it as before.
- `CompileInput.kept` (new field): the turn's compile passes the runner's `Kept`; everything else passes `None`.
- `transcripts.rs` (new) and `Store::scan_kept`: each session's transcript kept across turns as **cold** stubs
  (`Stub::cold`: fields and position, no bytes, no node, so it pins nothing past the heat cache), bounded at 256
  sessions and 400,000 stubs, LRU. A turn's first read takes them and scans only the session's records after the last
  position it read. Debug builds' existing check (kept against a whole read of the store, at every turn) ran through
  the whole core suite with it.
- `provider/digest.rs` (new): `ProviderRequest::digest` and the body the provider sends are serialized from the
  request with no copy of it (`body()` cloned every message to hash it, and again to send it), keys sorted as before:
  the stored digest pin (`request_body_digest_is_stable...`) still holds, and a new test holds both to `body()`.
- `bench turn --soak-turns N` (`theseus-sim/src/perf/soak.rs`): one session, the stand-in model, every fifth turn
  with a call, CPU (`utime`+`stime`, which keeps ended threads' time) and RSS every 50 turns, the CPU ratio against
  the window ending at turn 100 and the RSS growth a turn after it; then 12 s quiet and RSS again. `--check` holds
  1.5x and 10 kB a turn. With `--dir`, each sample's turn result and health go there.

## Commits

- `a3b97e8` store: a session's transcript kept across turns as cold stubs (`transcripts.rs`, `Stub::cold`, `Store::scan_kept`).
- `dc59104` provider: the digest and the body sent written from the request with no copy (`provider/digest.rs`).
- `2719136` compiler: the ring's guess by segment sums (`compiler/ring/sums.rs`), the render as a state
  (`compiler/render.rs`) kept across loops (`compiler/kept.rs`, `[context] kept_mb`), the phases' debug log
  (`compiler/phases.rs`), compiler.rs's tests moved to `compiler/tests.rs`, its long-files entry deleted, the core
  guide's lines.
- `d8da03b` sim: `bench turn --soak-turns`, `--session-nodes` as a list, `--first-turn-ms`, the sim guide's lines.
- `b93d41a` store: the kept read's first read goes through `transcript_records` (a release build warned it dead).

The four sub-steps were gated together, not one by one (the gate takes about 30 minutes here from a warm build, and a
full rebuild after the disk filled); each was checked with its own tests and clippy before its commit.

## How it was proved

- `compiler/tests_kept.rs::a_kept_render_makes_every_request_a_whole_render_makes` (proptest, 64 cases a run, up to
  60 steps each, windows from 4,000 to 1,000,000 tokens): tool loops, late results, answers cut at the window and
  replaced, a call answered after a later node (repair, then the real result), recall notes, a person's message
  inside a running turn, summaries, operator recompiles, clearing and ring: at every step the request's body, digest,
  estimate, repairs, prefix and tail counts, budget report and decision are equal with the kept render and without.
- `compiler/tests_kept.rs::appends_take_up_the_kept_render`: 300 appends, (299 taken up, 1 whole).
- `compiler/tests_ring_sums.rs::the_rings_guess_picks_the_cut_the_walk_over_every_cut_picked` (proptest, 48 cases):
  sessions of 3 to 40 exchanges with thinking, several calls, late results and unanswered messages, windows 6,000 to
  30,000: the ring's cut equals the earliest cut whose own whole render is under the target (else the last), computed
  in the test by rendering every candidate.
- `compiler/tests_ring_sums.rs::a_long_sessions_ring_after_a_start_decodes_past_its_cut_not_the_session`: 2,400
  exchanges, a stored ring compilation over the first 2,000, stubs read from records (decodes counted), then an
  append that rings: decoded at most the nodes past the stored cut (+20).
- `transcripts::tests::a_later_turn_reads_only_what_was_written_since`: the second turn's read scans 3 records, not 8.
- `provider::digest::tests::the_digest_is_the_bodys_without_copying_it`.
- `compiler/tests_ring.rs` unchanged and green (6 tests), and every moved `compiler::tests` test green (the keel guard
  matches all 16 as moves).
- **Planted reverts** (each restored and `touch`ed, `git status` checked after):
  - step 2: `ring.rs` back to the walk over every cut: `a_long_sessions_ring_after_a_start...` **timed out at
    nextest's 120 s** (the pure compile on 9,600 nodes); the guess property test and `tests_ring` still passed (they
    test the rule, which the walk also keeps).
  - the kept render (`kept::render` always whole): `appends_take_up_the_kept_render` failed, `(0, 0)` against
    `(299, 1)`; the byte-identity test passed (as it should: a whole render is the reference).
  - the kept transcript (`scan_kept` always whole): `a_later_turn_reads_only...` failed, 13 records scanned against 8.
- The core suite (`cargo nextest run -p theseus-core`, before the last two small changes): 2,086 of 2,087 passed; the
  one failure, `tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends` (the
  crate's `tests_kept`, outputs, not mine), read `got[0]` as the 3,000-line file's window: it ran while cargo was
  building beside it, and passed 3 of 3 runs alone. It looks like the three parallel reads' results landing in
  completion order; worth a look, not this change's.
- Targeted after the last changes: 96 tests (compiler, transcripts, provider, store, soak, tiering, one-turn,
  continue, the frame budget) passed; `cargo clippy -p theseus-core -p theseus-sim --all-targets -D warnings` clean.

### Numbers, release-thin, this VM (4 cores)

Built here at the end (`cargo build --profile release-thin -p theseusd -p theseus-sim -p theseus-index`):

`theseus-sim bench turn --theseusd <release-thin theseusd> --session-nodes 2000,10000,20000 --runs 5 --burst 0
--first-turn-ms 1000` (wall ms):

| Nodes | First turn after the start | Later p50 | Owner's machine before (Oct 12), first turn |
|---|---|---|---|
| 2,000 | 116 | 68 | 114 |
| 10,000 | 584 | 93 | 21,761 |
| 20,000 | 971 | 123 | 96,745 |

Twice the nodes now costs 1.7 times the first turn (it was 4.4 times), and 10,000 and 20,000 both pass the 1 s line,
the compaction's summary call on the stand-in included. Nodes decoded: the first turn 8,004 (the synthetic session
has no stored compilation, step 1), later turns 2.

`theseus-sim bench turn --theseusd <release-thin theseusd> --soak-turns 1650` (CPU ms a turn per 50-turn window; RSS MB):

| Turns | 50 | 100 | 300 | 500 | 800 | 1,000 | 1,300 | 1,650 |
|---|---|---|---|---|---|---|---|---|
| CPU | 20.6 | 21.3 | 30.2 | 39.1 | 52.1 | 59.6 | 71.8 | 83.7 |
| RSS | 65.5 | 69.2 | 92.1 | 118.7 | 154.8 | 167.5 | 174.0 | 202.6 |

After 12 s quiet: 118.6 MB. **Against the targets: CPU 3.9x the turn-100 window (target 1.5x; the owner's soak before
was 17 to about 280 ms, 16x), and RSS 88 kB a turn as sampled (target 10), about 30 kB a turn kept after the trim
(69 MB at turn 100 to 118.6 MB quiet at 1,650).** Not met. What still grows a turn is a handful of passes over the
whole request, each linear in its bytes: the estimate's census and `json_bytes`, the digest's serialization, the
send's serialization, `signals`' census of the tail (in the soak the compilation never recompiles, so its tail is
the whole session), the kept render's clone into the request, and recall's `in_context` set. A sample of the release
daemon could not name them (release-thin is stripped). The next step is a request built from kept per-message bytes
and census (each message's JSON and census kept beside the kept render, the request's body, digest and estimate
then made from them), which is a change of `ProviderRequest`'s shape; I did not start it this late.

### Numbers, debug build, this VM

`theseus-sim bench turn --session-nodes 2000,5000,10000 --runs 5 --burst 0` (wall ms, the first turn after the
start, then the later turns' p50):

| Nodes | First turn, ring fix only | Later p50, ring fix only | First turn, this branch | Later p50, this branch |
|---|---|---|---|---|
| 2,000 | 606 | 442 | 614 | 320 |
| 5,000 | n/a | n/a | 1,279 | 481 |
| 10,000 | 3,477 | 1,418 | 3,490 | 754 |

The old ring was not run here at 10,000 in a daemon: from the owner's release numbers (21.8 s at 10,000) a debug run
would take minutes; its pure compile on 9,600 nodes passed 120 s in the planted revert. First turns now grow about
linearly (5.7x for 5x the nodes). The first turn's remaining time is the clearing pass, the first render, and the
compaction's summary call.

The debug/release ratio: the owner's release later-turn p50 at 10,000 (104 ms) against this VM's debug p50 before the
kept render (1,418 ms) is 13.6x; `--first-turn-ms 12000` (12x the 1 s target) is the debug bound, which this branch
passes at 3.5 s and the walk over every cut fails.

`theseus-sim bench turn --soak-turns 300` (CPU ms a turn per window; RSS MB):

| Turns | CPU, ring fix only | CPU, this branch | RSS, this branch |
|---|---|---|---|
| 50 | 95.2 | 87.5 | 112.7 |
| 100 | 127.4 | 118.9 | 117.9 |
| 200 | 200.2 | 182.0 | 135.8 |
| 300 | 248.8 | 245.7 | 161.7 (117.6 after 12 s quiet) |

**The soak's targets are not met in debug on this VM**: the CPU ratio is 2.07x (target 1.5x) and the sampled RSS growth
224 kB a turn (target 10), though kept memory is flat (above). In debug the per-turn growth that remains is mostly
recall (its span: 56 ms at turn 50, 133 ms at turn 300; `in_context` and the pipeline over a session whose every
candidate is in context), the compile (31 to 72 ms: the debug-only whole-store check in `transcript`, the census, the
digest and the id comparison, all O(request)), and the continuation (5 to 26 ms, the turn's first transcript read with
its debug check). A release build drops the debug check and runs the byte passes at memcpy speed; the maintainer's
release soak is the number that counts.
Sampled with gdb at turns 250 to 400 (30 samples of every thread), half the daemon's busy stacks (15 of 31) were in
that debug-only check: `Store::transcript`'s `debug_assertions` block reading every node record of the session again,
one `pread` a record. The rest: JSON serialization (the census's `json_len`, the digest), the compile, the memory
pass.

## Live checks for the maintainer

```
scripts/build.sh --profile release-thin
target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --session-nodes 10000,20000 --runs 5 --burst 0 --first-turn-ms 1000
target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --soak-turns 1650 --dir /tmp/soak --check
THESEUS_LOG=info,theseus_core::compiler::phases=debug target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --session-nodes 10000 --runs 3 --burst 0 --dir /tmp/long && grep -a 'ring:\|compile' /tmp/long/theseusd.log | head
target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --runs 10 --check
```
Expect (as here, release-thin on 4 cores: 584 and 971 ms): the first turn at 10,000 and 20,000 nodes under 1 s, the
compaction's summary call on the stand-in included, and 20,000 under twice 10,000; the log's `ring: ... renders=3`; the soak's windows
and its line after 12 s quiet (kept memory), and whether release meets 1.5x (unknown here); the turn bench's frames 4
and 8 as before (no frame was added: every cache here is in memory).

## Left, uncertain, and for the owner

- **Not met**: the soak's 1.5x CPU and 10 kB RSS, in debug and in release-thin (3.9x and 88 kB a turn sampled, about
  30 kB a turn kept, at 1,650 turns). What remains per turn is listed in step 1; the next
  candidates are recall's `in_context` (a set of every node id each turn: could be kept with the transcript), the
  estimate's census over the kept part (kept per message beside the render), and the digest's serialization (kept
  per message as bytes). The RSS target as sampled measures held heap; the soak's 12 s quiet line is kept memory, and
  is flat. Telling the owner: a `malloc_trim` after each turn would make the sampled line flat, at a syscall a turn.
- **A whole-transcript recompile still decodes and renders the whole session once**: a new session (the bench's
  synthetic one), an operator's `transcript` recompile, and a `system_changed` or `tools_changed` recompile (every
  install that changes the tools) start from the whole visible transcript, render it, and clear or ring from there.
  Linear, but at 10,000 nodes it is most of the first turn. A shortcut that skips the whole render when the sums show
  it plainly past the window would change what the clearing pass sees, so I left it.
- **The ring's rule assumes a later cut never estimates more than an earlier one.** Census sums make that so, except
  where images and PDFs are budgeted in render order (`attach::Spend`): dropping an earlier PDF can let a later one
  render whole. There the settled cut is the earliest under the target near the guess, not necessarily the first in
  the session. The property test has no media.
- **The ring's `BudgetDrop.tokens`** and everything else in the report are as before (the settled cut is rendered
  whole, as before).
- `compiler.rs` is 1,109 lines now (its tests moved to `compiler/tests.rs`, the render to `compiler/render.rs`), so its
  `scripts/long-files.txt` entry is deleted, as the shape check asks.
- Siblings: thread-context will find `system_blocks` and `render_request` where they were; the render's loop moved to
  `compiler/render.rs` (`State::item`). request-census and deferred-tools change the head: the kept render keys on the
  compilation, whose manifest's digests make a head change a new compilation. recall-time and recall-precision: a
  Recall node is still rendered by `recall::render::render(items, sources)` inside `State::item`.
- Docs to write at review: the spec's Part III item and status.md's line for theseus-0l3g; the numbers above as the
  debug row; `docs/benchmarks/` for the maintainer's release runs. `crates/theseus-core/AGENTS.md` (Tiering) and
  `crates/theseus-sim/AGENTS.md` (bench turn) are updated in the branch.

## Keel findings expected

None. `python3 scripts/keel-guard.py`: `keel: ok (... 0 findings acked)`; the 16 tests moved from `compiler.rs` to
`compiler/tests.rs` are matched as moves. One `#[expect(clippy::too_many_lines)]` went away with `render_messages`'
loop (split into `State::item`); none was added. The `long-files.txt` change deletes an entry (a file now far under
2,500 lines), which the shape check requires.

## The gate

The final gate, on `b93d41a` (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, with
`CARGO_INCREMENTAL=0` and `CARGO_PROFILE_{DEV,TEST}_DEBUG=line-tables-only`: the full-debug-info test build filled
this VM's disk once, at 28 GB of `target/debug/deps`): door, keel (0 findings), keel tests, fmt, shape, features,
clippy, cockpit, test build, reader rule passed; **suite: 4,388 run, 4,354 passed, 34 failed**:
- 33 are the known L1 tests (20 in theseus-sandbox, `spawn_100` among them, and 13 in theseusd's `sandbox`): a root
  daemon's job has no job cgroup (theseus-pv6i).
- `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`: `security.v2 took 509.5ms`, a
  wall-clock bound in a crate this branch does not touch, under the suite's load; it passed alone 3 of 3. Not on the
  known list: worth adding to it, or a looser clock.

The phases after the suite, run by hand: protocol types (no change in `cockpit/src/protocol.gen`: no protocol type
changed), the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`: frames 4, 8 and 14, syncs 2, the first
request 38.6 KB, all ok), and deny (`cargo deny fetch` then `cargo deny --offline check`: advisories, bans, licenses,
sources ok). The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.

The gate before (on the same tree less `b93d41a`) failed the suite the same way, plus
`theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends` (also once in
the core suite run beside a build): it reads results by WAL position (`session_nodes`, a whole read, not the kept
transcript) and asserts the first is the 600-line file's, while the three `fs_read`s of one response run together
and land in completion order. It passed 5 of 5 alone. Not this branch's; worth a look.
