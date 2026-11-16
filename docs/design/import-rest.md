# Why the daemon rested higher after the nightly import (issue 21)

Read from the installed daemon's own log: idle overnight it rested at 50 to 92 MiB. The nightly `theseus import sync`
read 565 sessions (8,440 messages), 555 of them new, then ran its people pass. In the first seconds the daemon went from
80 to 327 MiB, fell to 232 within a minute, and then rested at 180 to 186 MiB for the next three hours with nothing
running. The minute's trim gave back nothing. Two readings fit that: data the daemon still held, or free memory in
pieces too small to hand back. This note says which it was, how that was measured, and what changed.

## The measurement

`bench/memory/rss_import.py` starts a scratch daemon on a copy of a synthetic store (`theseus-sim synth-memory --nodes
266000`, with the people-and-topics board on top: `theseus-sim synth-store --sessions 0 --categories 414 --memberships
71178`), makes a synthetic OpenClaw state directory of the nightly size (565 sessions, about 8,300 messages of invented
text, one in eight naming a person), and runs the real `theseus import sync` against it, with the stand-in model for the
people pass. It reads the daemon's anonymous and file-backed memory from `/proc/<pid>/smaps_rollup` every 50 ms, and the
rest again 20, 80 and 150 s after the import, after the daemon's own trim, with the service's `MALLOC_ARENA_MAX=2`.

To say whose bytes the live ones are, `bench/memory/malloc_prof.c` is a sampling heap profiler loaded by `LD_PRELOAD`
(a sample about every 4 KiB allocated, each kept with its call stack until it is freed; a profile is written when a
trigger file appears), and `bench/memory/heap_owners.py` names the stacks. It also writes glibc's `malloc_info`, which
says how much the allocator holds free. What it cannot see: memory the process maps itself, bytes allocated before it
loaded, and the pieces of free memory too small to matter (that is what `malloc_info` is for). The box has no package
manager open to it, so heaptrack and jemalloc's profiler were not available; the shim stands in for them.

## What it found

On the synthetic store, the daemon after the import rested 46 to 66 MiB above where it began (message median 3,300 and
6,000 bytes). Health's own count said why: its `node cache` read 0 MiB before and 44.4 MiB after. The heap profile
agreed: live bytes grew by 51 MiB, and 40 MiB of that was `Node` decoded from JSON by `NodeCache::node`, called from
`Store::session_nodes`, called from the import's people step (`import::people::assign_unless`). **It was live data, not
stranded free memory**: `malloc_info` at rest held about 15 MiB free of 83, and almost none of it in pieces under 4 KiB.

The heat cache keeps every node it decodes, up to `[memory] node_cache_mb` (64) of record bytes, so that a turn which
comes back to a node finds it decoded. A pass that reads every node of 565 sessions once keeps them all, and no turn
ever reads them. A decoded node holds more than its record's bytes, so the cache's 64 MiB bound is more than 64 MiB of
the daemon. The cache never shrinks while the daemon runs, and its bound is why the rise saturates and a second night
does not add another 100 MiB.

## What changed

`Store::session_nodes_once` reads a session's nodes as `session_nodes` does and keeps none of them: a node the cache
already holds is served untouched (the read does not make it look hot), and any other is decoded and dropped
(`NodeCache::node_once`). The import's readers use it: the people step's tag pass (`import/people.rs`), an append's check
of what is stored (`import/append.rs`), an erase (`import/write.rs`), and the identity-file read the people house makes
(`judge/people/house.rs`). `session_nodes` is unchanged and keeps filling the cache for turns. The tests are
`import/tests_once.rs`: an import, its people step, an append and an erase leave the cache's entries at 0 while the
people step still decodes every node, and a once-read serves a kept node and keeps none it decodes.

| main (message median 3,300) | with the change |
|---|---|
| rest grew 55.5 MiB (15.6 to 71.1) | rest grew -2.1 MiB (25.0 to 22.9) |
| peak 73.5 MiB | peak 58.3 MiB |
| heap in use after: 64.1 MiB | 16.4 MiB |

With messages of median 6,000 bytes: main grew 66.1 MiB, the change 17.3 MiB (live data 2.5 MiB more, redb's index pages;
the rest is allocator slack and the run-to-run spread of the starting rest, 15 to 26 MiB). That is inside the guard's 20 MiB
but not by much: a night of larger messages than the owner's would be close to it.

A turn's time is unchanged. A turn reads its transcript through the stub path and `session_nodes`, which this does not
touch; only the import's four readers changed. Measured with `bench/memory/rss_rest.py` (60 turns, each in a session
just given topics) on the same box and store: a turn's p50 was 20.5 ms on main in two runs, and 19.7 and 11.9 ms with the
change in two runs (the box's spread between runs is as large as that; no run was slower than main's).

## What it does not do

- It does not bound the peak. The people step reads every touched session's new text before it asks the model about the
  first (`people_sync_plan`), so the peak holds all of it at once. It comes back; it is not what rests.
- Other passes that read every session once (the people backfill, the learning backfill, a rebuild) still fill the cache
  through `session_nodes`. They are not the nightly import; they could take `session_nodes_once` the same way.
- It was measured on a synthetic store. The owner's store reads the same way on the night after the install.

## The guard

`bench/memory/rss_import.py` is the guard, and is not a gate step (an import run takes about four minutes). It starts
the daemon, imports a night, waits for the daemon's trim and then 70 s more, and exits 1 when the anonymous memory at rest
is more than `--margin-mib` (20) above what it was before, with both numbers in the message:

```text
import guard FAIL: the daemon rested at 15.6 MiB anonymous before the import and 71.1 after it (+55.5 MiB, margin 20.0)
```

Run it on a box, from a checkout, against a synthetic or a copied store, never a live one:

```text
python3 bench/memory/rss_import.py --theseusd target/release-thin/theseusd --theseus target/release-thin/theseus \
  --sim-bin target/release-thin/theseus-sim --store STORE --work SCRATCH --people --median 3300
```

`--nights 3` imports three nights of new sessions in a row, which is how to see growth that comes with every import.
`--prof PREFIX --preload malloc_prof.so` runs the daemon under the profiler and writes a profile before the import and at
the rest. The 20 MiB margin is the issue's own; the change leaves 0 to 17 MiB over the shapes above.
