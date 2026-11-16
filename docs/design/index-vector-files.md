# The vector table in two mapped files (issue 21 of 1.0.7, step one)

_Notes for the index crate's vector side, written beside its `AGENTS.md`, which has the short form. The next step of the
issue (a hot set in memory, cold groups on disk) is built on this layout, so this note says what it holds, what it
leaves in memory and why, and where it has room._

## The problem

Every search by meaning scans a short code (256 int8 numbers) for each text ever stored, with a row of bookkeeping
beside each, and all of it sat in the index's own memory: the codes, a 64-byte row per chunk, each node's id and its
row list, and two lookups by text. About 500 bytes a chunk, growing with every chunk written, and a part of it as freed
memory the allocator could not give back. At 417,000 chunks that was about 340 MiB.

## What is built

Two files, both mapped read-only (`mapped::Map`, the weights' own wrapper; no dependency added), so the kernel pages
what a search touches and drops the rest when memory is short.

| File | Made | Holds | Per record |
|---|---|---|---|
| `<stamp key>.scan`, beside the `.vec` file | once per vector file, again at a fold | the vector file's records in columns: text hashes, scales, the int8 cuts, an open-addressed table of entries by hash | 16 + 4 + 256 bytes, and 4 for the table |
| the rows file, **no name** (`O_TMPFILE` in the index's directory) | at every start, as the index's nodes stream in | the nodes' ids and offsets, a 48-byte key a row (chunk, node, position, text hash, tokens, session, kind, external), and the rows' numbers sorted by text hash | 48 + 16 + 4 |

- **The scan file is a projection of the `.vec` file.** The `.vec` file stays the truth: whole records, the 768-d
  half-float vectors the re-score reads, a CRC each. The scan file is made from it with no re-embedding and is accepted
  only if it is that file's: its length, its stamp's dimensions, its entry count against the vector file's, and three
  records (the first, the middle, the last it covers) read back from the `.vec` and compared by hash and CRC. Anything
  else (absent, another stamp's, torn, made from a file since compacted) and it is made again. So an index of before
  opens and converts by itself, and a crash costs a rebuild, never an answer.
- **The rows file is rebuilt at each start**, because the rows are a function of the nodes the index holds and that is
  what a start reads. It has no name, so nothing can open, truncate or replace it by path, a crash leaves nothing
  behind, and its blocks go back when the last map goes. Where the directory's filesystem has no `O_TMPFILE`, a file is
  made and unlinked at once, which is the same for the reader.
- **Files are only written new and renamed into place, never in place.** A search's map is therefore of a file that does
  not change under it, so a map is never of a file that something truncates (which would fault the process). The table
  keeps the descriptor of each file it maps. A new scan file is written beside its name, synced and renamed; the rows
  file is swapped for a new one under the table's write lock, which no search holds.
- **Results do not change.** Row numbers run the file's first and the tail's after, in the order a start always gave
  them, so the scan, its ties and its re-score are as they were; the candidates and their scores are the same to the bit.
  `vectors/probe.rs` puts a fixed set of queries to an index, under five filters, and writes the answers.

## What stays in memory, and why

| What | Size | Why it is not in a file |
|---|---|---|
| a bit a row for "alive" | 1 bit | a forget or an overwrite changes it; a file is never written in place |
| the vector a row answers with | 4 bytes (the file among the table's caches in the top four bits, the entry in the rest) | it is found as vectors are embedded, after the file was written |
| a bit a record for "dead" | 1 bit | a text no chunk holds; the next compaction drops it |
| the **tail**: rows and records that arrived since the file was written | the rows and records themselves, at most 16,384 of either | a commit, an embedded batch |
| the table's names for sessions and kinds | small, by distinct names, not by chunks | a filter names them |

The tail is the only part that holds whole rows. At 16,384 rows or 16,384 records, or when half the rows are dead, it is
written into a new file (`Table::fold`, `Cache::fold_scan`) and the memory goes back. What is left that grows with the
store is a bit and four bytes a row, and a bit a record: about 1.7 MiB at 400,000 chunks, 3.4 MiB at 800,000. That is
the remainder a later step can remove for the cold part.

A table made with no directory (the older tests') keeps every row and record in memory, as before: that is the path
those tests still exercise. The model's unload lets the process's copy of the mapped pages go (`release_pages`); the
kernel keeps them.

## What it costs

Resident memory counts file pages a search has touched, and those are the kernel's: reclaimable, and dropped first when
the machine needs memory. The index's **own** (anonymous) memory is what no longer grows like the store.

Measured on a synthetic store of the owner's shape and on twice that (a box with a volume that reads 333 MB/s
sequentially; a real store on a laptop's disk will differ, and the figures are repeated on a copy of the real store):

| | 398,583 chunks | 796,584 chunks |
|---|---|---|
| own memory at rest, main's table | 221 MiB | 439 MiB |
| own memory at rest, mapped files | 14.1 MiB | 23.5 MiB |
| the same with `MALLOC_ARENA_MAX=1` | 7.6 MiB | 11.0 MiB |
| the index with no vector side at all | 9.3 MiB | 8.1 MiB |
| what the table says it holds | 3.2 MiB | 6.4 MiB |
| start, main's table / mapped files | 1.24 s / 0.85 s | 2.48 s / 1.71 s |
| search, warm, median of 60 mixed queries, main's / mapped | 58 / 62 ms | 135 / 146 ms |
| first search after the kernel dropped the files, main's / mapped | 0.16 s / 1.45 s | 0.28 s / 2.5 s |

What the growth is: the table's own bytes (a bit and four bytes a row, and the sessions' names) are 8 bytes a chunk.
The rest of the rise in own memory is freed memory that glibc's per-thread pool keeps; it goes away under
`MALLOC_ARENA_MAX=1`, and is the same kind of memory the index's third line of this issue was about. A warm search
costs a few percent more than a search over a table in the heap (the same scan, reached through the map). The first search
after the kernel dropped the files has to read them back (about 134 MB at the smaller size) and does so well below the
volume's sequential speed; main's table, in the heap, did not have this cost because the start had paid it.

## Where a later step has room

- **A version bump costs nothing.** Both files start with a magic (`thsscn01`, `thsrows1`) and a header with unused
  bytes after its last field (the scan file's header is 160 bytes and uses 136; the rows file's is 128 and uses 72).
  A scan file that is not the current version is made again from the `.vec` file, and the rows file is made again at
  every start, so a layout change is a new magic and a conversion that already runs. No migration is needed.
- **A per-record tier.** The scan file's columns are each addressed from the header by a computed offset
  (`scan::layout`). A column added after the entry table (a tier byte, a group number, a last-used time) changes the
  file's length, which is checked, and nothing else. The records are in the `.vec` file's order; a hot set is a list of
  entry numbers or a bit column, and cold groups can be a second column of entry numbers sorted by group, so a group is
  a run of the file a search reads and the rest is never touched.
- **Row numbers are a mapping, not an address.** The file's rows are `0..B` and the tail's follow. A step that orders
  the rows by group writes a new rows file in that order (a fold already writes every alive row into a new file in
  node-id order and renumbers; each keeps its vector); only the table's lookups by row number, which all go through
  `Table::row` and its field readers, see the numbering.
- **A fold is written under the table's write lock**, so searches and commits wait while it runs: the scan file's whole
  records (about 220 MB at 800,000 records) or the rows file's whole rows and their sort. It happens once in 16,384 new
  records or rows; not timed here. A step that makes folds more frequent or larger should write the new file outside the
  lock and swap it in under it, which the write-new-and-rename shape already allows.
- **The tail's size and the fold's trigger are constants** (`FOLD_ROWS`, `FOLD`), one place each.
- **What it does not do:** it does not bound the kernel's cached pages (a search touches every code, so they all become
  resident while it runs, and are reclaimable); it does not change the search, which is still exact. The hot set and the
  groups are what bound those.
