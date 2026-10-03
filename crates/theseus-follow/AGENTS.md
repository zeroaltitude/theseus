# theseus-follow

The WAL follower: a store's log read from outside the process that writes it, from a cursor, woken by inotify.

Key modules: `lib.rs`, `wake.rs`. Read by: `theseus-index` (and step 15's durability tender).
