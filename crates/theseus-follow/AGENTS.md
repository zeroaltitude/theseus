# theseus-follow

The WAL follower: a store's log read from outside the process that writes it, from a cursor, woken by inotify.

Key modules: `lib.rs`, `wake.rs`. Read by: `theseus-index` (and step 15's durability tender).

- `WalFollower::read_upto` reads only frames whose positions are at or before a bound, and stops before the next one
  (`Stop::Held`), never naming a segment it stopped inside sealed: the durability tender ships only frames the writer
  synced (theseus-mgw.12). `read` is `read_upto` with no bound; theseus-index uses it unchanged.
