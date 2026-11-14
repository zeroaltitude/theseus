# theseus-follow

The WAL follower: a store's log read from outside the process that writes it, from a cursor, woken by inotify.

Key modules: `lib.rs`, `wake.rs`. Read by: `theseus-index` (and step 15's durability tender).

- `WalFollower::read_upto` reads only frames whose positions are at or before a bound, and stops before the next one
  (`Stop::Held`), never naming a segment it stopped inside sealed: the durability tender ships only frames the writer
  synced (theseus-mgw.12). `read` is `read_upto` with no bound; theseus-index uses it unchanged.
- `WalFollower::skipping_wrong` (theseus-index only) reads on past a frame that passes its checksum but is wrong, listing it in
  `Batch::skipped`: only when a whole frame follows (a cursor never rests on one: it would not verify when reopened), and the next
  frame's positions may begin past the expected one. The default stops with `Corrupt`, which is what a shipper wants.
