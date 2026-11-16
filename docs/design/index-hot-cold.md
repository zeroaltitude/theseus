# The index's hot set and cold groups (issue 21 of 1.0.7, step two)

_Notes for the index crate's vector side, written beside `index-vector-files.md`, which says how the table lives in mapped
files. This note says how a search chooses what to read from them, and what keeps the answer good._

## The problem

After step one the index's **own** memory no longer grew with the store, but every search by meaning still scanned every
record, so (a) the pages a search touched, which count as resident while they are cached, grew with the store, (b) the first
search after the kernel dropped the files cost 1.5 s at today's size and 2.8 s at twice, against a recall deadline of
250 ms, and (c) search time grew in step. The owner asked for the indexer to have "recent, frequently used, and important
stuff always at hand, and … indexes for things that can be very fast lookups".

## What is built

| Part | Where | What it does |
|---|---|---|
| the budget | `[index] resident_mb` (128), `layout.rs` | the index's resident memory at rest, the model unloaded, whatever the store's size; what is left after the index's own parts (52 MiB, measured: the table's bits and pointers, the signals, the pages of the rows a search touches, freed heap, the word index) is the hot region (a cut, a scale and an entry number: 264 bytes a record); refused below 68 |
| the score | `heat.rs` | recency by the node's **own** time (an import of old sessions is old; the half-life is 30 days), use (`memory.used`), importance (the gate's durability), the operator's labels (useful and should-have are hot, wrong and stale never) |
| the signals | `heat.rs`, the tender | read off the log's ledger rows as the tender follows it, kept in `heat.log` beside the vectors; a signal only changes what is read first, never an answer |
| the layout | `tier.rs`, `<stamp key>.tier` | copies of the int8 cuts in region order: the hot region, then groups of similar records, each group together |
| the groups | `groups.rs` | k-means over the cuts, about the square root of the cold count of groups, from a sample of at most 30,000 records, eight rounds |
| the search | `layout.rs` | the whole hot region, every record no layout places (new text is hot until placed), the promoted records, and the nearest eighth of the groups; then the same 768-d re-score as before |
| the pass | `layout.rs` | scores, chooses, groups, writes the file, checks it, puts it in place |

**While everything fits, nothing changes.** At today's size and the default budget every record is hot: no tier file is made,
and a search is the exact scan it always was (a test holds that).

## Why a derived file beside the scan file, and not a reordering of it

Entry numbers are the vector file's order throughout (the 768-d re-score reads record `e` at `header + e * rec`, a compaction
keeps order, the scan file is checked against the vector file by index). Reordering the scan file for the groups would have
changed all of that, and rewritten the 1,816-byte records for every move. The tier file is instead a **projection of the scan
file** with no truth of its own: a copy of the cuts in the order a search wants them, validated at open against the scan file
(its length and columns, a checksum of its order column, the greatest entry it places and that entry's text hash, every entry
placed once). Anything doubtful and it is removed and the search is exact. A compaction that drops records renumbers them, so
`Cache::apply` removes the tier file with the records, and the table drops its copy of it: **a forgotten record's cut is in no
copy**. It costs 264 bytes a record on disk (the cuts again); the page cache holds only what a search read.

## What a search reads, and how it stays right

A search by a turn's recall (the query's `fast` flag) reads the hot region, every record the layout does not place, the
promoted ones, and the nearest eighth of the groups. Each record names rows by its text; each alive row is filtered exactly as
the exact scan filters it. So:

- a forgotten or erased node's rows are dead and no region can return them, before any file is rewritten;
- a record is scanned once (the hot region, the unplaced entries and the groups are disjoint; a promoted record is skipped in
  its group);
- scores and ties are the exact scan's: a record's rows score as its cut does, and the best `n` rows are chosen by the same
  total order;
- Wrong and Stale labels are still applied by the core after the index answers, as before.

**Explicit searches are exact.** The memory search tool, the CLI and the neighbours lookup never ask for `fast`, and scan
everything, hot and cold. After a search under a layout the pages it touched outside the hot region are let go
(`madvise(DONTNEED)`: the kernel keeps them cached, the process stops counting them).

## Moving between hot and cold

- **Promotion**: a cold record whose node a turn used or the operator vouched for is searched with the hot region at once (an
  in-memory list of at most 2,048 entries, found again from the signals after a restart), and the next layout makes it hot.
