# What laying results kept by reference reads, per turn (2026-11-25)

**The answer first.** Laying a look back kept by reference reads **three store records per quote** when the view is
frozen, and **one more per quote at every call** of the turn. A window of 40 look backs of 50 quotes each (2,000
quotes, far more than a turn lays) took **6,000 reads and 12.6 ms (median; 14.8 ms the first time)** to lay on a release build and
**2,000 reads and 3.9 ms** to check at each call. A small window (10 look backs of 10 quotes) took **300 reads and
0.64 ms**, and **100 reads and 0.19 ms** a call. The cost grows in a straight line with the number of quotes.

## The question

A result kept by reference (issue 49) stores no words: each reader renders its parts from the records they quote.
Issue 49 never measured what that costs a turn. This measures the reads and the wall time of the two places a turn
pays for them:

- **the window's lay** (`quoted::lay_window`, when the view is frozen): for each result, a check that each quoted node
  is not erased (its newest record), then each quote read (its node's newest record and its record by position);
- **each call's check** (`quoted::erased_since`, from `view_gone`): the newest record of each node the lay read.

A turn's own results kept by reference are rendered once, by the same rendering (`quoted::freeze`), so they cost
what the window's do, once each. A child's opening (`arrangement::laid`) was not measured.

## Setup

- `crates/theseus-core/src/quoted/tests_cost.rs`, `the_read_cost_of_laying_by_reference` (`#[ignore]`). A store in a
  temp dir, nothing else opened. 2,000 messages that no look back quotes, then each look back's messages.
- Each look back is a page of `q` lines, one quote of a different message each, in a window of `n` look backs. No
  message is quoted twice and nothing is marked wrong, so no retraction rows are read.
- Counted: the newest records the erase check reads (a test-only count in `quoted::erased`) and the quotes the
  renderer reads (through `quotes::render_with`, the renderer's own loop). Store reads = erase checks + 2 × quotes.
- Timed: the first lay, and the median of 20 lays and 20 checks, on one `c7i.8xlarge` fleet box (Intel Xeon Platinum
  8488C, 32 vCPU). The `dev` profile is the test build (this crate unoptimized, its dependencies at `opt-level` 2).
  The `release` profile is `opt-level` 3 without fat LTO, close to an install's build.

Run it again with:

```
cargo nextest run -p theseus-core --cargo-profile release --run-ignored only --no-capture \
  -E 'test(the_read_cost_of_laying_by_reference)'
```

## Results

| Look backs × quotes | Erase checks | Quotes read | Store reads, lay | Reads, each call | Lay, release: median (first) | Call, release | Lay, dev | Call, dev |
|---|---|---|---|---|---|---|---|---|
| 1 × 10 | 10 | 10 | 30 | 10 | 62 µs (205 µs) | 19 µs | 196 µs | 52 µs |
| 1 × 50 | 50 | 50 | 150 | 50 | 311 µs (494 µs) | 96 µs | 970 µs | 256 µs |
| 10 × 10 | 100 | 100 | 300 | 100 | 642 µs (852 µs) | 190 µs | 1.99 ms | 517 µs |
| 10 × 50 | 500 | 500 | 1,500 | 500 | 3.14 ms (3.74 ms) | 951 µs | 9.83 ms | 2.61 ms |
| 40 × 10 | 400 | 400 | 1,200 | 400 | 2.57 ms (3.03 ms) | 762 µs | 8.03 ms | 2.08 ms |
| 40 × 50 | 2,000 | 2,000 | 6,000 | 2,000 | 12.6 ms (14.8 ms) | 3.92 ms | 39.9 ms | 10.7 ms |

The counts are the same on every run and on both profiles.

## What it teaches

- **The erase check reads one newest record per quote, not per quoted node.** Here every quote is of a different
  message, so the two are the same count. But by the code, `quoted::rendering` checks each quote's node before it makes
  the list distinct, so a look back that quotes one message many times reads its record as many times. Making the list
  distinct first would read each node once. This is not changed here.
- **The call's check reads one record per node the lay read, at every call.** The list is distinct per result but
  not across results, so a node two look backs both quote is checked twice a call.
- **Each record read is a lookup and a decode through the store's node cache.** The first lay pays more than the
  median (the cache is cold); later lays and checks hit it.
- **For a turn as it is today it is small.** A window holds a few look backs, each of a few dozen lines. That is
  hundreds of reads and about a millisecond a lay or less on a release build.

## Threats to validity

- A synthetic store: the records are small, there is no other load, and the whole store fits in memory. On a large
  store a cold read costs a disk read the cache does not hide.
- The release build here has no fat LTO and 16 codegen units; an install's has fat LTO and one.
- One machine, one run of 20 lays each.
