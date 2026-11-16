# The memory peak's record (issue 21, step one)

Several times a day the daemon's resident memory briefly rose to 1 to 2 GiB and fell back within a minute, and the
journal said nothing in those minutes. This note says what the daemon records now, so the next peak names its own
cause, and what the record found.

## What is recorded

- **A window of activity.** Every entry point of work feeds `resident::activity` a name, how long it took and, where
  the code knows it cheaply, how many bytes it returned or read. The window holds one aggregate per name (count, wall
  time in all and longest, bytes in all and largest), so a request the cockpit makes every 2 s is one entry. At most 512
  names a window; a name a client made up is cut at 64 bytes. It holds no content: method and pass names, counts,
  sizes and times.
- **The feeders.** `Core::handle` (every method served, with the size of its answer), `learning::tender::tend` (each
  nightly pass, by the name of the function that built it), the heartbeat and the driver (`harness`), the push's
  seed and its frames, the memory pass's sessions, the keep-warm's rounds, the self pipeline's ticks, the index
  tender's one request function (`tender::call`, with the size of the answer), and the AWS cross-check (below).
- **The trigger.** The memory tender (`resident::act`) takes the window at every trim's turn. When the resident set
  is over `resident::PEAK_FLOOR` (512 MiB) it writes one `resident.peak` ledger row and one log line; any other trim
  drops the window and writes nothing. The trigger is the resident set before the trim, not what the trim gave back:
  a peak the allocator could not return (the memory is live) is named too, with `rss_after_bytes` null where no trim
  ran.
- **Work still running** when the trim comes is listed as `running`, with how long it has run: it holds memory the
  trim cannot return.

## Reading a row

`theseus ledger --kind resident.peak`. A row's `ran` lists the longest-running entries first, then the largest of the
rest (24 and 16 at most; `left_out` counts the others):

```text
{"rss_before_bytes": ..., "rss_after_bytes": ..., "floor_bytes": 536870912,
 "since_ms": ..., "until_ms": ...,
 "ran": [{"kind": "job", "name": "aws.crosscheck", "count": 1, "ms": 9120, "max_ms": 9120},
         {"kind": "job", "name": "aws.crosscheck.tail/recall.ran", "count": 31000, "ms": 0, "max_ms": 0,
          "bytes": ..., "max_bytes": ...}, ...]}
```

`kind` is `rpc` (a method served), `job` (a background pass) or `index` (a request to the index tender). `bytes` is
what the work returned (an RPC's answer, the index's answer) or read (a pass's input); it is absent where the code
does not know it. An entry that took no time and has bytes is a part of a pass that was sized without being timed
(the cross-check's tail, by ledger kind).

## What cost what

The feeding costs one lock and one hash lookup per piece of work, and an RPC's answer is sized by a counting
serializer that allocates nothing (about as long as the answer takes to write to its connection). The AWS
cross-check's tail is sized only when the process is already over the floor, so a pass that is not a peak pays
nothing for it.

## What it found

Of the 65 peaks in the trim log from 2026-10-24 to 2026-11-15, 52 came 58 to 60 minutes after a daemon start (an
install), and the journal shows the others after restarts the install record does not list. The AWS CloudTrail
cross-check (`aws::tend::crosscheck_daily`) is the only pass that starts one hour after serving
(`aws::crosscheck::LAG`), and it reads the ledger's last 100,000 rows into memory, decoded, before it compares them
with CloudTrail's events. Its events are bounded (20 pages of 50); its ledger tail is bounded by a count of rows, not
by their size. The record now names it as `aws.crosscheck`, with the tail's weight by ledger kind.

The second step of the issue is its own change. What the check keeps of the tail is two sets of ids (every row's
`correlation_id`, and the `request_id`s of the `aws.called` rows: `Ledgered::of`), so the tail can be read a page at a
time and dropped, with only those ids kept.