- **Demotion and re-grouping are the layout pass**: it re-chooses the hot set from every record's score (the lowest go to their
  nearest group) and makes the groups again. It runs on the embedding thread when the machine is quiet, and is due when the
  records embedded since pass 5% of the layout (16,384 at least), the cold part has moved a fifth from what the groups were
  made of, the promotion list is full, or the store is back under the budget.
- **A move holds no query**: the scores are read in slices of 32,768 rows under separate read locks; the file is written
  outside the table's lock; the swap takes the write lock for a pointer. The embedding work already waits for queries.
- **A crash**: the file is written beside its name, synced and renamed, never in place; a torn or stale one fails its checks at
  the next open and is removed.

## The proof, and the fallback

Before a layout is put in place it is replayed against the exact scan on up to 64 **real recall queries**: the index
remembers the vectors of the last 256 fast queries it served, each with the position it was asked as of and the sessions
it left out, and replays each as it was asked (so it never sees its own message, stored after it). Until it has served
32, it is replayed instead on up to 64 of its own cold records' vectors, spread through the groups, each leaving its own
node out so that a record cannot find itself;
that is a kinder test than a turn's words, so its bar is 97% and not 95%. The layout is written as `<key>.tier.cand` and gets
its name only once it has passed. Under the bar of the exact best ten it is not put in place: searches stay exact, and
`index.status`'s `vectors.tier` (with `checked_on`) and the log say so (and why). A layout nothing could check is not put in
place either, and is tried again a minute later.
A layout that failed is tried again after 24 hours or when the store has grown by a tenth. A bad grouping costs time, never
results.

`theseus-index replay --index <a private copy> --queries <file>` puts a file of query vectors (one a line: 768 numbers, a JSON
array, or `{"vector": [...], "as_of": N, "exclude_sessions": [...]}`) to the exact scan and the tiered search over a copy of an
index, under `--resident-mb` or `--hot-records`, and prints the shares of the exact best 6 and best 10 kept, overall and for the
results in the cold part, and both searches' times. It writes the layout into the copy: never the index of a running tender.

## What it does not do

- It does not bound the lexical index or the word segments (about 10 to 22 MiB, in the fixed part).
- While a stamp is being re-embedded (older vectors still answer) searches are exact.
- A forget or a compaction removes the layout; the search is exact until the next pass (minutes, when the machine is quiet).
- Standing records, threads and the board's entries are not signals the index can read from the log; they would arrive as
  further `Signal`s (`vectors/heat.rs`) from the core.

## Figures

The measure on a synthetic store (`vectors/measure.rs`: vectors drawn round a few hundred centres, since random vectors have no
similar groups) is in the issue's runner report; the real check is `theseus-index replay` on a copy of the real store and the
last 30 days of recalls.

## The layout pass at five times the store, and what the budget holds (measured, synthetic)

A layout is a pass over every record: it scores them, trains the groups' centres on a sample (30,000 records, 8 rounds),
puts every cold record in its nearest group in one pass in file order, writes the tier file and checks it. It runs on the
embedding thread's own pool (so it has no more threads than that thread is given) and holds no query: a search while one
runs is as fast as an idle one. On a store of 1,995,572 records (five times today's), a budget of 128 and 32 cores it takes
about 20 s; on four threads with the files' pages dropped first, about 26 s. Its cost is records times groups times 256 (a
dot product of int8 cuts for each pair), which is why `dot_i8` takes sixteen products a step where the machine has AVX2.
`a_layout_of_five_times_the_store_finishes_in_time` (ignored; the test's own comment says how to run it) fails over two minutes.

What the budget counts at rest and after searches, vector side only (the word index's 10 to 22 MiB is added by hand):
290,730 records (today's size; all of it hot): 10.3 MiB anonymous + 92.3 file-backed; 1,995,572 records at 128 MiB (349,525
hot at a fixed part of 40, since changed to 52): 19.7 + 100.9. The pass gives back what it used (the heap is trimmed after it:
81 MiB of anonymous memory remained at five times the store before, 18 after). The table's own anonymous memory grows with
the store (about 8 bytes a chunk text), so "whatever the store's size" is bounded by that: at five times today's it is about
20 MiB, and the fixed part is sized for the largest of each part measured, not for a store of any size.

